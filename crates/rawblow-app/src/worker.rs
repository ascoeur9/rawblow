//! 백그라운드 디코딩 워커 (F7). UI 스레드는 렌더만, 디코딩은 스레드풀에서.
//!
//! 두 개의 요청 레인을 둔다: `prio`(현재 보고 있는 이미지)는 일반 레인보다 항상
//! 먼저 처리되어, 화살표 연타로 요청이 쌓여도 현재 사진이 빨리 뜬다.

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use rawblow_core::decode::{decode_file, DecodeOptions, DecodedImage};
use std::path::PathBuf;

/// 디코딩 요청. `generation`은 폴더 전환 등으로 무효화된 오래된 결과를 버리기 위함.
pub struct DecodeRequest {
    pub id: usize,
    pub path: PathBuf,
    pub full_raw: bool,
    /// 결과 최대 변 길이 제한(다운스케일). 썸네일은 작게, 프리뷰는 크게.
    pub max_edge: Option<u32>,
    /// true면 썸네일 캐시로, false면 프리뷰 캐시로.
    pub thumb: bool,
    pub generation: u64,
}

pub struct DecodeResult {
    pub id: usize,
    pub generation: u64,
    pub full_raw: bool,
    pub thumb: bool,
    pub image: Result<DecodedImage, String>,
}

pub struct Worker {
    tx: Sender<DecodeRequest>,
    prio_tx: Sender<DecodeRequest>,
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
        let (res_tx, res_rx) = unbounded::<DecodeResult>();

        for _ in 0..threads.max(1) {
            let req_rx = req_rx.clone();
            let prio_rx = prio_rx.clone();
            let res_tx = res_tx.clone();
            let cache_dir = cache_dir.clone();
            std::thread::spawn(move || loop {
                // 우선순위 레인을 먼저 비운다. 없으면 둘 중 먼저 오는 것을 기다린다.
                let req = match prio_rx.try_recv() {
                    Ok(req) => req,
                    Err(_) => select! {
                        recv(prio_rx) -> m => match m { Ok(r) => r, Err(_) => break },
                        recv(req_rx) -> m => match m { Ok(r) => r, Err(_) => break },
                    },
                };
                let path = req.path.clone();
                let full_raw = req.full_raw;
                let max_edge = req.max_edge;
                let is_thumb = req.thumb;

                // 썸네일은 디스크 캐시 먼저 조회 → 히트면 디코딩을 건너뛰고 즉시 반환(#22).
                let cache_key = if is_thumb {
                    rawblow_core::cache::thumb_key(&path, max_edge.unwrap_or(0))
                } else {
                    None
                };
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
                            thumb: true,
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

                // 디코딩 성공한 썸네일은 디스크 캐시에 저장(다음 실행/재오픈 때 즉시 표시).
                if let (Some(key), Ok(img)) = (&cache_key, &image) {
                    rawblow_core::cache::store(&cache_dir, key, img);
                }

                let _ = res_tx.send(DecodeResult {
                    id: req.id,
                    generation: req.generation,
                    full_raw: req.full_raw,
                    thumb: req.thumb,
                    image,
                });
            });
        }

        Worker {
            tx: req_tx,
            prio_tx,
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
}
