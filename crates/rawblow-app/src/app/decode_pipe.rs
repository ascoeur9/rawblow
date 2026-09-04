//! 디코드 파이프라인 글루(분해 6/8): 워커 요청(프리뷰·썸네일·프리로드·프리페치 윈도우)·
//! 결과 드레인(GPU 업로드 상한)·백그라운드 메타 로더. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

impl RawBlowApp {
    /// 현재 위치 주변의 **제한된 윈도우**만 백그라운드(최저 우선순위)로 디스크 캐시에 미리
    /// 디코딩한다(#perf). 폴더 전체(수천 장)를 한꺼번에 큐에 넣으면 느린 디스크가 포화돼
    /// 보이는 셀(prio)이 진행 중인 배경 읽기 뒤에서 굶어, 썸네일이 수십 초~수 분 지연됐다.
    /// 진행 방향(전방 편향) 일정 개수만 데우고, 윈도우는 이동에 따라 슬라이드한다. GPU 업로드
    /// 없이 디스크 저장만 하므로 VRAM·메인 스레드 부담은 없다. 이미 캐시/대기 중인 건 건너뛴다.
    pub(super) fn request_prefetch_window(&mut self) {
        const AHEAD: usize = 80; // 진행 방향으로 데울 개수
        const BEHIND: usize = 16; // 되돌아가기 대비 소량
        let f = self.filtered();
        if f.is_empty() {
            return;
        }
        let cur = self.index.min(f.len() - 1);
        let lo = cur.saturating_sub(BEHIND);
        let hi = (cur + AHEAD).min(f.len() - 1);
        for &real in &f[lo..=hi] {
            // 이미 GPU 캐시에 있거나, 곧 prio로 처리되거나, 이미 프리페치 대기면 건너뛴다.
            // 전경에서 실패한/죽은 파일은 백그라운드가 무한 재시도하지 않는다(#100).
            let display = &self.items[real].entry.display;
            if self.thumbs.contains(real)
                || self.pending_thumb.contains(&real)
                || self.failed_thumb.contains(&real)
                || self.decode_dead(real)
                || rawblow_core::heif::is_heic_path(display)
                || !self.pending_prefetch.insert(real)
            {
                continue;
            }
            let path = display.clone();
            self.worker.request_background(DecodeRequest {
                id: real,
                path,
                full_raw: false,
                max_edge: Some(THUMB_EDGE),
                thumb: true,
                prefetch: true,
                generation: self.generation,
            });
        }
    }

    /// 현재 항목의 메타(EXIF, show_af면 AF·orientation까지)를 백그라운드로 요청한다.
    /// UI 스레드에서 파일을 읽지 않는다 — NAS에서 프리픽스 read가 수백 ms 걸려도
    /// 사진 넘김이 멈추지 않게(표시 먼저, 오버레이는 도착하는 대로). 동시 1건만 띄워
    /// 빠른 연속 넘김 때 건너뛴 항목들이 큐로 쌓여 현재 항목을 막는 일을 차단한다.
    pub(super) fn request_meta(&mut self, ctx: &egui::Context) {
        if self.meta_inflight {
            return;
        }
        let Some(real) = self.current_real() else { return };
        let Some(it) = self.items.get(real) else { return };
        let need_exif = !it.exif_loaded;
        let need_af = self.show_af && !it.af_loaded;
        if !need_exif && !need_af {
            return;
        }
        self.meta_inflight = true;
        let tx = self.meta_tx.clone();
        let path = it.entry.display.clone();
        let generation = self.generation;
        std::thread::spawn(move || {
            // 파서가 패닉해도(손상 파일 등) 결과를 보내 inflight 고착을 막는다.
            let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let exif = need_exif.then(|| read_exif(&path));
                // 표시 배율의 기준이 되는 원본 크기(#48). EXIF와 같은 시점에 한 번만 구한다 —
                // IFD + 블롭당 64KB head probe라 EXIF 읽기와 비슷한 비용이고, 여기서 얻어 두면
                // 텍스처가 프리뷰↔ORIG로 바뀌어도 기준이 흔들리지 않는다.
                let orig_long = need_exif.then(|| rawblow_core::decode::orig_long_edge(&path));
                let af = need_af.then(|| {
                    (
                        rawblow_core::af::parse_af(&path),
                        rawblow_core::meta::orientation(&path),
                    )
                });
                (exif, orig_long, af)
            }));
            let (exif, orig_long, af) = parsed.unwrap_or((
                need_exif.then_some(None),
                need_exif.then_some(None),
                need_af.then_some((None, 1)),
            ));
            let _ = tx.send(MetaResult { generation, real, exif, orig_long, af });
        });
        // 워커 스레드는 egui를 못 깨우므로 결과 수신을 짧은 리페인트로 폴링.
        ctx.request_repaint_after(Duration::from_millis(80));
    }

    /// 메타 로더 결과 반영. 폴더가 바뀐(generation 불일치) 결과는 버린다.
    pub(super) fn drain_meta(&mut self, ctx: &egui::Context) {
        while let Ok(res) = self.meta_rx.try_recv() {
            self.meta_inflight = false;
            if res.generation != self.generation {
                continue;
            }
            if let Some(it) = self.items.get_mut(res.real) {
                if let Some(exif) = res.exif {
                    it.exif = exif;
                    it.exif_loaded = true;
                }
                if let Some(orig_long) = res.orig_long {
                    it.orig_long = orig_long;
                }
                if let Some((af, orient)) = res.af {
                    it.af = af;
                    it.af_loaded = true;
                    it.orient = Some(orient);
                }
            }
        }
        if self.meta_inflight {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    /// 현재 위치 기준 전방 편향 윈도우를 선디코딩 요청한다.
    /// 현재 장은 우선 레인, 이웃은 일반 레인. update가 깨어 있는 한(=keep-alive)
    /// 유휴 상태에서도 윈도우를 끝까지 채운다.
    pub(super) fn request_preload(&mut self) {
        let f = self.filtered();
        if f.is_empty() {
            return;
        }
        let cur = self.index.min(f.len() - 1);
        let real = f[cur];

        // 현재 장 썸네일(즉시 열화 표시용)은 항상.
        self.request_thumb(real, true);

        // 프리뷰는 단일/전체화면에서만 디코딩한다. 그리드에서는 썸네일만 보이므로 프리뷰가
        // 불필요한데, 키보드 ↓로 빠르게 이동하면 인덱스가 매 프레임 바뀌어 프리뷰 윈도우가
        // 격하게 churn(텍스처 대량 생성/파괴)되고, 그 와중에 wgpu가 "Texture destroyed"로
        // 크래시했다. 그리드 내비 중에는 프리뷰를 만들지 않아 churn을 없앤다(전환 시 로드).
        if self.view == ViewMode::Single || self.fullscreen {
            // ORIG(full_raw)면 원본 크기(GPU 한계 내 ORIG_EDGE)로, 아니면 빠른 프리뷰 크기로.
            let want_full = self.full_raw;
            let cur_edge = if self.full_raw { Some(ORIG_EDGE) } else { Some(PREVIEW_EDGE) };
            self.request_preview(real, cur_edge, want_full, true);
            // 전방 편향 윈도우 프리뷰(일반 레인). 윈도우 폭은 사용자 설정(cfg.preload)을 따른다
            // — 이전엔 상수(8)를 써서 설정값이 표시·저장만 되고 실제로 무시됐다(연결 누락 수정).
            let ahead = (self.cfg.preload.max(0) as usize).min(64);
            let behind = if ahead == 0 { 0 } else { (ahead / 3).max(1) };
            let lo = cur.saturating_sub(behind);
            let hi = (cur + ahead).min(f.len() - 1);
            for (fi, &real) in f.iter().enumerate().take(hi + 1).skip(lo) {
                if fi != cur {
                    self.request_preview(real, Some(PREVIEW_EDGE), false, false);
                }
            }
        }

        // 썸네일은 보이는 셀에 대해서만 주문형으로 디코딩한다(그리드·필름스트립이 직접
        // 가시 범위를 우선 요청). 폴더 전체를 미리 GPU 텍스처로 올리면 VRAM이 수GB로 폭증해
        // 업로드 도중 메인 스레드가 GPU에서 멈추는(행) 문제가 있어 사전 일괄 채우기는 폐지.
    }

    /// 프리뷰 디코딩 요청(캐시/진행/실패 가드 포함).
    pub(super) fn request_preview(&mut self, real: usize, max_edge: Option<u32>, full: bool, prio: bool) {
        if prio && !self.decode_dead(real) {
            // 현재 보고 있는 이미지: 일시적으로 실패 마킹됐어도 항상 재시도(디코딩은 사실상 100%).
            // 단, 누적 3회 실패(decode_dead)면 영구 손상으로 보고 재시도를 멈춘다(#64).
            self.failed_preview.remove(&real);
        }
        if self.failed_preview.contains(&real)
            || self.cache.contains_full(real, full)
            || self.pending_preview.contains(&real)
        {
            return;
        }
        if let Some(it) = self.items.get(real) {
            self.pending_preview.insert(real);
            let req = DecodeRequest {
                id: real,
                path: it.entry.display.clone(),
                full_raw: full,
                max_edge,
                thumb: false,
                prefetch: false,
                generation: self.generation,
            };
            if prio {
                self.worker.request_preview(req);
            } else {
                self.worker.request_normal(req);
            }
        }
    }

    /// 썸네일 디코딩 요청(캐시/진행/실패 가드 포함). `prio`면 우선 레인.
    pub(super) fn request_thumb(&mut self, real: usize, prio: bool) {
        if self.thumbs.contains(real) {
            return; // 이미 캐시됨
        }
        if prio {
            // 누적 3회 실패(decode_dead)면 영구 손상으로 보고 재시도를 멈춘다(#64).
            if self.decode_dead(real) {
                return;
            }
            // 보이는 셀: 실패했어도 재시도하고, 일반 레인에 묶여 있어도 우선 레인으로
            // "한 번" 승격 요청한다(pending_thumb_prio로 중복/플러드 차단) → 절대 고착 안 됨.
            self.failed_thumb.remove(&real);
            if self.pending_thumb_prio.contains(&real) {
                return;
            }
        } else if self.failed_thumb.contains(&real) || self.pending_thumb.contains(&real) {
            return;
        }
        if let Some(it) = self.items.get(real) {
            self.pending_thumb.insert(real);
            let req = DecodeRequest {
                id: real,
                path: it.entry.display.clone(),
                full_raw: false,
                max_edge: Some(THUMB_EDGE),
                thumb: true,
                prefetch: false,
                generation: self.generation,
            };
            if prio {
                self.pending_thumb_prio.insert(real);
            }
            // 썸네일은 모두 Thumb 레인(최신 우선)으로 보낸다 — 현재 화면이 먼저 디코딩된다.
            self.worker.request_thumb(req);
        }
    }

    /// HEIC 디코드 실패 시 같은 항목의 RAW로 한 번 재시도(#97).
    fn retry_heic_as_raw(&mut self, real: usize, thumb: bool) -> bool {
        let Some(it) = self.items.get(real) else { return false };
        if !rawblow_core::heif::is_heic_path(&it.entry.display) {
            return false;
        }
        let Some(raw) = it
            .entry
            .members
            .iter()
            .find(|p| rawblow_core::model::kind_of(p) == Some(rawblow_core::model::Kind::Raw))
            .cloned()
        else {
            return false;
        };
        if thumb {
            self.pending_thumb.insert(real);
            self.worker.request_thumb(DecodeRequest {
                id: real,
                path: raw,
                full_raw: false,
                max_edge: Some(THUMB_EDGE),
                thumb: true,
                prefetch: false,
                generation: self.generation,
            });
        } else {
            self.pending_preview.insert(real);
            self.worker.request_preview(DecodeRequest {
                id: real,
                path: raw,
                full_raw: self.full_raw,
                max_edge: if self.full_raw { Some(ORIG_EDGE) } else { Some(PREVIEW_EDGE) },
                thumb: false,
                prefetch: false,
                generation: self.generation,
            });
        }
        true
    }

    /// 워커 결과를 텍스처로 업로드.
    pub(super) fn drain_results(&mut self, ctx: &egui::Context) {
        // 프레임당 GPU 업로드 수를 제한해 버스트로 메인 스레드가 멈추는 것을 방지.
        // 남은 결과는 다음 프레임으로 미루고 즉시 재페인트해 곧바로 이어 비운다.
        let mut uploads = 0usize;
        while uploads < THUMB_UPLOADS_PER_FRAME {
            let res = match self.worker.rx.try_recv() {
                Ok(res) => res,
                Err(_) => break,
            };
            // 큐 상한 초과로 버려진 요청: 해당 pending만 풀어 필요하면(아직 보이면) 재요청되게 한다.
            // **현재 세대일 때만** 푼다: 폴더 전환 직후 도착한 옛 세대 dropped가 새 폴더의 동일 id
            // pending(라이브)을 잘못 지워 중복 디코딩시키지 않게(프리페치·정상 결과 경로와 대칭).
            if res.dropped {
                if res.generation == self.generation {
                    if res.prefetch {
                        self.pending_prefetch.remove(&res.id);
                    } else if res.thumb {
                        self.pending_thumb.remove(&res.id);
                        self.pending_thumb_prio.remove(&res.id);
                    } else {
                        self.pending_preview.remove(&res.id);
                    }
                }
                continue;
            }
            // 프리페치 완료 통지: 디스크 캐시만 채웠으므로 GPU 업로드 없이 pending만 정리한다.
            // (업로드 상한 uploads를 소비하지 않아 보이는 셀 업로드를 막지 않는다.)
            if res.prefetch {
                if res.generation == self.generation {
                    self.pending_prefetch.remove(&res.id);
                    if res.image.is_err() {
                        self.failed_thumb.insert(res.id);
                        let n = self.decode_fails.entry(res.id).or_insert(0);
                        *n = n.saturating_add(1);
                    }
                }
                continue;
            }
            if res.thumb {
                self.pending_thumb.remove(&res.id);
                self.pending_thumb_prio.remove(&res.id);
            } else {
                self.pending_preview.remove(&res.id);
            }
            if res.generation != self.generation {
                continue; // 오래된 결과
            }
            if let Ok(img) = res.image {
                // 성공 디코드 → 누적 실패 카운터·실패 마킹 리셋(#75). 일시적(NAS 끊김 등) 실패가
                // 성공 사이에 쌓여 정상 파일이 영구 손상(decode_dead)으로 오판되지 않게, 임계는
                // "연속" 실패에 가깝게 유지한다(성공하면 0으로).
                self.decode_fails.remove(&res.id);
                self.failed_thumb.remove(&res.id);
                self.failed_preview.remove(&res.id);
                if res.thumb {
                    // 이미 캐시돼 있으면 중복 결과는 버린다(우선-승격으로 같은 썸네일이
                    // 두 번 디코딩될 수 있는데, 사용 중인 텍스처 핸들을 드롭하면 wgpu가
                    // 파괴된 텍스처를 참조해 크래시함 → 재삽입 금지).
                    if !self.thumbs.contains(res.id) {
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [img.width as usize, img.height as usize],
                            &img.rgba,
                        );
                        let handle = ctx.load_texture(
                            format!("thumb{}", res.id),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumbs.insert(res.id, handle, false);
                        uploads += 1;
                    }
                } else {
                    self.histo.insert(res.id, compute_histo(&img.rgba));
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [img.width as usize, img.height as usize],
                        &img.rgba,
                    );
                    let handle = ctx.load_texture(
                        format!("tex{}", res.id),
                        color,
                        egui::TextureOptions::LINEAR,
                    );
                    self.cache.insert(res.id, handle, res.full_raw);
                    uploads += 1;
                    if self.full_raw && !res.full_raw && self.current_real() == Some(res.id) {
                        self.toast_info(tr(self.lang, "원본 해상도를 못 구해 프리뷰로 표시합니다").into());
                    }
                }
            } else if res.thumb {
                let n = self.decode_fails.entry(res.id).or_insert(0);
                *n = n.saturating_add(1);
                if *n == 1 && self.retry_heic_as_raw(res.id, true) {
                    continue;
                }
                self.failed_thumb.insert(res.id);
            } else {
                let n = self.decode_fails.entry(res.id).or_insert(0);
                *n = n.saturating_add(1);
                if *n == 1 && self.retry_heic_as_raw(res.id, false) {
                    continue;
                }
                self.failed_preview.insert(res.id);
            }
        }
        // 업로드 상한에 걸려 남은 결과가 있으면 곧바로 다음 프레임에서 이어 비운다.
        if !self.worker.rx.is_empty() {
            ctx.request_repaint();
        }
    }
}
