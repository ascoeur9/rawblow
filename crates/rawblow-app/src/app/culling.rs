//! AI 컬링(#50~#53)(분해 7/8): 설정 다이얼로그·채점 파이프라인(start/pump/apply)·
//! 확인 모달·모델 다운로드·컬링 캐시·메타 수집. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

/// AI 컬링(#50) 백그라운드 채점 완료 메시지. 진행률은 공유 원자 카운터(`AiCullJob::progress`)로
/// 전달하므로(워커가 여러 개라 메시지 순서가 뒤섞이지 않게), 채널은 최종 결과만 보낸다.
pub(super) enum AiCullMsg {
    /// 채점 완료. (원본 items 인덱스, CV 판정, CLIP-IQA P(good)) 목록 + 그룹 컬링용 부가정보.
    Done(
        Vec<(usize, rawblow_core::quality::Verdict, Option<f32>)>,
        std::collections::HashMap<usize, CullExtra>,
    ),
}

/// 그룹 컬링(메타·연사·중복)용 컷별 부가정보. 본 판정 흐름과 분리해 워커가 부가 수집한다.
#[derive(Clone, Default)]
pub(super) struct CullExtra {
    /// 그룹 내 베스트 랭킹용 선명도(미적 점수 없을 때 fallback).
    pub(super) sharp: f32,
    pub(super) dhash: Option<u64>,
    pub(super) shot_time: Option<i64>,
    pub(super) meta: rawblow_core::cull_ext::PhotoMeta,
    /// 얼굴 존재(YuNet). "얼굴 있는 컷"·"장르 인물/풍경" 하드필터에 사용. 미검사면 None.
    pub(super) face: Option<bool>,
    /// CLIP sharp 축 P(sharp). "AI 선명도" 하드필터에 사용. 미검사면 None.
    pub(super) sharp_ai: Option<f32>,
    /// 설정 객체 클래스 포함 여부(YOLO). "객체 포함" 하드필터에 사용. 미검사면 None.
    pub(super) object_match: Option<bool>,
}

/// 진행 중인 AI 컬링 채점(#50). 워커 풀(여러 스레드)이 디코딩+채점을 병렬로 수행하고,
/// 코디네이터 스레드가 전원 합류 후 결과를 한 번에 보낸다. 진행 개수는 `progress` 원자로 읽는다.
/// 백그라운드 비차단 실행: 채점 중에도 앱은 조작 가능하되, 결과가 덮어쓸 분류축(`target`)은
/// 잠그고, 폴더가 바뀌면(`generation` 불일치) 어긋난 인덱스의 결과를 폐기한다.
pub(super) struct AiCullJob {
    pub(super) rx: crossbeam_channel::Receiver<AiCullMsg>,
    pub(super) cancel: Arc<AtomicBool>,
    /// 완료한 이미지 수(워커들이 fetch_add로 증가, 메인이 매 프레임 읽어 프로그레스바에 표시).
    pub(super) progress: Arc<std::sync::atomic::AtomicUsize>,
    /// 캐시에서 재사용한 장 수(디코드/추론 생략). 완료 토스트에 표시 — 캐시 동작 확인용.
    pub(super) cache_hits: Arc<AtomicUsize>,
    pub(super) total: usize,
    /// 작업 시작 시점의 폴더 세대. 완료 시 현재 세대와 다르면 인덱스가 무효 → 결과 폐기.
    pub(super) generation: u64,
    /// 이 작업이 결과를 배정할 분류축. 채점 중 이 축의 수동 편집을 막는다.
    pub(super) target: AiCullTarget,
}

/// 컬링 결과 캐시 항목(#50). 같은 파일을 같은 설정으로 재컬링할 때 디코드+채점을 건너뛴다.
/// `sig`는 (검사 신호 on/off + AF + 모델 식별자)의 해시 — 임계값(focus_thresh 등)은 제외하므로
/// **임계값만 바꿔 재실행하면 전부 캐시 적중**해 즉시 재판정된다(신호/모델을 바꾸면 미스→재계산).
#[derive(Clone)]
pub(super) struct CullCacheEntry {
    pub(super) mtime: std::time::SystemTime,
    pub(super) sig: u64,
    pub(super) report: rawblow_core::quality::QualityReport,
    /// dHash(시각중복용). 미적/CV만 돌린 옛 캐시엔 없을 수 있음(None) → dedup 필요 시 재디코드.
    pub(super) dhash: Option<u64>,
}

/// 디스크 직렬화용 미러(세션 간 재컬링 즉시화). SystemTime은 UNIX epoch 나노초로 저장.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CullCacheDisk {
    pub(super) path: String,
    pub(super) mtime_nanos: u64,
    pub(super) sig: u64,
    pub(super) report: rawblow_core::quality::QualityReport,
    #[serde(default)]
    pub(super) dhash: Option<u64>,
}

/// 컬링 캐시 파일 경로(`config_dir/cull-cache.json`).
pub(super) fn cull_cache_file() -> PathBuf {
    config::config_dir().join("cull-cache.json")
}

/// 디스크에서 캐시 로드(없거나 손상 시 빈 맵 — 안전: 적중은 mtime+sig로만 발생).
pub(super) fn load_cull_cache() -> std::collections::HashMap<PathBuf, CullCacheEntry> {
    load_cull_cache_from(&cull_cache_file())
}

pub(super) fn load_cull_cache_from(path: &std::path::Path) -> std::collections::HashMap<PathBuf, CullCacheEntry> {
    use std::time::{Duration, UNIX_EPOCH};
    let mut map = std::collections::HashMap::new();
    let Ok(s) = std::fs::read_to_string(path) else { return map };
    let Ok(list) = serde_json::from_str::<Vec<CullCacheDisk>>(&s) else { return map };
    for d in list {
        map.insert(
            PathBuf::from(d.path),
            CullCacheEntry { mtime: UNIX_EPOCH + Duration::from_nanos(d.mtime_nanos), sig: d.sig, report: d.report, dhash: d.dhash },
        );
    }
    map
}

/// 캐시를 디스크에 저장(최대 50k 항목으로 상한 — 파일 크기 ~7MB 이하 유지).
pub(super) fn save_cull_cache(map: &std::collections::HashMap<PathBuf, CullCacheEntry>) {
    let _ = std::fs::create_dir_all(config::config_dir());
    save_cull_cache_to(&cull_cache_file(), map);
}

pub(super) fn save_cull_cache_to(path: &std::path::Path, map: &std::collections::HashMap<PathBuf, CullCacheEntry>) {
    use std::time::UNIX_EPOCH;
    let mut list: Vec<CullCacheDisk> = map
        .iter()
        .filter_map(|(p, e)| {
            let nanos = e.mtime.duration_since(UNIX_EPOCH).ok()?.as_nanos() as u64;
            Some(CullCacheDisk { path: p.to_string_lossy().into_owned(), mtime_nanos: nanos, sig: e.sig, report: e.report, dhash: e.dhash })
        })
        .collect();
    list.truncate(50_000);
    if let Ok(s) = serde_json::to_string(&list) {
        // 원자적 교체: 저장 도중 크래시에도 기존 캐시가 잘린 채 남지 않는다(재컬링 방지).
        let _ = rawblow_core::fsio::write_atomic(path, s.as_bytes());
    }
}

/// 그룹 컬링용 컷별 부가정보 구성. need_meta면 EXIF(헤더만, 저렴)를 읽어 촬영시각·메타를 채운다.
pub(super) fn cull_extra(
    report: &rawblow_core::quality::QualityReport,
    dhash: Option<u64>,
    path: &std::path::Path,
    need_meta: bool,
) -> CullExtra {
    let (shot_time, meta) = if need_meta {
        match read_exif(path) {
            Some(e) => {
                let m = rawblow_core::cull_ext::PhotoMeta::from_exif(&e);
                (m.datetime_secs, m)
            }
            None => (None, Default::default()),
        }
    } else {
        (None, Default::default())
    };
    CullExtra { sharp: report.focus.sharpness, dhash, shot_time, meta, face: report.face, sharp_ai: report.sharp_ai, object_match: report.object_match }
}

/// CLIP-IQA 모델 자동 다운로드(#50) 진행 메시지.
#[cfg(feature = "model-download")]
pub(super) enum ModelDlMsg {
    /// (다운로드된 바이트, 전체 바이트 — 0이면 미확인).
    Progress(u64, u64),
    /// 다운로드+검증 완료. Err이면 오류 메시지.
    Done(Result<(), String>),
}

/// 진행 중인 모델 다운로드.
#[cfg(feature = "model-download")]
pub(super) struct ModelDlJob {
    pub(super) rx: crossbeam_channel::Receiver<ModelDlMsg>,
    pub(super) label: String,
    pub(super) done: u64,
    pub(super) total: u64,
}

/// AI 컬링 메타 수집 결과(카메라/렌즈 distinct, 정렬됨).
pub(super) struct CullMetaResult {
    pub(super) generation: u64,
    pub(super) cameras: Vec<String>,
    pub(super) lenses: Vec<String>,
}

impl RawBlowApp {
    /// AI 컬링 카메라/렌즈 리스트박스용 distinct 목록을 백그라운드로 한 번 수집한다(#51 후속).
    /// 현재 generation에 대해 아직 수집을 시작하지 않았을 때만 띄운다 — 폴더가 바뀌면 generation이
    /// 올라가 자동 재수집. EXIF prefix read를 워커 스레드에서 돌려 UI(특히 NAS)를 막지 않는다.
    pub(super) fn ensure_cull_meta_scan(&mut self, ctx: &egui::Context) {
        if self.cull_meta_gen == Some(self.generation) {
            return; // 이번 폴더는 이미 수집(또는 진행 중).
        }
        self.cull_meta_gen = Some(self.generation);
        self.cull_meta_cameras.clear();
        self.cull_meta_lenses.clear();
        // 이미 로드된 EXIF는 재독 없이 바로 쓰고, 나머지만 워커에서 prefix read.
        let mut have_cam: std::collections::BTreeSet<String> = Default::default();
        let mut have_lens: std::collections::BTreeSet<String> = Default::default();
        let mut paths: Vec<PathBuf> = Vec::new();
        for it in &self.items {
            if it.exif_loaded {
                if let Some(e) = &it.exif {
                    if let Some(c) = e.camera.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                        have_cam.insert(c.to_string());
                    }
                    if let Some(l) = e.lens.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                        have_lens.insert(l.to_string());
                    }
                }
            } else {
                paths.push(it.entry.display.clone());
            }
        }
        self.cull_meta_cameras = have_cam.iter().cloned().collect();
        self.cull_meta_lenses = have_lens.iter().cloned().collect();
        if paths.is_empty() {
            return; // 전부 캐시에 있었음 — 워커 불필요.
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        self.cull_meta_rx = Some(rx);
        let gen = self.generation;
        std::thread::spawn(move || {
            let mut cams = have_cam;
            let mut lenses = have_lens;
            for p in paths {
                if let Some(e) = read_exif(&p) {
                    if let Some(c) = e.camera.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                        cams.insert(c.to_string());
                    }
                    if let Some(l) = e.lens.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                        lenses.insert(l.to_string());
                    }
                }
            }
            let _ = tx.send(CullMetaResult {
                generation: gen,
                cameras: cams.into_iter().collect(),
                lenses: lenses.into_iter().collect(),
            });
        });
        ctx.request_repaint_after(Duration::from_millis(120));
    }

    /// 카메라/렌즈 수집 결과 반영(폴더가 바뀐 결과는 버린다).
    pub(super) fn drain_cull_meta(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.cull_meta_rx {
            match rx.try_recv() {
                Ok(res) => {
                    if res.generation == self.generation {
                        self.cull_meta_cameras = res.cameras;
                        self.cull_meta_lenses = res.lenses;
                    }
                    self.cull_meta_rx = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.cull_meta_rx = None,
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(120));
                }
            }
        }
    }

    /// 진행 중인 컬링이 결과를 배정할 분류축(잠긴 축). 그 축의 수동 편집을 막는다(#50).
    pub(super) fn cull_locked_target(&self) -> Option<AiCullTarget> {
        self.ai_cull.as_ref().map(|j| j.target)
    }

    /// 잠긴 축 편집 시도 시 토스트로 안내. true면 호출부가 편집을 건너뛴다.
    pub(super) fn cull_axis_locked(&mut self, axis: AiCullTarget) -> bool {
        if self.cull_locked_target() == Some(axis) {
            let msg = match axis {
                AiCullTarget::Label => tr(self.lang, "AI 컬링 중 — 선택(라벨)이 잠겨 있습니다"),
                AiCullTarget::Stars => tr(self.lang, "AI 컬링 중 — 별점이 잠겨 있습니다"),
                AiCullTarget::Tag => tr(self.lang, "AI 컬링 중 — 색 태그가 잠겨 있습니다"),
            };
            self.toast = Some((msg.into(), Instant::now()));
            return true;
        }
        false
    }

    /// AI 컬링(#50) 모델 파일의 로컬 경로(config_dir()/models/…). 모델은 번들하지 않고
    /// 처음 쓸 때 자동 다운로드한다(sha256 검증).
    pub(super) fn model_path(spec: config::ModelSpec) -> std::path::PathBuf {
        config::config_dir().join("models").join(spec.file)
    }

    /// 모델 파일이 다운로드되어 있는지 확인.
    pub(super) fn model_present(spec: config::ModelSpec) -> bool {
        Self::model_path(spec).exists()
    }

    /// 모델 파일을 백그라운드 스레드로 다운로드(#50). ureq 스트리밍 + sha256 검증.
    #[cfg(feature = "model-download")]
    pub(super) fn start_model_download(&mut self, spec: config::ModelSpec, label: String) {
        let dest = Self::model_path(spec);
        let url = spec.url.to_string();
        let sha = spec.sha256.to_string();
        let expected = spec.bytes;
        let (tx, rx) = crossbeam_channel::unbounded::<ModelDlMsg>();
        let lang = self.lang; // 오류 메시지가 토스트로 사용자에게 노출되므로 번역해 보낸다.
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Some(parent) = dest.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    let _ = tx.send(ModelDlMsg::Done(Err(format!("mkdir: {e}"))));
                    return;
                }
            }
            let resp = match ureq::get(&url).call() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ModelDlMsg::Done(Err(format!("HTTP: {e}"))));
                    return;
                }
            };
            let tmp = dest.with_extension("onnx.tmp");
            let mut file = match std::fs::File::create(&tmp) {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "파일 생성: {}", &[&e.to_string()]))));
                    return;
                }
            };
            let mut reader = resp.into_reader();
            let mut buf = vec![0u8; 1 << 16];
            let mut downloaded = 0u64;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Err(e) = file.write_all(&buf[..n]) {
                            let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "쓰기 오류: {}", &[&e.to_string()]))));
                            return;
                        }
                        downloaded += n as u64;
                        let _ = tx.send(ModelDlMsg::Progress(downloaded, expected));
                    }
                    Err(e) => {
                        let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "읽기 오류: {}", &[&e.to_string()]))));
                        return;
                    }
                }
            }
            drop(file);
            // sha256 검증.
            let actual = {
                use sha2::{Digest, Sha256};
                use std::io::BufReader;
                let f = match std::fs::File::open(&tmp) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "검증 열기: {}", &[&e.to_string()]))));
                        return;
                    }
                };
                let mut hasher = Sha256::new();
                let mut r = BufReader::new(f);
                let mut b = [0u8; 1 << 16];
                loop {
                    match r.read(&mut b) {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&b[..n]),
                        Err(e) => {
                            let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "해시 읽기: {}", &[&e.to_string()]))));
                            return;
                        }
                    }
                }
                format!("{:x}", hasher.finalize())
            };
            if actual != sha {
                let _ = std::fs::remove_file(&tmp);
                let _ = tx.send(ModelDlMsg::Done(Err(trf(
                    lang,
                    "sha256 불일치 — 다시 시도해주세요\n예상: {}\n실제: {}",
                    &[&sha, &actual],
                ))));
                return;
            }
            if let Err(e) = std::fs::rename(&tmp, &dest) {
                let _ = tx.send(ModelDlMsg::Done(Err(trf(lang, "파일 이동: {}", &[&e.to_string()]))));
                return;
            }
            let _ = tx.send(ModelDlMsg::Done(Ok(())));
        });
        self.model_dl = Some(ModelDlJob { rx, label, done: 0, total: expected });
    }

    /// 모델 다운로드 진행 모달(#50).
    #[cfg(feature = "model-download")]
    pub(super) fn ui_model_dl_progress(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut job = self.model_dl.take().unwrap();
        let mut done_result: Option<Result<(), String>> = None;
        loop {
            match job.rx.try_recv() {
                Ok(ModelDlMsg::Progress(d, t)) => { job.done = d; job.total = t; }
                Ok(ModelDlMsg::Done(r)) => { done_result = Some(r); break; }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    done_result = Some(Err(tr(lang, "연결 끊김").into())); break;
                }
            }
        }
        if let Some(result) = done_result {
            match result {
                Ok(()) => {
                    self.toast = Some((
                        tr(lang, "모델 다운로드 완료").into(),
                        std::time::Instant::now(),
                    ));
                    // 다운로드 완료 후 다이얼로그 다시 열기.
                    self.ai_cull_open = true;
                }
                Err(e) => {
                    self.toast = Some((
                        trf(lang, "다운로드 실패: {}", &[&e]),
                        std::time::Instant::now(),
                    ));
                    self.ai_cull_open = true;
                }
            }
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        let frac = if job.total > 0 {
            (job.done as f32 / job.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mb_done = job.done as f32 / 1_000_000.0;
        let mb_total = job.total as f32 / 1_000_000.0;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("model_dl_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });
        egui::Window::new("model_dl_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(440.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "모델 다운로드"), &job.label);
                ui.add(egui::ProgressBar::new(frac).fill(theme::ACCENT).desired_height(10.0));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!("{:.1} / {:.1} MB", mb_done, mb_total))
                        .font(mono(11.0))
                        .color(theme::INK3),
                );
            });
        self.model_dl = Some(job);
    }

    pub(super) fn ui_ai_cull_dialog(&mut self, ctx: &egui::Context) {
        // 카메라/렌즈 리스트박스용 목록을 (필요하면) 백그라운드로 수집 시작 + 클로저용 복제.
        self.ensure_cull_meta_scan(ctx);
        let cull_cameras = self.cull_meta_cameras.clone();
        let cull_lenses = self.cull_meta_lenses.clone();
        let cull_meta_loading = self.cull_meta_rx.is_some();

        let lang = self.lang;
        let mut c = self.cfg.ai_cull.clone();
        // 메뉴에서 제외된 미동작 옵션(#53)은 강제 해제 — 옛 설정의 잔류 true가 any_enabled를
        // 무의미하게 켜거나 보이지 않는 필터로 작동하지 않게 한다(저장 시 함께 꺼짐).
        c.use_eyes_open = false;
        c.use_custom_prompt = false;
        let mut do_start = false;
        let mut do_cancel = false;
        // model-download 기능이 켜진 빌드에서만 다운로드 버튼이 값을 채운다.
        #[cfg_attr(not(feature = "model-download"), allow(unused_mut))]
        let mut do_download: Option<config::ModelSpec> = None;

        // 채점 대상 수(scope에 따라). 미리 계산(클로저 안에서 self 차용 회피).
        let total_items = self.items.len();
        let filtered_count = self.filtered().len();

        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("aicull_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        let tag_col = |t: ColorTag| {
            t.color_rgb()
                .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                .unwrap_or(theme::INK3)
        };

        // 전송 다이얼로그와 동일한 구조로 통일(#53): Area+Frame 카드 → 고정 HEADER →
        // 본문만 스크롤 → 고정 FOOTER(시작/취소). 취소·시작이 항상 보여 스크롤 불필요.
        let card_w = 560.0;
        let card_pos = egui::Pos2::new(screen.center().x - card_w / 2.0, (screen.center().y - 320.0).max(8.0));
        let inner_w = card_w - 44.0;
        egui::Area::new(egui::Id::new("aicull_card"))
            .order(egui::Order::Foreground)
            .fixed_pos(card_pos)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(Stroke::new(1.0, theme::LINE2))
                    .rounding(10.0)
                    .show(ui, |ui| {
                        ui.set_min_width(card_w);
                        ui.set_max_width(card_w);
                        // ── HEADER (고정) ──
                        egui::Frame::none()
                            .inner_margin(egui::Margin { left: 22.0, right: 22.0, top: 18.0, bottom: 14.0 })
                            .show(ui, |ui| {
                                ui.set_width(inner_w);
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(tr(lang, "AI 컬링")).font(prop(15.0)).color(theme::INK));
                                        ui.label(egui::RichText::new(tr(lang, "사진을 분석해 흐림·노출·기울기로 자동 분류 · 전부 로컬 처리")).font(mono(10.5)).color(theme::INK3));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        let cc = xr.center();
                                        let col = if xresp.hovered() { theme::INK } else { theme::INK3 };
                                        ui.painter().line_segment([cc + Vec2::new(-4.0, -4.0), cc + Vec2::new(4.0, 4.0)], Stroke::new(1.5, col));
                                        ui.painter().line_segment([cc + Vec2::new(4.0, -4.0), cc + Vec2::new(-4.0, 4.0)], Stroke::new(1.5, col));
                                        if xresp.clicked() { do_cancel = true; }
                                        if xresp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                                    });
                                });
                            });
                        hline_full(ui);
                        // ── BODY (본문만 스크롤) ──
                        let body_max = (screen.height() - card_pos.y - 8.0 - 132.0).max(160.0);
                        egui::ScrollArea::vertical()
                            .id_salt("aicull_body_scroll")
                            .max_height(body_max)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                egui::Frame::none()
                                    .inner_margin(egui::Margin::symmetric(22.0, 16.0))
                                    .show(ui, |ui| {
                                        ui.set_width(inner_w);
                // 본문 위젯 높이를 칩(약 28px)에 맞춰 통일 — 콤보·드래그값·슬라이더·버튼이
                // 칩과 같은 높이로 정렬되도록(#53 레이아웃 정리). 행 간격도 일정하게.
                ui.spacing_mut().interact_size.y = 26.0;
                ui.spacing_mut().button_padding = Vec2::new(8.0, 5.0);
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);

                section_label(ui, "CHECKS");
                ui.horizontal_wrapped(|ui| {
                    if check_chip(ui, tr(lang, "초점(선명도)"), None, theme::ACCENT, c.use_focus) {
                        c.use_focus = !c.use_focus;
                    }
                    if check_chip(ui, tr(lang, "노출"), None, theme::ACCENT, c.use_exposure) {
                        c.use_exposure = !c.use_exposure;
                    }
                    if check_chip(ui, tr(lang, "수평 기울기"), None, theme::ACCENT, c.use_tilt) {
                        c.use_tilt = !c.use_tilt;
                    }
                    if check_chip(ui, tr(lang, "미적(구도) AI"), None, theme::ACCENT, c.use_aesthetic) {
                        c.use_aesthetic = !c.use_aesthetic;
                    }
                });
                ui.add_space(16.0);

                // ── OPTIONS ── 켠 검사의 세부값. METADATA와 동일 위계: [이름/토글 | 값].
                if c.use_focus || c.use_exposure || c.use_tilt || c.use_aesthetic {
                    section_label(ui, "OPTIONS");
                }
                let opt_spec = c.model_spec();
                let opt_model_ok = c.use_aesthetic && Self::model_present(opt_spec);
                egui::Grid::new("aicull_options_grid")
                    .num_columns(2)
                    .min_col_width(140.0)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        if c.use_focus {
                            ui.label(egui::RichText::new(tr(lang, "흐림 임계")).font(mono(11.0)).color(theme::INK3));
                            ui.add(egui::Slider::new(&mut c.focus_thresh, 0.2..=0.8).fixed_decimals(2));
                            ui.end_row();
                            if check_chip(ui, tr(lang, "AF 측거점만"), None, theme::ACCENT, c.use_af_focus) {
                                c.use_af_focus = !c.use_af_focus;
                            }
                            ui.label("");
                            ui.end_row();
                        }
                        if c.use_exposure {
                            ui.label(egui::RichText::new(tr(lang, "노출 하한")).font(mono(11.0)).color(theme::INK3));
                            ui.add(egui::Slider::new(&mut c.exposure_min, 0.2..=0.9).fixed_decimals(2));
                            ui.end_row();
                        }
                        if c.use_tilt {
                            ui.label(egui::RichText::new(tr(lang, "허용 기울기")).font(mono(11.0)).color(theme::INK3));
                            ui.add(egui::Slider::new(&mut c.tilt_max_deg, 0.5..=10.0).suffix("°").fixed_decimals(1));
                            ui.end_row();
                        }
                        if c.use_aesthetic {
                            if check_chip(ui, tr(lang, "⚡ GPU 고속 모드"), None, theme::ACCENT, c.use_gpu) {
                                c.use_gpu = !c.use_gpu;
                            }
                            ui.label("");
                            ui.end_row();
                            if !c.use_gpu {
                                ui.label(egui::RichText::new(tr(lang, "백본")).font(mono(11.0)).color(theme::INK3));
                                ui.horizontal(|ui| {
                                    for b in ClipIqaBackbone::ALL {
                                        let sel = c.backbone == b;
                                        let present = Self::model_present(b.spec());
                                        let col = if present { theme::ACCENT } else { theme::INK3 };
                                        if check_chip(ui, b.label(), None, col, sel) {
                                            c.backbone = b;
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                            if opt_model_ok {
                                ui.label(egui::RichText::new(tr(lang, "선택 기준")).font(mono(11.0)).color(theme::INK3));
                                let topn_sel = if c.top_n > 0 { 0 } else { 1 };
                                if let Some(i) = segmented(
                                    ui,
                                    &[
                                        (tr(lang, "상위 N장"), tr(lang, "최고 점수")),
                                        (tr(lang, "임계값"), tr(lang, "P(good) 하한")),
                                    ],
                                    topn_sel,
                                ) {
                                    c.top_n = if i == 0 { c.top_n.max(20) } else { 0 };
                                }
                                ui.end_row();
                                if c.top_n > 0 {
                                    ui.label(egui::RichText::new(tr(lang, "최고 N장")).font(mono(11.0)).color(theme::INK3));
                                    ui.add(egui::DragValue::new(&mut c.top_n).range(1..=9999).suffix(tr(lang, "장")));
                                } else {
                                    ui.label(egui::RichText::new(tr(lang, "P(good) ≥")).font(mono(11.0)).color(theme::INK3));
                                    ui.add(egui::Slider::new(&mut c.aesthetic_min, 0.1..=0.9).fixed_decimals(2));
                                }
                                ui.end_row();
                            }
                        }
                    });
                if c.use_aesthetic {
                    if opt_model_ok {
                        ui.label(egui::RichText::new(tr(lang, "✓ 미적 모델 준비됨")).font(mono(9.5)).color(theme::ACCENT));
                    } else {
                        ui.label(egui::RichText::new(tr(lang, "미적 모델 없음 — 아래 버튼으로 받으세요")).font(mono(10.0)).color(theme::WARN));
                        #[cfg(feature = "model-download")]
                        if ui.add(egui::Button::new(
                            egui::RichText::new(format!("  {} ({:.0} MB)  ", tr(lang, "미적 모델 다운로드"), opt_spec.bytes as f64 / 1e6))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20))
                        ).fill(theme::ACCENT)).clicked() {
                            do_download = Some(opt_spec);
                        }
                    }
                }
                ui.add_space(16.0);

                // ── METADATA FILTER (Tier1, 모델 불필요) ── 2열 Grid로 줄맞춤:
                //   왼쪽=항목 토글/이름, 오른쪽=값/콤보. 값들이 같은 x에서 시작해 정렬된다(#53).
                section_label(ui, "METADATA FILTER");
                let all_label = tr(lang, "(전체)");
                egui::Grid::new("aicull_meta_grid")
                    .num_columns(2)
                    .min_col_width(140.0)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        // 방향(택1).
                        ui.label(egui::RichText::new(tr(lang, "방향")).font(mono(11.0)).color(theme::INK3));
                        ui.horizontal(|ui| {
                            for (lbl, o) in [
                                (tr(lang, "전체"), config::OrientationFilter::Any),
                                (tr(lang, "세로"), config::OrientationFilter::Portrait),
                                (tr(lang, "가로"), config::OrientationFilter::Landscape),
                            ] {
                                if check_chip(ui, lbl, None, theme::ACCENT, c.filter_orientation == o) {
                                    c.filter_orientation = o;
                                }
                            }
                        });
                        ui.end_row();
                        // ISO 상한.
                        if check_chip(ui, tr(lang, "ISO 상한"), None, theme::ACCENT, c.use_iso_max) {
                            c.use_iso_max = !c.use_iso_max;
                        }
                        if c.use_iso_max {
                            ui.add(egui::DragValue::new(&mut c.iso_max).range(50..=409600).speed(50));
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        // 초점거리(범위).
                        if check_chip(ui, tr(lang, "초점거리"), None, theme::ACCENT, c.use_focal_range) {
                            c.use_focal_range = !c.use_focal_range;
                        }
                        if c.use_focal_range {
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut c.focal_min_mm).range(0.0..=2000.0).suffix("mm"));
                                ui.label(egui::RichText::new("~").color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.focal_max_mm).range(0.0..=2000.0).suffix("mm"));
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        // 조리개 상한.
                        if check_chip(ui, tr(lang, "조리개 ≤"), None, theme::ACCENT, c.use_aperture_max) {
                            c.use_aperture_max = !c.use_aperture_max;
                        }
                        if c.use_aperture_max {
                            ui.add(egui::DragValue::new(&mut c.aperture_max).range(0.7..=32.0).speed(0.1).prefix("f/"));
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        // 셔터 하한.
                        if check_chip(ui, tr(lang, "셔터 하한"), None, theme::ACCENT, c.use_shutter_min) {
                            c.use_shutter_min = !c.use_shutter_min;
                        }
                        if c.use_shutter_min {
                            ui.horizontal(|ui| {
                                let mut denom = (1.0 / c.shutter_min_secs.max(1e-6)).round() as u32;
                                if ui.add(egui::DragValue::new(&mut denom).range(1..=8000).prefix("1/")).changed() {
                                    c.shutter_min_secs = 1.0 / denom.max(1) as f32;
                                }
                                ui.label(egui::RichText::new(tr(lang, "초 (손떨림)")).font(mono(9.0)).color(theme::INK4));
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        // 카메라/렌즈는 현재 폴더에 실재하는 값에서 고른다(#51). "(전체)"=필터 없음.
                        for (id, label, opts, val) in [
                            ("cull_camera_sel", tr(lang, "카메라"), &cull_cameras, &mut c.camera_contains),
                            ("cull_lens_sel", tr(lang, "렌즈"), &cull_lenses, &mut c.lens_contains),
                        ] {
                            ui.label(egui::RichText::new(label).font(mono(11.0)).color(theme::INK3));
                            let shown = if val.trim().is_empty() { all_label } else { val.as_str() };
                            egui::ComboBox::from_id_salt(id)
                                .selected_text(egui::RichText::new(shown).font(mono(11.0)))
                                .width(300.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(val, String::new(), all_label);
                                    for opt in opts {
                                        ui.selectable_value(val, opt.clone(), opt.as_str());
                                    }
                                });
                            ui.end_row();
                        }
                    });
                if cull_meta_loading {
                    ui.label(egui::RichText::new(tr(lang, "메타 수집 중… (잠시 후 목록이 채워집니다)")).font(mono(9.0)).color(theme::INK4));
                }
                ui.add_space(16.0);

                // ── DEDUP / BEST-OF (Tier2+3a) ── METADATA와 동일 위계: [토글 | 값(오른쪽)].
                section_label(ui, "DEDUP / BEST-OF");
                egui::Grid::new("aicull_dedup_grid")
                    .num_columns(2)
                    .min_col_width(140.0)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        if check_chip(ui, tr(lang, "연사 베스트-N"), None, theme::ACCENT, c.use_burst) {
                            c.use_burst = !c.use_burst;
                        }
                        if c.use_burst {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(tr(lang, "간격 ≤")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.burst_gap_secs).range(1..=30).suffix(tr(lang, "초")));
                                ui.add_space(10.0);
                                ui.label(egui::RichText::new(tr(lang, "그룹당")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.burst_keep).range(1..=20).suffix(tr(lang, "장")));
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        if check_chip(ui, tr(lang, "시각적 중복 묶기"), None, theme::ACCENT, c.use_dedup) {
                            c.use_dedup = !c.use_dedup;
                        }
                        if c.use_dedup {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(tr(lang, "해밍 ≤")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.dedup_hamming).range(2..=16));
                                ui.add_space(10.0);
                                ui.label(egui::RichText::new(tr(lang, "클러스터당")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.dedup_keep).range(1..=20).suffix(tr(lang, "장")));
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    });
                if c.use_burst || c.use_dedup {
                    ui.label(egui::RichText::new(tr(lang, "그룹 내 베스트는 점수(미적>선명도) 기준 선택")).font(mono(9.0)).color(theme::INK4));
                }
                ui.add_space(16.0);

                // ── AI AXES ── 장르=YuNet, AI 선명도=CLIP sharp. [토글 | 값] 위계 통일.
                // (커스텀 프롬프트는 텍스트 인코더 필요로 메뉴 제외 — #53. config 필드는 유지.)
                section_label(ui, "AI AXES");
                egui::Grid::new("aicull_aiaxes_grid")
                    .num_columns(2)
                    .min_col_width(140.0)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        if check_chip(ui, tr(lang, "장르 픽"), None, theme::ACCENT, c.use_genre) {
                            c.use_genre = !c.use_genre;
                        }
                        if c.use_genre {
                            let gsel = if c.genre_portrait { 0 } else { 1 };
                            if let Some(i) = segmented(
                                ui,
                                &[
                                    (tr(lang, "인물"), tr(lang, "얼굴 있음")),
                                    (tr(lang, "풍경"), tr(lang, "얼굴 없음")),
                                ],
                                gsel,
                            ) {
                                c.genre_portrait = i == 0;
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        if check_chip(ui, tr(lang, "AI 선명도"), None, theme::ACCENT, c.use_sharp_ai) {
                            c.use_sharp_ai = !c.use_sharp_ai;
                        }
                        if c.use_sharp_ai {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(tr(lang, "P(sharp) ≥")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::Slider::new(&mut c.sharp_min, 0.1..=0.9).fixed_decimals(2));
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    });
                if c.use_genre {
                    ui.label(egui::RichText::new(tr(lang, "장르 픽: 얼굴 검출(YuNet) 기반")).font(mono(9.0)).color(theme::INK4));
                }
                if c.use_sharp_ai {
                    if !Self::model_present(config::CLIP_AXES_MODEL) {
                        ui.label(egui::RichText::new(tr(lang, "CLIP 다축 모델(89MB) 없음 — 아래 버튼으로 받으세요")).font(mono(10.0)).color(theme::WARN));
                        #[cfg(feature = "model-download")]
                        if ui.add(egui::Button::new(
                            egui::RichText::new(format!("  {} (CLIP 다축 89MB)  ", tr(lang, "AI 선명도 모델 다운로드")))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20))
                        ).fill(theme::ACCENT)).clicked() {
                            do_download = Some(config::CLIP_AXES_MODEL);
                        }
                    } else {
                        ui.label(egui::RichText::new(tr(lang, "✓ CLIP 다축 모델 준비됨")).font(mono(10.0)).color(theme::ACCENT));
                    }
                }
                ui.add_space(16.0);

                // ── DETECT ── 얼굴(YuNet)·객체(YOLOv10n) 실동작.
                // (눈 뜬 컷은 눈감음 분류기가 필요해 메뉴에서 제외 — #53. config 필드는 유지.)
                section_label(ui, "DETECT");
                egui::Grid::new("aicull_detect_grid")
                    .num_columns(2)
                    .min_col_width(140.0)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        if check_chip(ui, tr(lang, "얼굴 있는 컷만"), None, theme::ACCENT, c.use_face) {
                            c.use_face = !c.use_face;
                        }
                        ui.label("");
                        ui.end_row();
                        if check_chip(ui, tr(lang, "객체 포함"), None, theme::ACCENT, c.use_object) {
                            c.use_object = !c.use_object;
                        }
                        if c.use_object {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut c.object_class).desired_width(120.0).hint_text(tr(lang, "person")));
                                // 클래스 해석 결과 안내.
                                match rawblow_core::object_detect::coco_index(&c.object_class) {
                                    Some(idx) => {
                                        ui.label(egui::RichText::new(trf(lang, "→ {}", &[rawblow_core::object_detect::COCO_NAMES[idx as usize]])).font(mono(9.5)).color(theme::INK4));
                                    }
                                    None => {
                                        ui.label(egui::RichText::new(tr(lang, "COCO 클래스명")).font(mono(9.0)).color(theme::WARN));
                                    }
                                }
                            });
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    });
                if c.use_object && !Self::model_present(config::OBJECT_MODEL) {
                    ui.label(egui::RichText::new(tr(lang, "객체 모델(YOLOv10n, 9MB) 없음 — 아래 버튼으로 받으세요")).font(mono(10.0)).color(theme::WARN));
                    #[cfg(feature = "model-download")]
                    if ui.add(egui::Button::new(
                        egui::RichText::new(format!("  {} (YOLOv10n 9MB)  ", tr(lang, "객체 모델 다운로드")))
                            .color(Color32::from_rgb(0x0a, 0x14, 0x20))
                    ).fill(theme::ACCENT)).clicked() {
                        do_download = Some(config::OBJECT_MODEL);
                    }
                } else if c.use_object {
                    ui.label(egui::RichText::new(tr(lang, "✓ 객체 모델 준비됨 (YOLOv10n)")).font(mono(10.0)).color(theme::ACCENT));
                }
                // 얼굴 모델 상태(장르·얼굴 공용). 없으면 다운로드 버튼.
                if (c.use_face || c.use_genre) && !Self::model_present(config::FACE_MODEL) {
                    ui.label(egui::RichText::new(tr(lang, "얼굴 모델(YuNet, 0.2MB) 없음 — 아래 버튼으로 받으세요")).font(mono(10.0)).color(theme::WARN));
                    #[cfg(feature = "model-download")]
                    if ui.add(egui::Button::new(
                        egui::RichText::new(format!("  {} (YuNet 0.2MB)  ", tr(lang, "얼굴 모델 다운로드")))
                            .color(Color32::from_rgb(0x0a, 0x14, 0x20))
                    ).fill(theme::ACCENT)).clicked() {
                        do_download = Some(config::FACE_MODEL);
                    }
                } else if c.use_face || c.use_genre {
                    ui.label(egui::RichText::new(tr(lang, "✓ 얼굴 모델 준비됨 (YuNet)")).font(mono(10.0)).color(theme::ACCENT));
                }
                ui.add_space(16.0);

                if !c.any_enabled() {
                    ui.label(egui::RichText::new(tr(lang, "검사 항목을 하나 이상 켜세요.")).font(mono(10.0)).color(theme::WARN));
                }
                ui.add_space(16.0);

                section_label(ui, "ASSIGN RESULT TO");
                let tgt_sel = match c.target {
                    AiCullTarget::Label => 0,
                    AiCullTarget::Stars => 1,
                    AiCullTarget::Tag => 2,
                };
                if let Some(i) = segmented(
                    ui,
                    &[
                        (tr(lang, "선택"), tr(lang, "Pick / Reject")),
                        (tr(lang, "별점"), tr(lang, "높음 / 낮음")),
                        (tr(lang, "색 태그"), tr(lang, "두 색")),
                    ],
                    tgt_sel,
                ) {
                    c.target = match i {
                        0 => AiCullTarget::Label,
                        1 => AiCullTarget::Stars,
                        _ => AiCullTarget::Tag,
                    };
                }
                ui.add_space(8.0);
                match c.target {
                    AiCullTarget::Label => {
                        ui.label(egui::RichText::new(tr(lang, "좋음 → 선택(Pick) · 탈락 → 제외(Reject)")).font(mono(10.5)).color(theme::INK4));
                    }
                    AiCullTarget::Stars => {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "좋음")).font(mono(11.0)).color(theme::INK3));
                            ui.add(egui::Slider::new(&mut c.good_stars, 0..=5).suffix("★"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "탈락")).font(mono(11.0)).color(theme::INK3));
                            ui.add(egui::Slider::new(&mut c.bad_stars, 0..=5).suffix("★"));
                        });
                    }
                    AiCullTarget::Tag => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "좋음")).font(mono(11.0)).color(theme::INK3));
                            for t in ColorTag::ALL {
                                if check_chip(ui, " ", None, tag_col(t), c.good_tag == t) {
                                    c.good_tag = t;
                                }
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "탈락")).font(mono(11.0)).color(theme::INK3));
                            for t in ColorTag::ALL {
                                if check_chip(ui, " ", None, tag_col(t), c.bad_tag == t) {
                                    c.bad_tag = t;
                                }
                            }
                        });
                    }
                }
                ui.add_space(16.0);

                section_label(ui, "SCOPE");
                let scope_sel = if c.scope_all { 0 } else { 1 };
                let all_lbl = trf(lang, "{} 항목", &[&total_items.to_string()]);
                let filt_lbl = trf(lang, "{} 항목", &[&filtered_count.to_string()]);
                if let Some(i) = segmented(
                    ui,
                    &[
                        (tr(lang, "전체"), all_lbl.as_str()),
                        (tr(lang, "현재 필터"), filt_lbl.as_str()),
                    ],
                    scope_sel,
                ) {
                    c.scope_all = i == 0;
                }

                                    }); // body inner Frame
                            }); // body ScrollArea
                        hline_full(ui);
                        // ── FOOTER (고정) ── 전송 다이얼로그와 동일: 통계 + 시작/취소가 항상 보임.
                        let count = if c.scope_all { total_items } else { filtered_count };
                        // 모드별 대략 처리량(디코드 병목이라 미적 GPU/CPU 차이는 작다).
                        let rate = if c.use_aesthetic {
                            if c.use_gpu { "~100장/초 내외 (GPU)" } else { "~90장/초 내외 (CPU)" }
                        } else {
                            "~200장/초+ (모델 불필요·가장 빠름)"
                        };
                        egui::Frame::none()
                            .fill(theme::BG1)
                            .rounding(egui::Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 })
                            .inner_margin(egui::Margin::symmetric(22.0, 14.0))
                            .show(ui, |ui| {
                                ui.set_width(inner_w);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("WILL ANALYZE").font(prop(10.0)).color(theme::INK3));
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new(count.to_string()).font(mono(13.0)).color(theme::ACCENT));
                                    ui.label(egui::RichText::new(tr(lang, "장")).font(mono(10.0)).color(theme::INK3));
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new(tr(lang, rate)).font(mono(9.5)).color(theme::INK4));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let aesthetic_blocks = c.use_aesthetic && !Self::model_present(c.model_spec());
                                        let face_blocks = (c.use_face || c.use_genre) && !Self::model_present(config::FACE_MODEL);
                                        let sharp_blocks = c.use_sharp_ai && !Self::model_present(config::CLIP_AXES_MODEL);
                                        let object_blocks = c.use_object
                                            && (!Self::model_present(config::OBJECT_MODEL)
                                                || rawblow_core::object_detect::coco_index(&c.object_class).is_none());
                                        let can_start = c.any_enabled() && count > 0 && !aesthetic_blocks && !face_blocks && !sharp_blocks && !object_blocks;
                                        if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "컬링 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                                            do_start = true;
                                        }
                                        ui.add_space(8.0);
                                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                                            do_cancel = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Esc");
                                    });
                                });
                            });
                    }); // card Frame
            }); // card Area

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            do_cancel = true;
        }
        // 편집한 설정은 매 프레임 cfg에 반영(영속화는 시작/취소 시).
        self.cfg.ai_cull = c;
        if do_cancel {
            let _ = config::save(&self.cfg);
            self.ai_cull_open = false;
        } else if do_start {
            let _ = config::save(&self.cfg);
            self.ai_cull_open = false;
            self.start_ai_cull();
        } else if let Some(spec) = do_download {
            let _ = config::save(&self.cfg);
            self.ai_cull_open = false;
            #[cfg(feature = "model-download")]
            {
                let label = if spec.file == config::FACE_MODEL.file {
                    tr(lang, "YuNet (얼굴)").to_string()
                } else if spec.file == config::CLIP_AXES_MODEL.file {
                    tr(lang, "CLIP 다축 (AI 선명도)").to_string()
                } else if spec.file == config::OBJECT_MODEL.file {
                    tr(lang, "YOLOv10n (객체)").to_string()
                } else if self.cfg.ai_cull.use_gpu {
                    tr(lang, "RN50 (GPU 고속)").to_string()
                } else {
                    self.cfg.ai_cull.backbone.label().to_string()
                };
                self.start_model_download(spec, label);
            }
            #[cfg(not(feature = "model-download"))]
            let _ = spec;
        }
    }

    /// 모델 복사본이 차지할 메모리(~2GB 예산)와 논리 코어 수로 워커 풀 크기를 정한다.
    /// 각 워커는 자기 ONNX 세션(intra=1)을 들고 디코딩+채점을 병렬 수행한다.
    pub(super) fn cull_worker_count(model_bytes: Option<u64>) -> usize {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let cap = match model_bytes {
            Some(sz) if sz > 0 => (2_000_000_000u64 / sz).max(1) as usize,
            _ => usize::MAX, // CV 전용(모델 없음)은 메모리 부담 작음 → 코어 수만 제한.
        };
        cores.min(cap).clamp(1, 12)
    }

    /// AI 컬링 채점을 백그라운드 **워커 풀**에서 시작(#50). 각 항목을 작은 해상도로 디코딩해
    /// 노출·초점·기울기를 채점하고(AF 옵션 시 합초 측거점 영역), `ai`+모델이 있으면 CLIP-IQA
    /// 미적 점수도 낸다. 워커마다 자기 세션(intra=1)을 써서 코어를 꽉 채우는 게 최고 처리량
    /// (M1 Max 실측: ViT-B/32 55→190 img/s, ViT-L/14 5.6→12.5 img/s). 진행률은 원자 카운터로.
    pub(super) fn start_ai_cull(&mut self) {
        let cfg = self.cfg.ai_cull.clone();
        let mut criteria = cfg.criteria();
        let use_af = cfg.use_af_focus && cfg.use_focus;
        // top_n은 미적 채점(ai 게이트) 분기에서만 쓰인다 — ai off 빌드에선 바인딩 자체를 제거.
        #[cfg(feature = "ai")]
        let top_n = cfg.top_n;
        // 디코드 해상도를 가장 디테일이 필요한 켜진 신호에 맞춘다(디코드가 실파이프라인 병목):
        //   초점 ON → 1024(블러 판별 디테일) / 기울기 ON → 512(엣지 방향) / 둘 다 OFF → 256
        //   (노출은 해상도 무관, 미적은 224만 필요). 작을수록 디코드 대폭 빨라짐.
        let cull_edge: u32 = if cfg.use_focus {
            AI_CULL_EDGE
        } else if cfg.use_tilt {
            512
        } else {
            256
        };
        let targets: Vec<(usize, PathBuf)> = if cfg.scope_all {
            self.items.iter().enumerate().map(|(i, it)| (i, it.entry.display.clone())).collect()
        } else {
            self.filtered().into_iter().map(|r| (r, self.items[r].entry.display.clone())).collect()
        };
        let total = targets.len();

        // 미적 모델: GPU 모드면 RN50 fp32 + CoreML/WebGPU, 아니면 선택 백본 int8 + CPU.
        let spec = cfg.model_spec();
        #[cfg(feature = "ai")]
        let model_path: Option<PathBuf> =
            if cfg.use_aesthetic && Self::model_present(spec) { Some(Self::model_path(spec)) } else { None };
        #[cfg(not(feature = "ai"))]
        let model_path: Option<PathBuf> = None;

        // 모델 파일이 없으면 미적 판정을 꺼 둔다(오판 방지).
        if model_path.is_none() {
            criteria.use_aesthetic = false;
        }

        // 얼굴 검출(YuNet): "얼굴 있는 컷"·"장르 인물/풍경"에 필요. 모델 없으면 비활성(오판 방지).
        #[cfg(feature = "ai")]
        let face_model_path: Option<PathBuf> = if (cfg.use_face || cfg.use_genre)
            && Self::model_present(config::FACE_MODEL)
        {
            Some(Self::model_path(config::FACE_MODEL))
        } else {
            None
        };
        #[cfg(not(feature = "ai"))]
        let face_model_path: Option<PathBuf> = None;
        let need_face = face_model_path.is_some();

        // CLIP 다축(AI 선명도): sharp 축 사용. 모델 없으면 비활성.
        #[cfg(feature = "ai")]
        let axes_model_path: Option<PathBuf> = if cfg.use_sharp_ai
            && Self::model_present(config::CLIP_AXES_MODEL)
        {
            Some(Self::model_path(config::CLIP_AXES_MODEL))
        } else {
            None
        };
        #[cfg(not(feature = "ai"))]
        let axes_model_path: Option<PathBuf> = None;
        let need_sharp = axes_model_path.is_some();

        // 객체 검출(YOLO): "객체 포함". 클래스 텍스트를 COCO 인덱스로 해석, 모델·인덱스 둘 다 있어야.
        #[cfg(feature = "ai")]
        let object_class_idx: Option<u8> =
            if cfg.use_object { rawblow_core::object_detect::coco_index(&cfg.object_class) } else { None };
        #[cfg(not(feature = "ai"))]
        let object_class_idx: Option<u8> = None;
        #[cfg(feature = "ai")]
        let object_model_path: Option<PathBuf> = if cfg.use_object
            && object_class_idx.is_some()
            && Self::model_present(config::OBJECT_MODEL)
        {
            Some(Self::model_path(config::OBJECT_MODEL))
        } else {
            None
        };
        #[cfg(not(feature = "ai"))]
        let object_model_path: Option<PathBuf> = None;
        let need_object = object_model_path.is_some();
        // GPU 모드: CoreML/WebGPU EP, intra 무관. 워커는 동시 세션으로 GPU 처리량을 채운다(메모리 ≈ N×모델).
        // CPU 모드: intra=1 세션을 코어 수만큼.
        let use_gpu = cfg.use_gpu && model_path.is_some();
        #[cfg(feature = "ai")]
        let accel = if use_gpu { rawblow_core::quality::Accel::Gpu } else { rawblow_core::quality::Accel::Cpu };
        #[cfg(feature = "ai")]
        let intra = if use_gpu { None } else { Some(1usize) };
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let model_bytes = model_path.as_ref().and_then(|p| std::fs::metadata(p).ok().map(|m| m.len()));
        // GPU는 한 워커가 chunk를 모아 배치 추론, CPU는 1장씩.
        // **WebGPU(Windows)는 동시 세션이 크래시**('command encoder already encoding')하므로 워커들이
        // 세션 하나를 공유(Mutex가 추론을 직렬화, 디코드+CV는 병렬). CoreML(mac)은 동시 세션 OK라 워커별.
        let share_session = (use_gpu && cfg!(target_os = "windows")) || std::env::var("RB_SHARE").is_ok();
        let batch_size = if use_gpu { if share_session { 32 } else { 8 } } else { 1usize };
        let n_workers = if use_gpu {
            if share_session {
                // WebGPU 공유 세션: 워커는 디코드+CV 병렬화용(추론은 1세션에 직렬). 코어만큼
                // (세션은 1개라 메모리 부담 없음).
                cores.min(8).min(total.max(1))
            } else {
                // CoreML 동시 세션(실측 8워커×배치8 ≈ 421 img/s). 워커마다 모델 사본을 들므로
                // cull_worker_count로 메모리(~2GB 예산)를 존중하고, GPU 경합 방지로 8 상한·배치 수 상한.
                let batches = total.div_ceil(batch_size);
                Self::cull_worker_count(model_bytes).min(8).min(batches.max(1))
            }
        } else {
            Self::cull_worker_count(model_bytes).min(total.max(1))
        };

        // 공유 세션 모드: 모델을 한 번만 로드해 모든 워커가 Arc로 공유(WebGPU 동시성 크래시 방지).
        #[cfg(feature = "ai")]
        let shared_model: Option<std::sync::Arc<rawblow_core::quality::AestheticModel>> = if share_session {
            model_path
                .as_ref()
                .and_then(|p| rawblow_core::quality::AestheticModel::load_tuned(p, accel, intra).ok())
                .map(std::sync::Arc::new)
        } else {
            None
        };

        let (tx, rx) = crossbeam_channel::bounded::<AiCullMsg>(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let next = Arc::new(AtomicUsize::new(0));
        let results = std::sync::Arc::new(std::sync::Mutex::new(
            Vec::<(usize, Verdict, Option<f32>)>::with_capacity(total),
        ));
        // 그룹 컬링(메타·연사·중복) 부가정보. 필요할 때만 EXIF/dHash를 수집(없으면 비용 0).
        let extras = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::<usize, CullExtra>::with_capacity(total),
        ));
        let need_dhash = cfg.use_dedup;
        let need_meta = cfg.use_burst
            || cfg.filter_orientation != config::OrientationFilter::Any
            || cfg.use_iso_max
            || cfg.use_focal_range
            || cfg.use_aperture_max
            || cfg.use_shutter_min
            || !cfg.camera_contains.trim().is_empty()
            || !cfg.lens_contains.trim().is_empty();
        let targets = std::sync::Arc::new(targets);

        // 캐시 서명: 검사 신호 + AF + 디코드 해상도 + 모델 식별자(임계값은 제외 → 임계만 바꿔
        // 재실행 시 전부 적중). 모델 미사용이면 "none".
        let model_id: String = if cfg.use_aesthetic { spec.file.to_string() } else { "none".into() };
        let sig: u64 = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (criteria.use_focus, criteria.use_exposure, criteria.use_tilt, use_af, cull_edge).hash(&mut h);
            model_id.hash(&mut h);
            // 얼굴·sharp·객체 검사 여부는 보고서를 바꾸므로 캐시 네임스페이스를 분리한다.
            need_face.hash(&mut h);
            need_sharp.hash(&mut h);
            (need_object, object_class_idx).hash(&mut h);
            h.finish()
        };
        let cache = self.cull_cache.clone();
        let cache_hits = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let targets = targets.clone();
            let next = next.clone();
            let progress = progress.clone();
            let cancel_w = cancel.clone();
            let results = results.clone();
            let extras = extras.clone();
            let cache = cache.clone();
            let cache_hits = cache_hits.clone();
            #[cfg(feature = "ai")]
            let model_path = model_path.clone();
            #[cfg(feature = "ai")]
            let shared_model = shared_model.clone();
            #[cfg(feature = "ai")]
            let face_model_path = face_model_path.clone();
            #[cfg(feature = "ai")]
            let axes_model_path = axes_model_path.clone();
            #[cfg(feature = "ai")]
            let object_model_path = object_model_path.clone();
            handles.push(std::thread::spawn(move || {
                // 공유 세션 모드면 Arc 클론, 아니면 워커 전용 세션 로드(CPU=intra1, CoreML=동시).
                // 로드 실패 시 CV-only 폴백.
                #[cfg(feature = "ai")]
                let model: Option<std::sync::Arc<rawblow_core::quality::AestheticModel>> = if share_session {
                    shared_model
                } else {
                    model_path
                        .as_ref()
                        .and_then(|p| rawblow_core::quality::AestheticModel::load_tuned(p, accel, intra).ok())
                        .map(std::sync::Arc::new)
                };
                // 얼굴 모델은 가벼워 워커별 CPU 로드(YuNet ~232KB). 로드 실패면 얼굴 검사 생략.
                #[cfg(feature = "ai")]
                let face_model: Option<std::sync::Arc<rawblow_core::face_detect::FaceModel>> = face_model_path
                    .as_ref()
                    .and_then(|p| rawblow_core::face_detect::FaceModel::load(p).ok())
                    .map(std::sync::Arc::new);
                // CLIP 다축 모델(AI 선명도). 워커별 CPU 로드. 실패면 sharp 검사 생략.
                #[cfg(feature = "ai")]
                let axes_model: Option<std::sync::Arc<rawblow_core::axes::AxesModel>> = axes_model_path
                    .as_ref()
                    .and_then(|p| rawblow_core::axes::AxesModel::load(p).ok())
                    .map(std::sync::Arc::new);
                // YOLO 객체 모델(객체 포함). 워커별 CPU 로드. 실패면 객체 검사 생략.
                #[cfg(feature = "ai")]
                let object_model: Option<std::sync::Arc<rawblow_core::object_detect::ObjectModel>> = object_model_path
                    .as_ref()
                    .and_then(|p| rawblow_core::object_detect::ObjectModel::load(p).ok())
                    .map(std::sync::Arc::new);
                loop {
                    if cancel_w.load(Ordering::Relaxed) {
                        break;
                    }
                    let start = next.fetch_add(batch_size, Ordering::Relaxed);
                    if start >= targets.len() {
                        break;
                    }
                    let end = (start + batch_size).min(targets.len());
                    // CV 판정용(미적 제외) — 캐시 적중 시 보고서로부터 판정 재도출.
                    let mut cv_only = criteria;
                    cv_only.use_aesthetic = false;
                    // 1단계: 각 장을 캐시 조회. 적중(파일·설정 동일)이면 디코드+추론 생략하고 즉시 적재.
                    // 미스만 디코드+CV해 두고(미적 추론은 2단계 배치), 캐시 키(경로·mtime)도 보관.
                    let mut imgs: Vec<rawblow_core::decode::DecodedImage> = Vec::with_capacity(end - start);
                    let mut metas: Vec<(usize, rawblow_core::quality::QualityReport, Verdict)> =
                        Vec::with_capacity(end - start);
                    let mut miss_keys: Vec<(PathBuf, Option<std::time::SystemTime>)> = Vec::with_capacity(end - start);
                    for idx in start..end {
                        // 배치(최대 32)가 클 수 있으니 장마다 취소를 확인해 즉시 멈춘다.
                        if cancel_w.load(Ordering::Relaxed) {
                            break;
                        }
                        let (real, path) = &targets[idx];
                        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                        // 캐시 적중: 같은 파일(mtime)·같은 설정(sig)이면 보고서 재사용(디코드/추론 생략).
                        let cached = mtime.and_then(|mt| {
                            cache.lock().ok().and_then(|c| {
                                c.get(path)
                                    .filter(|e| e.mtime == mt && e.sig == sig && (!need_dhash || e.dhash.is_some()))
                                    .map(|e| (e.report, e.dhash))
                            })
                        });
                        if let Some((report, dh)) = cached {
                            let cv = cv_only.verdict(&report);
                            if let Ok(mut g) = results.lock() {
                                g.push((*real, cv, report.aesthetic));
                            }
                            if need_meta || need_dhash || need_face || need_sharp || need_object {
                                let ex = cull_extra(&report, dh, path, need_meta);
                                if let Ok(mut m) = extras.lock() {
                                    m.insert(*real, ex);
                                }
                            }
                            cache_hits.fetch_add(1, Ordering::Relaxed);
                            progress.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        // 미스: 디코드+CV(패닉 격리). 미적 추론은 2단계에서.
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            cull_decode_cv(path, criteria, use_af, cull_edge)
                        }))
                        .ok()
                        .flatten();
                        if let Some((img, q, cv)) = r {
                            // ai 빌드에서만 얼굴/객체 검사 결과를 q에 써 넣는다(off 빌드에선 불변).
                            #[cfg(feature = "ai")]
                            let mut q = q;
                            // 얼굴 검사(YuNet): 디코드된 이미지에서 존재만 판정해 보고서에 기록(캐시됨).
                            #[cfg(feature = "ai")]
                            if let Some(fm) = face_model.as_ref() {
                                q.face = Some(fm.has_face(&img));
                            }
                            // AI 선명도(CLIP sharp 축): 보고서에 기록(캐시됨).
                            #[cfg(feature = "ai")]
                            if let Some(am) = axes_model.as_ref() {
                                q.sharp_ai = am.scores(&img).map(|s| s[rawblow_core::axes::AXIS_SHARP]);
                            }
                            // 객체 포함(YOLO): 설정 클래스 포함 여부를 보고서에 기록(캐시됨).
                            #[cfg(feature = "ai")]
                            if let (Some(om), Some(idx)) = (object_model.as_ref(), object_class_idx) {
                                q.object_match = Some(om.contains(&img, idx));
                            }
                            imgs.push(img);
                            metas.push((*real, q, cv));
                            miss_keys.push((path.clone(), mtime));
                        }
                        progress.fetch_add(1, Ordering::Relaxed);
                    }
                    // 2단계: 미적 추론. GPU=배치, CPU=1장씩(threshold면 CV 통과자만 — CLIP-skip).
                    #[cfg(feature = "ai")]
                    if criteria.use_aesthetic {
                        if let Some(m) = model.as_ref() {
                            if batch_size > 1 {
                                let refs: Vec<&rawblow_core::decode::DecodedImage> = imgs.iter().collect();
                                let scores = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    m.score_batch(&refs)
                                }))
                                .ok()
                                .and_then(|r| r.ok());
                                if let Some(scores) = scores {
                                    for (meta, s) in metas.iter_mut().zip(scores) {
                                        meta.1.aesthetic = Some(s);
                                    }
                                }
                            } else {
                                for (i, meta) in metas.iter_mut().enumerate() {
                                    if top_n > 0 || matches!(meta.2, Verdict::Good) {
                                        meta.1.aesthetic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            m.score(&imgs[i]).ok()
                                        }))
                                        .ok()
                                        .flatten();
                                    }
                                }
                            }
                        }
                    }
                    // 3단계: 미스 결과 적재(CV 판정 + 미적 점수). 최종 조합은 apply_cull_verdicts가 한다.
                    // dHash(시각중복 필요 시) — 디코드된 이미지에서. imgs[k] ↔ metas[k] ↔ miss_keys[k].
                    let dhashes: Vec<Option<u64>> = if need_dhash {
                        imgs.iter().map(|im| Some(rawblow_core::cull_ext::dhash(im))).collect()
                    } else {
                        vec![None; metas.len()]
                    };
                    if let Ok(mut g) = results.lock() {
                        for (real, q, cv) in &metas {
                            g.push((*real, *cv, q.aesthetic));
                        }
                    }
                    if need_meta || need_dhash || need_face || need_sharp || need_object {
                        if let Ok(mut m) = extras.lock() {
                            for (k, (real, q, _)) in metas.iter().enumerate() {
                                let path = &miss_keys[k].0;
                                m.insert(*real, cull_extra(q, dhashes.get(k).copied().flatten(), path, need_meta));
                            }
                        }
                    }
                    // 4단계: 미스 보고서를 캐시에 저장(다음 재컬링에서 디코드/추론 생략).
                    if let Ok(mut c) = cache.lock() {
                        for (k, ((_, q, _), (path, mtime))) in metas.iter().zip(miss_keys.iter()).enumerate() {
                            if let Some(mt) = mtime {
                                c.insert(
                                    path.clone(),
                                    CullCacheEntry { mtime: *mt, sig, report: *q, dhash: dhashes.get(k).copied().flatten() },
                                );
                            }
                        }
                    }
                }
            }));
        }

        // 코디네이터: 워커 전원 합류 후 결과를 한 번에 전송.
        std::thread::spawn(move || {
            for h in handles {
                let _ = h.join();
            }
            let out = match std::sync::Arc::try_unwrap(results) {
                Ok(m) => m.into_inner().unwrap_or_default(),
                Err(arc) => arc.lock().map(|g| g.clone()).unwrap_or_default(),
            };
            let ex = match std::sync::Arc::try_unwrap(extras) {
                Ok(m) => m.into_inner().unwrap_or_default(),
                Err(arc) => arc.lock().map(|g| g.clone()).unwrap_or_default(),
            };
            let _ = tx.send(AiCullMsg::Done(out, ex));
        });

        self.ai_cull = Some(AiCullJob {
            rx,
            cancel,
            progress,
            cache_hits,
            total,
            generation: self.generation,
            target: cfg.target,
        });
    }

    /// 진행 중인 컬링의 진행률을 펌프하고(매 프레임, 비차단) 완료 시 결과를 적용한다(#50).
    /// 폴더가 바뀌었으면(generation 불일치) 인덱스가 무효이므로 결과를 폐기한다.
    pub(super) fn pump_ai_cull(&mut self, ctx: &egui::Context) {
        let Some(job) = self.ai_cull.take() else { return };

        // (판정 목록, real→부가정보) — AiCullMsg::Done 페이로드와 동일 형태.
        type CullDone = (Vec<(usize, Verdict, Option<f32>)>, std::collections::HashMap<usize, CullExtra>);
        let mut done_results: Option<CullDone> = None;
        let mut disconnected = false;
        match job.rx.try_recv() {
            Ok(AiCullMsg::Done(v, ex)) => done_results = Some((v, ex)),
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => disconnected = true,
        }

        if let Some((v, ex)) = done_results {
            self.ai_cull = None;
            self.ai_cull_cancel_confirm = false;
            // 폴더가 바뀌면 캡처한 real 인덱스가 다른 사진을 가리킨다 → 결과 폐기(오염 방지).
            if job.generation == self.generation {
                let hits = job.cache_hits.load(Ordering::Relaxed);
                self.apply_cull_verdicts(v, ex, hits);
                // 갱신된 캐시를 디스크에 저장(다음 세션 재컬링 즉시화). 메인 스레드 I/O 히치를 피해
                // 스냅샷만 잠금 안에서 뜨고(빠른 memcpy) 직렬화·쓰기는 백그라운드에서.
                let cache = self.cull_cache.clone();
                std::thread::spawn(move || {
                    let snapshot = cache.lock().map(|c| c.clone()).unwrap_or_default();
                    save_cull_cache(&snapshot);
                });
            } else {
                self.toast = Some((
                    tr(self.lang, "폴더가 바뀌어 AI 컬링 결과를 버렸습니다").into(),
                    Instant::now(),
                ));
            }
            return;
        }
        if disconnected {
            self.ai_cull = None;
            self.ai_cull_cancel_confirm = false;
            return;
        }

        // 진행 중에는 빠른 리페인트로 진행률을 갱신(레일 버튼의 프로그레스바).
        ctx.request_repaint_after(Duration::from_millis(120));
        self.ai_cull = Some(job);
    }

    /// 진행 중 컬링을 취소한다(부분 결과는 적용하지 않음).
    pub(super) fn cancel_ai_cull(&mut self) {
        if let Some(job) = self.ai_cull.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.ai_cull_cancel_confirm = false;
    }

    /// 컬링 버튼(프로그레스바) 재클릭 시 뜨는 취소 확인 모달(#50).
    pub(super) fn ui_ai_cull_cancel_confirm(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // 확인 도중 컬링이 끝나면 취소할 대상이 없으므로 닫는다.
        if self.ai_cull.is_none() {
            self.ai_cull_cancel_confirm = false;
            return;
        }
        let (done, total) = self.ai_cull.as_ref().map(|j| (j.progress.load(Ordering::Relaxed), j.total)).unwrap_or((0, 0));
        let mut keep = false; // 계속 진행(닫기)
        let mut stop = false; // 컬링 취소

        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("aicull_cancel_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("aicull_cancel_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(420.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "AI 컬링 진행 중"), tr(lang, "분석을 멈추시겠어요?"));
                let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.add(egui::ProgressBar::new(frac).fill(theme::ACCENT).desired_height(10.0));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(trf(lang, "{} / {} 장", &[&done.to_string(), &total.to_string()]))
                        .font(mono(11.0)).color(theme::INK3),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(format!("  {}  ", tr(lang, "컬링 취소"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))
                    ).fill(theme::REJECT)).clicked() {
                        stop = true;
                    }
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "계속 진행"), false).clicked() {
                        keep = true;
                    }
                });
            });

        if stop {
            self.cancel_ai_cull();
            self.toast = Some((tr(lang, "AI 컬링을 취소했습니다").into(), Instant::now()));
        } else if keep || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ai_cull_cancel_confirm = false;
        }
    }

    /// 컬링 중 폴더 전환을 시도하면 뜨는 확인 모달(#50). 예 → 컬링 취소 후 그 폴더를 연다.
    pub(super) fn ui_ai_cull_folder_confirm(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // 확인 도중 컬링이 끝났으면 더 막을 이유가 없으니 바로 연다.
        if self.ai_cull.is_none() {
            if let Some(folder) = self.ai_cull_folder_confirm.take() {
                self.open_folder(folder);
            }
            return;
        }
        let mut go = false;     // 폴더 전환(컬링 취소)
        let mut cancel = false; // 폴더 전환 취소(컬링 유지)

        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("aicull_folder_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("aicull_folder_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(440.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(
                    ui,
                    tr(lang, "컬링이 진행 중입니다"),
                    tr(lang, "폴더를 바꾸면 진행 중인 AI 컬링이 취소됩니다."),
                );
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(format!("  {}  ", tr(lang, "폴더 바꾸기"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))
                    ).fill(theme::ACCENT)).clicked() {
                        go = true;
                    }
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "계속 컬링"), false).clicked() {
                        cancel = true;
                    }
                });
            });

        if go {
            self.cancel_ai_cull();
            if let Some(folder) = self.ai_cull_folder_confirm.take() {
                self.open_folder(folder);
            }
        } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ai_cull_folder_confirm = None;
        }
    }

    /// AI 컬링 판정을 선택한 분류축에 적용(#50). "양쪽 다 표시" — 좋음/탈락 모두 표시.
    /// `cache_hits`: 캐시에서 재사용해 디코드/추론을 건너뛴 장 수(토스트 표시).
    pub(super) fn apply_cull_verdicts(
        &mut self,
        mut results: Vec<(usize, Verdict, Option<f32>)>,
        extras: std::collections::HashMap<usize, CullExtra>,
        cache_hits: usize,
    ) {
        let lang = self.lang;
        let c = self.cfg.ai_cull.clone();

        // 미적 설정에 따른 최종 Good/Bad 조합(top-N 랭킹 또는 임계). 코어의 테스트된 함수에 위임.
        rawblow_core::quality::finalize_cull_verdicts(&mut results, c.use_aesthetic, c.top_n, c.aesthetic_min);

        // 점수 범위 수집(진단용 토스트).
        let scores: Vec<f32> = results.iter().filter_map(|(_, _, a)| *a).collect();
        let score_info = if scores.is_empty() {
            String::new()
        } else {
            let min = scores.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            format!(" (P(good) {:.2}–{:.2})", min, max)
        };

        // 그룹 컬링(메타 제외 → 연사 베스트 → 시각중복). 활성 시에만. None=제외(손대지 않음).
        let meta_active = c.filter_orientation != config::OrientationFilter::Any
            || c.use_iso_max
            || c.use_focal_range
            || c.use_aperture_max
            || c.use_shutter_min
            || !c.camera_contains.trim().is_empty()
            || !c.lens_contains.trim().is_empty();
        let group_active = meta_active || c.use_burst || c.use_dedup;
        let group_verdicts: Vec<Option<bool>> = if group_active {
            use rawblow_core::cull_ext::{CullItem, GroupCullParams};
            let items: Vec<CullItem> = results
                .iter()
                .map(|(real, v, a)| {
                    let ex = extras.get(real);
                    CullItem {
                        good: matches!(v, Verdict::Good),
                        rank: a.unwrap_or_else(|| ex.map(|e| e.sharp).unwrap_or(0.0)),
                        dhash: ex.and_then(|e| e.dhash),
                        shot_time: ex.and_then(|e| e.shot_time),
                        meta: ex.map(|e| e.meta.clone()).unwrap_or_default(),
                    }
                })
                .collect();
            let p = GroupCullParams {
                use_meta: meta_active,
                meta_filter: c.meta_filter(),
                use_burst: c.use_burst,
                burst_gap_secs: c.burst_gap_secs as i64,
                burst_keep: c.burst_keep as usize,
                use_dedup: c.use_dedup,
                dedup_hamming: c.dedup_hamming,
                dedup_keep: c.dedup_keep as usize,
            };
            rawblow_core::cull_ext::apply_group_culling(&items, &p)
        } else {
            Vec::new()
        };

        let (mut good, mut bad, mut skipped) = (0usize, 0usize, 0usize);
        // 얼굴/장르 하드필터(YuNet): 조건 불충족이면 탈락으로 강등(미적·CV 점수와 무관, #51 후속).
        //   얼굴 있는 컷만 → 얼굴 없으면 탈락 / 장르 인물 → 얼굴 없으면 탈락 / 장르 풍경 → 얼굴 있으면 탈락.
        //   face=None(모델 없음·미검출)은 안전하게 제외하지 않는다.
        let face_excluded = |real: &usize| -> bool {
            let ex = extras.get(real);
            // 얼굴/장르(YuNet).
            let face_bad = if c.use_face || c.use_genre {
                match ex.and_then(|e| e.face) {
                    Some(has) => {
                        (c.use_face && !has)
                            || (c.use_genre && c.genre_portrait && !has)
                            || (c.use_genre && !c.genre_portrait && has)
                    }
                    None => false,
                }
            } else {
                false
            };
            // AI 선명도(CLIP sharp 축): P(sharp) < sharp_min이면 탈락.
            let sharp_bad = c.use_sharp_ai
                && ex.and_then(|e| e.sharp_ai).map(|s| s < c.sharp_min).unwrap_or(false);
            // 객체 포함(YOLO): 설정 클래스가 없으면 탈락.
            let object_bad = c.use_object
                && ex.and_then(|e| e.object_match).map(|has| !has).unwrap_or(false);
            face_bad || sharp_bad || object_bad
        };

        for (i, (real, v, _)) in results.iter().enumerate() {
            // 그룹 컬링 결과가 본 판정을 덮어쓴다(None=제외, 손대지 않음).
            let is_good = if group_active {
                match group_verdicts.get(i).copied().flatten() {
                    Some(g) => g,
                    None => {
                        skipped += 1;
                        continue;
                    }
                }
            } else {
                matches!(v, Verdict::Good)
            };
            // 얼굴/장르 하드필터로 강등.
            let is_good = is_good && !face_excluded(real);
            let Some(it) = self.items.get_mut(*real) else { continue };
            if is_good { good += 1; } else { bad += 1; }
            match c.target {
                AiCullTarget::Label => {
                    it.entry.label = if is_good { Label::Pick } else { Label::Reject };
                }
                AiCullTarget::Stars => {
                    it.entry.stars = if is_good { c.good_stars.min(5) } else { c.bad_stars.min(5) };
                }
                AiCullTarget::Tag => {
                    it.entry.tag = if is_good { c.good_tag } else { c.bad_tag };
                }
            }
        }
        self.sidecar_dirty = true;
        let cache_info = if cache_hits > 0 { trf(lang, " · 캐시 {}장", &[&cache_hits.to_string()]) } else { String::new() };
        let skip_info = if skipped > 0 { trf(lang, " · 제외 {}장", &[&skipped.to_string()]) } else { String::new() };
        self.toast = Some((
            format!(
                "{}{}{}{}",
                trf(lang, "AI 컬링 완료 · 좋음 {} · 탈락 {}", &[&good.to_string(), &bad.to_string()]),
                score_info,
                skip_info,
                cache_info
            ),
            Instant::now(),
        ));
    }
}

/// 컬링 1장: 디코딩 + 켜진 CV 신호 채점(+AF 영역 초점) + CV 판정(미적 제외)(#50).
/// 디코드 이미지를 함께 돌려줘 호출부가 단장/배치 미적 추론에 재사용한다. 손상·실패 시 None.
pub(super) fn cull_decode_cv(
    path: &std::path::Path,
    criteria: rawblow_core::quality::CullCriteria,
    use_af: bool,
    max_edge: u32,
) -> Option<(
    rawblow_core::decode::DecodedImage,
    rawblow_core::quality::QualityReport,
    Verdict,
)> {
    let img = rawblow_core::decode::decode_file(
        path,
        rawblow_core::decode::DecodeOptions { full_raw: false, max_edge: Some(max_edge) },
    )
    .ok()?;
    // 켜진 신호만 계산. AF 영역 초점을 쓸 땐 전체 프레임 초점은 건너뛴다(중복 제거).
    let want_whole_focus = criteria.use_focus && !use_af;
    let mut q = rawblow_core::quality::analyze_selective(
        &img,
        criteria.use_exposure,
        want_whole_focus,
        criteria.use_tilt,
    );
    if use_af {
        if let Some(af) = rawblow_core::af::parse_af(path) {
            let orient = rawblow_core::meta::orientation(path);
            let regions: Vec<(f32, f32, f32, f32)> = af
                .points
                .iter()
                .filter(|p| p.in_focus)
                .map(|p| {
                    let (cx, cy, w, h) = af_display_coords(p, orient);
                    (cx as f32, cy as f32, w as f32, h as f32)
                })
                .collect();
            if !regions.is_empty() {
                q.focus = rawblow_core::quality::focus_report_regions(&img, &regions);
            }
        }
    }
    let mut cv_only = criteria;
    cv_only.use_aesthetic = false;
    let cv = cv_only.verdict(&q);
    Some((img, q, cv))
}

#[cfg(test)]
mod tests {
    use super::{load_cull_cache_from, save_cull_cache_to, CullCacheEntry};

    #[test]
    fn cull_cache_disk_round_trips() {
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::time::{Duration, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!("rb_cull_cache_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cull-cache.json");

        let mut report = rawblow_core::quality::QualityReport {
            exposure: rawblow_core::quality::ExposureReport { mean: 0.5, highlight_clip: 0.0, shadow_clip: 0.0, dynamic_range: 0.8, score: 0.9 },
            focus: rawblow_core::quality::FocusReport { sharpness: 0.7, acutance: 12.0, in_focus: true },
            tilt: rawblow_core::quality::TiltReport { degrees: 1.5, confidence: 0.6 },
            aesthetic: Some(0.42),
            face: None,
            sharp_ai: None,
            object_match: None,
        };
        let mtime = UNIX_EPOCH + Duration::from_nanos(1_700_000_000_123_456_789);
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("/photos/IMG_0001.CR2"),
            CullCacheEntry { mtime, sig: 0xABCD, report, dhash: Some(0x0123_4567_89AB_CDEF) },
        );

        save_cull_cache_to(&file, &map);
        let loaded = load_cull_cache_from(&file);
        let e = loaded.get(&PathBuf::from("/photos/IMG_0001.CR2")).expect("entry survives round-trip");
        assert_eq!(e.sig, 0xABCD);
        assert_eq!(e.mtime, mtime, "mtime 나노초 왕복 보존");
        assert_eq!(e.report.aesthetic, Some(0.42));
        assert_eq!(e.report.focus.sharpness, 0.7);
        assert_eq!(e.dhash, Some(0x0123_4567_89AB_CDEF), "dhash 왕복 보존");

        // 손상/부재 파일 → 빈 맵(안전 폴백).
        report.aesthetic = None; // (사용 안 함, 경고 회피)
        let _ = report;
        let missing = load_cull_cache_from(&dir.join("nope.json"));
        assert!(missing.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
