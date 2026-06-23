//! 백그라운드 디코딩 워커. UI 스레드는 렌더만, 디코딩은 스레드풀에서.
//!
//! **뷰포트 기반 최신 우선(LIFO) + 상한 스케줄러.** 빠른 스크롤로 요청이 쏟아져도 큐가
//! 무한정 쌓이지 않고, 항상 **가장 최근(현재 화면)** 요청부터 디코딩한다. 큐가 상한을
//! 넘으면 가장 오래된(지나간 화면) 요청을 버리고 UI에 통지해 pending을 풀게 한다. 과거의
//! FIFO 무한 큐에서는 수천 장 backlog 뒤에 현재 화면이 줄서 썸네일이 수 분씩 지연됐다.
//!
//! 레인 우선순위: Preview(현재 보는 이미지) > Thumb(보이는 셀) > Normal(프리뷰 이웃) >
//! Bg(프리페치 윈도우). 각 레인은 LIFO(최신 우선). Bg는 일부 스레드만 처리해 느린 디스크에서
//! 전경 대역폭을 남긴다.

use crossbeam_channel::{unbounded, Receiver, Sender};
use rawblow_core::decode::{decode_file, DecodeOptions, DecodedImage};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// 디코딩 요청. `generation`은 폴더 전환 등으로 무효화된 오래된 요청/결과를 버리기 위함.
pub struct DecodeRequest {
    pub id: usize,
    pub path: PathBuf,
    pub full_raw: bool,
    /// 결과 최대 변 길이 제한(다운스케일). 썸네일은 작게, 프리뷰는 크게.
    pub max_edge: Option<u32>,
    /// true면 썸네일 캐시로, false면 프리뷰 캐시로.
    pub thumb: bool,
    /// true면 백그라운드 프리페치: 디스크 캐시에만 채우고(이미 있으면 디코딩도 생략)
    /// 결과 픽셀은 보내지 않는다(GPU 업로드·메모리 부담 없음).
    pub prefetch: bool,
    pub generation: u64,
}

pub struct DecodeResult {
    pub id: usize,
    pub generation: u64,
    pub full_raw: bool,
    pub thumb: bool,
    /// 프리페치 완료 통지(픽셀 없음). UI는 pending만 정리하고 업로드하지 않는다.
    pub prefetch: bool,
    /// 큐 상한 초과로 디코딩 없이 버려진 요청 통지. UI가 해당 pending을 풀어 필요하면 재요청.
    pub dropped: bool,
    pub image: Result<DecodedImage, String>,
}

/// 레인별 상한(요청 개수). 현재 화면 + 약간의 여유. 초과 시 가장 오래된 요청을 버린다.
const PREVIEW_CAP: usize = 4;
const THUMB_CAP: usize = 256;
const NORMAL_CAP: usize = 32;
const BG_CAP: usize = 192;

#[derive(Clone, Copy)]
enum Lane {
    Preview,
    Thumb,
    Normal,
    Bg,
}

struct Lanes {
    preview: VecDeque<DecodeRequest>,
    thumb: VecDeque<DecodeRequest>,
    normal: VecDeque<DecodeRequest>,
    bg: VecDeque<DecodeRequest>,
    shutdown: bool,
}

struct Sched {
    lanes: Mutex<Lanes>,
    cv: Condvar,
}

pub struct Worker {
    sched: Arc<Sched>,
    res_tx: Sender<DecodeResult>,
    /// 현재 폴더 세대. 워커는 이보다 오래된 요청을 디코딩하지 않고 버린다.
    cur_gen: Arc<AtomicU64>,
    pub rx: Receiver<DecodeResult>,
}

impl Worker {
    pub fn new(threads: usize, cache_dir: PathBuf) -> Self {
        let (res_tx, res_rx) = unbounded::<DecodeResult>();
        let cur_gen = Arc::new(AtomicU64::new(0));
        let sched = Arc::new(Sched {
            lanes: Mutex::new(Lanes {
                preview: VecDeque::new(),
                thumb: VecDeque::new(),
                normal: VecDeque::new(),
                bg: VecDeque::new(),
                shutdown: false,
            }),
            cv: Condvar::new(),
        });

        // 백그라운드(프리페치) 레인은 일부 스레드만 처리한다. 느린 디스크에서 모든 워커가
        // 프리페치 읽기에 매달리면 전경(보이는 셀)이 대역폭을 못 얻어 굶는다.
        let bg_threads = (threads / 2).clamp(1, 2);
        // 스레드 0은 Preview·Normal 전용 — Thumb 레인을 절대 받지 않는다. 스크롤 중 모든 스레드가
        // 썸네일 디코딩에 묶여 있을 때도, 스레드 0은 곧바로 Preview를 집어 단일뷰 지연을 방지한다.
        // 나머지 스레드는 기존 우선순위(Preview > Thumb > Normal > Bg)로 모든 레인을 처리한다.
        for idx in 0..threads.max(1) {
            let sched = sched.clone();
            let res_tx = res_tx.clone();
            let cache_dir = cache_dir.clone();
            let cur_gen = cur_gen.clone();
            // 스레드 0은 Preview/Normal 전용(스레드 수가 2 이상일 때만). 단일 스레드 환경에서는
            // 유일한 워커라 전체 레인을 처리해야 한다.
            let serves_thumb = idx > 0 || threads < 2;
            let serves_bg = serves_thumb && idx < bg_threads;
            std::thread::spawn(move || loop {
                // 최신 우선으로 한 요청을 꺼낸다(레인 우선순위 + LIFO). 없으면 대기.
                let req = {
                    let mut g = sched.lanes.lock().unwrap();
                    loop {
                        if g.shutdown {
                            return;
                        }
                        if let Some(r) = g.preview.pop_back() {
                            break r;
                        }
                        if serves_thumb {
                            if let Some(r) = g.thumb.pop_back() {
                                break r;
                            }
                        }
                        if let Some(r) = g.normal.pop_back() {
                            break r;
                        }
                        if serves_bg {
                            if let Some(r) = g.bg.pop_back() {
                                break r;
                            }
                        }
                        g = sched.cv.wait(g).unwrap();
                    }
                };

                // 폴더 전환 등으로 무효화된 오래된 요청은 디코딩하지 않고 버린다(UI는 폴더 전환
                // 시 pending을 모두 비우므로 결과를 안 보내도 묶임이 남지 않는다).
                if req.generation < cur_gen.load(Ordering::Relaxed) {
                    continue;
                }

                let path = req.path.clone();
                let full_raw = req.full_raw;
                let max_edge = req.max_edge;
                let is_thumb = req.thumb;

                // 디스크 캐시 대상: 썸네일 + (full_raw 아닌) 빠른 프리뷰. 키에 max_edge가 들어가
                // 썸네일(320)·프리뷰(1600)가 공존. ORIG(full_raw)는 원본 화질이라 캐시하지 않는다.
                let cacheable = is_thumb || !full_raw;
                let cache_key = if cacheable {
                    rawblow_core::cache::thumb_key(&path, max_edge.unwrap_or(0))
                } else {
                    None
                };

                // 백그라운드 프리페치: 이미 캐시에 있으면 디코딩조차 건너뛴다. 없으면 디코딩 후
                // 디스크에만 저장(픽셀 미전송 → GPU/메모리 비용 0).
                if req.prefetch {
                    let have = cache_key
                        .as_ref()
                        .map(|k| rawblow_core::cache::exists(&cache_dir, k))
                        .unwrap_or(false);
                    if !have {
                        if let Some(key) = &cache_key {
                            let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                decode_file(&path, DecodeOptions { full_raw, max_edge })
                            }))
                            .ok()
                            .and_then(|r| r.ok());
                            if let Some(img) = img {
                                rawblow_core::cache::store(&cache_dir, key, &img);
                            }
                        }
                    }
                    let _ = res_tx.send(DecodeResult {
                        id: req.id,
                        generation: req.generation,
                        full_raw,
                        thumb: req.thumb,
                        prefetch: true,
                        dropped: false,
                        image: Ok(empty_image()),
                    });
                    continue;
                }

                if let Some(key) = &cache_key {
                    // 캐시 로드도 패닉 격리: 손상 캐시 JPEG에서 디코더가 패닉해도 워커가 죽지 않게
                    // None(미스)으로 강등 → 정상 디코딩 경로로 폴백.
                    let cached = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        rawblow_core::cache::load(&cache_dir, key)
                    }))
                    .unwrap_or(None);
                    if let Some(img) = cached {
                        let _ = res_tx.send(DecodeResult {
                            id: req.id,
                            generation: req.generation,
                            full_raw: req.full_raw,
                            thumb: req.thumb,
                            prefetch: false,
                            dropped: false,
                            image: Ok(img),
                        });
                        continue;
                    }
                }

                // 패닉 격리: rawloader 등 디코더가 특정 파일에 패닉해도 워커가 죽지 않게 Err 변환.
                #[cfg(debug_assertions)]
                let _t0 = std::time::Instant::now();
                let image = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_file(&path, DecodeOptions { full_raw, max_edge })
                })) {
                    Ok(r) => r.map_err(|e| e.to_string()),
                    Err(_) => Err("panic during decode".to_string()),
                };
                #[cfg(debug_assertions)]
                if !is_thumb && !req.prefetch {
                    eprintln!(
                        "[preview] {:>4}ms  {:?}",
                        _t0.elapsed().as_millis(),
                        path.file_name().unwrap_or_default()
                    );
                }

                // 디코딩 성공한 캐시 대상은 디스크 캐시에 저장(재오픈·재방문 즉시 표시).
                if let (Some(key), Ok(img)) = (&cache_key, &image) {
                    rawblow_core::cache::store(&cache_dir, key, img);
                }

                let _ = res_tx.send(DecodeResult {
                    id: req.id,
                    generation: req.generation,
                    full_raw: req.full_raw,
                    thumb: req.thumb,
                    prefetch: false,
                    dropped: false,
                    image,
                });
            });
        }

        Worker {
            sched,
            res_tx,
            cur_gen,
            rx: res_rx,
        }
    }

    /// 한 레인에 요청을 넣는다(LIFO). 상한 초과 시 가장 오래된 요청을 버리고 UI에 통지한다.
    fn push(&self, lane: Lane, req: DecodeRequest) {
        let dropped = {
            let mut g = self.sched.lanes.lock().unwrap();
            let (q, cap) = match lane {
                Lane::Preview => (&mut g.preview, PREVIEW_CAP),
                Lane::Thumb => (&mut g.thumb, THUMB_CAP),
                Lane::Normal => (&mut g.normal, NORMAL_CAP),
                Lane::Bg => (&mut g.bg, BG_CAP),
            };
            q.push_back(req);
            if q.len() > cap {
                q.pop_front()
            } else {
                None
            }
        };
        self.sched.cv.notify_all();
        if let Some(d) = dropped {
            // 상한 초과로 버려진 요청 → UI가 pending을 풀도록 통지(없으면 그 셀이 영영 안 뜸).
            let _ = self.res_tx.send(DecodeResult {
                id: d.id,
                generation: d.generation,
                full_raw: d.full_raw,
                thumb: d.thumb,
                prefetch: d.prefetch,
                dropped: true,
                image: Err(String::new()),
            });
        }
    }

    /// 현재 보는 이미지의 프리뷰(최우선).
    pub fn request_preview(&self, req: DecodeRequest) {
        self.push(Lane::Preview, req);
    }

    /// 프리뷰 이웃 선디코딩(현재 다음 우선).
    pub fn request_normal(&self, req: DecodeRequest) {
        self.push(Lane::Normal, req);
    }

    /// 보이는 셀 썸네일(현재 화면 최신 우선).
    pub fn request_thumb(&self, req: DecodeRequest) {
        self.push(Lane::Thumb, req);
    }

    /// 백그라운드 프리페치(가장 낮은 우선순위). 디스크 캐시만 채운다.
    pub fn request_background(&self, req: DecodeRequest) {
        self.push(Lane::Bg, req);
    }

    /// 현재 폴더 세대를 갱신한다. 이보다 오래된 요청은 워커가 디코딩 없이 버린다.
    pub fn set_generation(&self, generation: u64) {
        self.cur_gen.store(generation, Ordering::Relaxed);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut g) = self.sched.lanes.lock() {
            g.shutdown = true;
        }
        self.sched.cv.notify_all();
    }
}

fn empty_image() -> DecodedImage {
    DecodedImage {
        width: 0,
        height: 0,
        rgba: Vec::new(),
        color_managed: false,
        full_raw: false,
    }
}
