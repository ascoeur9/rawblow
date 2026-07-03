//! RawBlow GUI 본체. 핸드오프 디자인(Studio 기본 / Cinema 풀스크린)을 egui로 구현.

use crate::i18n::{tr, trf};
use crate::theme;
use crate::widgets::{self, draw_thumb, hud_text, kbd, mono, prop, section_head, ThumbInfo, TexCache};
use crate::worker::{DecodeRequest, Worker};
use eframe::egui;
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use rawblow_core::cache;
use rawblow_core::config::{self, AiCullTarget, ClipIqaBackbone, Config, Lang};
use rawblow_core::quality::Verdict;
use rawblow_core::meta::{read_exif, ExifInfo};
use rawblow_core::organize::{self, OrganizeKey, OrganizeRequest};
use rawblow_core::transfer::{
    self, Action, Companions, ConflictPolicy, Numbering, Progress, RenameRule, TransferReport,
    TransferRequest,
};
use rawblow_core::{
    scan, sidecar, ColorTag, Entry, Filter, Label, MatchMode, SortOrder, StarFilter, TagFilter,
    ViewMode,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod ui_helpers;
use ui_helpers::*;
mod update_check;
mod dialogs;
mod settings_ui;
mod transfer_ui;
mod decode_pipe;
mod culling;
use culling::*;
use transfer_ui::*;

const TOOLBAR_H: f32 = 40.0;
const RAIL_W: f32 = 188.0;
const FILMSTRIP_H: f32 = 92.0;
const STATUS_H: f32 = 24.0;

/// 단일/전체화면 프리뷰 최대 변(px). RW2 등은 내장 프리뷰가 네이티브 1920px이라,
/// 1920을 요청하면 기존 1600 대비 (1) 더 높은 해상도(1600→1920)이면서 (2) 다운스케일
/// 리샘플이 0이 돼 오히려 더 빠르다(샘플 RW2 실측: 1600=108ms vs 1920=29ms, I/O 동일 0.95MB).
/// 2560 이상은 8144px 풀해상도 임베디드를 읽어야 해 I/O가 ~10배(0.95→9.3MB)·시간이 ~5배로
/// 늘어, 매 넘김마다 디코딩하는 컬링 기본값으로는 느린 드라이브에서 손해다(#preview-audit).
/// 더 선명한 확인이 필요하면 ORIG(D)로 8192px 풀해상도를 본다(요청 시 디코더가 풀해상도
/// 임베디드를 DCT 축소하는 경로는 그대로 유지 — 회귀 테스트로 검증).
const PREVIEW_EDGE: u32 = 1920;
/// ORIG(원본 보기) 최대 변(px). GPU 텍스처 한계(보통 8192) 안에서 원본 디테일 확보.
const ORIG_EDGE: u32 = 8192;
/// 그리드·필름스트립 썸네일 최대 변(px). 작게 → 빠른 디코딩·작은 메모리.
const THUMB_EDGE: u32 = 320;
/// AI 컬링 채점용 디코딩 최대 변(px)(#50). 흐림 판별에 충분한 디테일과 속도(장당 수십 ms)의 절충.
const AI_CULL_EDGE: u32 = 1024;
/// 프리뷰 선디코딩 윈도우는 사용자 설정 `cfg.preload`(전방)를 따른다(request_preload).
/// 프리뷰 텍스처 캐시 용량(윈도우+여유 — 현재 장이 절대 eviction되지 않게).
const PREVIEW_CAP: usize = 24;
/// 썸네일 텍스처 캐시 용량. 보이는 셀 + 최근 스크롤 이력을 넉넉히 담되 VRAM은 묶는다
/// (320px 썸네일 ≈ 0.27MB → 1500장 ≈ 0.4GB). 폴더가 더 커도 GPU 메모리는 일정.
/// 넘어가면 LRU eviction → 다시 스크롤해 오면 주문형으로 빠르게 재디코딩.
const THUMB_CAP: usize = 1500;
/// 한 프레임에 GPU로 올리는 썸네일 텍스처 상한. 빠른 스크롤로 결과가 한꺼번에 쏟아져도
/// 업로드를 분산해 GPU 버스트로 메인 스레드가 멈추는(행) 것을 막는다.
const THUMB_UPLOADS_PER_FRAME: usize = 32;

/// 한 항목의 GUI 상태(코어 Entry + 지연 로드된 EXIF).
struct Item {
    entry: Entry,
    exif: Option<ExifInfo>,
    exif_loaded: bool,
    /// AF 포인트(#37). 오버레이가 켜진 항목에서만 지연 파싱(prefix 2MB 재독).
    af: Option<rawblow_core::af::AfInfo>,
    af_loaded: bool,
    /// EXIF Orientation(1..8). AF 좌표(센서 기준)를 표시 좌표로 돌릴 때 필요.
    orient: Option<u16>,
}



pub struct RawBlowApp {
    cfg: Config,
    folder: Option<PathBuf>,
    items: Vec<Item>,
    index: usize, // 필터된 목록 기준 위치
    view: ViewMode,
    fullscreen: bool,
    /// OS 창 풀스크린이 `fullscreen`에 반영된 상태(#31). 변할 때만 ViewportCommand 전송.
    fs_applied: bool,
    /// 시작 창 크기(1440pt)가 모니터보다 클 때 1회 축소했는지. eframe의 시작 클램프는
    /// 모니터 열거가 비정상인 환경(일부 원격 세션)에서 무력해, 창이 화면 밖으로 넘쳐
    /// 우측 앵커 UI(GPS 지도 패널 등)가 안 보이게 된다 — 런타임 monitor_size로 보강.
    size_clamped: bool,
    filter: Filter,
    star_filter: StarFilter, // 별점 필터(라벨 필터와 독립 AND).
    tag_filter: TagFilter,   // 컬러 태그 필터(라벨·별점 필터와 독립 AND)(#27).
    show_exif: bool,
    show_hist: bool,
    full_raw: bool, // ORIG(원본 보기): 풀 RAW/최대 임베디드를 원본 크기로 디코딩
    // 단일/전체화면 줌·이동 상태.
    fit: bool,          // true = 창에 맞춤(zoom 자동), false = 명시적 배율
    zoom: f32,          // 절대 배율(화면픽셀/이미지픽셀). 1.0 = 1:1
    pan: Vec2,          // 중앙 기준 이동(화면 px)
    zoom_for: Option<usize>, // 줌 상태가 적용된 항목(real). 바뀌면 fit으로 리셋
    last_view_size: Option<Vec2>, // #48: 마지막으로 표시한 텍스처 크기(px). 같은 항목에서 해상도가 바뀌면(ORIG 토글) 화면상 배율을 유지.
    af_zoom_pending: bool,   // #49: 다음 1:1 확대를 AF 측거점 중심에 맞추라는 요청.
    grid_cols: usize,
    sort: SortOrder,

    // 그리드 다중 선택(Ctrl/Shift+클릭) — 항목(real) 인덱스 집합 + 범위 선택 앵커(필터 인덱스).
    selected: std::collections::HashSet<usize>,
    sel_anchor: Option<usize>,
    // 그리드 키보드 내비 시 선택 셀이 보이도록 스크롤할 목표 행(다음 프레임에 적용).
    grid_scroll_to: Option<usize>,
    // 마지막 프레임에 그리드에 보였던 행 범위(스크롤 필요 여부 판단용).
    grid_visible_rows: std::ops::Range<usize>,

    worker: Worker,
    cache: TexCache,  // 단일/전체화면 프리뷰(큰 해상도)
    thumbs: TexCache, // 그리드/필름스트립/열화 폴백(작은 해상도)
    pending_preview: std::collections::HashSet<usize>,
    pending_thumb: std::collections::HashSet<usize>,
    pending_thumb_prio: std::collections::HashSet<usize>, // 우선 레인으로 승격된 썸네일
    pending_prefetch: std::collections::HashSet<usize>,   // 백그라운드 디스크 캐시 프리페치 중
    failed_preview: std::collections::HashSet<usize>,
    failed_thumb: std::collections::HashSet<usize>,
    histo: std::collections::HashMap<usize, Histo>,
    generation: u64,

    sidecar_dirty: bool,
    last_save: Instant,

    transfer: Option<TransferDialogState>,
    organize: Option<OrganizeDialogState>,
    // 진행 중인 백그라운드 파일 작업(전송/정리)의 프로그레스바 상태(#35).
    progress: Option<ProgressJob>,
    result: Option<TransferReport>,
    show_settings: bool,
    // AI 컬링(#50): 설정 다이얼로그 표시 여부 + 진행 중인 채점 작업.
    ai_cull_open: bool,
    ai_cull: Option<AiCullJob>,
    // 진행 중 컬링 버튼(프로그레스바) 재클릭 시 뜨는 취소 확인 모달.
    ai_cull_cancel_confirm: bool,
    // 컬링 중 폴더를 바꾸려 할 때 뜨는 확인 모달(예 누르면 컬링 취소 후 이 폴더를 연다).
    ai_cull_folder_confirm: Option<PathBuf>,
    // 컬링 결과 캐시(#50): (파일 경로 → mtime+설정서명+QualityReport). 재컬링 시 파일이 안 바뀌고
    // 검사 신호·모델이 같으면 디코드/추론을 건너뛰어 즉시 재판정(임계값만 바꿔 재실행할 때 큰 이득).
    // 워커가 공유하므로 Arc<Mutex<>>. 폴더 전환 시 비운다.
    cull_cache: Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, CullCacheEntry>>>,
    // CLIP-IQA 모델 다운로드 진행.
    #[cfg(feature = "model-download")]
    model_dl: Option<ModelDlJob>,
    // 오픈소스 라이센스 페이지(#39). 설정에서 열며, Some이면 설정 대신 이 페이지를 그린다.
    licenses: Option<crate::licenses::LicensesPage>,
    last_dest: Option<PathBuf>,
    // 설정 화면에 표시할 썸네일 캐시 사용량(#22). 설정을 열 때 한 번 계산해 캐싱.
    cache_size: Option<u64>,
    // 마지막 캐시 trim 시각. 세션 중에도 주기적으로(시간 기반) 상한을 정리한다.
    last_trim: Instant,

    jump_open: bool,
    jump_text: String,
    /// 점프 모드(#52): true=순번(현재/전체의 순번, 기본), false=파일명 일부 매칭.
    jump_by_number: bool,

    // 일괄 분류 변경 모달 (#3) — 그리드에서 파일명으로 다수 항목을 한 번에 라벨링.
    bulk_open: bool,
    bulk_text: String,
    bulk_exact: bool,
    bulk_target: Label,
    bulk_hits: Vec<usize>,
    bulk_searched: bool,

    toast: Option<(String, Instant)>,
    // 성능 표시
    last_frame: Instant,
    frame_ms: f32,

    // UI 표시 언어(#30). cfg.lang(저장값) 또는 OS 감지값으로 시작 시 결정, 설정에서 변경.
    lang: Lang,

    // 설정의 사진 배경색 HEX 입력 버퍼(#36). 설정을 열 때 현재 색으로 동기화.
    bg_hex: String,

    // 새 릴리즈 안내(#33): 실행 후 유휴 시 1회만 백그라운드로 확인.
    update_checked: bool,                                            // 확인을 이미 시작했는지
    update_rx: Option<crossbeam_channel::Receiver<Option<String>>>, // 결과 채널(Some(ver)=새 버전)
    update_available: Option<String>,                               // 새 버전 표시 문자열(있으면 배너)

    // GPS 미니 지도(#38) / AF 포인트 오버레이(#37). cfg 저장값으로 시작, M/A로 토글.
    show_map: bool,
    show_af: bool,
    map_state: Option<MapState>, // 현재 항목·줌의 지도 패널 상태(항목 바뀌면 교체).
    map_zoom: u8,                // 지도 줌(패널 ±버튼). 세션 한정.

    // 메타데이터(EXIF·AF·orientation) 백그라운드 로더. 예전엔 현재 항목의 EXIF/AF를
    // UI 스레드에서 동기로 읽어(NAS면 프리픽스 read가 수백 ms) 사진 넘김이 걸렸다 —
    // 표시 먼저, 메타는 도착하는 대로. 동시 1건만(현재 보는 항목 우선), 폴더가 바뀌면
    // generation 불일치로 결과 폐기.
    meta_rx: crossbeam_channel::Receiver<MetaResult>,
    meta_tx: crossbeam_channel::Sender<MetaResult>,
    meta_inflight: bool,
    // AI 컬링 카메라/렌즈 리스트박스용 distinct 목록(#51 후속). EXIF prefix read를 UI 스레드에서
    // 돌리면 NAS에서 멈추므로, 다이얼로그가 처음 열릴 때 백그라운드로 한 번 수집해 캐시한다.
    // `cull_meta_gen`이 현재 generation과 같으면 이미 수집(또는 진행 중) — 폴더가 바뀌면 재수집.
    cull_meta_cameras: Vec<String>,
    cull_meta_lenses: Vec<String>,
    cull_meta_gen: Option<u64>,
    cull_meta_rx: Option<crossbeam_channel::Receiver<CullMetaResult>>,
}


/// 백그라운드 메타 로더 결과. 요청 시점에 없던 조각(exif/af)만 채워 보낸다.
struct MetaResult {
    generation: u64,
    real: usize,
    exif: Option<Option<ExifInfo>>, // 바깥 Option=이번에 읽었는지, 안쪽=파일에 있었는지
    af: Option<(Option<rawblow_core::af::AfInfo>, u16)>, // (AF, orientation)
}

/// GPS 미니 지도(#38) 패널 상태. (항목, 줌)당 하나 — 바뀌면 새로 만든다.
struct MapState {
    real: usize,
    zoom: u8,
    lat: f64,
    lon: f64,
    rx: Option<crossbeam_channel::Receiver<Option<crate::map::MapImage>>>,
    tex: Option<egui::TextureHandle>,
    failed: bool,
}

impl RawBlowApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let cfg = config::load();
        // 워커 스레드 수: 코어 수에 비례하되 상한으로 CPU 피크를 억제. 디코딩이 DCT 축소로
        // 가벼워 과한 스레드는 불필요하지만, 멀티코어에선 더 많은 전경 스레드가 빠른 스크롤
        // 썸네일 처리량을 높인다. env RB_THREADS로 실험 override 가능.
        let threads = std::env::var("RB_THREADS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| (n.get() / 2).clamp(2, 8))
                    .unwrap_or(4)
            });
        let worker = Worker::new(threads, config::cache_dir());
        // UI 언어(#30): 저장값(cfg.lang) 있으면 그걸, 없으면 OS 언어 감지.
        let lang = crate::i18n::effective_lang(&cfg);
        // 폰트는 활성 언어의 폰트를 primary로 설치(#32 후속: 일본어 글자 세로 어긋남 방지).
        crate::fonts::install(&cc.egui_ctx, lang);

        let (meta_tx, meta_rx) = crossbeam_channel::unbounded();

        let mut app = RawBlowApp {
            lang,
            view: ViewMode::Single,
            fullscreen: false,
            fs_applied: false,
            size_clamped: false,
            filter: Filter::All,
            star_filter: StarFilter::Any,
            tag_filter: TagFilter::Any,
            show_exif: cfg.show_exif,
            show_hist: cfg.show_histogram,
            full_raw: false,
            fit: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            zoom_for: None,
            last_view_size: None,
            af_zoom_pending: false,
            grid_cols: cfg.grid_cols.clamp(4, 12),
            sort: SortOrder::Name,
            selected: std::collections::HashSet::new(),
            sel_anchor: None,
            grid_scroll_to: None,
            grid_visible_rows: 0..0,
            folder: None,
            items: Vec::new(),
            index: 0,
            worker,
            cache: TexCache::new(PREVIEW_CAP, 32),
            thumbs: TexCache::new(THUMB_CAP, 256),
            pending_preview: std::collections::HashSet::new(),
            pending_thumb: std::collections::HashSet::new(),
            pending_thumb_prio: std::collections::HashSet::new(),
            pending_prefetch: std::collections::HashSet::new(),
            failed_preview: std::collections::HashSet::new(),
            failed_thumb: std::collections::HashSet::new(),
            histo: std::collections::HashMap::new(),
            generation: 0,
            sidecar_dirty: false,
            last_save: Instant::now(),
            transfer: None,
            organize: None,
            ai_cull_open: false,
            ai_cull: None,
            ai_cull_cancel_confirm: false,
            ai_cull_folder_confirm: None,
            cull_cache: Arc::new(std::sync::Mutex::new(load_cull_cache())),
            #[cfg(feature = "model-download")]
            model_dl: None,
            progress: None,
            result: None,
            show_settings: false,
            licenses: None,
            last_dest: None,
            cache_size: None,
            last_trim: Instant::now(),
            jump_open: false,
            jump_text: String::new(),
            jump_by_number: true,
            bulk_open: false,
            bulk_text: String::new(),
            bulk_exact: false,
            bulk_target: Label::Pick,
            bulk_hits: Vec::new(),
            bulk_searched: false,
            toast: None,
            last_frame: Instant::now(),
            frame_ms: 0.0,
            bg_hex: String::new(),
            update_checked: false,
            update_rx: None,
            update_available: None,
            show_map: cfg.show_map,
            show_af: cfg.show_af,
            map_state: None,
            map_zoom: 13, // 동네 수준 컨텍스트. 패널 ±로 3~17 조절.
            meta_rx,
            meta_tx,
            meta_inflight: false,
            cull_meta_cameras: Vec::new(),
            cull_meta_lenses: Vec::new(),
            cull_meta_gen: None,
            cull_meta_rx: None,
            cfg,
        };

        // 마지막 폴더 자동 복원.
        if let Some(last) = app.cfg.last_folder.clone() {
            let p = PathBuf::from(last);
            if p.is_dir() {
                app.open_folder(p);
            }
        }
        // 시작 시 한 번 캐시 상한 정리(폴더를 안 열어도 지난 세션 누적분을 회수).
        app.schedule_cache_trim();
        app
    }

    fn open_folder(&mut self, folder: PathBuf) {
        // 폴더를 바꾸기 전, 디바운스 대기 중인 분류/별점 변경을 현재 폴더 사이드카에 먼저 확정한다.
        // (Move 후 재스캔(#24)이나 폴더 전환 시 미저장 변경이 옛 사이드카로 롤백·유실되지 않게.)
        if self.sidecar_dirty {
            if let Some(cur) = &self.folder {
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                let _ = sidecar::save(cur, &entries);
            }
            self.sidecar_dirty = false;
        }
        // 컬링 캐시는 절대 경로+mtime+설정 서명으로 키잉되므로 폴더가 바뀌어도 안전(다른 파일은
        // 절대 적중 안 함). 비우지 않고 유지 → 이전에 컬링한 폴더로 돌아와 재컬링해도 즉시(세션·재시작 무관).
        let entries = scan::scan_folder(&folder, self.cfg.recursive, self.sort);
        let mut items: Vec<Item> = entries
            .into_iter()
            .map(|entry| Item {
                entry,
                exif: None,
                exif_loaded: false,
                af: None,
                af_loaded: false,
                orient: None,
            })
            .collect();

        // 사이드카 복원.
        if let Some(session) = sidecar::load(&folder) {
            let mut tmp: Vec<Entry> = items.iter().map(|i| i.entry.clone()).collect();
            sidecar::apply(&session, &mut tmp);
            for (it, e) in items.iter_mut().zip(tmp) {
                it.entry.label = e.label;
                it.entry.stars = e.stars;
                it.entry.tag = e.tag;
            }
        }

        self.items = items;
        self.index = 0;
        self.generation += 1;
        // 워커가 이전 폴더의 프리페치 큐(수천 건)를 헛디코딩하지 않도록 세대를 먼저 올린다.
        self.worker.set_generation(self.generation);
        // 캐시를 통째로 새로 만들지 **않는다**: 그러면 옛 핸들이 즉시 드롭→GPU 텍스처가
        // 파괴되어, 직전 프레임을 제출(submit) 중인 wgpu가 이를 참조하면 크래시한다.
        // 대신 retire_all로 활성 맵만 비우고 핸들은 TTL 동안 살려 in-flight 참조를 보호한다.
        self.cache.retire_all();
        self.thumbs.retire_all();
        self.pending_preview.clear();
        self.pending_thumb.clear();
        self.pending_thumb_prio.clear();
        self.pending_prefetch.clear();
        self.failed_preview.clear();
        self.failed_thumb.clear();
        self.histo.clear();
        self.selected.clear();
        self.sel_anchor = None;
        self.grid_scroll_to = None;
        self.grid_visible_rows = 0..0;
        self.cfg.push_recent(&folder.to_string_lossy());
        let _ = config::save(&self.cfg);
        self.folder = Some(folder);
        self.toast = Some((trf(self.lang, "{} 항목 로드", &[&self.items.len().to_string()]), Instant::now()));
        self.schedule_cache_trim(); // 폴더 열 때 캐시 상한 정리(다른/오래된 폴더 썸네일 회수).
        // 프리페치는 폴더 전체가 아니라 현재 위치 주변 윈도우만(update에서 매 프레임 슬라이드).
    }


    /// 현재 필터를 통과하는 항목 인덱스(원본 items 기준).
    fn filtered(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                self.filter.accepts(it.entry.label)
                    && self.star_filter.accepts(it.entry.stars)
                    && self.tag_filter.accepts(it.entry.tag)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0); // pick, hold, reject, unrated
        for it in &self.items {
            match it.entry.label {
                Label::Pick => c.0 += 1,
                Label::Hold => c.1 += 1,
                Label::Reject => c.2 += 1,
                Label::Unrated => c.3 += 1,
            }
        }
        c
    }

    /// 현재 항목의 원본 인덱스.
    fn current_real(&self) -> Option<usize> {
        let f = self.filtered();
        f.get(self.index.min(f.len().saturating_sub(1))).copied()
    }





    fn set_label(&mut self, label: Label) {
        if self.cull_axis_locked(AiCullTarget::Label) {
            return;
        }
        // 그리드에서 다중 선택 중이면 선택한 항목 전부에 일괄 적용(토글·자동진행 없음).
        if self.view == ViewMode::Grid && !self.selected.is_empty() {
            let targets: Vec<usize> = self.selected.iter().copied().collect();
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.label = label;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            if let Some(it) = self.items.get_mut(real) {
                // 같은 라벨 재입력 시 미선택으로 토글.
                it.entry.label = if it.entry.label == label {
                    Label::Unrated
                } else {
                    label
                };
                self.sidecar_dirty = true;
                if self.cfg.auto_advance && it.entry.label != Label::Unrated {
                    self.advance(1);
                }
            }
        }
    }

    /// 별점(0~5) 설정. 라벨과 독립이며(#23), 그리드 다중 선택 중이면 선택 전부에 일괄 적용한다.
    /// 0은 별점 해제(Backtick). 같은 별점 재입력 시 0으로 토글(라벨 토글과 동일한 감각).
    /// `allow_advance`: 키보드 입력은 auto_advance를 따르고(true), 레일 마우스 클릭은 넘기지 않는다(false).
    fn set_stars(&mut self, stars: u8, allow_advance: bool) {
        if self.cull_axis_locked(AiCullTarget::Stars) {
            return;
        }
        let stars = stars.min(5);
        // 그리드 다중 선택 → 선택 전부에 그대로 적용(토글·자동진행 없음).
        if self.view == ViewMode::Grid && !self.selected.is_empty() {
            let targets: Vec<usize> = self.selected.iter().copied().collect();
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.stars = stars;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            if let Some(it) = self.items.get_mut(real) {
                it.entry.stars = if stars != 0 && it.entry.stars == stars { 0 } else { stars };
                self.sidecar_dirty = true;
                if allow_advance && self.cfg.auto_advance && it.entry.stars != 0 {
                    self.advance(1);
                }
            }
        }
    }

    /// 별점별 항목 수 `[무별점, 1★, 2★, 3★, 4★, 5★]`(전체 items 기준).
    fn star_counts(&self) -> [usize; 6] {
        let mut c = [0usize; 6];
        for it in &self.items {
            c[it.entry.stars.min(5) as usize] += 1;
        }
        c
    }

    /// 컬러 태그(#27) 설정. 라벨·별점과 독립. 그리드 다중 선택 중이면 선택 전부에 일괄 적용.
    /// 같은 태그 재입력 시 무태그(None)로 토글. 보조 축이라 자동진행은 하지 않는다.
    fn set_tag(&mut self, tag: ColorTag) {
        if self.cull_axis_locked(AiCullTarget::Tag) {
            return;
        }
        if self.view == ViewMode::Grid && !self.selected.is_empty() {
            let targets: Vec<usize> = self.selected.iter().copied().collect();
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.tag = tag;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            if let Some(it) = self.items.get_mut(real) {
                it.entry.tag = if it.entry.tag == tag { ColorTag::None } else { tag };
                self.sidecar_dirty = true;
            }
        }
    }

    /// 태그별 항목 수 `[Red, Yellow, Green, Blue, Purple]`(전체 items 기준, 무태그 제외)(#27).
    fn tag_counts(&self) -> [usize; 5] {
        let mut c = [0usize; 5];
        for it in &self.items {
            if let Some(i) = it.entry.tag.index() {
                c[i] += 1;
            }
        }
        c
    }

    fn advance(&mut self, delta: i64) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        let cur = self.index as i64 + delta;
        self.index = cur.clamp(0, len as i64 - 1) as usize;
        self.full_raw = false; // 이동 시 프리뷰로 복귀
    }

    /// 그리드에서 ↑/↓ 한 번에 이동할 칸 수(= 열 수). 단일뷰는 1.
    fn row_step(&self) -> i64 {
        if self.view == ViewMode::Grid {
            self.grid_cols.clamp(4, 12) as i64
        } else {
            1
        }
    }

    /// 키보드 내비게이션: 위치 이동 + (그리드면) 단일 커서로 전환하고 선택 셀이
    /// 보이도록 스크롤을 예약한다.
    fn nav(&mut self, delta: i64) {
        self.advance(delta);
        if self.view == ViewMode::Grid {
            self.selected.clear();
            self.sel_anchor = Some(self.index);
            let cols = self.grid_cols.clamp(4, 12);
            self.grid_scroll_to = Some(self.index / cols);
        }
    }

    /// 썸네일 캐시 자동 상한 정리를 백그라운드 스레드로 예약한다(#22). UI를 멈추지 않으며,
    /// 동시 호출은 cache::trim 내부에서 1개로 합쳐진다. 상한 0(무제한)이면 아무 것도 안 한다.
    fn schedule_cache_trim(&self) {
        let max = self.cfg.cache_limit_mb.saturating_mul(1024 * 1024);
        if max == 0 {
            return;
        }
        let dir = config::cache_dir();
        std::thread::spawn(move || cache::trim(&dir, max));
    }

    /// 세션 중 주기적으로(시간 기반) 디스크 캐시 상한을 정리한다. 폴더를 안 바꾸고 오래
    /// 보는 동안 프리뷰 캐시가 상한을 넘는 것을 막되(#D 회귀), 결과마다 trim을 돌려 캐시
    /// 디렉토리를 반복 스캔(전경 I/O와 경합)하지 않도록 충분히 드물게만 호출한다.
    fn trim_cache_if_due(&mut self) {
        if self.last_trim.elapsed() > Duration::from_secs(120) {
            self.last_trim = Instant::now();
            self.schedule_cache_trim();
        }
    }

    fn save_sidecar_if_due(&mut self) {
        if self.sidecar_dirty && self.last_save.elapsed() > Duration::from_millis(300) {
            if let Some(folder) = &self.folder {
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                let _ = sidecar::save(folder, &entries);
                self.sidecar_dirty = false;
                self.last_save = Instant::now();
            }
        }
    }

    /// [개발 전용] 벤치: 그리드를 실제 "화살표 쭉 누름"처럼 시간당 일정 행 자동 스크롤하며,
    /// 현재 화면 셀의 썸네일 캐시 적중률·프레임시간을 0.5s마다 temp 로그에 기록한다.
    /// "썸네일이 따라오는가"를 그라운드 트루스로 측정. env RB_BENCH로만 동작, 끝나면 프로세스 종료.
    /// 디버그 빌드에만 컴파일(릴리즈 제외).
    #[cfg(debug_assertions)]
    fn bench_step(&mut self, ctx: &egui::Context) {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::OnceLock;
        static START: OnceLock<Instant> = OnceLock::new();
        static LAST_LOG_MS: AtomicU64 = AtomicU64::new(u64::MAX);
        let start = *START.get_or_init(Instant::now);
        let elapsed = start.elapsed().as_secs_f64();
        let log = |s: String| {
            let path = std::env::temp_dir().join("rawblow_bench.log");
            if let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let _ = fh.write_all(s.as_bytes());
            }
        };

        self.view = ViewMode::Grid;
        let f = self.filtered();
        let len = f.len();
        if len == 0 {
            ctx.request_repaint();
            return;
        }
        let cols = self.grid_cols.clamp(4, 12);

        // 시간당 20행 이동(빠른 키 반복 수준). index를 시간 기준으로 전진.
        let target_row = (elapsed * 20.0) as usize;
        self.index = (target_row * cols).min(len - 1);
        self.grid_scroll_to = Some(self.index / cols);

        // 실제 그리드 가시 범위(지난 프레임 ui_grid가 채움) 셀의 썸네일 캐시 적중 = "따라오는가".
        let vis = self.grid_visible_rows.clone();
        let lo = (vis.start * cols).min(len);
        let hi = (vis.end * cols).min(len);
        let vis_n = hi.saturating_sub(lo).max(1);
        let cached = (lo..hi).filter(|&fi| self.thumbs.contains(f[fi])).count();

        let now_ms = (elapsed * 1000.0) as u64;
        let last = LAST_LOG_MS.load(Ordering::Relaxed);
        if last == u64::MAX || now_ms.saturating_sub(last) >= 500 {
            LAST_LOG_MS.store(now_ms, Ordering::Relaxed);
            log(format!(
                "{:6.1}s idx={:5}/{} rows={:?} vis_cached={}/{} thumbs={:5} pend_thumb={:4} pend_pref={:4} frame={:.0}ms\n",
                elapsed, self.index, len, vis, cached, vis_n,
                self.thumbs.len(), self.pending_thumb.len(), self.pending_prefetch.len(), self.frame_ms,
            ));
        }
        if (self.index >= len - 1 && elapsed > 2.0) || elapsed > 180.0 {
            log(format!("DONE elapsed={:.1}s idx={}/{} thumbs={}\n", elapsed, self.index, len, self.thumbs.len()));
            std::process::exit(0);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for RawBlowApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 프레임 시간 측정.
        let now = Instant::now();
        self.frame_ms = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;

        // [개발 전용] 벤치 모드: env RB_BENCH 설정 시 그리드를 자동 스크롤하며 썸네일 채움을 기록.
        // 디버그 빌드에만 포함(릴리즈 바이너리에는 들어가지 않음).
        #[cfg(debug_assertions)]
        if std::env::var_os("RB_BENCH").is_some() {
            self.bench_step(ctx);
        }

        // 은퇴 텍스처의 TTL을 깎아 in-flight 참조가 끝난 핸들만 드롭(GPU 파괴)한다.
        self.cache.tick();
        self.thumbs.tick();

        self.drain_results(ctx);

        // AI 컬링(#50)은 백그라운드 비차단 — 모달과 무관하게 매 프레임 진행률을 펌프하고
        // 완료 시 결과를 적용한다(좌측 레일 버튼이 프로그레스바로 표시).
        self.pump_ai_cull(ctx);

        // 탐색기/Finder에서 폴더(또는 파일)를 창에 끌어다 놓으면 그 폴더를 연다(#42).
        self.handle_drops(ctx);

        if !self.has_modal() {
            self.handle_keys(ctx);
        }

        // 창이 모니터보다 크면 작업 영역 안으로 1회 축소(+좌상단 이동). eframe의 시작
        // 클램프가 모니터 열거 실패(일부 원격 세션)로 빠질 수 있고, 작은 모니터 쪽에
        // 열린 멀티모니터 케이스도 있어 런타임 값으로 보강한다. monitor_size가 창보다
        // 충분히 크거나 미보고면 아무것도 하지 않는다.
        if !self.size_clamped && !self.fullscreen {
            let (inner, monitor) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().monitor_size));
            if let (Some(inner), Some(mon)) = (inner, monitor) {
                self.size_clamped = true; // 값이 보고된 프레임에 1회만 판단(첫 프레임 미보고 대비).
                if mon.x > 1.0 && mon.y > 1.0 && (inner.width() > mon.x || inner.height() > mon.y) {
                    // 타이틀바·테두리 여유분(점 단위, 보수적으로 살짝 안쪽).
                    let want = Vec2::new(inner.width().min(mon.x - 16.0), inner.height().min(mon.y - 56.0));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(0.0, 0.0)));
                }
            }
        }

        // OS 창 풀스크린을 self.fullscreen에 동기화(#31). 토글 출처(F11·버튼·Esc) 무관하게 한 곳에서
        // 처리해 ViewportCommand를 상태가 바뀔 때만 보낸다(매 프레임 전송 금지).
        if self.fullscreen != self.fs_applied {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            self.fs_applied = self.fullscreen;
        }

        // 현재 항목 메타(EXIF·AF)는 백그라운드 로드 — 사진 넘김을 막지 않는다.
        self.drain_meta(ctx);
        self.drain_cull_meta(ctx);
        self.request_meta(ctx);
        self.request_preload();
        self.request_prefetch_window(); // 현재 위치 주변만 디스크 캐시 워밍(폴더 전체 플러드 금지).
        self.save_sidecar_if_due();
        self.trim_cache_if_due(); // 세션 중 주기적 캐시 상한 정리(드물게 — 전경 I/O 비경합).

        // 프리뷰(현재 주변)는 즉시 리페인트로 빠르게 채운다.
        // 썸네일 채우기는 이 100ms 타이머만으로 구동한다(워커는 egui를 건드리지 않음).
        // 매 틱마다 drain_results가 채널에 쌓인 결과를 한 번에 비우므로 10fps여도 처리량은
        // 충분하고, 다 끝나면(pending 비면) 타이머가 꺼져 0% CPU로 유휴.
        if !self.pending_preview.is_empty() {
            ctx.request_repaint();
        } else if !self.pending_thumb.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if !self.pending_prefetch.is_empty() {
            // 배경 프리페치 진행 중에는 느린 keep-alive로 drain_results 펌프를 돌려 결과
            // 채널·pending 집합이 무한정 쌓이지 않게 한다(워커는 egui를 못 깨우므로 필요).
            // 우선순위가 가장 낮고(200ms), 프리페치가 끝나면 꺼져 0% CPU로 유휴.
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // 화면 분기.
        if self.folder.is_none() {
            self.ui_open(ctx);
        } else if self.licenses.is_some() {
            self.ui_licenses(ctx);
        } else if self.show_settings {
            self.ui_settings(ctx);
        } else if self.fullscreen {
            self.ui_fullscreen(ctx);
        } else {
            self.ui_shell(ctx);
        }

        // 모달. 진행 중 작업(전송/정리)이 있으면 그 프로그레스바가 최우선 — 다른 모달을 가린다.
        #[cfg(feature = "model-download")]
        let model_dl_active = self.model_dl.is_some();
        #[cfg(not(feature = "model-download"))]
        let model_dl_active = false;
        if self.progress.is_some() {
            self.ui_progress(ctx);
        } else if model_dl_active {
            #[cfg(feature = "model-download")]
            self.ui_model_dl_progress(ctx);
        } else if self.transfer.is_some() {
            self.ui_transfer_dialog(ctx);
        } else if self.organize.is_some() {
            self.ui_organize_dialog(ctx);
        } else if self.ai_cull_open {
            self.ui_ai_cull_dialog(ctx);
        } else if self.ai_cull_cancel_confirm {
            // AI 컬링 확인 모달은 같은 else-if 사슬에 둬 서로/다른 모달과 동시에 그려지지 않게 한다.
            self.ui_ai_cull_cancel_confirm(ctx);
        } else if self.ai_cull_folder_confirm.is_some() {
            self.ui_ai_cull_folder_confirm(ctx);
        }
        if self.result.is_some() {
            self.ui_transfer_result(ctx);
        }
        if self.jump_open {
            self.ui_jump(ctx);
        }
        if self.bulk_open {
            self.ui_bulk(ctx);
        }

        // 새 릴리즈 안내(#33): 유휴 시 1회 백그라운드 확인. 결과 배너는 좌측 레일 정리 버튼 위에 뜬다.
        self.maybe_check_update(ctx);

        // 토스트 만료.
        if let Some((_, t)) = &self.toast {
            if t.elapsed() > Duration::from_secs(3) {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }
}

impl RawBlowApp {
    fn has_modal(&self) -> bool {
        // show_settings 포함: 설정 화면이 떠 있을 때 1~5/QWER/백틱 등 단축키가 뒤의 '현재 항목'을
        // 몰래 바꾸거나 설정의 DragValue 입력과 충돌하지 않게 키 처리를 막는다.
        self.transfer.is_some()
            || self.organize.is_some()
            || self.progress.is_some()
            || self.result.is_some()
            || self.jump_open
            || self.bulk_open
            || self.show_settings
            || self.licenses.is_some()
            || self.ai_cull_open
            // 진행 중 컬링(ai_cull.is_some())은 **비차단** — has_modal에 넣지 않는다(다른 작업 가능).
            // 단 컬링 관련 확인 모달은 차단.
            || self.ai_cull_cancel_confirm
            || self.ai_cull_folder_confirm.is_some()
            || {
                #[cfg(feature = "model-download")]
                { self.model_dl.is_some() }
                #[cfg(not(feature = "model-download"))]
                { false }
            }
    }

    // ── 입력 ──────────────────────────────────────────────
    /// 탐색기·Finder에서 폴더(또는 파일)를 창에 드래그앤드롭하면 그 폴더를 연다(#42).
    /// 폴더를 끌면 그 폴더를, 파일을 끌면 파일이 든 폴더를 연다. 여러 항목을 끌면 폴더를
    /// 우선 채택한다. 전송/정리가 진행 중일 때는 폴더 전환을 막아 작업 도중 상태가 뒤집히지 않게 한다.
    fn handle_drops(&mut self, ctx: &egui::Context) {
        // 모달(확인창·설정·진행 등)이 떠 있으면 드롭으로 폴더를 바꾸지 않는다(상태 충돌 방지).
        // 백그라운드 컬링은 모달이 아니므로(has_modal=false) 이때의 드롭은 request_open_folder가
        // 받아 확인창을 띄운다. 컬링 취소/폴더 확인 모달이 떠 있는 동안은 여기서 막혀
        // 두 확인창이 겹치는 일이 없다.
        if self.has_modal() {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let mut folder: Option<PathBuf> = None;
        for f in &dropped {
            let Some(p) = f.path.as_ref() else { continue };
            if p.is_dir() {
                folder = Some(p.clone());
                break; // 폴더를 만나면 그걸 우선 채택.
            }
            // 파일이면 그 파일이 든 폴더를 후보로 둔다(이후 폴더를 만나면 덮어쓴다).
            if folder.is_none() {
                if let Some(parent) = p.parent() {
                    if parent.is_dir() {
                        folder = Some(parent.to_path_buf());
                    }
                }
            }
        }
        if let Some(folder) = folder {
            // 같은 폴더를 다시 열어 재스캔·인덱스 리셋하지 않는다.
            if self.folder.as_deref() != Some(folder.as_path()) {
                // 컬링 중이면 확인 모달 경유(인덱스 안정성).
                self.request_open_folder(folder);
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (keys, modifiers, scroll) = ctx.input(|i| {
            (
                i.keys_down.clone(),
                i.modifiers,
                i.raw_scroll_delta.y,
            )
        });
        let _ = keys;

        ctx.input(|i| {
            use egui::Key;
            for ev in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    let cmd = modifiers.command;
                    match key {
                        // 그리드: ←/→ = ±1, ↑/↓ = ±한 행(cols). 단일뷰: 모두 ±1.
                        Key::ArrowRight => self.nav(1),
                        Key::ArrowLeft => self.nav(-1),
                        Key::ArrowDown => {
                            let d = self.row_step();
                            self.nav(d);
                        }
                        Key::ArrowUp => {
                            let d = self.row_step();
                            self.nav(-d);
                        }
                        Key::Q => self.set_label(Label::Pick),
                        Key::W => self.set_label(Label::Hold),
                        Key::E if cmd => self.open_transfer(), // Ctrl/⌘E 전송
                        Key::E => self.set_label(Label::Reject),
                        Key::R => self.set_label(Label::Unrated),
                        // 컬러 태그(#27): Shift+1~5 = 5색(Red/Yellow/Green/Blue/Purple), Shift+0 = 해제.
                        // 별점(1~5)·라벨(QWER)과 독립. shift 아래 별점 arm보다 먼저 와야 가로채진다.
                        Key::Num1 if modifiers.shift && !cmd => self.set_tag(ColorTag::Orange),
                        Key::Num2 if modifiers.shift && !cmd => self.set_tag(ColorTag::Pink),
                        Key::Num3 if modifiers.shift && !cmd => self.set_tag(ColorTag::Teal),
                        Key::Num4 if modifiers.shift && !cmd => self.set_tag(ColorTag::Blue),
                        Key::Num5 if modifiers.shift && !cmd => self.set_tag(ColorTag::Purple),
                        Key::Num0 if modifiers.shift && !cmd => self.set_tag(ColorTag::None),
                        // 별점(#23): 1~5로 지정, `(백틱)으로 해제. 라벨(QWER)과 독립·중복 사용 가능.
                        // !cmd 가드: ⌘/Ctrl+숫자(타 앱 탭 전환 습관 등)가 실수로 별점을 매기지 않게.
                        Key::Num1 if !cmd => self.set_stars(1, true),
                        Key::Num2 if !cmd => self.set_stars(2, true),
                        Key::Num3 if !cmd => self.set_stars(3, true),
                        Key::Num4 if !cmd => self.set_stars(4, true),
                        Key::Num5 if !cmd => self.set_stars(5, true),
                        Key::Backtick if !cmd => self.set_stars(0, true),
                        Key::T => {
                            self.view = match self.view {
                                ViewMode::Single => ViewMode::Grid,
                                ViewMode::Grid => ViewMode::Single,
                            }
                        }
                        Key::I => self.show_exif = !self.show_exif,
                        Key::H => self.show_hist = !self.show_hist,
                        // GPS 미니 지도(#38) / AF 포인트(#37): 토글 즉시 저장(요구: 설정 유지).
                        Key::M => {
                            self.show_map = !self.show_map;
                            self.cfg.show_map = self.show_map;
                            let _ = config::save(&self.cfg);
                        }
                        Key::A => {
                            self.show_af = !self.show_af;
                            self.cfg.show_af = self.show_af;
                            let _ = config::save(&self.cfg);
                        }
                        Key::Space | Key::Z => {
                            // 창맞춤 ↔ 1:1 토글.
                            if self.fit {
                                self.fit = false;
                                self.zoom = 1.0;
                                self.pan = Vec2::ZERO;
                                // #49: AF 포인트 표시 중이면 측거점 기준으로 확대(photo_view가 pan 보정).
                                self.af_zoom_pending = self.show_af;
                            } else {
                                self.fit = true;
                                self.pan = Vec2::ZERO;
                                self.af_zoom_pending = false;
                            }
                        }
                        Key::D => self.full_raw = !self.full_raw, // ORIG(원본 보기) 토글
                        Key::F => self.filter = self.filter.next(),
                        Key::G => self.jump_open = true,
                        Key::B if self.view == ViewMode::Grid => {
                            self.bulk_open = true;
                            self.bulk_searched = false;
                            self.bulk_hits.clear();
                        }
                        Key::F11 => self.fullscreen = !self.fullscreen,
                        Key::Escape => self.fullscreen = false,
                        Key::Enter => self.open_transfer(),
                        Key::O if cmd => self.pick_folder(),
                        _ => {}
                    }
                }
            }
        });

        // 마우스 휠 이동(단일 뷰): 창맞춤 상태에서 Ctrl 없이 휠 → 사진 넘김.
        // (확대 상태이거나 Ctrl 휠은 photo_view에서 줌으로 처리.)
        if self.view == ViewMode::Single && self.fit && !modifiers.ctrl && !modifiers.command {
            if scroll < -1.0 {
                self.advance(1);
            } else if scroll > 1.0 {
                self.advance(-1);
            }
        }
    }

    fn pick_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.request_open_folder(dir);
        }
    }


    /// 사용자 의도의 폴더 전환 진입점(#50). 컬링이 진행 중이면 바로 열지 않고 확인 모달을
    /// 띄운다(예 → 컬링 취소 후 연다). 컬링이 없으면 즉시 연다. 전송/정리 완료 후의 내부
    /// 재오픈은 이 래퍼를 거치지 않고 `open_folder`를 직접 부른다.
    fn request_open_folder(&mut self, folder: PathBuf) {
        if self.ai_cull.is_some() {
            self.ai_cull_folder_confirm = Some(folder);
        } else {
            self.open_folder(folder);
        }
    }

    // ── Open Folder 화면 ──────────────────────────────────
    fn ui_open(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    // 로고 마크(3겹 셰브론).
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(72.0, 72.0), Sense::hover());
                    crate::logo::draw_mark(ui.painter(), rect);
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("RawBlow").font(prop(30.0)).color(theme::INK));
                    ui.label(
                        egui::RichText::new(format!("FAST RAW CULLING · {}", tr(lang, "사진 셀렉 뷰어")))
                            .font(mono(11.0))
                            .color(theme::INK3),
                    );
                    ui.add_space(28.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(format!("  {}  {}O  ", tr(lang, "폴더 열기"), cmd_key())).color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                        ).fill(theme::ACCENT))
                        .clicked()
                    {
                        self.pick_folder();
                    }
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("JPG · HEIC · PNG · RW2 · CR3 · ARW · NEF · DNG · …")
                            .font(mono(10.0))
                            .color(theme::INK4),
                    );

                    // 최근 폴더.
                    if !self.cfg.recent_folders.is_empty() {
                        ui.add_space(32.0);
                        ui.label(egui::RichText::new("RECENT FOLDERS").font(prop(10.0)).color(theme::INK3));
                        ui.add_space(6.0);
                        let recents = self.cfg.recent_folders.clone();
                        for r in recents.iter().take(8) {
                            if ui
                                .add(egui::Label::new(egui::RichText::new(r).font(mono(11.0)).color(theme::INK2)).sense(Sense::click()))
                                .clicked()
                            {
                                let p = PathBuf::from(r);
                                if p.is_dir() {
                                    self.open_folder(p);
                                }
                            }
                        }
                    }
                });
            });
    }

    // ── Studio 셸: 툴바 + 좌측레일 + 필름스트립 + 상태바 + 중앙 ──
    /// 사진 표시 화면(매팅) 배경색(#36). 설정값이 있으면 그 색, 없으면 앱 기본(near-black void).
    fn photo_bg(&self) -> Color32 {
        match self.cfg.photo_bg {
            Some([r, g, b]) => Color32::from_rgb(r, g, b),
            None => theme::BG0,
        }
    }

    /// 현재 사진 배경 RGB(설정값 또는 기본) — 설정의 HEX/RGB 편집 시작값(#36).
    fn photo_bg_rgb(&self) -> [u8; 3] {
        self.cfg.photo_bg.unwrap_or(theme::BG0_RGB)
    }

    /// 스트립·그리드 셀 표기(선택 표시·별점·색상 태그)의 크기 배율(#44). 설정에 따라 크게(1.8)/작게(1.0).
    fn badge_scale(&self) -> f32 {
        if self.cfg.large_badges {
            1.8
        } else {
            1.0
        }
    }


    fn ui_shell(&mut self, ctx: &egui::Context) {
        self.ui_toolbar(ctx);
        self.ui_status_bar(ctx);
        self.ui_left_rail(ctx);
        if self.view == ViewMode::Single {
            self.ui_filmstrip(ctx);
        }
        let bg = self.photo_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| match self.view {
                ViewMode::Single => self.ui_single(ui),
                ViewMode::Grid => self.ui_grid(ui),
            });
    }

    fn ui_toolbar(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // 로고 마크(작게).
                    let (mr, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                    crate::logo::draw_mark(ui.painter(), mr);
                    ui.add_space(6.0);
                    vsep(ui);
                    // 폴더 열기(이슈 #2: 폴더 연 상태에서 다른 폴더 여는 버튼 없음 — ⌘O를 못 찾음).
                    if toggle_btn(ui, &format!("{} ({}O)", tr(lang, "폴더 열기"), cmd_key()), false).clicked() {
                        self.pick_folder();
                    }
                    vsep(ui);
                    let single = self.view == ViewMode::Single;
                    if toggle_btn(ui, "Single (T)", single).clicked() {
                        self.view = ViewMode::Single;
                    }
                    if toggle_btn(ui, "Grid (T)", !single).clicked() {
                        self.view = ViewMode::Grid;
                    }
                    vsep(ui);

                    if ui.add(egui::Button::new("◀").frame(false)).clicked() {
                        self.advance(-1);
                    }
                    let f = self.filtered();
                    let pos = format!("{:03} / {}", (self.index + 1).min(f.len().max(1)), f.len());
                    ui.label(egui::RichText::new(pos).font(mono(12.0)).color(theme::INK).background_color(theme::BG2));
                    if ui.add(egui::Button::new("▶").frame(false)).clicked() {
                        self.advance(1);
                    }
                    vsep(ui);

                    if toggle_btn(ui, "Fit (Space)", self.fit).clicked() {
                        self.fit = true;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "1:1 (Space)", !self.fit && (self.zoom - 1.0).abs() < 0.01).clicked() {
                        self.fit = false;
                        self.zoom = 1.0;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "ORIG (D)", self.full_raw).clicked() {
                        self.full_raw = !self.full_raw;
                    }
                    // 전체화면 토글(#31). 버튼/F11 어느 쪽이든 OS 창 풀스크린으로(update에서 동기화).
                    if toggle_btn(ui, "Full (F11)", self.fullscreen).clicked() {
                        self.fullscreen = !self.fullscreen;
                    }
                    vsep(ui);
                    if toggle_btn(ui, "EXIF (I)", self.show_exif).clicked() {
                        self.show_exif = !self.show_exif;
                    }
                    if toggle_btn(ui, "Hist (H)", self.show_hist).clicked() {
                        self.show_hist = !self.show_hist;
                    }
                    // (필터 변경은 좌측 레일에서 전담 — 상단 Filter 버튼 제거, #toolbar feedback)

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(egui::RichText::new(format!(" {} ({}E) ", tr(lang, "전송"), cmd_key())).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT))
                            .clicked()
                        {
                            self.open_transfer();
                        }
                        // 폴더 자동 분류(#34)는 좌측 레일 하단으로 이동(테스트 피드백) — 여긴 전송/점프/일괄만.
                        if toggle_btn(ui, &format!("{} (G)", tr(lang, "점프")), false).clicked() {
                            self.jump_open = true;
                        }
                        // 그리드 모드 한정: 파일명으로 일괄 라벨링(#3).
                        if self.view == ViewMode::Grid
                            && toggle_btn(ui, &format!("{} (B)", tr(lang, "일괄")), false).clicked()
                        {
                            self.bulk_open = true;
                            self.bulk_searched = false;
                            self.bulk_hits.clear();
                        }
                        if toggle_btn(ui, "⚙", self.show_settings).clicked() {
                            self.show_settings = true;
                            self.cache_size = None; // 설정 열 때 캐시 용량 새로 계산.
                            self.bg_hex = hex_str(self.photo_bg_rgb()); // 배경 HEX 입력 버퍼 동기화(#36).
                        }
                    });
                });
            });
    }

    fn ui_left_rail(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let (pick, hold, reject, unrated) = self.counts();
        let total = self.items.len().max(1);
        egui::SidePanel::left("rail")
            .exact_width(RAIL_W)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG1))
            .show(ctx, |ui| {
                section_head(ui, "Classify", Some("Q W E R"));
                let rows = [
                    (Label::Pick, "PICK", pick, "Q"),
                    (Label::Hold, "HOLD", hold, "W"),
                    (Label::Reject, "REJECT", reject, "E"),
                    (Label::Unrated, "UNRATED", unrated, "R"),
                ];
                let cur_label = self.current_real().and_then(|r| self.items.get(r)).map(|i| i.entry.label);
                for (label, name, n, key) in rows {
                    let active = cur_label == Some(label) && !matches!(label, Label::Unrated);
                    let resp = ui.allocate_response(Vec2::new(RAIL_W - 16.0, 30.0), Sense::click());
                    let rect = resp.rect;
                    let p = ui.painter();
                    if active {
                        p.rect(rect, Rounding::same(5.0), theme::BG2, Stroke::new(1.0, widgets::with_alpha(theme::label_color(label), 64)));
                    }
                    p.circle_filled(Pos2::new(rect.left() + 12.0, rect.center().y), 4.0, theme::label_color(label));
                    p.text(Pos2::new(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, name, prop(11.5), theme::INK2);
                    p.text(Pos2::new(rect.right() - 36.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), mono(11.0), theme::INK);
                    p.text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, key, mono(10.0), theme::INK3);
                    if resp.clicked() {
                        self.set_label(label);
                    }
                }

                // ── Rating (별점, #23) ── 라벨과 독립. 현재 항목 별점을 1~5로 지정/해제.
                section_head(ui, "Rating", Some("1–5 · `"));
                let cur_stars = self
                    .current_real()
                    .and_then(|r| self.items.get(r))
                    .map(|i| i.entry.stars)
                    .unwrap_or(0);
                let mut clicked_star: Option<u8> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for n in 1..=5u8 {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        let filled = n <= cur_stars;
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            if filled { "★" } else { "☆" },
                            prop(16.0),
                            if filled { theme::HOLD } else { theme::INK4 },
                        );
                        if resp.clicked() {
                            clicked_star = Some(n);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                    ui.add_space(8.0);
                    let (cr, cresp) = ui.allocate_exact_size(Vec2::new(28.0, 22.0), Sense::click());
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        tr(lang, "해제"),
                        prop(10.5),
                        if cresp.hovered() { theme::INK } else { theme::INK3 },
                    );
                    if cresp.clicked() {
                        clicked_star = Some(0);
                    }
                    if cresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });
                if let Some(n) = clicked_star {
                    // 레일에서 마우스로 별을 클릭할 땐 사진을 넘기지 않는다(방금 매긴 컷을 계속 보게).
                    self.set_stars(n, false);
                }

                // ── Color tag (#27) ── 라벨·별점과 독립. 현재 항목에 5색 중 하나 부여/해제(⇧1~5).
                section_head(ui, "Color", Some("⇧1–5"));
                let cur_tag = self
                    .current_real()
                    .and_then(|r| self.items.get(r))
                    .map(|i| i.entry.tag)
                    .unwrap_or(ColorTag::None);
                let tag_cnt_in = self.tag_counts();
                let tag_names: Vec<String> =
                    ColorTag::ALL.iter().map(|t| self.cfg.tag_label(*t, lang)).collect();
                let mut clicked_tag: Option<ColorTag> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                        let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        let active = cur_tag == *tag;
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        ui.painter().circle_filled(r.center(), if active { 9.0 } else { 6.5 }, col);
                        if active {
                            ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5, theme::INK));
                        }
                        if resp.clicked() {
                            clicked_tag = Some(*tag);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        resp.on_hover_text(format!("{} · {}", tag_names[i], tag_cnt_in[i]));
                    }
                    ui.add_space(6.0);
                    let (cr, cresp) = ui.allocate_exact_size(Vec2::new(28.0, 22.0), Sense::click());
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        tr(lang, "해제"),
                        prop(10.5),
                        if cresp.hovered() { theme::INK } else { theme::INK3 },
                    );
                    if cresp.clicked() {
                        clicked_tag = Some(ColorTag::None);
                    }
                    if cresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });
                if let Some(t) = clicked_tag {
                    self.set_tag(t);
                }

                section_head(ui, "Progress", None);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let labeled = pick + hold + reject;
                    let pct = labeled as f32 / total as f32 * 100.0;
                    ui.label(egui::RichText::new(format!("{:.1}%", pct)).font(mono(10.0)).color(theme::INK3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(14.0);
                        ui.label(egui::RichText::new(format!("{} / {}", labeled, total)).font(mono(10.0)).color(theme::INK3));
                    });
                });

                section_head(ui, "Filter View", None);
                for filt in [Filter::All, Filter::Pick, Filter::Hold, Filter::Reject, Filter::Unrated] {
                    let active = self.filter == filt;
                    let resp = ui.allocate_response(Vec2::new(RAIL_W - 16.0, 24.0), Sense::click());
                    let rect = resp.rect;
                    let p = ui.painter();
                    if active {
                        p.rect_filled(rect, Rounding::same(4.0), theme::BG3);
                    }
                    p.text(Pos2::new(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, filt.name(lang), prop(12.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        self.filter = filt;
                        self.index = 0;
                    }
                }

                // 별점 필터(#23 후속): 라벨 필터와 독립 AND. 정확히 N점만 표시. `전체`=별점 무시.
                section_head(ui, "Filter Stars", None);
                let star_sel = self.star_filter;
                let star_cnt = self.star_counts(); // [미부여, 1★ .. 5★]
                let mut new_star: Option<StarFilter> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 3.0;
                    // 전체(Any)
                    let active = star_sel == StarFilter::Any;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), Sense::click());
                    if active {
                        ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, tr(lang, "전체"), prop(11.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        new_star = Some(StarFilter::Any);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    // ★1 ~ ★5 (정확히 n점). 해당 점수 항목이 없으면 흐리게.
                    for n in 1..=5u8 {
                        let active = star_sel == StarFilter::Exact(n);
                        let empty = star_cnt[n as usize] == 0;
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                        }
                        let col = if active {
                            theme::HOLD
                        } else if empty {
                            theme::INK4
                        } else {
                            theme::INK2
                        };
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("★{n}"), prop(10.5), col);
                        if resp.clicked() {
                            new_star = Some(StarFilter::Exact(n));
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
                if let Some(sf) = new_star {
                    // 같은 별점 칩 재클릭 = 토글 해제(전체로). 그 외엔 해당 별점만.
                    self.star_filter = if sf == self.star_filter { StarFilter::Any } else { sf };
                    self.index = 0;
                }

                // 컬러 태그 필터(#27): 라벨·별점 필터와 독립 AND. 특정 색만 표시. `전체`=태그 무시.
                section_head(ui, "Filter Color", None);
                let tag_sel = self.tag_filter;
                let tcnt = self.tag_counts();
                let mut new_tag_filter: Option<TagFilter> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let active = tag_sel == TagFilter::Any;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), Sense::click());
                    if active {
                        ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, tr(lang, "전체"), prop(11.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        new_tag_filter = Some(TagFilter::Any);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        let active = tag_sel == TagFilter::Only(*tag);
                        let empty = tcnt[i] == 0;
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                        }
                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                        let mut col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        if empty && !active {
                            col = widgets::with_alpha(col, 64);
                        }
                        ui.painter().circle_filled(r.center(), 6.5, col);
                        if resp.clicked() {
                            new_tag_filter =
                                Some(if active { TagFilter::Any } else { TagFilter::Only(*tag) });
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
                if let Some(tf) = new_tag_filter {
                    self.tag_filter = tf;
                    self.index = 0;
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    // 폴더 자동 분류(#34): 툴바에서 레일 하단으로 이동(테스트 피드백). 풀폭 버튼.
                    // 자동저장(saved/SESSION) 상태표시는 하단 상태바로 이동 — 여기선 제거.
                    // 폴더 아이콘은 벡터(컬러)로 직접 그린다 — 이모지(🗂)가 폰트에서 두부(□)로 깨져서(#33 후속).
                    ui.add_space(10.0); // 레일 하단 여백(bottom_up이라 버튼 아래쪽 패딩).
                    // 컬링 진행 중에는 폴더를 바꾸는 무거운 작업(정리)을 잠가 인덱스 안정성을 지킨다.
                    let org_enabled = !self.items.is_empty() && self.ai_cull.is_none();
                    // 폭은 항상 사이드바 가용 너비에 맞춘다 — 환경(스크롤바 등)에 따라 좌측으로
                    // 쏠리던 문제 방지. 내용(아이콘+글자)은 셀 중앙 기준이라 자동으로 가운데 정렬.
                    let (org_rect, org_resp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 32.0),
                        if org_enabled { Sense::click() } else { Sense::hover() },
                    );
                    {
                        let p = ui.painter();
                        let txt_col = if org_enabled { theme::INK } else { theme::INK4 };
                        let fill = if org_enabled && org_resp.hovered() { theme::BG4 } else { theme::BG3 };
                        p.rect(org_rect, Rounding::same(6.0), fill, Stroke::new(1.0, theme::LINE2));
                        // 아이콘 + 글자 묶음을 셀 가운데에 배치(총 너비 계산 후 시작 x 산출).
                        let icon = 16.0;
                        let gap = 7.0;
                        let font = prop(12.0);
                        let label = tr(lang, "정리");
                        let galley = p.layout_no_wrap(label.to_string(), font.clone(), txt_col);
                        let x0 = org_rect.center().x - (icon + gap + galley.size().x) / 2.0;
                        let icon_rect = Rect::from_min_size(
                            Pos2::new(x0, org_rect.center().y - icon / 2.0),
                            Vec2::splat(icon),
                        );
                        widgets::draw_folder_icon(p, icon_rect, org_enabled);
                        p.text(Pos2::new(x0 + icon + gap, org_rect.center().y), Align2::LEFT_CENTER, label, font, txt_col);
                    }
                    if org_enabled {
                        if org_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if org_resp.clicked() {
                            self.open_organize();
                        }
                    }

                    // AI 컬링(#50): 정리 버튼 **바로 위**(bottom_up이라 코드상 뒤가 위). 강조색 테두리로
                    // 일반 '정리'와 구분. 항목이 있을 때만 활성. 진행 중에는 버튼이 프로그레스바로 바뀌고
                    // 클릭하면 취소 확인창을 띄운다.
                    ui.add_space(8.0);
                    let cull_progress = self.ai_cull.as_ref().map(|j| (j.progress.load(Ordering::Relaxed), j.total));
                    if let Some((done, total)) = cull_progress {
                        // 진행 중: 프로그레스바 + "AI 분석 nn%" — 클릭 시 취소 확인.
                        let (ai_rect, ai_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.0),
                            Sense::click(),
                        );
                        let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                        {
                            let p = ui.painter();
                            // 외곽선 없이 — 진행률은 채움(ACCENT_DIM)으로 표현.
                            p.rect(ai_rect, Rounding::same(6.0), theme::BG3, Stroke::NONE);
                            // 채움(진행률) — 좌측에서 frac 비율만큼.
                            if frac > 0.0 {
                                let fill_rect = Rect::from_min_size(
                                    ai_rect.min,
                                    Vec2::new(ai_rect.width() * frac, ai_rect.height()),
                                );
                                p.rect_filled(fill_rect, Rounding::same(6.0), theme::ACCENT_DIM);
                            }
                            let label = format!("✨ {} {}%", tr(lang, "분석"), (frac * 100.0).round() as i32);
                            p.text(ai_rect.center(), Align2::CENTER_CENTER, label, prop(12.0), theme::ACCENT);
                        }
                        if ai_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if ai_resp.clicked() {
                            self.ai_cull_cancel_confirm = true;
                        }
                    } else {
                        let ai_enabled = !self.items.is_empty();
                        let (ai_rect, ai_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.0),
                            if ai_enabled { Sense::click() } else { Sense::hover() },
                        );
                        {
                            let p = ui.painter();
                            let txt_col = if ai_enabled { theme::ACCENT } else { theme::INK4 };
                            let fill = if ai_enabled && ai_resp.hovered() { theme::BG4 } else { theme::BG3 };
                            // 외곽선 없이 채움만(강조 테두리가 거슬린다는 피드백). 강조는 텍스트 색으로.
                            p.rect(ai_rect, Rounding::same(6.0), fill, Stroke::NONE);
                            let label = format!("✨ {}", tr(lang, "AI 컬링"));
                            p.text(ai_rect.center(), Align2::CENTER_CENTER, label, prop(12.0), txt_col);
                        }
                        if ai_enabled {
                            if ai_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if ai_resp.clicked() {
                                self.ai_cull_open = true;
                            }
                        }
                    }

                    // 새 릴리즈 안내(#33): 정리 버튼 **위에** 강조 배너(bottom_up이라 코드상 뒤가 위).
                    // 3줄 — 컬러 스파클 아이콘 / "새로운 버전이 있습니다" / 버전. 각각 한 줄씩 가운데 정렬.
                    // 아이콘은 벡터(컬러)로 직접 그림(🎉 이모지 두부 회피). 클릭 시 Releases 열고 배너 닫음.
                    if let Some(ver) = self.update_available.clone() {
                        ui.add_space(8.0);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::click());
                        {
                            let p = ui.painter();
                            let dark = Color32::from_rgb(0x0a, 0x14, 0x20);
                            let fill = if resp.hovered() { Color32::from_rgb(0x8e, 0xc9, 0xff) } else { theme::ACCENT };
                            p.rect(rect, Rounding::same(8.0), fill, Stroke::new(1.0, theme::ACCENT));
                            // 1줄: 컬러 스파클 아이콘(가운데 상단).
                            let icon = 22.0;
                            let icon_rect = Rect::from_center_size(
                                Pos2::new(rect.center().x, rect.top() + 6.0 + icon / 2.0),
                                Vec2::splat(icon),
                            );
                            widgets::draw_sparkles(p, icon_rect);
                            // 2줄: 안내 문구.
                            p.text(
                                Pos2::new(rect.center().x, rect.top() + 42.0),
                                Align2::CENTER_CENTER,
                                tr(lang, "새로운 버전이 있습니다"),
                                prop(12.0),
                                dark,
                            );
                            // 3줄: 버전.
                            p.text(
                                Pos2::new(rect.center().x, rect.top() + 62.0),
                                Align2::CENTER_CENTER,
                                &ver,
                                prop(12.5),
                                dark,
                            );
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            open_url("https://github.com/ascoeur9/rawblow/releases/latest");
                            self.update_available = None;
                        }
                    }
                });
            });
    }

    fn ui_status_bar(&mut self, ctx: &egui::Context) {
        let (pick, hold, reject, unrated) = self.counts();
        let f_len = self.filtered().len();
        let fps = if self.frame_ms > 0.0 { 1000.0 / self.frame_ms } else { 0.0 };
        egui::TopBottomPanel::bottom("status")
            .exact_height(STATUS_H)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(12.0, 4.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("●").color(theme::OK).size(8.0));
                    ui.label(egui::RichText::new(format!("READY · {} ITEMS · {}/{}", self.items.len(), (self.index + 1).min(f_len.max(1)), f_len)).font(mono(10.5)).color(theme::INK3));
                    ui.label(egui::RichText::new(format!("· P {pick} H {hold} X {reject} · {unrated}")).font(mono(10.5)).color(theme::INK3));
                    // 로딩·프레임 통계: 우측 → 좌측으로 이동(테스트 피드백).
                    ui.label(egui::RichText::new(format!("· {:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", self.frame_ms, fps, self.cfg.preload)).font(mono(10.5)).color(theme::INK4));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 맨 오른쪽: 자동저장 상태(레일에서 이동). right_to_left이므로 먼저 추가 = 최우측.
                        // 글자(우) 왼쪽에 상태 점을 둬 "● saved"/"● saving…"로 보이게 한다.
                        let (dot, txt) = if self.sidecar_dirty { (theme::HOLD, "saving…") } else { (theme::OK, "saved") };
                        ui.label(egui::RichText::new(txt).font(mono(10.5)).color(theme::INK3));
                        ui.label(egui::RichText::new("●").color(dot).size(8.0));
                        // 그 왼쪽: 배율(단일/전체화면에서만 의미).
                        if self.view == ViewMode::Single || self.fullscreen {
                            ui.label(egui::RichText::new("·").font(mono(10.5)).color(theme::INK4));
                            ui.label(
                                egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                                    .font(mono(10.5))
                                    .color(theme::INK2),
                            );
                        }
                    });
                });
            });
    }

    fn ui_filmstrip(&mut self, ctx: &egui::Context) {
        let f = self.filtered();
        egui::TopBottomPanel::bottom("filmstrip")
            .exact_height(FILMSTRIP_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 10.0)))
            .show(ctx, |ui| {
                if f.is_empty() {
                    return;
                }
                let cur = self.index.min(f.len() - 1);
                let thumb_w = 108.0;
                let avail = ui.available_width();
                let visible = ((avail / (thumb_w + 6.0)).floor() as usize).max(1);
                let half = visible / 2;
                let start = cur.saturating_sub(half);
                let end = (start + visible).min(f.len());
                ui.horizontal_centered(|ui| {
                    for (fi, &real) in f.iter().enumerate().take(end).skip(start) {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(thumb_w, 72.0), Sense::click());
                        let (tex, tsize) = match self.thumbs.get(real) {
                            Some((t, _, s)) => (Some(t), s),
                            None => (None, Vec2::ZERO),
                        };
                        if tex.is_none() {
                            self.request_thumb(real, true); // 보이는 셀 우선
                            ui.ctx().request_repaint(); // 회색이면 채워질 때까지 재요청·재그리기(자가복구)
                        }
                        let it = &self.items[real];
                        let info = ThumbInfo {
                            label: it.entry.label,
                            raw_badge: it.entry.shows_raw_badge(),
                            raw_only: it.entry.has_raw && !it.entry.has_image,
                            active: fi == cur,
                            focused: false,
                            selected: false,
                            stars: it.entry.stars,
                            tag: it.entry.tag,
                        };
                        draw_thumb(ui, rect, tex, tsize, &info, self.badge_scale());
                        if resp.clicked() {
                            self.index = fi;
                        }
                    }
                });
            });
    }

    fn ui_single(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let rect = ui.max_rect();
        let real = match self.current_real() {
            Some(r) => r,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(tr(lang, "표시할 항목이 없습니다")).color(theme::INK3));
                });
                return;
            }
        };
        self.photo_view(ui, rect, real);
        if !self.has_modal() {
            let suffix = if self.full_raw { "ORIG · sRGB" } else { "FIT · sRGB" };
            self.paint_hud(ui, rect, real, suffix);
            self.ui_map_overlay(ui, rect, real);
        }
    }

    /// 사진 영역: 줌/이동 인터랙션 + 그리기.
    /// - 클릭(드래그 아님): 창맞춤(fit) ↔ 1:1 토글
    /// - Ctrl+휠 / 터치패드 핀치: 연속 확대·축소(커서 기준)
    /// - 드래그: 확대 상태에서 이동(pan)
    ///
    /// 프리뷰가 있으면 그것을, 없으면 썸네일을 열화 표시, 둘 다 없으면 "디코딩 중".
    fn photo_view(&mut self, ui: &mut egui::Ui, area: Rect, real: usize) {
        let lang = self.lang;
        // 표시할 항목이 바뀌면 줌 상태 리셋(fit).
        if self.zoom_for != Some(real) {
            self.fit = true;
            self.pan = Vec2::ZERO;
            self.zoom_for = Some(real);
            self.last_view_size = None; // 항목이 바뀜 → 해상도 추적 리셋(#48)
            self.af_zoom_pending = false;
        }
        let texsize = self
            .cache
            .get(real)
            .map(|(t, _, s)| (t, s))
            .or_else(|| self.thumbs.get(real).map(|(t, _, s)| (t, s)));
        let (tex, size) = match texsize {
            Some(v) => v,
            None => {
                ui.painter()
                    .text(area.center(), Align2::CENTER_CENTER, tr(lang, "디코딩 중…"), mono(12.0), theme::INK3);
                ui.ctx().request_repaint();
                return;
            }
        };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        let resp = ui.interact(area, ui.id().with("photoview"), Sense::click_and_drag());

        // 창맞춤 배율.
        let avail = Vec2::new((area.width() - 32.0).max(1.0), (area.height() - 32.0).max(1.0));
        let fit_scale = (avail.x / size.x).min(avail.y / size.y);
        if self.fit {
            self.zoom = fit_scale;
            self.pan = Vec2::ZERO;
        }
        let min_zoom = fit_scale.min(1.0);
        let max_zoom = 8.0_f32.max(fit_scale); // 최소 8x(작은 이미지도 확대 가능)

        // #48: 같은 항목에서 표시 해상도가 바뀌면(ORIG 로드/언로드) 화면상 배율·보던 위치를 유지한다.
        // zoom은 화면픽셀/이미지픽셀이라 해상도가 K배면 같은 zoom이 K배 확대로 보인다 → zoom을 1/K로
        // 맞춰 scaled(=size*zoom, 화면상 크기)를 보존하면 pan(화면픽셀)도 그대로 들어맞는다.
        if !self.fit {
            if let Some(prev) = self.last_view_size {
                if prev.x > 0.0 && (prev.x - size.x).abs() > 0.5 {
                    self.zoom = (self.zoom * prev.x / size.x).clamp(min_zoom, max_zoom);
                }
            }
        }
        self.last_view_size = Some(size);

        // 줌 입력: Ctrl+휠 또는 터치패드 핀치(커서 기준 확대).
        if resp.hovered() {
            let (scroll_y, zd, mods, ptr) = ui.input(|i| {
                (
                    i.raw_scroll_delta.y,
                    i.zoom_delta(),
                    i.modifiers,
                    i.pointer.hover_pos(),
                )
            });
            let mut nz = self.zoom;
            if (zd - 1.0).abs() > 1e-4 {
                nz *= zd; // 핀치(또는 egui가 접어 넣은 Ctrl+휠)
            } else if (mods.ctrl || mods.command) && scroll_y.abs() > 0.0 {
                nz *= (scroll_y * 0.004).exp(); // Ctrl+휠 → 매끄러운 곱셈 줌
            }
            nz = nz.clamp(min_zoom, max_zoom);
            if (nz - self.zoom).abs() > f32::EPSILON {
                if let Some(p) = ptr {
                    // 커서 아래 지점이 고정되도록 pan 보정.
                    let a = p - area.center();
                    let k = nz / self.zoom;
                    self.pan = a * (1.0 - k) + self.pan * k;
                }
                self.zoom = nz;
                self.fit = nz <= fit_scale * 1.001;
            }
        }

        // 클릭(드래그 아님): fit ↔ 1:1.
        if resp.clicked() {
            if self.fit {
                self.fit = false;
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            } else {
                self.fit = true;
            }
        }
        // 드래그: 이동.
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }

        // pan 클램프(이미지가 영역보다 클 때만 이동 허용 — 화면 밖으로 날아가지 않게).
        let scaled = size * self.zoom;
        // #49: AF 중심 확대 요청 소비 — 측거점(합초 우선)이 화면 중앙에 오도록 pan을 설정한다.
        // 표시좌표(0..1)의 (cx,cy)가 area.center()에 오려면 pan = scaled * (0.5 - c). 이후 클램프로
        // 가장자리 측거점이어도 화면 밖으로 벗어나지 않게 보정된다.
        if self.af_zoom_pending {
            self.af_zoom_pending = false;
            if let Some((cx, cy)) = self
                .items
                .get(real)
                .and_then(|it| it.af.as_ref().map(|af| (af, it.orient.unwrap_or(1))))
                .and_then(|(af, orient)| af_focus_center(af, orient))
            {
                self.pan = Vec2::new(scaled.x * (0.5 - cx as f32), scaled.y * (0.5 - cy as f32));
            }
        }
        let max_px = ((scaled.x - area.width()) * 0.5).max(0.0);
        let max_py = ((scaled.y - area.height()) * 0.5).max(0.0);
        self.pan.x = self.pan.x.clamp(-max_px, max_px);
        self.pan.y = self.pan.y.clamp(-max_py, max_py);

        // 그리기(영역으로 클립).
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        let target = Rect::from_center_size(area.center() + self.pan, scaled);
        ui.painter().with_clip_rect(area).image(tex, target, uv, Color32::WHITE);

        // AF 포인트 오버레이(#37): 측거점 사각형을 사진 위에 그린다. 좌표는 센서(미회전)
        // 기준 0..1 정규화이므로 EXIF orientation으로 표시 좌표계로 돌린다. 데이터 없는
        // 바디·MF는 af가 None → 조용히 미표시.
        if self.show_af {
            // AF·orientation은 백그라운드 메타 로더(request_meta)가 채운다 — 도착 전엔 미표시.
            if let Some(it) = self.items.get(real) {
                if let Some(af) = &it.af {
                    let orient = it.orient.unwrap_or(1);
                    let p = ui.painter().with_clip_rect(area);
                    for pt in &af.points {
                        let (cx, cy, w, h) = af_display_coords(pt, orient);
                        let center = Pos2::new(
                            target.min.x + cx as f32 * target.width(),
                            target.min.y + cy as f32 * target.height(),
                        );
                        // 크기 미기록(0.0)은 고정 비율 박스 폴백(구형 캐논 단일점·소니 위치점).
                        let bw = if w > 0.0 { w as f32 * target.width() } else { 0.045 * target.width().min(target.height()) };
                        let bh = if h > 0.0 { h as f32 * target.height() } else { bw };
                        let r = Rect::from_center_size(center, Vec2::new(bw.max(8.0), bh.max(8.0)));
                        // 합초점은 초록 굵게, 선택만 된 점은 밝게, 나머지는 흐리게(DPP 스타일).
                        let (color, width) = if pt.in_focus {
                            (theme::OK, 2.0)
                        } else if pt.selected {
                            (theme::INK2, 1.2)
                        } else {
                            (theme::INK4, 1.0)
                        };
                        p.rect_stroke(r, Rounding::same(2.0), Stroke::new(width, color));
                    }
                }
            }
        }

        // 이동 가능하면 손 커서.
        if !self.fit && (max_px > 0.0 || max_py > 0.0) {
            let cur = if resp.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(cur);
        }
    }

    fn paint_hud(&self, ui: &egui::Ui, area: Rect, real: usize, counter_suffix: &str) {
        let lang = self.lang;
        let it = &self.items[real];
        let f = self.filtered();
        // TL: 라벨 + 파일명.
        let name = it.entry.display.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut tl = area.left_top() + Vec2::new(20.0, 22.0);
        let chip = format!("[{}]", it.entry.label.name(lang));
        hud_text(ui, tl, Align2::LEFT_TOP, &chip, mono(12.0), theme::label_color(it.entry.label));
        tl.x += 64.0;
        let badge = if it.entry.shows_raw_badge() { "  +RAW" } else { "" };
        hud_text(ui, tl, Align2::LEFT_TOP, &format!("{name}{badge}"), mono(12.0), theme::INK);
        // 별점(#23): 라벨/파일명 아래 줄에 채워진 별로 표시.
        if it.entry.stars > 0 {
            let stars_str = "★".repeat(it.entry.stars.min(5) as usize);
            hud_text(ui, area.left_top() + Vec2::new(20.0, 42.0), Align2::LEFT_TOP, &stars_str, mono(13.0), theme::HOLD);
        }

        // TR: 카운터.
        let tr = area.right_top() + Vec2::new(-18.0, 14.0);
        hud_text(ui, tr, Align2::RIGHT_TOP, &format!("{:03} / {}", (self.index + 1).min(f.len().max(1)), f.len()), mono(30.0), theme::INK);
        hud_text(ui, tr + Vec2::new(0.0, 34.0), Align2::RIGHT_TOP, counter_suffix, mono(10.0), theme::INK3);

        // BL: EXIF.
        if self.show_exif {
            if let Some(ex) = &it.exif {
                let mut y = area.bottom() - 16.0;
                let lines = exif_lines(ex);
                for line in lines.iter().rev() {
                    hud_text(ui, Pos2::new(area.left() + 16.0, y), Align2::LEFT_BOTTOM, line, mono(12.0), theme::INK2);
                    y -= 18.0;
                }
            }
        }

        // BR: 히스토그램.
        if self.show_hist {
            if let Some(h) = self.histo.get(&real) {
                let w = 232.0_f32.min(area.width() * 0.35);
                let hh = 56.0;
                let hr = Rect::from_min_size(
                    Pos2::new(area.right() - w - 16.0, area.bottom() - hh - 16.0),
                    Vec2::new(w, hh),
                );
                widgets::draw_histogram(ui, hr, &h.bins, h.max);
            }
        }

    }

    fn ui_grid(&mut self, ui: &mut egui::Ui) {
        let f = self.filtered();
        let cols = self.grid_cols.clamp(4, 12);
        let cur = self.index.min(f.len().saturating_sub(1));
        let gap = 6.0;
        let avail = ui.available_width();
        let cell_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(40.0);
        let cell_h = (cell_w * 0.7).clamp(80.0, 160.0);
        let row_h = cell_h + gap;
        let rows = f.len().div_ceil(cols);
        let avail_h = ui.available_height();
        // 키보드 내비로 예약된 스크롤: 대상 행이 화면 밖일 때만 가장자리 정렬로 따라간다
        // (화면 안 작은 이동에서는 튀지 않게).
        let mut area = egui::ScrollArea::vertical();
        if let Some(target_row) = self.grid_scroll_to.take() {
            let vis = self.grid_visible_rows.clone();
            if vis.is_empty() || target_row < vis.start || target_row >= vis.end {
                let off = if target_row < vis.start {
                    target_row as f32 * row_h // 위로: 상단 정렬
                } else {
                    ((target_row + 1) as f32 * row_h - avail_h).max(0.0) // 아래로: 하단 정렬
                };
                let max_off = (rows as f32 * row_h - avail_h).max(0.0);
                area = area.vertical_scroll_offset(off.clamp(0.0, max_off));
            }
        }
        // 가상화: 보이는 행만 생성·디코딩(수천 장에서도 매 프레임 비용이 일정).
        area.show_rows(ui, row_h, rows, |ui, row_range| {
            self.grid_visible_rows = row_range.clone();
            ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
            // 가시 범위 위아래 여유분(±2행)까지 미리 우선 요청 → 스크롤 가장자리·부분행 회색 방지.
            let pf_start = row_range.start.saturating_sub(2);
            let pf_end = (row_range.end + 2).min(rows);
            for row in pf_start..pf_end {
                for c in 0..cols {
                    let fi = row * cols + c;
                    if fi < f.len() && !self.thumbs.contains(f[fi]) {
                        self.request_thumb(f[fi], true);
                    }
                }
            }
            for row in row_range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let fi = row * cols + c;
                        if fi >= f.len() {
                            break;
                        }
                        let real = f[fi];
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(cell_w, cell_h), Sense::click());
                        let (tex, tsize) = match self.thumbs.get(real) {
                            Some((t, _, s)) => (Some(t), s),
                            None => (None, Vec2::ZERO),
                        };
                        if tex.is_none() {
                            self.request_thumb(real, true); // 보이는 셀 우선
                            ui.ctx().request_repaint(); // 회색이면 채워질 때까지 재요청·재그리기(자가복구)
                        }
                        let it = &self.items[real];
                        let info = ThumbInfo {
                            label: it.entry.label,
                            raw_badge: it.entry.shows_raw_badge(),
                            raw_only: it.entry.has_raw && !it.entry.has_image,
                            active: fi == cur,
                            focused: false,
                            selected: self.selected.contains(&real),
                            stars: it.entry.stars,
                            tag: it.entry.tag,
                        };
                        draw_thumb(ui, rect, tex, tsize, &info, self.badge_scale());
                        if resp.clicked() {
                            // Ctrl/⌘+클릭 = 토글, Shift+클릭 = 앵커~클릭 범위, 그냥 클릭 = 단일.
                            let m = ui.input(|i| i.modifiers);
                            if m.command {
                                if !self.selected.insert(real) {
                                    self.selected.remove(&real);
                                }
                                self.sel_anchor = Some(fi);
                            } else if m.shift {
                                let a = self.sel_anchor.unwrap_or(fi);
                                let (lo, hi) = (a.min(fi), a.max(fi));
                                for k in lo..=hi {
                                    if k < f.len() {
                                        self.selected.insert(f[k]);
                                    }
                                }
                            } else {
                                self.selected.clear();
                                self.sel_anchor = Some(fi);
                            }
                            self.index = fi;
                        }
                        if resp.double_clicked() {
                            self.index = fi;
                            self.view = ViewMode::Single;
                        }
                    }
                });
            }
        });
    }

    /// GPS 미니 지도 패널(#38). 우상단 코너, 현재 항목에 GPS가 있을 때만. 합성은
    /// 백그라운드 스레드(타일: 디스크 캐시 → OSM), 시작은 사진 디코딩이 유휴일 때만.
    /// 클릭하면 브라우저에서 OSM으로 크게 보기, ±로 줌(3~17).
    fn ui_map_overlay(&mut self, ui: &mut egui::Ui, area: Rect, real: usize) {
        if !self.show_map {
            self.map_state = None;
            return;
        }
        let lang = self.lang;
        // EXIF는 백그라운드 메타 로더(request_meta)가 채운다 — GPS가 도착하면 그 프레임부터 표시.
        let Some(gps) = self.items.get(real).and_then(|it| it.exif.as_ref()).and_then(|e| e.gps) else {
            self.map_state = None;
            return; // GPS 없는 사진은 자동 미표시.
        };
        const MW: u32 = 220;
        const MH: u32 = 150;
        let zoom = self.map_zoom;
        // 항목·줌이 바뀌면 새로 합성. 단, 시작은 디코딩 유휴 시에만(사진 표시가 우선, #33 패턴).
        let stale = self.map_state.as_ref().map(|m| m.real != real || m.zoom != zoom).unwrap_or(true);
        if stale {
            let idle = self.pending_preview.is_empty() && self.pending_thumb.is_empty();
            if idle {
                let (tx, rx) = crossbeam_channel::bounded(1);
                let cache = crate::map::cache_dir();
                let (lat, lon) = (gps.lat, gps.lon);
                std::thread::spawn(move || {
                    let _ = tx.send(crate::map::compose(lat, lon, zoom, MW, MH, &cache));
                });
                self.map_state = Some(MapState {
                    real,
                    zoom,
                    lat: gps.lat,
                    lon: gps.lon,
                    rx: Some(rx),
                    tex: None,
                    failed: false,
                });
            } else {
                // 디코딩 끝나면 시작하도록 다음 프레임 재확인.
                ui.ctx().request_repaint_after(Duration::from_millis(200));
                if self.map_state.is_none() {
                    return;
                }
            }
        }
        // 결과 수신 → 텍스처 업로드(네트워크 스레드는 egui를 못 깨우므로 폴링).
        if let Some(st) = &mut self.map_state {
            if let Some(rx) = &st.rx {
                match rx.try_recv() {
                    Ok(Some(img)) => {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.rgba);
                        st.tex = Some(ui.ctx().load_texture(
                            format!("map_{}_{}", st.real, st.zoom),
                            ci,
                            egui::TextureOptions::LINEAR,
                        ));
                        st.rx = None;
                    }
                    Ok(None) => {
                        st.failed = true; // 오프라인 + 캐시 없음.
                        st.rx = None;
                    }
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        st.failed = true;
                        st.rx = None;
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => {
                        ui.ctx().request_repaint_after(Duration::from_millis(300));
                    }
                }
            }
        }
        let Some(st) = &self.map_state else { return };
        let (tex_id, failed, lat, lon) = (st.tex.as_ref().map(|t| t.id()), st.failed, st.lat, st.lon);

        // 패널: 우상단 카운터 아래.
        let panel = Rect::from_min_size(
            Pos2::new(area.right() - MW as f32 - 18.0, area.top() + 64.0),
            Vec2::new(MW as f32, MH as f32),
        );
        let p = ui.painter();
        p.rect_filled(panel.expand(4.0), Rounding::same(6.0), Color32::from_black_alpha(150));
        if let Some(tid) = tex_id {
            p.image(tid, panel, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            // 위치 마커(패널 정중앙 = 촬영 좌표).
            let c = panel.center();
            p.circle_filled(c, 5.0, theme::REJECT);
            p.circle_stroke(c, 5.0, Stroke::new(1.5, Color32::WHITE));
            // OSM attribution(타일 정책 필수). 지도 위 가독성용 어두운 띠.
            let att = panel.with_min_y(panel.bottom() - 13.0);
            p.rect_filled(att, Rounding::ZERO, Color32::from_black_alpha(120));
            p.text(
                att.right_center() + Vec2::new(-4.0, 0.0),
                Align2::RIGHT_CENTER,
                "© OpenStreetMap contributors",
                mono(8.0),
                Color32::from_gray(0xdd),
            );
        } else {
            let msg = if failed { tr(lang, "지도를 불러올 수 없음 (오프라인?)") } else { tr(lang, "지도 로딩…") };
            p.text(panel.center(), Align2::CENTER_CENTER, msg, mono(10.0), theme::INK3);
        }
        // 좌표 라벨(패널 아래).
        p.text(
            panel.left_bottom() + Vec2::new(0.0, 8.0),
            Align2::LEFT_TOP,
            format!("{:.5}, {:.5}", lat, lon),
            mono(9.0),
            theme::INK3,
        );
        // 클릭 → 브라우저로 크게 보기.
        let resp = ui.interact(panel, ui.id().with("map_panel"), Sense::click());
        if resp.clicked() {
            open_url(&crate::map::osm_url(lat, lon, zoom.max(15)));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // 줌 ±(패널 좌상단).
        for (i, (label, delta)) in [("+", 1i16), ("−", -1i16)].iter().enumerate() {
            let br = Rect::from_min_size(panel.left_top() + Vec2::new(4.0 + i as f32 * 22.0, 4.0), Vec2::splat(18.0));
            let r = ui.interact(br, ui.id().with(("map_zoom_btn", i)), Sense::click());
            let pb = ui.painter();
            pb.rect_filled(br, Rounding::same(4.0), Color32::from_black_alpha(170));
            pb.text(br.center(), Align2::CENTER_CENTER, *label, mono(12.0), if r.hovered() { theme::INK } else { theme::INK2 });
            if r.clicked() {
                self.map_zoom = (self.map_zoom as i16 + delta).clamp(3, 17) as u8;
            }
        }
    }

    fn ui_fullscreen(&mut self, ctx: &egui::Context) {
        let bg = self.photo_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                if let Some(real) = self.current_real() {
                    self.photo_view(ui, rect, real);
                    self.paint_hud(ui, rect, real, "FULLSCREEN · ESC");
                    self.ui_map_overlay(ui, rect, real);
                }
            });
    }


}





