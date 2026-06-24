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

/// 전송 다이얼로그 리네임 모드(#26). 프리셋 3종 + 자유 템플릿.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameMode {
    Off,    // 원본 이름 유지
    Seq,    // 순번 {seq:03}
    Grade,  // 별점등급 {gradeseq} (A1,B1…)
    Custom, // 자유 템플릿
}

#[derive(Clone)]
struct TransferDialogState {
    labels: Vec<Label>,
    /// 전송 대상 별점 집합(1~5). 라벨과 합집합(OR)으로 묶인다(#23).
    stars: Vec<u8>,
    /// 전송 대상 컬러 태그 집합(#27). 라벨·별점과 합집합(OR).
    tags: Vec<ColorTag>,
    action: Action,
    companions: Companions,
    split_by_label: bool,
    /// 태그별 하위폴더 분기(#27).
    split_by_tag: bool,
    conflict: ConflictPolicy,
    dest: String,
    /// 파일명 변경(#26).
    rename_mode: RenameMode,
    rename_template: String,
    rename_numbering: Numbering,
}

impl TransferDialogState {
    /// 다이얼로그 상태 → 전송 리네임 규칙(#26). Off면 None.
    fn rename_rule(&self) -> Option<RenameRule> {
        match self.rename_mode {
            RenameMode::Off => None,
            RenameMode::Seq => Some(RenameRule {
                template: "{seq:03}".into(),
                numbering: Numbering::Order,
            }),
            RenameMode::Grade => Some(RenameRule {
                template: "{gradeseq}".into(),
                numbering: Numbering::GradeGrouped,
            }),
            RenameMode::Custom => Some(RenameRule {
                template: self.rename_template.clone(),
                numbering: self.rename_numbering,
            }),
        }
    }
}

impl Default for TransferDialogState {
    fn default() -> Self {
        TransferDialogState {
            labels: vec![Label::Pick],
            stars: Vec::new(),
            tags: Vec::new(),
            action: Action::Copy,
            companions: Companions::Both,
            split_by_label: false,
            split_by_tag: false,
            conflict: ConflictPolicy::AutoIncrement,
            dest: String::new(),
            rename_mode: RenameMode::Off,
            rename_template: "{gradeseq}_{orig}".into(),
            rename_numbering: Numbering::GradeGrouped,
        }
    }
}

/// 폴더 자동 분류 다이얼로그 상태(#34). 셀렉 전송과 별개로, 폴더 안의 사진을 기준별로
/// 하위폴더에 나눠 담는다.
#[derive(Clone)]
struct OrganizeDialogState {
    key: OrganizeKey,
    action: Action,
    /// 분류 결과를 담을 루트(기본: 현재 폴더 — in-place로 하위폴더 생성).
    dest: String,
    conflict: ConflictPolicy,
}

impl Default for OrganizeDialogState {
    fn default() -> Self {
        OrganizeDialogState {
            key: OrganizeKey::Date,
            action: Action::Move, // 이슈 #34는 "폴더분류/이동"이 기본 의도.
            dest: String::new(),
            conflict: ConflictPolicy::AutoIncrement,
        }
    }
}

/// 백그라운드 파일 작업(전송/정리)에서 메인 스레드로 보내는 메시지(#35).
enum JobMsg {
    Progress(Progress),
    Done(TransferReport),
}

/// 완료 후 폴더를 어떻게 다시 열지(#35/#34).
#[derive(Clone, Copy)]
enum ReopenMode {
    /// 현재 폴더를 현재 설정대로 다시 스캔(Move 전송 후 사라진 항목 정리, #24).
    Current,
    /// 분류 결과(생성된 하위폴더)를 보도록 하위 폴더 포함으로 다시 연다(#34).
    Recursive,
}

/// 진행 중인 백그라운드 파일 작업(전송/정리)(#35). 별도 스레드에서 실행하고 진행 상황을
/// 채널로 받아 프로그레스바로 표시한다 — 큰 폴더·느린 드라이브에서 UI가 멈춘 것처럼 보이지 않게.
struct ProgressJob {
    /// 다이얼로그 제목(tr 키, 한국어 원문).
    title: &'static str,
    rx: crossbeam_channel::Receiver<JobMsg>,
    cancel: Arc<AtomicBool>,
    latest: Progress,
    /// 완료 후 폴더 재오픈 방식.
    reopen: Option<ReopenMode>,
    /// 결과 다이얼로그의 "대상 폴더 열기"에 쓸 경로.
    dest: Option<PathBuf>,
}

/// AI 컬링(#50) 백그라운드 채점 완료 메시지. 진행률은 공유 원자 카운터(`AiCullJob::progress`)로
/// 전달하므로(워커가 여러 개라 메시지 순서가 뒤섞이지 않게), 채널은 최종 결과만 보낸다.
enum AiCullMsg {
    /// 채점 완료. (원본 items 인덱스, CV 판정, CLIP-IQA P(good)) 목록.
    Done(Vec<(usize, rawblow_core::quality::Verdict, Option<f32>)>),
}

/// 진행 중인 AI 컬링 채점(#50). 워커 풀(여러 스레드)이 디코딩+채점을 병렬로 수행하고,
/// 코디네이터 스레드가 전원 합류 후 결과를 한 번에 보낸다. 진행 개수는 `progress` 원자로 읽는다.
/// 백그라운드 비차단 실행: 채점 중에도 앱은 조작 가능하되, 결과가 덮어쓸 분류축(`target`)은
/// 잠그고, 폴더가 바뀌면(`generation` 불일치) 어긋난 인덱스의 결과를 폐기한다.
struct AiCullJob {
    rx: crossbeam_channel::Receiver<AiCullMsg>,
    cancel: Arc<AtomicBool>,
    /// 완료한 이미지 수(워커들이 fetch_add로 증가, 메인이 매 프레임 읽어 프로그레스바에 표시).
    progress: Arc<std::sync::atomic::AtomicUsize>,
    total: usize,
    /// 작업 시작 시점의 폴더 세대. 완료 시 현재 세대와 다르면 인덱스가 무효 → 결과 폐기.
    generation: u64,
    /// 이 작업이 결과를 배정할 분류축. 채점 중 이 축의 수동 편집을 막는다.
    target: AiCullTarget,
}

/// CLIP-IQA 모델 자동 다운로드(#50) 진행 메시지.
#[cfg(feature = "model-download")]
enum ModelDlMsg {
    /// (다운로드된 바이트, 전체 바이트 — 0이면 미확인).
    Progress(u64, u64),
    /// 다운로드+검증 완료. Err이면 오류 메시지.
    Done(Result<(), String>),
}

/// 진행 중인 모델 다운로드.
#[cfg(feature = "model-download")]
struct ModelDlJob {
    rx: crossbeam_channel::Receiver<ModelDlMsg>,
    label: String,
    done: u64,
    total: u64,
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
    jump_exact: bool,

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
            jump_exact: false,
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

    /// 현재 위치 주변의 **제한된 윈도우**만 백그라운드(최저 우선순위)로 디스크 캐시에 미리
    /// 디코딩한다(#perf). 폴더 전체(수천 장)를 한꺼번에 큐에 넣으면 느린 디스크가 포화돼
    /// 보이는 셀(prio)이 진행 중인 배경 읽기 뒤에서 굶어, 썸네일이 수십 초~수 분 지연됐다.
    /// 진행 방향(전방 편향) 일정 개수만 데우고, 윈도우는 이동에 따라 슬라이드한다. GPU 업로드
    /// 없이 디스크 저장만 하므로 VRAM·메인 스레드 부담은 없다. 이미 캐시/대기 중인 건 건너뛴다.
    fn request_prefetch_window(&mut self) {
        const AHEAD: usize = 80; // 진행 방향으로 데울 개수
        const BEHIND: usize = 16; // 되돌아가기 대비 소량
        let f = self.filtered();
        if f.is_empty() {
            return;
        }
        let cur = self.index.min(f.len() - 1);
        let lo = cur.saturating_sub(BEHIND);
        let hi = (cur + AHEAD).min(f.len() - 1);
        for fi in lo..=hi {
            let real = f[fi];
            // 이미 GPU 캐시에 있거나, 곧 prio로 처리되거나, 이미 프리페치 대기면 건너뛴다.
            if self.thumbs.contains(real)
                || self.pending_thumb.contains(&real)
                || !self.pending_prefetch.insert(real)
            {
                continue;
            }
            let path = self.items[real].entry.display.clone();
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

    /// 현재 항목의 메타(EXIF, show_af면 AF·orientation까지)를 백그라운드로 요청한다.
    /// UI 스레드에서 파일을 읽지 않는다 — NAS에서 프리픽스 read가 수백 ms 걸려도
    /// 사진 넘김이 멈추지 않게(표시 먼저, 오버레이는 도착하는 대로). 동시 1건만 띄워
    /// 빠른 연속 넘김 때 건너뛴 항목들이 큐로 쌓여 현재 항목을 막는 일을 차단한다.
    fn request_meta(&mut self, ctx: &egui::Context) {
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
                let af = need_af.then(|| {
                    (
                        rawblow_core::af::parse_af(&path),
                        rawblow_core::meta::orientation(&path),
                    )
                });
                (exif, af)
            }));
            let (exif, af) =
                parsed.unwrap_or((need_exif.then_some(None), need_af.then(|| (None, 1))));
            let _ = tx.send(MetaResult { generation, real, exif, af });
        });
        // 워커 스레드는 egui를 못 깨우므로 결과 수신을 짧은 리페인트로 폴링.
        ctx.request_repaint_after(Duration::from_millis(80));
    }

    /// 메타 로더 결과 반영. 폴더가 바뀐(generation 불일치) 결과는 버린다.
    fn drain_meta(&mut self, ctx: &egui::Context) {
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
    fn request_preload(&mut self) {
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
            for fi in lo..=hi {
                if fi != cur {
                    self.request_preview(f[fi], Some(PREVIEW_EDGE), false, false);
                }
            }
        }

        // 썸네일은 보이는 셀에 대해서만 주문형으로 디코딩한다(그리드·필름스트립이 직접
        // 가시 범위를 우선 요청). 폴더 전체를 미리 GPU 텍스처로 올리면 VRAM이 수GB로 폭증해
        // 업로드 도중 메인 스레드가 GPU에서 멈추는(행) 문제가 있어 사전 일괄 채우기는 폐지.
    }

    /// 프리뷰 디코딩 요청(캐시/진행/실패 가드 포함).
    fn request_preview(&mut self, real: usize, max_edge: Option<u32>, full: bool, prio: bool) {
        if prio {
            // 현재 보고 있는 이미지: 일시적으로 실패 마킹됐어도 항상 재시도(디코딩은 사실상 100%).
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
    fn request_thumb(&mut self, real: usize, prio: bool) {
        if self.thumbs.contains(real) {
            return; // 이미 캐시됨
        }
        if prio {
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

    /// 워커 결과를 텍스처로 업로드.
    fn drain_results(&mut self, ctx: &egui::Context) {
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
                // 현재 세대일 때만 제거: 폴더 전환 직후 도착한 옛 세대 프리페치 결과가
                // 새 폴더의 동일 id pending을 잘못 지우지 않게 한다(그 항목 프리페치 누락 방지).
                if res.generation == self.generation {
                    self.pending_prefetch.remove(&res.id);
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
                }
            } else if res.thumb {
                // 디코딩 실패는 마킹해 무한 재시도(=keep-alive 무한 루프)를 막는다.
                self.failed_thumb.insert(res.id);
            } else {
                self.failed_preview.insert(res.id);
            }
        }
        // 업로드 상한에 걸려 남은 결과가 있으면 곧바로 다음 프레임에서 이어 비운다.
        if !self.worker.rx.is_empty() {
            ctx.request_repaint();
        }
    }

    /// 진행 중인 컬링이 결과를 배정할 분류축(잠긴 축). 그 축의 수동 편집을 막는다(#50).
    fn cull_locked_target(&self) -> Option<AiCullTarget> {
        self.ai_cull.as_ref().map(|j| j.target)
    }

    /// 잠긴 축 편집 시도 시 토스트로 안내. true면 호출부가 편집을 건너뛴다.
    fn cull_axis_locked(&mut self, axis: AiCullTarget) -> bool {
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
        if self.progress.is_some() {
            self.ui_progress(ctx);
        } else if {
            #[cfg(feature = "model-download")]
            { self.model_dl.is_some() }
            #[cfg(not(feature = "model-download"))]
            { false }
        } {
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

    fn open_transfer(&mut self) {
        // 컬링 중에는 파일 이동(폴더 재스캔으로 인덱스 무효화)을 막는다.
        if self.ai_cull.is_some() {
            self.toast = Some((tr(self.lang, "AI 컬링이 끝난 뒤 전송할 수 있습니다").into(), Instant::now()));
            return;
        }
        let mut st = TransferDialogState::default();
        if let Some(folder) = &self.folder {
            st.dest = format!("{}_selected", folder.to_string_lossy());
        }
        self.transfer = Some(st);
    }

    /// 폴더 자동 분류 다이얼로그를 연다(#34). 기본 대상은 현재 폴더(in-place 하위폴더 생성).
    fn open_organize(&mut self) {
        if self.ai_cull.is_some() {
            self.toast = Some((tr(self.lang, "AI 컬링이 끝난 뒤 정리할 수 있습니다").into(), Instant::now()));
            return;
        }
        let mut st = OrganizeDialogState::default();
        if let Some(folder) = &self.folder {
            st.dest = folder.to_string_lossy().to_string();
        }
        self.organize = Some(st);
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

    /// 새 릴리즈 안내(#33): 실행 후 로딩이 끝나고 유휴 상태일 때 **세션당 1회** 백그라운드로
    /// GitHub 최신 릴리즈를 확인한다(설정·시작 프롬프트 없이 자동). 결과는 채널로 받아 새 버전이면
    /// `update_available`에 담아 좌측 레일 배너로 알린다. 네트워크 실패·rate limit은 조용히 무시.
    fn maybe_check_update(&mut self, ctx: &egui::Context) {
        // 진행 중이면 결과 수신.
        if let Some(rx) = &self.update_rx {
            match rx.try_recv() {
                Ok(msg) => {
                    self.update_available = msg;
                    self.update_rx = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.update_rx = None,
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    // 결과 올 때까지 가끔 깨운다(네트워크 스레드는 egui를 못 깨움).
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
            }
        }
        if self.update_checked {
            return;
        }
        // 유휴 조건: 초기 디코딩/프리페치·진행 작업·모달이 모두 없을 때 한 번만.
        let idle = self.pending_preview.is_empty()
            && self.pending_thumb.is_empty()
            && self.pending_prefetch.is_empty()
            && self.progress.is_none()
            && !self.has_modal();
        if !idle {
            return;
        }
        self.update_checked = true;
        let (tx, rx) = crossbeam_channel::unbounded::<Option<String>>();
        std::thread::spawn(move || {
            let res = fetch_latest_release_tag().and_then(|tag| newer_version(&tag));
            let _ = tx.send(res);
        });
        self.update_rx = Some(rx);
        ctx.request_repaint_after(Duration::from_secs(1));
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
                    p.text(Pos2::new(rect.right() - 36.0, rect.center().y), Align2::RIGHT_CENTER, &n.to_string(), mono(11.0), theme::INK);
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
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, &format!("★{n}"), prop(10.5), col);
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
                            p.rect(ai_rect, Rounding::same(6.0), theme::BG3, Stroke::new(1.0, theme::ACCENT));
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
                            let border = if ai_enabled { theme::ACCENT } else { theme::LINE2 };
                            p.rect(ai_rect, Rounding::same(6.0), fill, Stroke::new(1.0, border));
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
                    for fi in start..end {
                        let real = f[fi];
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
        let rows = (f.len() + cols - 1) / cols;
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

    // ── 전송 다이얼로그 ──────────────────────────────────
    fn ui_transfer_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut st = self.transfer.clone().unwrap();
        let mut do_start = false;
        let mut do_cancel = false;
        let (pick, hold, reject, unrated) = self.counts();
        let star_cnt = self.star_counts();
        let tag_cnt = self.tag_counts();
        let tag_names: Vec<String> =
            ColorTag::ALL.iter().map(|t| self.cfg.tag_label(*t, lang)).collect();

        // 뒤 화면 어둡게(dim) + 클릭 차단. Middle 레이어 → 패널 위, 카드(Foreground) 아래.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("transfer_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        // 미리보기 계획(footer 통계).
        let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
        let plan = transfer::plan(&TransferRequest {
            entries: &entries,
            labels: st.labels.clone(),
            stars: st.stars.clone(),
            tags: st.tags.clone(),
            action: st.action,
            companions: st.companions,
            dest: PathBuf::from(&st.dest),
            split_by_label: st.split_by_label,
            split_by_tag: st.split_by_tag,
            conflict: st.conflict,
            rename: st.rename_rule(),
        });
        let raw_n = plan.iter().filter(|(p, _, _)| rawblow_core::model::kind_of(p) == Some(rawblow_core::model::Kind::Raw)).count();
        let img_n = plan.len().saturating_sub(raw_n);

        // 중앙 모달 카드. 너비 660 고정이라 left를 화면중앙-330으로 두면 가로 정중앙
        // (Area::anchor는 이전 프레임 크기 기반이라 수렴이 안 돼 fixed_pos로 직접 배치).
        let card_pos = egui::Pos2::new(screen.center().x - 330.0, (screen.center().y - 300.0).max(8.0));
        egui::Area::new(egui::Id::new("transfer_card"))
            .order(egui::Order::Foreground)
            .fixed_pos(card_pos)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(Stroke::new(1.0, theme::LINE2))
                    .rounding(10.0)
                    .show(ui, |ui| {
                        ui.set_min_width(660.0);
                        ui.set_max_width(660.0);
                        // ── HEADER ──
                        egui::Frame::none()
                            .inner_margin(egui::Margin { left: 22.0, right: 22.0, top: 18.0, bottom: 14.0 })
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                ui.horizontal(|ui| {
                                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                                    send_icon(ui.painter(), ir.center(), 14.0, theme::ACCENT);
                                    ui.add_space(9.0);
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(tr(lang, "파일 전송")).font(prop(15.0)).color(theme::INK));
                                        ui.label(egui::RichText::new(tr(lang, "선택한 라벨·별점의 파일을 복사/이동 · RAW 페어 처리")).font(mono(10.5)).color(theme::INK3));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        let c = xr.center();
                                        let col = if xresp.hovered() { theme::INK } else { theme::INK3 };
                                        ui.painter().line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(4.0, 4.0)], Stroke::new(1.5, col));
                                        ui.painter().line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(-4.0, 4.0)], Stroke::new(1.5, col));
                                        if xresp.clicked() { do_cancel = true; }
                                        if xresp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                                    });
                                });
                            });
                        hline_full(ui);
                        // ── BODY ── 노트북 등 낮은 해상도에서 카드가 화면을 넘으면 본문만
                        // 스크롤시켜 헤더·푸터(전송 시작 버튼)가 항상 보이게 한다(#41).
                        // 124 = 헤더(~68) + 푸터(~54) + 구분선 2.
                        let body_max = (screen.height() - card_pos.y - 8.0 - 124.0).max(140.0);
                        egui::ScrollArea::vertical()
                            .id_salt("transfer_body_scroll")
                            .max_height(body_max)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::symmetric(22.0, 18.0))
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                section_label(ui, "SOURCE LABELS");
                                ui.horizontal_wrapped(|ui| {
                                    for (label, n) in [(Label::Pick, pick), (Label::Hold, hold), (Label::Reject, reject), (Label::Unrated, unrated)] {
                                        let on = st.labels.contains(&label);
                                        if check_chip(ui, label.name(lang), Some(n), theme::label_color(label), on) {
                                            if on { st.labels.retain(|l| *l != label); } else { st.labels.push(label); }
                                        }
                                    }
                                });
                                ui.add_space(8.0);
                                if check_chip(ui, tr(lang, "라벨별 하위폴더로 분기 (/pick, /hold …)"), None, theme::ACCENT, st.split_by_label) {
                                    st.split_by_label = !st.split_by_label;
                                }
                                ui.add_space(16.0);

                                // 별점 기준(#23): 라벨과 합집합(OR). 각 별점 칸은 독립 체크.
                                section_label(ui, "STAR RATING");
                                ui.horizontal_wrapped(|ui| {
                                    for n in 1..=5u8 {
                                        let on = st.stars.contains(&n);
                                        let glyph = "★".repeat(n as usize);
                                        if check_chip(ui, &glyph, Some(star_cnt[n as usize]), theme::HOLD, on) {
                                            if on {
                                                st.stars.retain(|s| *s != n);
                                            } else {
                                                st.stars.push(n);
                                            }
                                        }
                                    }
                                });
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(tr(lang, "라벨 또는 별점 중 하나라도 해당하면 전송됩니다(합집합).")).font(mono(10.0)).color(theme::INK4));
                                ui.add_space(16.0);

                                // 컬러 태그 기준(#27): 라벨·별점과 합집합(OR). 태그별 하위폴더 분기 옵션.
                                section_label(ui, "COLOR TAGS");
                                ui.horizontal_wrapped(|ui| {
                                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                                        let on = st.tags.contains(tag);
                                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                                        let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                        if check_chip(ui, &tag_names[i], Some(tag_cnt[i]), col, on) {
                                            if on {
                                                st.tags.retain(|t| t != tag);
                                            } else {
                                                st.tags.push(*tag);
                                            }
                                        }
                                    }
                                });
                                ui.add_space(8.0);
                                if check_chip(ui, tr(lang, "태그별 하위폴더로 분기 (@teal …)"), None, theme::ACCENT, st.split_by_tag) {
                                    st.split_by_tag = !st.split_by_tag;
                                }
                                ui.add_space(16.0);

                                section_label(ui, "ACTION");
                                let act_sel = if st.action == Action::Copy { 0 } else { 1 };
                                if let Some(i) = segmented(ui, &[("Copy", tr(lang, "원본 유지")), ("Move", tr(lang, "원본 이동"))], act_sel) {
                                    st.action = if i == 0 { Action::Copy } else { Action::Move };
                                }
                                ui.add_space(16.0);

                                section_label(ui, "COMPANIONS");
                                let comp_sel = match st.companions { Companions::Both => 0, Companions::RawOnly => 1, Companions::ImageOnly => 2 };
                                if let Some(i) = segmented(ui, &[(tr(lang, "RAW+이미지"), tr(lang, "페어 함께")), (tr(lang, "RAW만"), tr(lang, "RAW만")), (tr(lang, "이미지만"), tr(lang, "JPG만"))], comp_sel) {
                                    st.companions = [Companions::Both, Companions::RawOnly, Companions::ImageOnly][i];
                                }
                                ui.add_space(16.0);

                                section_label(ui, "DESTINATION");
                                ui.horizontal(|ui| {
                                    let rest = (ui.available_width() - 96.0).max(120.0);
                                    ui.add(egui::TextEdit::singleline(&mut st.dest).font(mono(12.0)).desired_width(rest));
                                    if toggle_btn(ui, "Browse…", false).clicked() {
                                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                            st.dest = d.to_string_lossy().to_string();
                                        }
                                    }
                                });
                                ui.add_space(16.0);

                                section_label(ui, "ON FILENAME CONFLICT");
                                let conf_sel = if st.conflict == ConflictPolicy::AutoIncrement { 0 } else { 1 };
                                if let Some(i) = segmented(ui, &[(tr(lang, "자동 일련번호"), tr(lang, "_001 접미")), (tr(lang, "건너뛰기"), tr(lang, "기존 유지"))], conf_sel) {
                                    st.conflict = if i == 0 { ConflictPolicy::AutoIncrement } else { ConflictPolicy::Skip };
                                }
                                ui.add_space(16.0);

                                // 분류 기반 파일명 변경(#26): 프리셋 + 자유 템플릿 + 라이브 프리뷰.
                                section_label(ui, "RENAME");
                                let modes: [(RenameMode, &str); 4] = [
                                    (RenameMode::Off, tr(lang, "원본 유지")),
                                    (RenameMode::Seq, tr(lang, "순번 (1,2,3)")),
                                    (RenameMode::Grade, tr(lang, "별점등급 (A1,B1…)")),
                                    (RenameMode::Custom, tr(lang, "직접 입력")),
                                ];
                                ui.horizontal_wrapped(|ui| {
                                    for (mode, label) in modes {
                                        if check_chip(ui, label, None, theme::ACCENT, st.rename_mode == mode) {
                                            st.rename_mode = mode;
                                        }
                                    }
                                });
                                if st.rename_mode == RenameMode::Custom {
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut st.rename_template).font(mono(12.0)).desired_width(330.0).hint_text("{gradeseq}_{orig}"));
                                        let num_sel = if st.rename_numbering == Numbering::Order { 0 } else { 1 };
                                        if let Some(i) = segmented(ui, &[(tr(lang, "선택순"), tr(lang, "선택 순서")), (tr(lang, "등급순"), tr(lang, "별점 등급"))], num_sel) {
                                            st.rename_numbering = if i == 0 { Numbering::Order } else { Numbering::GradeGrouped };
                                        }
                                    });
                                    ui.add_space(3.0);
                                    ui.label(egui::RichText::new("{seq} {seq:03} {grade} {gradeseq} {stars} {label} {tag} {orig}").font(mono(9.0)).color(theme::INK4));
                                }
                                if st.rename_mode != RenameMode::Off {
                                    let preview_req = TransferRequest {
                                        entries: &entries,
                                        labels: st.labels.clone(),
                                        stars: st.stars.clone(),
                                        tags: st.tags.clone(),
                                        action: st.action,
                                        companions: st.companions,
                                        dest: PathBuf::new(),
                                        split_by_label: false,
                                        split_by_tag: false,
                                        conflict: st.conflict,
                                        rename: st.rename_rule(),
                                    };
                                    let pv = transfer::rename_preview(&preview_req, 4);
                                    ui.add_space(6.0);
                                    if pv.is_empty() {
                                        ui.label(egui::RichText::new(tr(lang, "대상 없음 — 위에서 라벨·별점·태그를 선택하세요.")).font(mono(9.5)).color(theme::INK4));
                                    } else {
                                        for (old, new) in &pv {
                                            ui.label(egui::RichText::new(format!("{}  →  {}", old, new)).font(mono(9.5)).color(theme::INK3));
                                        }
                                    }
                                }
                            });
                            });
                        hline_full(ui);
                        // ── FOOTER ──
                        egui::Frame::none()
                            .fill(theme::BG1)
                            .rounding(egui::Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 })
                            .inner_margin(egui::Margin::symmetric(22.0, 14.0))
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("WILL TRANSFER").font(prop(10.0)).color(theme::INK3));
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new(plan.len().to_string()).font(mono(13.0)).color(theme::ACCENT));
                                    ui.label(egui::RichText::new(tr(lang, "파일")).font(mono(10.0)).color(theme::INK3));
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(raw_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new("RAW").font(mono(9.5)).color(theme::INK3));
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(img_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new(tr(lang, "이미지")).font(mono(9.5)).color(theme::INK3));

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        // 라벨·별점 둘 다 비어 대상이 0건이면 시작 버튼 비활성(빈 전송 방지).
                                        let can_start = !plan.is_empty();
                                        if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "전송 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                                            do_start = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Enter");
                                        ui.add_space(14.0);
                                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                                            do_cancel = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Esc");
                                    });
                                });
                            });
                    });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            do_cancel = true;
        }

        if do_cancel {
            self.transfer = None;
            return;
        }
        if do_start {
            self.start_transfer(&st);
        } else {
            self.transfer = Some(st);
        }
    }

    /// 전송을 백그라운드 스레드에서 시작하고 진행 모달로 전환한다(#35). 큰 폴더에서
    /// 메인 스레드가 막히지 않게 별도 스레드에서 `execute_with_progress`를 돌리고,
    /// 진행 상황을 채널로 받는다(Move면 완료 후 폴더를 재스캔해 사라진 항목 정리, #24).
    fn start_transfer(&mut self, st: &TransferDialogState) {
        let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
        let labels = st.labels.clone();
        let stars = st.stars.clone();
        let tags = st.tags.clone();
        let action = st.action;
        let companions = st.companions;
        let dest = PathBuf::from(&st.dest);
        let split_by_label = st.split_by_label;
        let split_by_tag = st.split_by_tag;
        let conflict = st.conflict;
        let rename = st.rename_rule();

        let (tx, rx) = crossbeam_channel::unbounded::<JobMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_t = cancel.clone();
        let dest_t = dest.clone();
        std::thread::spawn(move || {
            let req = TransferRequest {
                entries: &entries,
                labels,
                stars,
                tags,
                action,
                companions,
                dest: dest_t,
                split_by_label,
                split_by_tag,
                conflict,
                rename,
            };
            let mut on = |p: &Progress| {
                let _ = tx.send(JobMsg::Progress(p.clone()));
                !cancel_t.load(Ordering::Relaxed)
            };
            let report = transfer::execute_with_progress(&req, &mut on);
            let _ = tx.send(JobMsg::Done(report));
        });

        // Move는 원본이 사라지므로 완료 후 재스캔 필요. Copy는 목록 불변이라 재오픈 불필요.
        let reopen = matches!(action, Action::Move).then_some(ReopenMode::Current);
        self.transfer = None;
        self.progress = Some(ProgressJob {
            title: "전송 중",
            rx,
            cancel,
            latest: Progress::default(),
            reopen,
            dest: Some(dest),
        });
    }

    /// 폴더 자동 분류를 백그라운드 스레드에서 시작하고 진행 모달로 전환한다(#34/#35).
    /// 완료 후 분류 결과(하위폴더)를 보도록 하위 폴더 포함으로 폴더를 다시 연다.
    fn start_organize(&mut self, st: &OrganizeDialogState) {
        let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
        let key = st.key;
        let action = st.action;
        let dest = PathBuf::from(&st.dest);
        let conflict = st.conflict;

        let (tx, rx) = crossbeam_channel::unbounded::<JobMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_t = cancel.clone();
        let dest_t = dest.clone();
        std::thread::spawn(move || {
            let req = OrganizeRequest {
                entries: &entries,
                key,
                action,
                dest_root: dest_t,
                conflict,
            };
            let mut on = |p: &Progress| {
                let _ = tx.send(JobMsg::Progress(p.clone()));
                !cancel_t.load(Ordering::Relaxed)
            };
            let report = organize::organize_with_progress(&req, &mut on);
            let _ = tx.send(JobMsg::Done(report));
        });

        // 재오픈 방식: 현재 폴더 안에 분류(in-place)했는지로 갈린다.
        //  - Move + in-place: 루트 파일이 하위폴더로 빠짐 → 하위폴더 포함 재스캔으로 결과 표시.
        //  - Move + 외부 대상: 현재 폴더가 비워짐 → 현재 설정대로 재스캔(사라진 항목 정리).
        //  - Copy: 원본이 그대로 남아 현재 보기가 유효 → 재오픈 불필요(복사본은 하위/외부에).
        let in_place = self
            .folder
            .as_ref()
            .map(|f| dest.starts_with(f))
            .unwrap_or(false);
        let reopen = match action {
            Action::Move if in_place => Some(ReopenMode::Recursive),
            Action::Move => Some(ReopenMode::Current),
            Action::Copy => None,
        };
        self.organize = None;
        self.progress = Some(ProgressJob {
            title: "폴더 정리 중",
            rx,
            cancel,
            latest: Progress::default(),
            reopen,
            dest: Some(dest),
        });
    }

    /// 진행 모달(#35): 채널을 비워 진행률을 갱신하고, 완료(Done)면 후처리(재오픈+결과)로 전환.
    fn ui_progress(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut job = self.progress.take().unwrap();

        // 채널 비우기: 진행 갱신은 마지막 값만 유지, 완료면 결과 확보.
        let mut done_report: Option<TransferReport> = None;
        let mut disconnected = false;
        loop {
            match job.rx.try_recv() {
                Ok(JobMsg::Progress(p)) => job.latest = p,
                Ok(JobMsg::Done(r)) => {
                    done_report = Some(r);
                    break;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if let Some(report) = done_report {
            let reopen = job.reopen;
            let dest = job.dest.clone();
            self.progress = None;
            match reopen {
                Some(ReopenMode::Current) => {
                    if let Some(folder) = self.folder.clone() {
                        self.open_folder(folder);
                    }
                }
                Some(ReopenMode::Recursive) => {
                    // 분류 결과(하위폴더)가 보이도록 재귀 스캔으로 전환·저장 후 재오픈.
                    if report.transferred > 0 {
                        self.cfg.recursive = true;
                        let _ = config::save(&self.cfg);
                    }
                    if let Some(folder) = self.folder.clone() {
                        self.open_folder(folder);
                    }
                }
                None => {}
            }
            self.last_dest = dest;
            self.result = Some(report);
            return;
        }
        if disconnected {
            // 작업 스레드가 결과 없이 종료(이례적 — 패닉 등). 진행 모달만 닫는다.
            self.progress = None;
            return;
        }

        // 작업이 도는 동안 결과 채널을 꾸준히 펌프(워커는 egui를 깨우지 못함).
        ctx.request_repaint_after(Duration::from_millis(80));

        let p = &job.latest;
        let frac = if p.total > 0 {
            (p.done as f32 / p.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut do_cancel = false;

        // 뒤 화면 어둡게 + 클릭 차단.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("progress_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("progress_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(480.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, job.title), "");
                ui.add(egui::ProgressBar::new(frac).fill(theme::ACCENT).desired_height(10.0));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(trf(lang, "{} / {} 파일", &[&p.done.to_string(), &p.total.to_string()]))
                        .font(mono(12.0))
                        .color(theme::INK),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{:.1} MB", p.bytes as f64 / 1_048_576.0))
                        .font(mono(10.5))
                        .color(theme::INK3),
                );
                if !p.current.is_empty() {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&p.current).font(mono(10.0)).color(theme::INK4));
                }
                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                            do_cancel = true;
                        }
                    });
                });
            });

        if do_cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // 취소 신호만 보내고 모달은 유지 — 작업 스레드가 다음 파일 직전에 멈춰 Done을 보낸다.
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.progress = Some(job);
    }

    fn ui_transfer_result(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let report = self.result.clone().unwrap();
        let mut close = false;
        let mut open_dest = false;
        egui::Window::new("transfer_result")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(560.0, 0.0))
            .frame(modal_frame())
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "전송 완료"), "");
                if report.canceled {
                    ui.label(egui::RichText::new(tr(lang, "취소됨")).font(prop(12.0)).color(theme::WARN));
                    ui.add_space(4.0);
                }
                ui.label(egui::RichText::new(trf(lang, "✓ {} 파일 전송 · {} 리네임 · {} 실패", &[&report.transferred.to_string(), &report.renamed.len().to_string(), &report.failed.len().to_string()])).font(prop(13.0)).color(theme::OK));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(trf(lang, "RAW {} · 이미지 {} · {:.1} MB", &[&report.raw_count.to_string(), &report.image_count.to_string(), &format!("{:.1}", report.bytes as f64 / 1_048_576.0)])).font(mono(11.0)).color(theme::INK2));
                if !report.renamed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("RENAMED · {}", report.renamed.len())).font(prop(10.0)).color(theme::WARN));
                    for (a, b) in report.renamed.iter().take(10) {
                        ui.label(egui::RichText::new(format!("{a} → {b}")).font(mono(10.5)).color(theme::INK3));
                    }
                }
                if !report.failed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("FAILED · {}", report.failed.len())).font(prop(10.0)).color(theme::REJECT));
                    for (p, e) in report.failed.iter().take(5) {
                        ui.label(egui::RichText::new(format!("{} — {e}", p.display())).font(mono(10.0)).color(theme::INK3));
                    }
                }
                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                let has_dest = self.last_dest.as_ref().map(|d| d.is_dir()).unwrap_or(false);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "닫기"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            close = true;
                        }
                        ui.add_space(8.0);
                        if has_dest && toggle_btn(ui, tr(lang, "대상 폴더 열기"), false).clicked() {
                            open_dest = true;
                        }
                    });
                });
            });
        if open_dest {
            if let Some(dest) = self.last_dest.clone() {
                if dest.is_dir() {
                    // 전송 결과의 "대상 폴더 열기"는 OS 파일 탐색기를 띄운다(Finder/Explorer).
                    // 과거에는 RawBlow가 작업 폴더를 그 경로로 전환했는데, macOS에서 이
                    // 경로가 강제종료를 일으켰고 UX 의도와도 맞지 않았다(#5).
                    reveal_in_file_manager(&dest);
                }
            }
            self.result = None;
            return;
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.result = None;
        }
    }

    /// 폴더 자동 분류 다이얼로그(#34). 셀렉 전송과 별개로 폴더 안 사진을 기준별 하위폴더로 정리.
    fn ui_organize_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut st = self.organize.clone().unwrap();
        let mut do_start = false;
        let mut do_cancel = false;

        // 대상 카운트(가벼움). 확장자 기준은 폴더 분포도 즉석 계산(EXIF 불필요).
        let file_count: usize = self.items.iter().map(|i| i.entry.members.len()).sum();
        let ext_breakdown: Vec<(String, usize)> = if st.key == OrganizeKey::Extension {
            use std::collections::BTreeMap;
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for it in &self.items {
                for src in &it.entry.members {
                    let e = src
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_uppercase())
                        .unwrap_or_else(|| "—".into());
                    *m.entry(e).or_default() += 1;
                }
            }
            m.into_iter().collect()
        } else {
            Vec::new()
        };

        // 뒤 화면 어둡게 + 클릭 차단.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("organize_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("organize_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(520.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(
                    ui,
                    tr(lang, "폴더 자동 분류"),
                    tr(lang, "폴더 안 사진을 기준별 하위폴더로 정리 · 셀렉 전송과 별개"),
                );

                section_label(ui, "BY");
                let keys: [(OrganizeKey, &str); 4] = [
                    (OrganizeKey::Date, tr(lang, "촬영일")),
                    (OrganizeKey::Camera, tr(lang, "카메라")),
                    (OrganizeKey::Lens, tr(lang, "렌즈")),
                    (OrganizeKey::Extension, tr(lang, "확장자")),
                ];
                ui.horizontal_wrapped(|ui| {
                    for (k, label) in keys {
                        if check_chip(ui, label, None, theme::ACCENT, st.key == k) {
                            st.key = k;
                        }
                    }
                });
                ui.add_space(16.0);

                section_label(ui, "ACTION");
                let act_sel = if st.action == Action::Copy { 0 } else { 1 };
                if let Some(i) = segmented(ui, &[("Copy", tr(lang, "원본 유지")), ("Move", tr(lang, "원본 이동"))], act_sel) {
                    st.action = if i == 0 { Action::Copy } else { Action::Move };
                }
                ui.add_space(16.0);

                section_label(ui, "DESTINATION");
                ui.horizontal(|ui| {
                    let rest = (ui.available_width() - 96.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut st.dest).font(mono(12.0)).desired_width(rest));
                    if toggle_btn(ui, "Browse…", false).clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            st.dest = d.to_string_lossy().to_string();
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr(lang, "대상 폴더 안에 기준별 하위폴더가 생성됩니다.")).font(mono(10.0)).color(theme::INK4));
                ui.add_space(16.0);

                section_label(ui, "ON FILENAME CONFLICT");
                let conf_sel = if st.conflict == ConflictPolicy::AutoIncrement { 0 } else { 1 };
                if let Some(i) = segmented(ui, &[(tr(lang, "자동 일련번호"), tr(lang, "_001 접미")), (tr(lang, "건너뛰기"), tr(lang, "기존 유지"))], conf_sel) {
                    st.conflict = if i == 0 { ConflictPolicy::AutoIncrement } else { ConflictPolicy::Skip };
                }
                ui.add_space(16.0);

                // 미리보기/안내: 확장자는 즉석 폴더 분포, EXIF 기준은 실행 중 분류 안내.
                if st.key == OrganizeKey::Extension && !ext_breakdown.is_empty() {
                    section_label(ui, "PREVIEW");
                    for (folder, n) in ext_breakdown.iter().take(6) {
                        ui.label(egui::RichText::new(format!("{}/  ·  {}", folder, n)).font(mono(10.5)).color(theme::INK3));
                    }
                    if ext_breakdown.len() > 6 {
                        ui.label(egui::RichText::new(format!("… +{}", ext_breakdown.len() - 6)).font(mono(10.0)).color(theme::INK4));
                    }
                } else if st.key != OrganizeKey::Extension {
                    ui.label(egui::RichText::new(tr(lang, "촬영일·카메라·렌즈 기준은 실행하며 EXIF를 읽어 분류합니다. RAW+JPG 페어는 같은 폴더로 유지됩니다.")).font(mono(10.0)).color(theme::INK4));
                }

                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("WILL ORGANIZE").font(prop(10.0)).color(theme::INK3));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(file_count.to_string()).font(mono(13.0)).color(theme::ACCENT));
                    ui.label(egui::RichText::new(tr(lang, "파일")).font(mono(10.0)).color(theme::INK3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can_start = file_count > 0 && !st.dest.trim().is_empty();
                        if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "정리 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            do_start = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                            do_cancel = true;
                        }
                    });
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            do_cancel = true;
        }
        if do_cancel {
            self.organize = None;
            return;
        }
        if do_start {
            self.start_organize(&st);
        } else {
            self.organize = Some(st);
        }
    }

    /// AI 컬링 설정 다이얼로그(#50). 어떤 신호로 거를지 + 임계값 + 결과 배정축을 고른다.
    /// 라이선스 문제로 미적(NIMA) 모델은 번들하지 않으므로 노출·초점·기울기 3종만 노출한다.
    /// CLIP-IQA 모델 파일의 로컬 경로(config_dir()/models/…).
    fn model_path(spec: config::ModelSpec) -> std::path::PathBuf {
        config::config_dir().join("models").join(spec.file)
    }

    /// 모델 파일이 다운로드되어 있는지 확인.
    fn model_present(spec: config::ModelSpec) -> bool {
        Self::model_path(spec).exists()
    }

    /// 모델 파일을 백그라운드 스레드로 다운로드(#50). ureq 스트리밍 + sha256 검증.
    #[cfg(feature = "model-download")]
    fn start_model_download(&mut self, spec: config::ModelSpec, label: String) {
        let dest = Self::model_path(spec);
        let url = spec.url.to_string();
        let sha = spec.sha256.to_string();
        let expected = spec.bytes;
        let (tx, rx) = crossbeam_channel::unbounded::<ModelDlMsg>();
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
                    let _ = tx.send(ModelDlMsg::Done(Err(format!("파일 생성: {e}"))));
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
                            let _ = tx.send(ModelDlMsg::Done(Err(format!("쓰기 오류: {e}"))));
                            return;
                        }
                        downloaded += n as u64;
                        let _ = tx.send(ModelDlMsg::Progress(downloaded, expected));
                    }
                    Err(e) => {
                        let _ = tx.send(ModelDlMsg::Done(Err(format!("읽기 오류: {e}"))));
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
                        let _ = tx.send(ModelDlMsg::Done(Err(format!("검증 열기: {e}"))));
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
                            let _ = tx.send(ModelDlMsg::Done(Err(format!("해시 읽기: {e}"))));
                            return;
                        }
                    }
                }
                format!("{:x}", hasher.finalize())
            };
            if actual != sha {
                let _ = std::fs::remove_file(&tmp);
                let _ = tx.send(ModelDlMsg::Done(Err(format!(
                    "sha256 불일치 — 다시 시도해주세요\n예상: {sha}\n실제: {actual}"
                ))));
                return;
            }
            if let Err(e) = std::fs::rename(&tmp, &dest) {
                let _ = tx.send(ModelDlMsg::Done(Err(format!("파일 이동: {e}"))));
                return;
            }
            let _ = tx.send(ModelDlMsg::Done(Ok(())));
        });
        self.model_dl = Some(ModelDlJob { rx, label, done: 0, total: expected });
    }

    /// 모델 다운로드 진행 모달(#50).
    #[cfg(feature = "model-download")]
    fn ui_model_dl_progress(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut job = self.model_dl.take().unwrap();
        let mut done_result: Option<Result<(), String>> = None;
        loop {
            match job.rx.try_recv() {
                Ok(ModelDlMsg::Progress(d, t)) => { job.done = d; job.total = t; }
                Ok(ModelDlMsg::Done(r)) => { done_result = Some(r); break; }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    done_result = Some(Err("연결 끊김".into())); break;
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
                        format!("다운로드 실패: {e}"),
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

    fn ui_ai_cull_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut c = self.cfg.ai_cull.clone();
        let mut do_start = false;
        let mut do_cancel = false;
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

        egui::Window::new("aicull_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(540.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(
                    ui,
                    tr(lang, "AI 컬링"),
                    tr(lang, "사진을 분석해 흐림·노출·기울기로 자동 분류 · 전부 로컬 처리"),
                );

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

                section_label(ui, "OPTIONS");
                if c.use_focus {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "흐림 임계(선명도↑ 엄격)")).font(mono(11.0)).color(theme::INK3));
                        ui.add(egui::Slider::new(&mut c.focus_thresh, 0.2..=0.8).fixed_decimals(2));
                    });
                    if check_chip(ui, tr(lang, "AF 측거점에서만 초점 측정"), None, theme::ACCENT, c.use_af_focus) {
                        c.use_af_focus = !c.use_af_focus;
                    }
                    ui.add_space(4.0);
                }
                if c.use_exposure {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "노출 양호도 하한")).font(mono(11.0)).color(theme::INK3));
                        ui.add(egui::Slider::new(&mut c.exposure_min, 0.2..=0.9).fixed_decimals(2));
                    });
                    ui.add_space(4.0);
                }
                if c.use_tilt {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "허용 기울기")).font(mono(11.0)).color(theme::INK3));
                        ui.add(egui::Slider::new(&mut c.tilt_max_deg, 0.5..=10.0).suffix("°").fixed_decimals(1));
                    });
                }
                if c.use_aesthetic {
                    // GPU 고속 모드 토글: RN50을 CoreML(mac)/WebGPU(win)로 배치·병렬 추론.
                    if check_chip(ui, tr(lang, "⚡ GPU 고속 모드"), None, theme::ACCENT, c.use_gpu) {
                        c.use_gpu = !c.use_gpu;
                    }
                    if c.use_gpu {
                        ui.label(
                            egui::RichText::new(tr(lang, "RN50을 GPU로 배치·병렬 추론 — 매우 빠름(수백 장/초). 화질은 ViT보다 낮음."))
                                .font(mono(9.5)).color(theme::INK3),
                        );
                    } else {
                        // 백본 선택(CPU int8).
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "백본 모델")).font(mono(11.0)).color(theme::INK3));
                            ui.add_space(4.0);
                            for b in ClipIqaBackbone::ALL {
                                let sel = c.backbone == b;
                                let present = Self::model_present(b.spec());
                                let col = if present { theme::ACCENT } else { theme::INK3 };
                                if check_chip(ui, b.label(), None, col, sel) {
                                    c.backbone = b;
                                }
                            }
                        });
                        ui.label(
                            egui::RichText::new(tr(lang, "ViT-B/32 권장. CPU에선 ViT-B가 가장 빠름. (강력한 GPU면 위 GPU 모드가 더 빠름)"))
                                .font(mono(9.5)).color(theme::INK3),
                        );
                    }
                    let spec = c.model_spec();
                    let model_ok = Self::model_present(spec);
                    if model_ok {
                        // 선택 모드: 상위 N장 vs 임계값.
                        let use_topn = c.top_n > 0;
                        ui.horizontal(|ui| {
                            if check_chip(ui, tr(lang, "상위 N장"), None, theme::ACCENT, use_topn) {
                                c.top_n = if use_topn { 0 } else { 20 };
                            }
                            if use_topn {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(tr(lang, "최고")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::DragValue::new(&mut c.top_n).range(1..=9999).suffix(tr(lang, "장")));
                            } else {
                                if check_chip(ui, tr(lang, "임계값"), None, theme::ACCENT, !use_topn) {
                                    // 이미 선택됨 — 토글 불필요
                                }
                            }
                        });
                        if !use_topn {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(tr(lang, "P(good) 하한")).font(mono(11.0)).color(theme::INK3));
                                ui.add(egui::Slider::new(&mut c.aesthetic_min, 0.1..=0.9).fixed_decimals(2));
                                ui.label(egui::RichText::new(tr(lang, "↑ 엄격할수록 탈락↑")).font(mono(9.0)).color(theme::INK3));
                            });
                        }
                        ui.label(
                            egui::RichText::new(tr(lang, "✓ 모델 준비됨"))
                                .font(mono(10.0)).color(theme::ACCENT),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(tr(lang, "모델 없음 — 아래 다운로드 버튼을 눌러주세요"))
                                .font(mono(10.0)).color(theme::WARN),
                        );
                        #[cfg(feature = "model-download")]
                        if ui.add(egui::Button::new(
                            egui::RichText::new(format!("  {} ({:.0} MB)  ", tr(lang, "다운로드"), spec.bytes as f64 / 1e6))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20))
                        ).fill(theme::ACCENT)).clicked() {
                            do_download = Some(spec);
                        }
                    }
                    ui.add_space(4.0);
                }
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

                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                let count = if c.scope_all { total_items } else { filtered_count };
                // 모드별 대략 처리량(실파이프라인=디코드 포함, M1 Max·JPEG 기준. RAW는 더 느림).
                // 실제 컬링은 디코드가 병목이라 미적 GPU/CPU 차이는 작다.
                let rate = if c.use_aesthetic {
                    if c.use_gpu { "~100장/초 내외 (GPU)" } else { "~90장/초 내외 (CPU)" }
                } else {
                    "~200장/초+ (모델 불필요·가장 빠름)"
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("WILL ANALYZE").font(prop(10.0)).color(theme::INK3));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(count.to_string()).font(mono(13.0)).color(theme::ACCENT));
                    ui.label(egui::RichText::new(tr(lang, "장")).font(mono(10.0)).color(theme::INK3));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr(lang, rate)).font(mono(9.5)).color(theme::INK4));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let aesthetic_blocks = c.use_aesthetic && !Self::model_present(c.model_spec());
                    let can_start = c.any_enabled() && count > 0 && !aesthetic_blocks;
                        if ui
                            .add_enabled(
                                can_start,
                                egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "컬링 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            do_start = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                            do_cancel = true;
                        }
                    });
                });
            });

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
                let label = if self.cfg.ai_cull.use_gpu {
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
    fn cull_worker_count(model_bytes: Option<u64>) -> usize {
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
    fn start_ai_cull(&mut self) {
        let cfg = self.cfg.ai_cull.clone();
        let mut criteria = cfg.criteria();
        let use_af = cfg.use_af_focus && cfg.use_focus;
        let top_n = cfg.top_n;
        // 디코드 해상도는 가장 디테일이 필요한 신호(초점)에 맞춘다. 초점 검사가 꺼져 있으면
        // 노출(무관)·기울기(512 내부)·미적(224)만 필요하므로 512로 줄여 디코드를 ~1.5배 빠르게.
        let cull_edge: u32 = if cfg.use_focus { AI_CULL_EDGE } else { 512 };
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
        let targets = std::sync::Arc::new(targets);

        let mut handles = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let targets = targets.clone();
            let next = next.clone();
            let progress = progress.clone();
            let cancel_w = cancel.clone();
            let results = results.clone();
            #[cfg(feature = "ai")]
            let model_path = model_path.clone();
            #[cfg(feature = "ai")]
            let shared_model = shared_model.clone();
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
                loop {
                    if cancel_w.load(Ordering::Relaxed) {
                        break;
                    }
                    let start = next.fetch_add(batch_size, Ordering::Relaxed);
                    if start >= targets.len() {
                        break;
                    }
                    let end = (start + batch_size).min(targets.len());
                    // 1단계: chunk의 각 장을 디코딩+CV(패닉 격리). 디코드 이미지는 미적 추론용으로 보관.
                    let mut imgs: Vec<rawblow_core::decode::DecodedImage> = Vec::with_capacity(end - start);
                    let mut metas: Vec<(usize, rawblow_core::quality::QualityReport, Verdict)> =
                        Vec::with_capacity(end - start);
                    for idx in start..end {
                        // 배치(최대 32)가 클 수 있으니 장마다 취소를 확인해 즉시 멈춘다.
                        if cancel_w.load(Ordering::Relaxed) {
                            break;
                        }
                        let (real, path) = &targets[idx];
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            cull_decode_cv(path, criteria, use_af, cull_edge)
                        }))
                        .ok()
                        .flatten();
                        if let Some((img, q, cv)) = r {
                            imgs.push(img);
                            metas.push((*real, q, cv));
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
                    // 3단계: 결과 적재(CV 판정 + 미적 점수). 최종 조합은 apply_cull_verdicts가 한다.
                    if let Ok(mut g) = results.lock() {
                        for (real, q, cv) in &metas {
                            g.push((*real, *cv, q.aesthetic));
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
            let _ = tx.send(AiCullMsg::Done(out));
        });

        self.ai_cull = Some(AiCullJob {
            rx,
            cancel,
            progress,
            total,
            generation: self.generation,
            target: cfg.target,
        });
    }

    /// 진행 중인 컬링의 진행률을 펌프하고(매 프레임, 비차단) 완료 시 결과를 적용한다(#50).
    /// 폴더가 바뀌었으면(generation 불일치) 인덱스가 무효이므로 결과를 폐기한다.
    fn pump_ai_cull(&mut self, ctx: &egui::Context) {
        let Some(job) = self.ai_cull.take() else { return };

        let mut done_results: Option<Vec<(usize, Verdict, Option<f32>)>> = None;
        let mut disconnected = false;
        match job.rx.try_recv() {
            Ok(AiCullMsg::Done(v)) => done_results = Some(v),
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => disconnected = true,
        }

        if let Some(v) = done_results {
            self.ai_cull = None;
            self.ai_cull_cancel_confirm = false;
            // 폴더가 바뀌면 캡처한 real 인덱스가 다른 사진을 가리킨다 → 결과 폐기(오염 방지).
            if job.generation == self.generation {
                self.apply_cull_verdicts(v);
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
    fn cancel_ai_cull(&mut self) {
        if let Some(job) = self.ai_cull.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.ai_cull_cancel_confirm = false;
    }

    /// 컬링 버튼(프로그레스바) 재클릭 시 뜨는 취소 확인 모달(#50).
    fn ui_ai_cull_cancel_confirm(&mut self, ctx: &egui::Context) {
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
    fn ui_ai_cull_folder_confirm(&mut self, ctx: &egui::Context) {
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
    fn apply_cull_verdicts(&mut self, mut results: Vec<(usize, Verdict, Option<f32>)>) {
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

        let (mut good, mut bad) = (0usize, 0usize);
        for (real, v, _) in results {
            let Some(it) = self.items.get_mut(real) else { continue };
            let is_good = matches!(v, Verdict::Good);
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
        self.toast = Some((
            format!(
                "{}{}",
                trf(lang, "AI 컬링 완료 · 좋음 {} · 탈락 {}", &[&good.to_string(), &bad.to_string()]),
                score_info
            ),
            Instant::now(),
        ));
    }

    fn ui_jump(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let mut go = false;
        egui::Window::new("jump_dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .fixed_size(Vec2::new(420.0, 0.0))
            .frame(modal_frame())
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "파일번호 점프"), tr(lang, "줄바꿈·쉼표·탭으로 구분"));
                ui.add(egui::TextEdit::multiline(&mut self.jump_text).font(mono(12.0)).desired_rows(2).desired_width(f32::INFINITY));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.jump_exact, tr(lang, "정확히 일치"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(format!("  {}  ⏎  ", tr(lang, "점프"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            go = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "닫기 (Esc)"), false).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if go || ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            let terms = transfer::parse_terms(&self.jump_text);
            let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
            let mode = if self.jump_exact { MatchMode::Exact } else { MatchMode::Contains };
            let hits = transfer::match_indices(&entries, &terms, mode);
            if let Some(&first) = hits.first() {
                // 필터 목록에서의 위치로 변환.
                let f = self.filtered();
                if let Some(pos) = f.iter().position(|&r| r == first) {
                    self.index = pos;
                }
                self.toast = Some((trf(lang, "{} 건 매칭", &[&hits.len().to_string()]), Instant::now()));
            } else {
                self.toast = Some((tr(lang, "매칭 없음").into(), Instant::now()));
            }
            close = true;
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.jump_open = false;
        }
    }

    /// 그리드 모드 한정 일괄 분류 변경 모달(#3).
    ///
    /// 파일명(또는 일부)으로 검색해 매칭되는 항목을 한꺼번에 Q/W/E/R 라벨로 적용한다.
    /// 매칭 규칙은 RawPull과 동일(stem 기준 contains/exact, 대소문자 무시) —
    /// transfer::parse_terms / match_indices를 그대로 재사용한다.
    fn ui_bulk(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let mut search = false;
        let mut apply = false;
        egui::Window::new("bulk_dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .fixed_size(Vec2::new(520.0, 0.0))
            .frame(modal_frame())
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "일괄 분류 변경"), tr(lang, "파일명·일부 → 매칭 → 라벨 적용"));
                ui.add(
                    egui::TextEdit::multiline(&mut self.bulk_text)
                        .font(mono(12.0))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr(lang, "파일명 또는 일부 — 줄바꿈·쉼표·탭으로 구분")),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.bulk_exact, tr(lang, "정확히 일치"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(format!("  {}  ⏎  ", tr(lang, "검색")))
                                        .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                                )
                                .fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            search = true;
                        }
                    });
                });

                // 검색이 끝났으면 결과 + 라벨 선택 영역 노출.
                if self.bulk_searched {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(trf(lang, "매칭 {}건", &[&self.bulk_hits.len().to_string()]))
                            .font(prop(11.0))
                            .color(theme::INK2),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if self.bulk_hits.is_empty() {
                                ui.label(
                                    egui::RichText::new(tr(lang, "매칭 결과 없음"))
                                        .font(mono(11.0))
                                        .color(theme::INK3),
                                );
                            } else {
                                for &idx in &self.bulk_hits {
                                    if let Some(it) = self.items.get(idx) {
                                        let lbl = it.entry.label;
                                        let mark = match lbl {
                                            Label::Pick => "●",
                                            Label::Hold => "◐",
                                            Label::Reject => "✕",
                                            Label::Unrated => "·",
                                        };
                                        let [r, g, b] = lbl.color_rgb();
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(mark)
                                                    .font(mono(11.0))
                                                    .color(Color32::from_rgb(r, g, b)),
                                            );
                                            ui.label(
                                                egui::RichText::new(&it.entry.stem)
                                                    .font(mono(11.0))
                                                    .color(theme::INK2),
                                            );
                                        });
                                    }
                                }
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(tr(lang, "적용할 라벨"))
                            .font(prop(11.0))
                            .color(theme::INK3),
                    );
                    ui.horizontal(|ui| {
                        for (lbl, key) in [
                            (Label::Pick, "Q"),
                            (Label::Hold, "W"),
                            (Label::Reject, "E"),
                            (Label::Unrated, "R"),
                        ] {
                            let active = self.bulk_target == lbl;
                            if toggle_btn(ui, &format!("{}  {}", key, lbl.name(lang)), active).clicked() {
                                self.bulk_target = lbl;
                            }
                        }
                    });
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can_apply = self.bulk_searched && !self.bulk_hits.is_empty();
                        let btn = egui::Button::new(
                            egui::RichText::new(format!("  {}  ", tr(lang, "적용")))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                        )
                        .fill(theme::ACCENT);
                        if ui.add_enabled(can_apply, btn).clicked() {
                            apply = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "닫기 (Esc)"), false).clicked() {
                            close = true;
                        }
                    });
                });
            });

        // 검색 트리거(버튼 또는 Enter — 입력이 비어있지 않을 때만).
        if search
            || (ctx.input(|i| i.key_pressed(egui::Key::Enter))
                && !self.bulk_text.trim().is_empty())
        {
            let terms = transfer::parse_terms(&self.bulk_text);
            let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
            let mode = if self.bulk_exact {
                MatchMode::Exact
            } else {
                MatchMode::Contains
            };
            self.bulk_hits = transfer::match_indices(&entries, &terms, mode);
            self.bulk_searched = true;
        }

        // 적용: 매칭된 모든 항목에 라벨을 일괄 적용하고 사이드카 저장을 앞당긴다.
        if apply && !self.bulk_hits.is_empty() {
            // 컬링이 라벨 축을 잠갔으면 일괄 라벨링도 막는다(결과 덮어쓰기 혼동 방지).
            if self.cull_axis_locked(AiCullTarget::Label) {
                self.bulk_open = false;
                return;
            }
            let target = self.bulk_target;
            let mut changed = 0usize;
            for &idx in &self.bulk_hits {
                if let Some(it) = self.items.get_mut(idx) {
                    if it.entry.label != target {
                        it.entry.label = target;
                        changed += 1;
                    }
                }
            }
            if changed > 0 {
                self.sidecar_dirty = true;
                // 다음 틱에서 즉시 사이드카가 저장되도록 last_save를 과거로.
                self.last_save = Instant::now() - Duration::from_millis(400);
            }
            self.toast = Some((
                trf(lang, "{}건 → {}", &[&self.bulk_hits.len().to_string(), target.name(lang)]),
                Instant::now(),
            ));
            self.bulk_open = false;
            return;
        }

        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.bulk_open = false;
        }
    }

    fn ui_settings(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::TopBottomPanel::top("settings_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button(format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        self.show_settings = false;
                        let _ = config::save(&self.cfg);
                        self.schedule_cache_trim(); // 변경된 상한으로 캐시 정리.
                    }
                    ui.label(egui::RichText::new("Settings — Keyboard & General").font(prop(14.0)).color(theme::INK));
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(egui::RichText::new("GENERAL").font(prop(11.0)).color(theme::INK3));
                    ui.checkbox(&mut self.cfg.auto_advance, tr(lang, "라벨링 후 자동 전진"));
                    ui.checkbox(&mut self.cfg.recursive, tr(lang, "하위 폴더 포함 스캔"));
                    ui.checkbox(&mut self.cfg.show_exif, tr(lang, "EXIF 오버레이 기본 표시"));
                    ui.checkbox(&mut self.cfg.show_histogram, tr(lang, "히스토그램 기본 표시"));
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "프리로드 ±"));
                        ui.add(egui::DragValue::new(&mut self.cfg.preload).range(0..=10));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "그리드 열 수"));
                        ui.add(egui::DragValue::new(&mut self.cfg.grid_cols).range(4..=12));
                    });
                    // 스트립·그리드 표기 크기(#44): 셀 위 선택 표시·별점·색상 태그를 크게(기본)/작게.
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "스트립·그리드 표기 크기"));
                        if ui.selectable_label(self.cfg.large_badges, tr(lang, "크게")).clicked() {
                            self.cfg.large_badges = true;
                            let _ = config::save(&self.cfg);
                        }
                        if ui.selectable_label(!self.cfg.large_badges, tr(lang, "작게")).clicked() {
                            self.cfg.large_badges = false;
                            let _ = config::save(&self.cfg);
                        }
                    });
                    // 언어 선택(#30): 시스템(자동)/한국어/English/日本語. 변경 즉시 적용·저장.
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "언어"));
                        let opts: [(Option<Lang>, &str); 4] = [
                            (None, tr(lang, "시스템 (자동)")),
                            (Some(Lang::Ko), Lang::Ko.native_name()),
                            (Some(Lang::En), Lang::En.native_name()),
                            (Some(Lang::Ja), Lang::Ja.native_name()),
                        ];
                        let mut sel = self.cfg.lang;
                        for (val, label) in opts {
                            if ui.selectable_label(sel == val, label).clicked() {
                                sel = val;
                            }
                        }
                        if sel != self.cfg.lang {
                            self.cfg.lang = sel;
                            self.lang = crate::i18n::effective_lang(&self.cfg);
                            // 폰트도 새 언어의 폰트를 primary로 교체(#32 후속: 세로 어긋남 방지).
                            crate::fonts::install(ui.ctx(), self.lang);
                            let _ = config::save(&self.cfg);
                        }
                    });
                    // ── PHOTO BACKGROUND (#36): 사진 표시 화면 배경색 — 프리셋 + HEX/RGB ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("PHOTO BACKGROUND").font(prop(11.0)).color(theme::INK3));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "사진 표시 화면 배경색 — 프리셋 또는 HEX/RGB로 지정(Lightroom Develop 기본값은 50% 회색)")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    // 프리셋: (라벨, Option<rgb>) — None은 앱 기본(near-black void).
                    let presets: [(&str, Option<[u8; 3]>); 6] = [
                        (tr(lang, "기본"), None),
                        (tr(lang, "검정"), Some([0x00, 0x00, 0x00])),
                        (tr(lang, "다크 그레이"), Some([0x1e, 0x1e, 0x1e])),
                        (tr(lang, "중간 회색"), Some([0x80, 0x80, 0x80])),
                        (tr(lang, "라이트 그레이"), Some([0xb3, 0xb3, 0xb3])),
                        (tr(lang, "흰색"), Some([0xff, 0xff, 0xff])),
                    ];
                    // 고정폭 셀 그리드: 라벨 길이가 달라도(검정/라이트 그레이/ミディアムグレー) 색견본과
                    // 글자가 같은 열에 맞도록 각 프리셋을 동일 크기 셀에 가운데 정렬한다(테스트 피드백).
                    const BG_CELL: Vec2 = Vec2::new(78.0, 50.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);
                        for (label, val) in presets {
                            let selected = self.cfg.photo_bg == val;
                            let swatch_rgb = val.unwrap_or(theme::BG0_RGB);
                            let cell = ui.allocate_ui_with_layout(
                                BG_CELL,
                                Layout::top_down(Align::Center),
                                |ui| {
                                    ui.set_width(BG_CELL.x);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    let clicked = bg_swatch(ui, swatch_rgb, selected);
                                    ui.label(
                                        egui::RichText::new(label)
                                            .font(mono(9.0))
                                            .color(if selected { theme::INK2 } else { theme::INK4 }),
                                    );
                                    clicked
                                },
                            );
                            if cell.inner {
                                self.cfg.photo_bg = val;
                                self.bg_hex = hex_str(self.photo_bg_rgb());
                                let _ = config::save(&self.cfg);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        // 현재 색 미리보기.
                        let cur = self.photo_bg_rgb();
                        let (sr, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        ui.painter().rect(sr, Rounding::same(4.0), Color32::from_rgb(cur[0], cur[1], cur[2]), Stroke::new(1.0, theme::LINE3));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("HEX").font(mono(10.0)).color(theme::INK3));
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.bg_hex).font(mono(12.0)).desired_width(84.0).hint_text("#101010"));
                        if resp.changed() {
                            if let Some(rgb) = parse_hex_rgb(&self.bg_hex) {
                                self.cfg.photo_bg = Some(rgb);
                                let _ = config::save(&self.cfg);
                            }
                        }
                        ui.add_space(8.0);
                        let mut rgb = self.photo_bg_rgb();
                        let mut changed = false;
                        ui.label(egui::RichText::new("R").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[0]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("G").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[1]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("B").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[2]).range(0..=255)).changed();
                        if changed {
                            self.cfg.photo_bg = Some(rgb);
                            self.bg_hex = hex_str(rgb);
                            let _ = config::save(&self.cfg);
                        }
                    });

                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("LABELS").font(prop(11.0)).color(theme::INK3));
                    let km = &self.cfg.keymap;
                    for (lbl, key) in [(Label::Pick, &km.pick), (Label::Hold, &km.hold), (Label::Reject, &km.reject), (Label::Unrated, &km.clear)] {
                        ui.horizontal(|ui| {
                            ui.label(lbl.name(lang));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kbd(ui, key);
                            });
                        });
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr(lang, "단축키 재바인딩 UI는 v1.1 예정 — 현재 기본값 QWER 고정 표시")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "별점 1~5 지정 · ` (백틱)으로 해제 — 라벨(QWER)과 독립으로 동시에 매겨집니다")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "M = 촬영 위치 미니 지도(GPS 있는 사진) · A = AF 포인트 표시 — 토글 상태는 저장됩니다")).font(mono(10.0)).color(theme::INK4));

                    // ── COLOR TAGS (#27): 색별 커스텀 이름. 비우면 기본 색 이름 표시 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("COLOR TAGS").font(prop(11.0)).color(theme::INK3));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "색별 이름을 지정해 보정 방식 등 나만의 분류로 — ⇧1~5로 부여")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(4.0);
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 6.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg.tag_names[i])
                                    .hint_text(tag.default_name(lang))
                                    .desired_width(220.0),
                            );
                        });
                    }

                    // ── CACHE (#22): 썸네일 디스크 캐시 사용량 + 비우기 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("CACHE").font(prop(11.0)).color(theme::INK3));
                    if self.cache_size.is_none() {
                        self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                    }
                    let size = self.cache_size.unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(trf(lang, "썸네일 캐시 사용량 · {}", &[&fmt_bytes(size)])).font(mono(11.0)).color(theme::INK2));
                        if toggle_btn(ui, tr(lang, "캐시 비우기"), false).clicked() {
                            let _ = cache::clear(&config::cache_dir());
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                            self.toast = Some((tr(lang, "썸네일 캐시를 비웠습니다").into(), Instant::now()));
                        }
                        if toggle_btn(ui, tr(lang, "새로고침"), false).clicked() {
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "자동 상한")).font(mono(11.0)).color(theme::INK2));
                        ui.add(egui::DragValue::new(&mut self.cfg.cache_limit_mb).speed(64.0).range(0..=1_048_576).suffix(" MB"));
                        ui.label(egui::RichText::new(tr(lang, "(0 = 무제한)")).font(mono(10.0)).color(theme::INK4));
                    });
                    ui.label(egui::RichText::new(tr(lang, "상한을 넘으면 오래된 썸네일부터 자동 삭제 — 폴더 열 때·설정 변경 시 정리됩니다.")).font(mono(10.0)).color(theme::INK4));
                    ui.label(egui::RichText::new(tr(lang, "폴더를 다시 열어도 재디코딩 없이 즉시 표시됩니다.")).font(mono(10.0)).color(theme::INK4));
                    ui.label(egui::RichText::new(config::cache_dir().to_string_lossy().to_string()).font(mono(9.5)).color(theme::INK4));

                    // ── ABOUT / LINKS (#18): 버전·릴리즈·이슈·제작자·cosly ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("ABOUT").font(prop(11.0)).color(theme::INK3));
                    ui.label(egui::RichText::new(format!("RawBlow v{}", env!("CARGO_PKG_VERSION"))).font(mono(11.0)).color(theme::INK2));
                    ui.add_space(6.0);
                    link_label(ui, tr(lang, "최신 버전 받기 · GitHub Releases"), "https://github.com/ascoeur9/rawblow/releases");
                    link_label(ui, tr(lang, "버그 제보 · GitHub Issues"), "https://github.com/ascoeur9/rawblow/issues");
                    // 오픈소스 라이센스 고지(#39): 포함된 구성요소 목록 + 전문 페이지.
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "오픈소스 라이센스"), false).clicked() {
                        self.licenses = Some(crate::licenses::LicensesPage::new());
                    }
                    ui.label(egui::RichText::new(tr(lang, "이 프로그램이 포함한 오픈소스 구성요소 목록과 라이센스 전문")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "만든 사람 · 하레 (Hare)")).font(prop(11.5)).color(theme::INK2));
                    ui.horizontal(|ui| {
                        link_label(ui, "X · @ascoeur9", "https://x.com/ascoeur9");
                        ui.label(egui::RichText::new("·").font(mono(10.0)).color(theme::INK4));
                        link_label(ui, "X · @hare_kig", "https://x.com/hare_kig");
                    });
                    ui.add_space(10.0);
                    link_label(ui, tr(lang, "투네이션으로 후원하기"), "https://toon.at/donate/hare");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "마음에 드시나요? 그럼 cosly도 이용해보세요.")).font(prop(11.5)).color(theme::INK2));
                    link_label(ui, "https://cosly.link", "https://cosly.link");
                });
            });
        self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
        self.show_exif = self.cfg.show_exif;
        self.show_hist = self.cfg.show_histogram;
    }

    /// 오픈소스 라이센스 페이지(#39): 좌측 구성요소 목록(검색 가능) + 우측 라이센스 전문.
    /// 설정의 ABOUT에서 열며, 돌아가기/Esc로 설정 화면에 복귀한다.
    fn ui_licenses(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let (total, generated) = self
            .licenses
            .as_ref()
            .map(|p| (p.doc.crates.len(), p.doc.generated.clone()))
            .unwrap_or((0, String::new()));
        egui::TopBottomPanel::top("licenses_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button(format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        close = true;
                    }
                    ui.label(egui::RichText::new(tr(lang, "오픈소스 라이센스")).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(trf(lang, "{} 구성요소", &[&total.to_string()])).font(mono(10.5)).color(theme::INK3));
                    if !generated.is_empty() {
                        ui.label(egui::RichText::new(format!("· {}", generated)).font(mono(10.5)).color(theme::INK4));
                    }
                });
            });
        let page = self.licenses.as_mut().unwrap();
        egui::SidePanel::left("licenses_list")
            .exact_width(320.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(10.0)))
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut page.filter)
                        .font(mono(12.0))
                        .hint_text(tr(lang, "검색"))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let f = page.filter.to_lowercase();
                    for (i, c) in page.doc.crates.iter().enumerate() {
                        if !f.is_empty() && !c.n.to_lowercase().contains(&f) && !c.l.to_lowercase().contains(&f) {
                            continue;
                        }
                        if ui.selectable_label(page.selected == i, format!("{} {}", c.n, c.v)).clicked() {
                            page.selected = i;
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(tr(lang, "이 프로그램은 아래의 오픈소스 소프트웨어를 포함합니다. LGPL 구성요소(rawloader · imagepipe · multicache)의 소스코드는 각 항목의 저장소 링크에서 구할 수 있습니다."))
                        .font(mono(10.0))
                        .color(theme::INK4),
                );
                ui.add_space(12.0);
                if let Some(c) = page.doc.crates.get(page.selected) {
                    ui.label(egui::RichText::new(format!("{} v{}", c.n, c.v)).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(c.l.as_str()).font(mono(11.0)).color(theme::INK3));
                    if let Some(r) = &c.r {
                        link_label(ui, r, r);
                    }
                    ui.add_space(10.0);
                    // 전문은 항목별 ScrollArea — selected를 id에 섞어 항목 전환 시 스크롤이 맨 위로.
                    egui::ScrollArea::vertical()
                        .id_salt(("license_text", page.selected))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for &ti in &c.t {
                                if let Some(t) = page.doc.texts.get(ti) {
                                    ui.label(egui::RichText::new(t.as_str()).font(mono(10.5)).color(theme::INK2));
                                    ui.add_space(16.0);
                                }
                            }
                        });
                }
            });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.licenses = None;
        }
    }
}

// ── 보조 위젯 ──────────────────────────────────────────────
fn toggle_btn(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(label).font(prop(12.0)).color(if active { theme::INK } else { theme::INK2 }))
        .fill(if active { theme::BG3 } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0, if active { theme::LINE2 } else { Color32::TRANSPARENT }));
    ui.add(btn)
}

fn vsep(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 18.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, theme::LINE);
    ui.add_space(4.0);
}

/// 사진 배경 색견본(클릭 가능, #36). 선택 시 accent 링을 두른다.
fn bg_swatch(ui: &mut egui::Ui, rgb: [u8; 3], selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    ui.painter().rect(rect, Rounding::same(5.0), col, Stroke::new(1.0, theme::LINE3));
    if selected {
        ui.painter().rect_stroke(rect.expand(2.0), Rounding::same(7.0), Stroke::new(2.0, theme::ACCENT));
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// 컬링 1장: 디코딩 + 켜진 CV 신호 채점(+AF 영역 초점) + CV 판정(미적 제외)(#50).
/// 디코드 이미지를 함께 돌려줘 호출부가 단장/배치 미적 추론에 재사용한다. 손상·실패 시 None.
fn cull_decode_cv(
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

/// AF 정규화 좌표(센서 기준, 원점 좌상단)를 EXIF orientation이 적용된 표시
/// 좌표계로 변환한다(#37). 90/270도 회전은 폭/높이도 맞바꾼다.
fn af_display_coords(pt: &rawblow_core::af::AfPoint, orient: u16) -> (f64, f64, f64, f64) {
    let (x, y, w, h) = (pt.cx, pt.cy, pt.w, pt.h);
    match orient {
        2 => (1.0 - x, y, w, h),         // 좌우 미러
        3 => (1.0 - x, 1.0 - y, w, h),   // 180°
        4 => (x, 1.0 - y, w, h),         // 상하 미러
        5 => (y, x, h, w),               // 전치
        6 => (1.0 - y, x, h, w),         // 90° CW
        7 => (1.0 - y, 1.0 - x, h, w),   // 전치 + 180°
        8 => (y, 1.0 - x, h, w),         // 90° CCW
        _ => (x, y, w, h),
    }
}

/// AF 중심 확대(#49)용: 측거점들의 대표 중심을 **표시좌표(0..1, orientation 적용)**로 반환.
/// 우선순위: 합초(in_focus) → 선택(selected) → 전체. 같은 그룹 안에서는 평균(존 AF 대응). 점이
/// 없으면 None → 호출부는 기존 중앙 기준 확대로 폴백한다.
fn af_focus_center(af: &rawblow_core::af::AfInfo, orient: u16) -> Option<(f64, f64)> {
    fn avg(
        af: &rawblow_core::af::AfInfo,
        orient: u16,
        pred: impl Fn(&rawblow_core::af::AfPoint) -> bool,
    ) -> Option<(f64, f64)> {
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0u32);
        for p in &af.points {
            if pred(p) {
                let (cx, cy, _, _) = af_display_coords(p, orient);
                sx += cx;
                sy += cy;
                n += 1;
            }
        }
        if n == 0 {
            None
        } else {
            Some((sx / n as f64, sy / n as f64))
        }
    }
    avg(af, orient, |p| p.in_focus)
        .or_else(|| avg(af, orient, |p| p.selected))
        .or_else(|| avg(af, orient, |_| true))
}

/// `#rrggbb` 또는 `rrggbb`(공백 허용) → [r,g,b](#36). 형식이 아니면 None.
fn parse_hex_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some([r, g, b])
}

/// [r,g,b] → `#RRGGBB`(#36).
fn hex_str(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

// ── 새 릴리즈 안내(#33) ─────────────────────────────────────────────

/// GitHub 최신 릴리즈 tag_name을 가져온다(네트워크, update-check 기능 시). 실패 시 None.
/// 동기 HTTP(ureq) — 호출부가 백그라운드 스레드에서 돌린다. User-Agent 필수(없으면 403).
#[cfg(feature = "update-check")]
fn fetch_latest_release_tag() -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .build();
    let body = agent
        .get("https://api.github.com/repos/ascoeur9/rawblow/releases/latest")
        .set("User-Agent", concat!("RawBlow/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("tag_name").and_then(|t| t.as_str()).map(|s| s.to_string())
}

/// update-check 기능을 끈 빌드(예: cc 없는 환경의 cargo check)에서는 항상 None.
#[cfg(not(feature = "update-check"))]
fn fetch_latest_release_tag() -> Option<String> {
    None
}

/// `v1.2.3`/`1.2.3`(프리릴리스/빌드 메타 허용) → (major, minor, patch). 실패 시 None.
fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let t = s.trim().trim_start_matches(['v', 'V']);
    let core = t.split(['-', '+']).next().unwrap_or(t);
    let mut it = core.split('.');
    let a = it.next()?.trim().parse().ok()?;
    let b = it.next().unwrap_or("0").trim().parse().ok()?;
    let c = it.next().unwrap_or("0").trim().parse().ok()?;
    Some((a, b, c))
}

/// 최신 태그가 현재 버전보다 높으면 표시용 버전 문자열을 반환(아니면 None).
fn newer_version(latest_tag: &str) -> Option<String> {
    let cur = parse_semver(env!("CARGO_PKG_VERSION"))?;
    let lat = parse_semver(latest_tag)?;
    (lat > cur).then(|| latest_tag.trim().to_string())
}

/// 모달 다이얼로그용 공통 프레임(앱 디자인에 맞춘 패널: 어두운 배경 + 테두리 + 둥근 모서리 + 여백).
/// egui 기본 윈도우 크롬 대신 이걸 쓰고 제목줄은 끈다(title_bar(false)).
fn modal_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::BG2)
        .stroke(Stroke::new(1.0, theme::LINE2))
        .rounding(12.0)
        .inner_margin(egui::Margin::same(22.0))
}

/// 모달 헤더(제목 + 부제 + 구분선).
fn modal_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(egui::RichText::new(title).font(prop(18.0)).color(theme::INK));
    if !subtitle.is_empty() {
        ui.add_space(3.0);
        ui.label(egui::RichText::new(subtitle).font(mono(11.0)).color(theme::INK3));
    }
    ui.add_space(12.0);
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
    ui.add_space(14.0);
}

/// 폼 섹션 캡션(대문자 작은 라벨).
fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).font(prop(10.0)).color(theme::INK3));
    ui.add_space(5.0);
}

/// 전체 너비 1px 구분선.
fn hline_full(ui: &mut egui::Ui) {
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
}

/// CheckChip: 라벨색 배경(체크 시 18%)+테두리 알약. 클릭되면 true.
fn check_chip(ui: &mut egui::Ui, label: &str, count: Option<usize>, color: Color32, checked: bool) -> bool {
    let fill = if checked { color.linear_multiply(0.18) } else { theme::BG1 };
    let stroke = Stroke::new(1.0, if checked { color.linear_multiply(0.6) } else { theme::LINE2 });
    let inner = egui::Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(12.0, 7.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                ui.painter().rect(
                    r,
                    Rounding::same(3.0),
                    if checked { color } else { Color32::TRANSPARENT },
                    Stroke::new(1.5, if checked { color } else { theme::LINE3 }),
                );
                if checked {
                    let p = ui.painter();
                    let dark = Color32::from_rgb(0x0a, 0x14, 0x20);
                    p.add(egui::Shape::line(
                        vec![
                            Pos2::new(r.left() + 3.0, r.center().y + 0.5),
                            Pos2::new(r.center().x - 0.5, r.bottom() - 3.5),
                            Pos2::new(r.right() - 2.5, r.top() + 3.5),
                        ],
                        Stroke::new(1.7, dark),
                    ));
                }
                ui.add_space(3.0);
                ui.label(egui::RichText::new(label).font(prop(12.0)).color(if checked { theme::INK } else { theme::INK2 }));
                if let Some(n) = count {
                    ui.label(egui::RichText::new(n.to_string()).font(mono(10.5)).color(theme::INK3));
                }
            });
        });
    let resp = inner.response.interact(Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Segmented: BG1 트랙 + 활성 칸 BG4. 각 칸은 (라벨, 서브라벨) 2줄. 클릭된 인덱스 반환.
fn segmented(ui: &mut egui::Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::none()
        .fill(theme::BG1)
        .stroke(Stroke::new(1.0, theme::LINE2))
        .rounding(6.0)
        .inner_margin(egui::Margin::same(3.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.horizontal(|ui| {
                for (i, (label, sub)) in options.iter().enumerate() {
                    let active = i == selected;
                    let cell = egui::Frame::none()
                        .fill(if active { theme::BG4 } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.0, if active { theme::LINE3 } else { Color32::TRANSPARENT }))
                        .rounding(4.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 5.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(*label).font(prop(12.0)).color(if active { theme::INK } else { theme::INK2 }));
                                if !sub.is_empty() {
                                    ui.label(egui::RichText::new(*sub).font(mono(9.5)).color(if active { theme::INK3 } else { theme::INK4 }));
                                }
                            });
                        });
                    let resp = cell.response.interact(Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}

/// 보내기 아이콘(오른쪽 삼각형) — accent.
fn send_icon(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let s = size * 0.5;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(center.x - s, center.y - s),
            Pos2::new(center.x + s, center.y),
            Pos2::new(center.x - s, center.y + s),
            Pos2::new(center.x - s * 0.4, center.y),
        ],
        color,
        Stroke::NONE,
    ));
}

/// 사진 위 HUD용 RGB 히스토그램(채널당 64 bins).
struct Histo {
    bins: [[u32; 64]; 3],
    max: u32,
}

/// 디코딩된 RGBA에서 히스토그램을 계산(대용량은 서브샘플로 비용 제한).
fn compute_histo(rgba: &[u8]) -> Histo {
    let mut bins = [[0u32; 64]; 3];
    let px = rgba.len() / 4;
    if px == 0 {
        return Histo { bins, max: 1 };
    }
    let step = (px / 200_000).max(1);
    let mut i = 0;
    while i < px {
        let o = i * 4;
        bins[0][rgba[o] as usize >> 2] += 1;
        bins[1][rgba[o + 1] as usize >> 2] += 1;
        bins[2][rgba[o + 2] as usize >> 2] += 1;
        i += step;
    }
    let max = bins.iter().flatten().copied().max().unwrap_or(1).max(1);
    Histo { bins, max }
}

fn exif_lines(ex: &ExifInfo) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(cam) = &ex.camera {
        lines.push(cam.clone());
    }
    if let Some(lens) = &ex.lens {
        lines.push(lens.clone());
    }
    let mut expo = Vec::new();
    if let Some(v) = &ex.aperture {
        expo.push(v.clone());
    }
    if let Some(v) = &ex.shutter {
        expo.push(format!("{}s", v));
    }
    if let Some(v) = &ex.iso {
        expo.push(format!("ISO {}", v));
    }
    if let Some(v) = &ex.focal_length {
        expo.push(v.clone());
    }
    if !expo.is_empty() {
        lines.push(expo.join("  "));
    }
    if let Some(dt) = &ex.datetime {
        lines.push(format_capture_datetime(dt));
    }
    lines
}

/// EXIF 촬영일시 표시 형식(#29): "YYYY:MM:DD HH:MM:SS" → "YYYY.MM.DD HH:MM:SS".
/// 날짜부의 콜론만 점으로 바꾸고 시각부(있으면)는 콜론을 유지한다. 예상과 다른 형식이면
/// 안전하게 원본을 그대로 반환한다(공백 없으면 날짜만 있는 것으로 보고 콜론→점).
fn format_capture_datetime(dt: &str) -> String {
    match dt.split_once(' ') {
        Some((date, time)) => format!("{} {}", date.replace(':', "."), time),
        None => dt.replace(':', "."),
    }
}

/// 플랫폼별 Command/Ctrl 표기(#32). macOS는 `⌘`, 그 외(Windows/Linux)는 `Ctrl+`.
/// Windows에서 `⌘` 글리프가 폴백 폰트로 그려지며 세로로 떠 보이는 문제 + 실제 키도 Ctrl이라
/// 텍스트로 대체한다(예: `{cmd}O` → mac "⌘O", win "Ctrl+O").
fn cmd_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// 외부 URL을 기본 브라우저로 연다(#18). reveal_in_file_manager와 같은 무음 spawn 패턴.
fn open_url(url: &str) {
    use std::process::Command;
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        // explorer로 http(s) URL을 열면 기본 브라우저가 뜬다(콘솔 창 깜빡임 없음).
        Command::new("explorer").arg(url).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
}

/// 설정 화면용 클릭 가능한 링크(accent 밑줄). 클릭하면 브라우저로 이동(#18).
fn link_label(ui: &mut egui::Ui, text: &str, url: &str) {
    let resp = ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(prop(11.5))
                .color(theme::ACCENT)
                .underline(),
        )
        .sense(Sense::click()),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        open_url(url);
    }
}

/// 바이트 수를 사람이 읽는 단위로(설정의 캐시 사용량 표시용, #22).
fn fmt_bytes(n: u64) -> String {
    let f = n as f64;
    if f >= 1_073_741_824.0 {
        format!("{:.2} GB", f / 1_073_741_824.0)
    } else if f >= 1_048_576.0 {
        format!("{:.1} MB", f / 1_048_576.0)
    } else if f >= 1024.0 {
        format!("{:.0} KB", f / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// OS 파일 탐색기에서 폴더를 띄운다(Finder / Explorer / xdg-open). 실패 시 무음.
///
/// spawn()의 반환을 `let _ =`로 버리는 점이 핵심이다. 예전에는 동작 자체를
/// 앱 내부 `open_folder()`(작업 폴더 전환)에 위임했는데, macOS에서 강제종료를
/// 일으키는 경로였다(#5). 외부 명령은 부재/오류 시에도 패닉 없이 그냥 무시한다.
fn reveal_in_file_manager(path: &std::path::Path) {
    use std::process::Command;
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer").arg(path).spawn()
    } else {
        Command::new("xdg-open").arg(path).spawn()
    };
}

#[cfg(test)]
mod tests {
    use super::{format_capture_datetime, hex_str, newer_version, parse_hex_rgb, parse_semver};

    #[test]
    fn capture_datetime_uses_dots_for_date_keeps_colons_for_time() {
        // EXIF 표준 "YYYY:MM:DD HH:MM:SS" → "YYYY.MM.DD HH:MM:SS" (#29).
        assert_eq!(format_capture_datetime("2024:06:03 14:30:45"), "2024.06.03 14:30:45");
        // 날짜만 있는 경우(공백 없음)도 안전하게 점으로.
        assert_eq!(format_capture_datetime("2024:06:03"), "2024.06.03");
        // 이미 점/다른 형식이어도 깨지지 않음.
        assert_eq!(format_capture_datetime("2024.06.03 14:30:45"), "2024.06.03 14:30:45");
    }

    #[test]
    fn hex_rgb_roundtrip_and_parsing() {
        // #/무접두, 대소문자 허용.
        assert_eq!(parse_hex_rgb("#808080"), Some([0x80, 0x80, 0x80]));
        assert_eq!(parse_hex_rgb("808080"), Some([0x80, 0x80, 0x80]));
        assert_eq!(parse_hex_rgb("  #FfFfFf "), Some([0xff, 0xff, 0xff]));
        // 잘못된 형식은 None(부분 입력 중 색이 튀지 않게).
        assert_eq!(parse_hex_rgb("#80808"), None);
        assert_eq!(parse_hex_rgb("xyzxyz"), None);
        assert_eq!(parse_hex_rgb(""), None);
        // 왕복.
        assert_eq!(hex_str([0x1e, 0x1e, 0x1e]), "#1E1E1E");
        assert_eq!(parse_hex_rgb(&hex_str([0x06, 0x07, 0x0a])), Some([0x06, 0x07, 0x0a]));
    }

    #[test]
    fn semver_parse_and_compare() {
        assert_eq!(parse_semver("v0.4.3"), Some((0, 4, 3)));
        assert_eq!(parse_semver("0.4.3"), Some((0, 4, 3)));
        assert_eq!(parse_semver("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_semver("v0.6.0-rc1"), Some((0, 6, 0)));
        assert_eq!(parse_semver("nope"), None);
        // 현재 버전보다 높은 태그만 새 버전으로 인식.
        assert!(newer_version("v999.0.0").is_some());
        assert!(newer_version("v0.0.1").is_none());
        assert!(newer_version(env!("CARGO_PKG_VERSION")).is_none(), "동일 버전은 안내 안 함");
    }
}
