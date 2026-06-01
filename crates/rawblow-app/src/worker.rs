//! 백그라운드 디코딩 워커 (F7). UI 스레드는 렌더만, 디코딩은 스레드풀에서.
//!
//! 두 개의 요청 레인을 둔다: `prio`(현재 보고 있는 이미지)는 일반 레인보다 항상
//! 먼저 처리되어, 화살표 연타로 요청이 쌓여도 현재 사진이 빨리 뜬다.

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use rawblow_core::decode::{decode_file, DecodeOptions, DecodedImage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 디코딩 요청. `generation`은 폴더 전환 등으로 무효화된 오래된 결과를 버리기 위함.
pub struct DecodeRequest {
    pub id: usize,
    pub path: PathBuf,
    pub full_raw: bool,
    /// 결과 최대 변 길이 제한(다운스케일). 썸네일은 작게, 프리뷰는 크게.
    pub max_edge: Option<u32>,
    /// true면 썸네일 캐시로, false면 프리뷰 캐시로.
    pub thumb: bool,
    /// true면 백그라운드 프리페치: 디스크 캐시에만 채우고(이미 있으면 디코딩도 생략)
    /// 결과 픽셀은 보내지 않는다(GPU 업로드·메모리 부담 없음). 가장 낮은 우선순위.
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
    pub image: Result<DecodedImage, String>,
}

pub struct Worker {
    tx: Sender<DecodeRequest>,
    prio_tx: Sender<DecodeRequest>,
    bg_tx: Sender<DecodeRequest>,
    /// 현재 폴더 세대. 워커는 이보다 오래된 요청을 디코딩하지 않고 버린다
    /// (폴더 전환 시 프리페치 큐에 쌓인 이전 폴더 전체를 헛디코딩하는 것을 막는다).
    cur_gen: Arc<AtomicU64>,
    pub rx: Receiver<DecodeResult>,
}

impl Worker {
    /// `threads`개 워커를 띄운다. 워커는 채널에만 결과를 넣고, UI 갱신(repaint)은
    /// 메인 스레드의 100ms keep-alive 타이머가 담당한다. (워커가 직접 egui를
    /// 건드리면 컨텍스트 락에서 메인 스레드와 엉켜 폴더 끝부분에서 멈추는 문제가 있었다.)
    ///
    /// `cache_dir`은 썸네일 디스크 캐시(#22) 위치다. 썸네일 요청은 디코딩 전에 캐시를
    /// 먼저 조회하고(히트면 즉시 반환), 미스면 디코딩 후 캐시에 저장한다. 프리뷰는 캐시하지 않는다.
    pub fn new(threads: usize, cache_dir: PathBuf) -> Self {
        let (req_tx, req_rx) = unbounded::<DecodeRequest>();
        let (prio_tx, prio_rx) = unbounded::<DecodeRequest>();
        let (bg_tx, bg_rx) = unbounded::<DecodeRequest>();
        let (res_tx, res_rx) = unbounded::<DecodeResult>();
        let cur_gen = Arc::new(AtomicU64::new(0));

        for _ in 0..threads.max(1) {
            let req_rx = req_rx.clone();
            let prio_rx = prio_rx.clone();
            let bg_rx = bg_rx.clone();
            let res_tx = res_tx.clone();
            let cache_dir = cache_dir.clone();
            let cur_gen = cur_gen.clone();
            std::thread::spawn(move || loop {
                // 우선순위: prio > 일반 > 백그라운드(프리페치). prio·일반을 먼저 비우고,
                // 셋 다 비면 먼저 오는 것을 기다린다. 프리페치는 항상 가장 늦게 처리돼
                // 현재 보거나 스크롤 중인 셀을 절대 막지 않는다.
                let req = match prio_rx.try_recv() {
                    Ok(req) => req,
                    Err(_) => match req_rx.try_recv() {
                        Ok(req) => req,
                        Err(_) => select! {
                            recv(prio_rx) -> m => match m { Ok(r) => r, Err(_) => break },
                            recv(req_rx) -> m => match m { Ok(r) => r, Err(_) => break },
                            recv(bg_rx) -> m => match m { Ok(r) => r, Err(_) => break },
                        },
                    },
                };

                // 폴더 전환 등으로 무효화된 오래된 요청은 디코딩하지 않고 버린다. (UI는 폴더
                // 전환 시 pending 집합을 모두 비우므로 결과를 안 보내도 묶임이 남지 않는다.)
                if req.generation < cur_gen.load(Ordering::Relaxed) {
                    continue;
                }

                let path = req.path.clone();
                let full_raw = req.full_raw;
                let max_edge = req.max_edge;
                let is_thumb = req.thumb;

                // 디스크 캐시 대상: 썸네일 + (full_raw 아닌) 빠른 프리뷰. 키에 max_edge가
                // 들어가 썸네일(320)·프리뷰(1600)가 서로 다른 항목으로 공존한다(#22 확장).
                // ORIG(full_raw)는 원본 화질이라 캐시하지 않고 항상 새로 현상한다.
                let cacheable = is_thumb || !full_raw;
                let cache_key = if cacheable {
                    rawblow_core::cache::thumb_key(&path, max_edge.unwrap_or(0))
                } else {
                    None
                };

                // 백그라운드 프리페치: 이미 캐시에 있으면 디코딩조차 건너뛴다. 없으면 디코딩 후
                // 디스크에만 저장한다. 픽셀은 보내지 않으므로 GPU 업로드·메모리 전송 비용이 0.
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
                        image: Ok(DecodedImage {
                            width: 0,
                            height: 0,
                            rgba: Vec::new(),
                            color_managed: false,
                            full_raw: false,
                        }),
                    });
                    continue;
                }

                if let Some(key) = &cache_key {
                    // 캐시 로드도 패닉 격리: 손상된 캐시 JPEG에서 image 디코더가 패닉해도
                    // 워커 스레드가 죽지 않게 None(미스)으로 강등 → 정상 디코딩 경로로 폴백.
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
                            image: Ok(img),
                        });
                        continue;
                    }
                }

                // 패닉 격리: rawloader 등 디코더가 특정 파일에 패닉해도 워커 스레드가
                // 죽지 않도록 catch_unwind로 Err 변환(스레드 풀 고갈·pending 묶임 방지).
                let image = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_file(&path, DecodeOptions { full_raw, max_edge })
                })) {
                    Ok(r) => r.map_err(|e| e.to_string()),
                    Err(_) => Err("panic during decode".to_string()),
                };

                // 디코딩 성공한 캐시 대상(썸네일·프리뷰)은 디스크 캐시에 저장(재오픈·재방문 즉시 표시).
                if let (Some(key), Ok(img)) = (&cache_key, &image) {
                    rawblow_core::cache::store(&cache_dir, key, img);
                }

                let _ = res_tx.send(DecodeResult {
                    id: req.id,
                    generation: req.generation,
                    full_raw: req.full_raw,
                    thumb: req.thumb,
                    prefetch: false,
                    image,
                });
            });
        }

        Worker {
            tx: req_tx,
            prio_tx,
            bg_tx,
            cur_gen,
            rx: res_rx,
        }
    }

    /// 일반 요청(프리로드·썸네일 등).
    pub fn request(&self, req: DecodeRequest) {
        let _ = self.tx.send(req);
    }

    /// 우선 요청(현재 보고 있는 이미지). 일반 요청보다 먼저 처리된다.
    pub fn request_priority(&self, req: DecodeRequest) {
        let _ = self.prio_tx.send(req);
    }

    /// 백그라운드 프리페치 요청(가장 낮은 우선순위). 디스크 캐시만 채운다.
    pub fn request_background(&self, req: DecodeRequest) {
        let _ = self.bg_tx.send(req);
    }

    /// 현재 폴더 세대를 갱신한다. 이보다 오래된 요청은 워커가 디코딩 없이 버린다.
    pub fn set_generation(&self, generation: u64) {
        self.cur_gen.store(generation, Ordering::Relaxed);
    }
}
