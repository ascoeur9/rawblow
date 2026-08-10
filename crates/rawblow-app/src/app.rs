//! RawBlow GUI 본체. 핸드오프 디자인(Studio 기본 / Cinema 풀스크린)을 egui로 구현.

use crate::i18n::{tr, trf};
use crate::theme;
use crate::widgets::{self, draw_thumb, hud_text, kbd, mono, prop, section_head, ThumbInfo, TexCache};
use crate::worker::{DecodeRequest, Worker};
use eframe::egui;
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use rawblow_core::cache;
use rawblow_core::config::{self, AiCullTarget, ClipIqaBackbone, Config, Lang, ViewCarry};
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
use std::path::{Path, PathBuf};
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
mod views;
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
/// 디코드 실패 고착 임계(#64, #75). 이 횟수 이상 **연속** 실패하면 영구 손상으로 보고 재시도를
/// 멈춘다(성공 시 카운터 리셋). NAS 등 일시적 끊김이 3회 임계에 쉽게 닿아 정상 파일이 고착되던
/// 문제(#75)로 3→5 상향. ⚠ 클릭 수동 재시도(retry_decode)로 고착 파일도 회복 가능.
const DECODE_DEAD_THRESHOLD: u8 = 5;

/// 촬영시간순 정렬(#56)용 백그라운드 EXIF 시각 수집 결과. 항목 순서는 수집 시작 시점의
/// items 순서와 같으며, generation이 다르면(폴더 전환 등) 버린다.
struct SortScanResult {
    generation: u64,
    times: Vec<Option<i64>>,
}

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

/// 우하단 토스트(#61) 심각도. 지속시간이 다르다: 정보는 짧게, 알림은 조금 길게, 오류는
/// 자동으로 사라지지 않는다(놓치면 안 되는 실패 보고 — 사용자가 클릭·✕로 닫아야 함).
#[derive(Clone, Copy, Debug, PartialEq)]
enum ToastKind {
    Info,
    Notice,
    Error,
}

impl ToastKind {
    /// 자동 만료까지의 시간. None이면 만료 없음(오류 — 클릭 전까지 유지).
    fn duration(self) -> Option<Duration> {
        match self {
            ToastKind::Info => Some(Duration::from_secs(3)),
            ToastKind::Notice => Some(Duration::from_secs(6)),
            ToastKind::Error => None,
        }
    }
}

/// 우하단에 잠깐 뜨는 상태 메시지(#61). 심각도별 색·지속시간으로 표시한다.
struct Toast {
    text: String,
    at: Instant,
    kind: ToastKind,
}

/// 컬링 편집 1회의 되돌리기 스냅샷(#78). 편집 대상 항목들의 편집 **전** (라벨·별점·태그)과
/// 편집 시점의 필터 뷰 위치를 담는다. undo는 값을 복원하고 그 사진으로 뷰를 되돌리며, redo는
/// 대칭으로 동작한다. real 인덱스는 폴더 전환·재정렬 시 재배정되므로 그때 스택을 비운다.
#[derive(Clone)]
struct CullEdit {
    /// (real, 라벨, 별점, 태그) — 편집 대상 항목들의 편집 전 상태.
    prev: Vec<(usize, Label, u8, ColorTag)>,
    /// 편집 시점의 self.index(필터 뷰 위치).
    index: usize,
}

/// undo/redo 이력 최대 길이. 빠른 컬링에서 충분히 길되 메모리는 묶는다.
const UNDO_LIMIT: usize = 300;

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
    zoom_for: Option<usize>, // 줌 상태가 적용된 항목(real). 바뀌면 keep_zoom을 복원하거나 fit으로 리셋
    last_view_size: Option<Vec2>, // #48: 마지막으로 표시한 텍스처 크기(px). 이것과 달라지면 같은 항목에서 해상도가 바뀐 프레임(ORIG 토글).
    /// #48: 표시 해상도가 바뀌어도(ORIG 로드/언로드) 유지할 화면상 배율(`ViewMag` 참조).
    view_mag: ViewMag,
    /// #48: 이 항목에서 본 텍스처 중 가장 긴 변(px). 확대 상한을 현재 텍스처가 아니라 이
    /// 기준으로 잡아야 프리뷰와 ORIG에서 같은 화면상 배율 범위가 나온다. 항목이 바뀌면 0.
    view_ref_long: f32,
    af_zoom_pending: bool,   // #49: 다음 1:1 확대를 AF 측거점 중심에 맞추라는 요청.
    /// #85: 사진을 넘겨도 이어받을 확대 상태. `zoom`/`pan`을 그대로 물려주면 안 된다 —
    /// `zoom`은 화면px/**텍스처**px이고 텍스처는 이미 회전·다운스케일된 것이라, 해상도나
    /// 가로/세로가 다른 다음 사진에서 배율이 튄다. 그래서 해상도·방향에 불변인 두 값으로
    /// 정규화해 둔다: `mag` = 이미지 **긴 변**이 차지하는 화면 px, `pan_norm` = 표시 크기
    /// 대비 이동 비율. 창맞춤(fit) 상태면 None(= 다음 사진도 창맞춤).
    keep_zoom: Option<(f32, Vec2)>,
    /// #85: 새 항목에 keep_zoom을 아직 확정 복원하지 못했다. 프리뷰 텍스처가 도착하기 전
    /// 프레임에서는 320px 썸네일이 대신 뜨는데, 거기에 맞춰 클램프하면 배율이 깎이므로
    /// 프리뷰가 올 때까지 매 프레임 다시 맞춘다.
    zoom_restore: bool,
    grid_cols: usize,
    sort: SortOrder,
    // 촬영시간순 정렬(#56): 백그라운드 EXIF 시각 수집 상태. gen이 현재와 같으면 수집 완료/진행 중.
    sort_scan_gen: Option<u64>,
    sort_rx: Option<crossbeam_channel::Receiver<SortScanResult>>,

    // 그리드 다중 선택(Ctrl/Shift+클릭) — 항목(real) 인덱스 집합 + 범위 선택 앵커(필터 인덱스).
    selected: std::collections::HashSet<usize>,
    sel_anchor: Option<usize>,
    // 그리드 키보드 내비 시 선택 셀이 보이도록 스크롤할 목표 행(다음 프레임에 적용).
    grid_scroll_to: Option<usize>,
    // 마지막 프레임에 그리드에 보였던 행 범위(스크롤 필요 여부 판단용).
    grid_visible_rows: std::ops::Range<usize>,

    // 컬링 되돌리기(#78): 라벨·별점·태그 변경 이력. Ctrl/⌘Z로 직전 편집을 취소, ⇧를 더해 재실행.
    // 폴더 전환·재정렬(real 인덱스 재배정) 시 비운다. 새 편집이 생기면 redo_stack은 무효화.
    undo_stack: Vec<CullEdit>,
    redo_stack: Vec<CullEdit>,

    worker: Worker,
    cache: TexCache,  // 단일/전체화면 프리뷰(큰 해상도)
    thumbs: TexCache, // 그리드/필름스트립/열화 폴백(작은 해상도)
    pending_preview: std::collections::HashSet<usize>,
    pending_thumb: std::collections::HashSet<usize>,
    pending_thumb_prio: std::collections::HashSet<usize>, // 우선 레인으로 승격된 썸네일
    pending_prefetch: std::collections::HashSet<usize>,   // 백그라운드 디스크 캐시 프리페치 중
    failed_preview: std::collections::HashSet<usize>,
    failed_thumb: std::collections::HashSet<usize>,
    // 항목(real)별 디코딩 누적 실패 횟수(#64). decode_dead()가 3 이상을 영구 손상으로 간주.
    decode_fails: std::collections::HashMap<usize, u8>,
    histo: std::collections::HashMap<usize, Histo>,
    generation: u64,

    sidecar_dirty: bool,
    last_save: Instant,
    // 사이드카 저장 실패 표면화(#62). 예전엔 save 결과를 버리고 dirty를 내려, 읽기 전용
    // 폴더·권한·용량 부족에서 상태바가 "saved"인 채 세션 전체가 무음 유실됐다.
    // None=정상. Some(원인)=마지막 저장 실패(상태바 '저장 실패' + hover 원인 표시).
    save_error: Option<String>,
    // 연속 저장 실패 횟수(#62). 재시도 백오프(sidecar_retry_interval) 판단용 —
    // 성공하거나 폴더를 바꾸면 0으로 리셋.
    save_fail_count: u32,

    transfer: Option<TransferDialogState>,
    organize: Option<OrganizeDialogState>,
    // 진행 중인 백그라운드 파일 작업(전송/정리)의 프로그레스바 상태(#35).
    progress: Option<ProgressJob>,
    result: Option<TransferReport>,
    // 결과 모달이 전송이 아니라 폴더 정리 작업의 결과인지(#63): 제목을 "정리 완료"로 바꾸는 데 쓴다.
    result_organize: bool,
    show_settings: bool,
    // 기본값 복원(#69): '기본값 복원' 버튼을 2단 확인으로. true면 확인 행(복원/취소)이 뜬다.
    // 설정을 열 때마다 false로 리셋(설정을 떠나면 arm 상태가 풀린다).
    settings_reset_armed: bool,
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

    // 단축키 치트시트 오버레이(#66). ?/F1·툴바 ? 버튼으로 여닫는다. 열려 있는 동안은
    // has_modal에 포함돼 사진 단축키(QWER·별점·M/A/Z/F 등)를 가로채지 않는다.
    show_help: bool,

    // 우하단 토스트(#61). 심각도별 색·지속시간 — helper(toast_info/notice/error)로만 설정.
    toast: Option<Toast>,
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
            view_mag: ViewMag::default(),
            view_ref_long: 0.0,
            af_zoom_pending: false,
            keep_zoom: None,
            zoom_restore: false,
            grid_cols: cfg.grid_cols.clamp(4, 12),
            sort: cfg.sort,
            sort_scan_gen: None,
            sort_rx: None,
            selected: std::collections::HashSet::new(),
            sel_anchor: None,
            grid_scroll_to: None,
            grid_visible_rows: 0..0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
            decode_fails: std::collections::HashMap::new(),
            histo: std::collections::HashMap::new(),
            generation: 0,
            sidecar_dirty: false,
            last_save: Instant::now(),
            save_error: None,
            save_fail_count: 0,
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
            result_organize: false,
            show_settings: false,
            settings_reset_armed: false,
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
            show_help: false,
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

    // ── 토스트(#61) ──────────────────────────────────────────
    // 우하단 오버레이 메시지. 심각도별 헬퍼로 통일해 색·지속시간이 한 곳(ToastKind)에서
    // 결정되게 한다. 모든 설정 지점은 이 셋 중 하나만 쓴다(튜플 직접 대입 금지).
    fn toast_info(&mut self, text: String) {
        self.toast = Some(Toast { text, at: Instant::now(), kind: ToastKind::Info });
    }
    fn toast_notice(&mut self, text: String) {
        self.toast = Some(Toast { text, at: Instant::now(), kind: ToastKind::Notice });
    }
    fn toast_error(&mut self, text: String) {
        self.toast = Some(Toast { text, at: Instant::now(), kind: ToastKind::Error });
    }

    fn open_folder(&mut self, folder: PathBuf) {
        // 폴더를 떠나기 전 지금 보던 사진을 재개 위치로 남긴다(#86). 아래 config::save가
        // 디스크까지 확정하므로 여기서 따로 저장하지 않는다.
        self.remember_folder_position();
        // 폴더를 바꾸기 전, 디바운스 대기 중인 분류/별점 변경을 현재 폴더 사이드카에 먼저 확정한다.
        // (Move 후 재스캔(#24)이나 폴더 전환 시 미저장 변경이 옛 사이드카로 롤백·유실되지 않게.)
        // 이 플러시가 실패해도 전환은 계속한다(#62): items가 곧 새 폴더 것으로 바뀌어 재시도할
        // 원본이 사라지므로 여기서 버틸 수 없다 — 대신 아래에서 유실 가능성을 Error 토스트로 알린다.
        let mut flush_failed: Option<String> = None;
        if self.sidecar_dirty {
            if let Some(cur) = &self.folder {
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                if sidecar::save(cur, &entries).is_err() {
                    // 안내용 옛 폴더명(마지막 경로 요소, lossy). 루트 등 file_name이 없으면 전체 경로.
                    flush_failed = Some(
                        cur.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| cur.to_string_lossy().into_owned()),
                    );
                }
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
        // 마지막으로 보던 사진에서 재개(#86). 기록이 없는 새 폴더는 종전대로 0번에서 시작한다.
        self.index = self.resume_index(&folder);
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
        self.decode_fails.clear(); // real 인덱스가 재배정되므로 실패 카운터도 함께 리셋(#64).
        self.undo_stack.clear(); // real 인덱스 재배정 → 되돌리기 스냅샷도 무효(#78).
        self.redo_stack.clear();
        self.histo.clear();
        self.selected.clear();
        self.sel_anchor = None;
        self.grid_scroll_to = None;
        self.grid_visible_rows = 0..0;
        // 확대 상태 유지(#85)는 **같은 폴더 안 이동**에만 적용한다 — 새 폴더는 창맞춤에서 시작.
        // (zoom_for를 비워 두지 않으면 새 폴더의 0번이 직전 폴더의 0번과 같은 real로 보여
        //  넘김 감지가 안 걸린다.)
        self.zoom_for = None;
        self.keep_zoom = None;
        self.zoom_restore = false;
        self.fit = true;
        self.pan = Vec2::ZERO;
        self.full_raw = false;
        // 저장 실패 상태는 폴더 단위(#62) — 새 폴더는 깨끗한 상태·기본 재시도 간격으로 시작.
        self.save_error = None;
        self.save_fail_count = 0;
        self.cfg.push_recent(&folder.to_string_lossy());
        let _ = config::save(&self.cfg);
        self.folder = Some(folder);
        self.toast_info(trf(self.lang, "{} 항목 로드", &[&self.items.len().to_string()]));
        // 플러시 실패 안내는 위 로드 토스트 **뒤에** 설정한다(#62): 토스트 슬롯이 하나뿐이라
        // 마지막 설정이 이기므로, 순서가 앞서면 정보 토스트가 실패 안내를 덮어 유실이 다시
        // 무음이 된다. (로드 안내를 굳이 막을 필요는 없고 — 정상 경로 동작 불변 — Error는
        // 자동 만료가 없어 이 순서로도 반드시 사용자 눈에 닿는다.)
        if let Some(name) = flush_failed {
            self.toast_error(trf(
                self.lang,
                "이전 폴더({}) 셀렉 저장 실패 — 최근 변경이 유실될 수 있습니다",
                &[&name],
            ));
        }
        self.schedule_cache_trim(); // 폴더 열 때 캐시 상한 정리(다른/오래된 폴더 썸네일 회수).
        // 프리페치는 폴더 전체가 아니라 현재 위치 주변 윈도우만(update에서 매 프레임 슬라이드).
    }


    /// 촬영시간순(#56): 현재 폴더의 EXIF 촬영시각을 백그라운드로 수집한다. 정렬 기준이
    /// 촬영시간순이고 이번 generation에 대해 아직 시작하지 않았을 때만 동작 — 폴더가 바뀌면
    /// generation이 올라가 자동 재수집. 이미 로드된 EXIF는 재독 없이 쓰고 나머지만 워커
    /// 스레드에서 prefix read(ensure_cull_meta_scan과 동일 패턴 — UI·NAS 비차단).
    fn ensure_capture_sort(&mut self, ctx: &egui::Context) {
        if self.sort != SortOrder::CaptureTime || self.items.is_empty() {
            return;
        }
        if self.sort_scan_gen == Some(self.generation) {
            return; // 이번 폴더는 이미 수집(또는 진행 중).
        }
        self.sort_scan_gen = Some(self.generation);
        let mut times: Vec<Option<i64>> = Vec::with_capacity(self.items.len());
        let mut todo: Vec<(usize, PathBuf)> = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if it.exif_loaded {
                times.push(item_capture_secs(it));
            } else {
                times.push(None);
                todo.push((i, it.entry.display.clone()));
            }
        }
        if todo.is_empty() {
            self.apply_capture_sort(times); // 전부 로드돼 있었음 — 즉시 재정렬.
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        self.sort_rx = Some(rx);
        let gen = self.generation;
        std::thread::spawn(move || {
            let mut times = times;
            for (i, p) in todo {
                times[i] = read_exif(&p)
                    .and_then(|e| e.datetime)
                    .as_deref()
                    .and_then(rawblow_core::cull_ext::parse_exif_datetime);
            }
            let _ = tx.send(SortScanResult { generation: gen, times });
        });
        ctx.request_repaint_after(Duration::from_millis(120));
    }

    /// 촬영시각 수집 결과 반영(#56). 폴더가 바뀐(generation 불일치) 결과는 버린다.
    fn drain_capture_sort(&mut self, ctx: &egui::Context) {
        let res = match &self.sort_rx {
            Some(rx) => rx.try_recv(),
            None => return,
        };
        match res {
            Ok(r) => {
                self.sort_rx = None;
                if r.generation == self.generation && self.sort == SortOrder::CaptureTime {
                    self.apply_capture_sort(r.times);
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => self.sort_rx = None,
        }
    }

    /// 촬영시각으로 items를 재정렬(#56). `times`는 현재 items 순서 기준.
    fn apply_capture_sort(&mut self, times: Vec<Option<i64>>) {
        if times.len() != self.items.len() {
            return; // 수집 중 항목 수가 변함(방어) — 다음 generation에서 재수집.
        }
        let keys: Vec<(Option<i64>, String)> = self
            .items
            .iter()
            .zip(&times)
            .map(|(it, t)| (*t, item_file_name(it)))
            .collect();
        let order = scan::capture_order(&keys);
        if order.iter().enumerate().all(|(i, &o)| i == o) {
            return; // 이미 촬영시간순 — 상태 리셋 불필요.
        }
        self.reorder_items(&order);
    }

    /// 정렬 기준 변경(#56, 설정 UI). 파일명순은 EXIF가 필요 없어 즉시 재정렬하고,
    /// 촬영시간순은 다음 프레임 ensure_capture_sort에서 백그라운드 수집을 시작한다.
    fn set_sort_order(&mut self, sort: SortOrder) {
        if self.sort == sort && self.cfg.sort == sort {
            return;
        }
        self.cfg.sort = sort;
        let _ = config::save(&self.cfg);
        self.sort = sort;
        match sort {
            SortOrder::Name | SortOrder::Modified => {
                self.sort_rx = None; // 진행 중 수집 결과는 무시(sort 검사로도 걸러짐).
                let keys: Vec<(Option<i64>, String)> =
                    self.items.iter().map(|it| (None, item_file_name(it))).collect();
                let order = scan::capture_order(&keys); // 시각 전부 None = 파일명 자연정렬.
                if !order.iter().enumerate().all(|(i, &o)| i == o) {
                    self.reorder_items(&order);
                }
            }
            SortOrder::CaptureTime => {
                self.sort_scan_gen = None; // 강제 재수집.
            }
        }
    }

    /// items를 주어진 순열로 재배열하고 인덱스 기반 상태를 리셋한다(#56).
    /// real 인덱스가 전부 바뀌므로 open_folder와 동일하게 세대를 올려 in-flight 디코딩
    /// 결과를 무효화하고 캐시·pending·히스토그램·선택을 비운다. 라벨·별점은 Item과 함께
    /// 이동하고 사이드카는 파일명 키라 안전. 현재 보던 사진은 새 위치로 따라간다.
    fn reorder_items(&mut self, order: &[usize]) {
        let cur_path = self
            .current_real()
            .and_then(|r| self.items.get(r))
            .map(|it| it.entry.display.clone());
        let mut old: Vec<Option<Item>> =
            std::mem::take(&mut self.items).into_iter().map(Some).collect();
        self.items = order.iter().map(|&i| old[i].take().expect("순열 인덱스 중복")).collect();
        self.generation += 1;
        self.worker.set_generation(self.generation);
        self.sort_scan_gen = Some(self.generation); // 방금 정렬한 결과 — 재수집 루프 방지.
        self.cache.retire_all();
        self.thumbs.retire_all();
        self.pending_preview.clear();
        self.pending_thumb.clear();
        self.pending_thumb_prio.clear();
        self.pending_prefetch.clear();
        self.failed_preview.clear();
        self.failed_thumb.clear();
        self.decode_fails.clear(); // real 인덱스가 재배정되므로 실패 카운터도 함께 리셋(#64).
        self.undo_stack.clear(); // real 인덱스 재배정 → 되돌리기 스냅샷도 무효(#78).
        self.redo_stack.clear();
        self.histo.clear();
        self.selected.clear();
        self.sel_anchor = None;
        self.grid_scroll_to = None;
        self.grid_visible_rows = 0..0;
        self.zoom_for = None;
        // 보던 사진의 새 위치로 index 복원(필터 목록 기준).
        self.index = cur_path
            .and_then(|p| {
                let f = self.filtered();
                f.iter().position(|&r| self.items[r].entry.display == p)
            })
            .unwrap_or(0);
    }

    /// 지금 보고 있는 사진을 폴더별 재개 위치로 기록한다(#86).
    ///
    /// `Config`에만 반영하고 디스크 저장은 호출부에 맡긴다 — 호출 지점(폴더 전환·앱 종료)이
    /// 모두 직후에 `config::save`를 하므로 이중 쓰기를 피한다. 사진을 넘길 때마다 저장하지
    /// 않는 이유도 같다: 셀렉 중 화살표 한 번마다 설정 파일을 쓰는 비용이 아깝고, 재개 위치는
    /// 폴더를 떠나는 순간의 값만 있으면 충분하다.
    fn remember_folder_position(&mut self) {
        let Some(folder) = self.folder.clone() else { return };
        let Some(real) = self.current_real() else { return };
        let Some(item) = self.items.get(real) else { return };
        let file = item.entry.display.to_string_lossy().into_owned();
        self.cfg.set_folder_resume(&folder.to_string_lossy(), &file);
    }

    /// 폴더를 열 때 시작할 필터 기준 인덱스(#86). 기록된 사진을 현재 `items`에서 찾아
    /// 필터 목록 상의 위치로 환산한다. 기록이 없거나, 그 파일이 사라졌거나, 현재 필터에
    /// 걸러졌으면 0(첫 사진)으로 폴백한다 — 어느 경우에도 오류 없이 열린다.
    ///
    /// `self.items`가 새 폴더 것으로 교체된 **뒤에** 불러야 한다.
    fn resume_index(&self, folder: &Path) -> usize {
        let Some(saved) = self.cfg.folder_resume_file(&folder.to_string_lossy()) else {
            return 0;
        };
        let Some(real) = self
            .items
            .iter()
            .position(|it| it.entry.display.to_string_lossy() == saved)
        else {
            return 0;
        };
        self.filtered().iter().position(|&r| r == real).unwrap_or(0)
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

    /// 필터 세 축(라벨·별점·태그) 중 하나라도 기본값이 아니면 true(#67). 빈 화면에서
    /// "폴더가 원래 비어 있음"과 "필터가 전량 배제함"을 구분하는 데 쓴다.
    fn any_filter_active(&self) -> bool {
        self.filter != Filter::All
            || self.star_filter != StarFilter::Any
            || self.tag_filter != TagFilter::Any
    }

    /// 필터 세 축을 모두 기본값으로 되돌린다(#67). 레일의 개별 필터 클릭 핸들러와 동일하게
    /// index도 0으로 리셋한다.
    fn reset_filters(&mut self) {
        self.filter = Filter::All;
        self.star_filter = StarFilter::Any;
        self.tag_filter = TagFilter::Any;
        self.index = 0;
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

    /// 항목(real)의 디코딩이 영구 실패로 판단되는지(#64). 임계 5회 = 일시적 I/O 오류(NAS 끊김 등)와
    /// 영구 손상을 구분 — 미만이면 prio 재시도를 계속하고, 이상이면 멈추고 에러 상태(⚠)를 보여준다.
    /// #75: 카운터는 성공 디코드 시 리셋되므로(drain_results) 이 임계는 **연속** 실패 횟수에 가깝다.
    /// 임계를 넘어 고착된 파일도 ⚠ 클릭(retry_decode)으로 카운터를 지워 회복할 수 있다.
    fn decode_dead(&self, real: usize) -> bool {
        self.decode_fails.get(&real).is_some_and(|&n| n >= DECODE_DEAD_THRESHOLD)
    }

    /// ⚠(디코드 실패 고착) 상태에서 사용자가 수동으로 재시도한다(#75). 실패 카운터·마킹을 지우고
    /// 프리뷰·썸네일을 우선 레인으로 다시 요청 — NAS/네트워크 복구 후 폴더를 다시 열지 않고 회복한다.
    fn retry_decode(&mut self, real: usize) {
        self.decode_fails.remove(&real);
        self.failed_preview.remove(&real);
        self.failed_thumb.remove(&real);
        let cur_edge = if self.full_raw { Some(ORIG_EDGE) } else { Some(PREVIEW_EDGE) };
        self.request_preview(real, cur_edge, self.full_raw, true);
        self.request_thumb(real, true);
    }





    /// 컬링 변경(라벨/별점/태그) 직전 상태를 undo 이력에 기록한다(#78). `reals` 중 실제 존재하는
    /// 항목만 스냅샷하며, 대상이 없으면 아무 것도 하지 않는다. 새 편집이므로 redo 스택은 비운다.
    fn push_undo(&mut self, reals: &[usize]) {
        let prev: Vec<(usize, Label, u8, ColorTag)> = reals
            .iter()
            .filter_map(|&r| self.items.get(r).map(|it| (r, it.entry.label, it.entry.stars, it.entry.tag)))
            .collect();
        if prev.is_empty() {
            return;
        }
        self.undo_stack.push(CullEdit { prev, index: self.index });
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 스냅샷의 (라벨·별점·태그)를 현재 항목들에 적용하고, 적용 **전** 상태를 반대 방향
    /// 스택용 스냅샷으로 반환한다. 편집 대상 사진으로 뷰 위치도 되돌린다(#78, undo/redo 공용).
    fn apply_cull_snapshot(&mut self, edit: &CullEdit) -> CullEdit {
        let inverse: Vec<(usize, Label, u8, ColorTag)> = edit
            .prev
            .iter()
            .filter_map(|&(r, ..)| self.items.get(r).map(|it| (r, it.entry.label, it.entry.stars, it.entry.tag)))
            .collect();
        let inverse_index = self.index;
        for &(r, label, stars, tag) in &edit.prev {
            if let Some(it) = self.items.get_mut(r) {
                it.entry.label = label;
                it.entry.stars = stars;
                it.entry.tag = tag;
            }
        }
        self.sidecar_dirty = true;
        self.focus_after_undo(edit.prev.first().map(|e| e.0), edit.index);
        CullEdit { prev: inverse, index: inverse_index }
    }

    /// undo/redo 후 편집 대상(real)이 현재 필터에 보이면 그 위치로, 아니면 저장된 인덱스로 뷰를 옮긴다.
    /// (라벨 복원으로 필터에서 빠졌던 항목이 다시 보이는 경우를 우선 처리 — 바로 재평가할 수 있게.)
    fn focus_after_undo(&mut self, real: Option<usize>, fallback: usize) {
        let f = self.filtered();
        if f.is_empty() {
            self.index = 0;
            return;
        }
        let pos = real
            .and_then(|r| f.iter().position(|&x| x == r))
            .unwrap_or_else(|| fallback.min(f.len() - 1));
        self.index = pos;
        self.keep_view_mode_on_move(); // 창맞춤이면 프리뷰로, 확대 중이면 ORIG 유지(#85)
        if self.view == ViewMode::Grid {
            self.selected.clear();
            self.sel_anchor = Some(self.index);
            let cols = self.grid_cols.clamp(4, 12);
            self.grid_scroll_to = Some(self.index / cols);
        }
    }

    /// 직전 컬링 편집을 되돌린다(#78, Ctrl/⌘Z). 값 복원 후 그 사진으로 뷰를 옮겨 바로 재평가할 수 있게 한다.
    fn undo(&mut self) {
        let Some(edit) = self.undo_stack.pop() else {
            self.toast_info(tr(self.lang, "되돌릴 작업이 없습니다").to_owned());
            return;
        };
        let inverse = self.apply_cull_snapshot(&edit);
        self.redo_stack.push(inverse);
        self.toast_info(tr(self.lang, "되돌렸습니다").to_owned());
    }

    /// 되돌린 편집을 다시 실행한다(#78, Ctrl/⌘⇧Z · Ctrl/⌘Y).
    fn redo(&mut self) {
        let Some(edit) = self.redo_stack.pop() else {
            self.toast_info(tr(self.lang, "다시 실행할 작업이 없습니다").to_owned());
            return;
        };
        let inverse = self.apply_cull_snapshot(&edit);
        self.undo_stack.push(inverse);
        self.toast_info(tr(self.lang, "다시 실행했습니다").to_owned());
    }

    fn set_label(&mut self, label: Label) {
        if self.cull_axis_locked(AiCullTarget::Label) {
            return;
        }
        // 그리드에서 다중 선택 중이면 선택한 항목 전부에 일괄 적용(토글·자동진행 없음).
        if self.view == ViewMode::Grid && !self.selected.is_empty() {
            let targets: Vec<usize> = self.selected.iter().copied().collect();
            self.push_undo(&targets); // #78
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.label = label;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            self.push_undo(&[real]); // #78
            if let Some(it) = self.items.get_mut(real) {
                // 같은 라벨 재입력 시 미선택으로 토글.
                it.entry.label = if it.entry.label == label {
                    Label::Unrated
                } else {
                    label
                };
                self.sidecar_dirty = true;
                let rated = it.entry.label != Label::Unrated;
                if self.cfg.auto_advance && rated {
                    self.advance_after_rate(real);
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
            self.push_undo(&targets); // #78
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.stars = stars;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            self.push_undo(&[real]); // #78
            if let Some(it) = self.items.get_mut(real) {
                it.entry.stars = if stars != 0 && it.entry.stars == stars { 0 } else { stars };
                self.sidecar_dirty = true;
                let rated = it.entry.stars != 0;
                if allow_advance && self.cfg.auto_advance && rated {
                    self.advance_after_rate(real);
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
            self.push_undo(&targets); // #78
            for real in targets {
                if let Some(it) = self.items.get_mut(real) {
                    it.entry.tag = tag;
                }
            }
            self.sidecar_dirty = true;
            return;
        }
        if let Some(real) = self.current_real() {
            self.push_undo(&[real]); // #78
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

    /// 평가(라벨·별점) 직후의 자동 진행(#55). 방금 평가한 항목이 현재 필터에서 빠지면
    /// 필터 목록이 한 칸 당겨져 index가 이미 다음 사진을 가리키므로 전진하지 않고 범위만
    /// 보정한다. 항목이 필터에 남아 있을 때만 실제로 한 칸 전진한다.
    fn advance_after_rate(&mut self, real: usize) {
        let still_visible = self.items.get(real).is_some_and(|it| {
            self.filter.accepts(it.entry.label)
                && self.star_filter.accepts(it.entry.stars)
                && self.tag_filter.accepts(it.entry.tag)
        });
        if let Some(next) = index_after_rate(self.index, still_visible, self.filtered().len()) {
            self.index = next;
            self.keep_view_mode_on_move(); // advance와 동일(#85)
        }
    }

    /// 사진을 넘길 때의 표시 모드 정리(#85/#87). 인덱스가 바뀌는 모든 경로(화살표·휠·
    /// 필름스트립 클릭·평가 후 자동 전진)에서 공통으로 불린다.
    ///
    /// - `ZoomOnly`(기본, #85 규칙): 창맞춤 상태였으면 빠른 프리뷰(1920px)로 돌아가 넘김을
    ///   가볍게 유지하고, **확대 중이었으면 ORIG(원본 보기)를 유지**한다 — 확대 배율은
    ///   이어받는데 해상도만 프리뷰로 떨어지면 화면이 뿌예져 새 버그처럼 보이기 때문.
    /// - `Keep`(#87): 보고 있던 상태를 그대로 들고 간다. ORIG면 창맞춤이어도 계속 ORIG,
    ///   프리뷰면 계속 프리뷰 — `full_raw`를 건드리지 않는 것이 곧 그 동작이다.
    ///
    /// 새 폴더를 열 때는 `open_folder`가 `full_raw=false`로 되돌리므로 두 옵션 모두
    /// 프리뷰에서 시작한다(ORIG 연속 디코딩이 새 폴더 첫 로드를 늦추지 않게).
    fn keep_view_mode_on_move(&mut self) {
        match self.cfg.view_carry {
            ViewCarry::ZoomOnly => self.full_raw = self.full_raw && !self.fit,
            ViewCarry::Keep => {}
        }
    }

    fn advance(&mut self, delta: i64) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        let cur = self.index as i64 + delta;
        // 끝 도달 피드백(#65): 마지막 사진 너머로 더 밀면(→·휠) 조용히 멈추는 대신
        // 남은 미분류 수를 알려 "다 봤는지"를 확인시켜 준다. 탐색 경로에만 달아
        // 라벨링 자동 전진(advance_after_rate)의 필터 축소 케이스와는 얽히지 않는다.
        if delta > 0 && cur >= len as i64 {
            let unrated = self.counts().3;
            self.toast_info(trf(self.lang, "마지막 사진 · 미분류 {}장", &[&unrated.to_string()]));
        }
        self.index = cur.clamp(0, len as i64 - 1) as usize;
        self.keep_view_mode_on_move(); // 창맞춤이면 프리뷰로 복귀, 확대 중이면 ORIG 유지(#85)
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
        if self.sidecar_dirty && self.last_save.elapsed() > sidecar_retry_interval(self.save_fail_count) {
            if let Some(folder) = &self.folder {
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                match sidecar::save(folder, &entries) {
                    Ok(()) => {
                        self.sidecar_dirty = false;
                        self.last_save = Instant::now();
                        self.save_error = None;
                        self.save_fail_count = 0;
                    }
                    Err(e) => {
                        // dirty를 **유지**해 위 간격으로 계속 재시도한다(#62). 예전엔 결과를
                        // 버리고 dirty를 내려, 상태바가 "saved"인 채 읽기 전용 폴더·용량 부족에서
                        // 세션 전체가 무음 유실됐다. last_save 갱신은 재시도 스로틀용(매 프레임
                        // 실패 I/O 반복 방지 — 다음 시도는 sidecar_retry_interval 뒤).
                        let msg = e.to_string();
                        self.save_fail_count += 1;
                        self.last_save = Instant::now();
                        // 토스트는 첫 실패에만: Error 토스트는 수동 닫기라 재시도마다 다시 띄우면
                        // 닫아도 계속 되살아난다. 이후엔 상태바 '저장 실패'(hover=원인)가 담당.
                        if self.save_fail_count == 1 {
                            self.toast_error(trf(
                                self.lang,
                                "셀렉 저장 실패: {} — 폴더 쓰기 권한·용량을 확인하세요",
                                &[&msg],
                            ));
                        }
                        self.save_error = Some(msg);
                    }
                }
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
        // 촬영시간순 정렬(#56): 필요 시 EXIF 시각 수집을 시작하고 결과를 반영한다.
        self.ensure_capture_sort(ctx);
        self.drain_capture_sort(ctx);
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
        // 단축키 치트시트 오버레이(#66). 다른 소형 모달과 같은 사슬 위치 — 어느 화면에서도 뜬다.
        if self.show_help {
            self.ui_help(ctx);
        }

        // 새 릴리즈 안내(#33): 유휴 시 1회 백그라운드 확인. 결과 배너는 좌측 레일 정리 버튼 위에 뜬다.
        self.maybe_check_update(ctx);

        // 토스트 만료(#61). 심각도별 지속시간을 따르고, 오류(duration None)는 자동으로
        // 사라지지 않는다(사용자가 클릭·✕로 닫아야 함). 살아 있는 비오류 토스트는 만료
        // 시점에 다시 그려 사라지도록 느린 keep-alive 리페인트를 건다.
        if let Some(t) = &self.toast {
            // Copy 값만 먼저 뽑아 t 차용을 끝낸다(이후 self.toast 변경과 충돌하지 않게).
            let (dur, age) = (t.kind.duration(), t.at.elapsed());
            match dur {
                Some(d) if age > d => self.toast = None,
                Some(_) => ctx.request_repaint_after(Duration::from_millis(500)),
                None => {}
            }
        }

        // 토스트 오버레이는 모든 모달·배너 위(Foreground)에 마지막으로 그린다(#61).
        self.ui_toast(ctx);
    }

    /// 프레임버퍼 클리어 색(#83). 화면의 대부분을 차지하는 photo void와 **같은 색**으로 맞춘다.
    ///
    /// 분수 DPI(예: 175%)에서는 패널 경계가 물리 픽셀에 딱 떨어지지 않아 경계 픽셀이 양쪽
    /// 패널 모두에게서 부분 커버리지만 받고, 남은 몫으로 클리어색이 비친다. 그 색이 주변과
    /// 다르면 그게 곧 "요소 사이의 줄"이다 → 가장 넓은 면과 같은 색으로 두면 비쳐도 안 보인다.
    /// photo void는 설정에서 바꿀 수 있으므로(#36) 하드코딩된 BG0가 아니라 현재 값을 쓴다.
    ///
    /// 주의: #83의 실제 원인은 이게 아니라 **OS 라이트 모드에서 egui light 슬롯의 기본
    /// 구분선 색(gray 190)이 새던 것**이었고 `theme::apply`가 두 슬롯을 모두 덮어 고쳤다.
    /// 이 오버라이드는 그 위의 방어선이다(밝은 photo_bg를 고른 경우 어두운 실선을 막는다).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.photo_bg().to_normalized_gamma_f32()
    }

    /// 종료 시 동기 플러시(#62). 마지막 라벨링 후 300ms 디바운스 창 안에서 앱을 닫으면
    /// 그 변경이 저장되지 않은 채 사라지던 갭을 막는다. 시그니처 주의: eframe 0.29의
    /// on_exit는 "glow" 피처가 켜져 있으면 `on_exit(&mut self, Option<&glow::Context>)`인데,
    /// 이 앱은 default-features=false + wgpu 빌드(glow 미포함)라 인자 없는 형태다.
    fn on_exit(&mut self) {
        if self.sidecar_dirty {
            if let Some(folder) = &self.folder {
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                // 결과는 버린다 — 창이 이미 닫히는 중이라 실패해도 알릴 UI가 없다(재시도 불가,
                // 다음 실행이 직전 정상 사이드카를 복원하는 것이 최선의 폴백).
                let _ = sidecar::save(folder, &entries);
            }
        }
        // 다음 실행에서 이어볼 수 있게 지금 보던 사진을 기록(#86). 바로 아래 저장에 실려 나간다.
        self.remember_folder_position();
        // 설정도 확정: 기존엔 설정 '돌아가기'·폴더 열기 때만 저장돼, 그 뒤 바뀐 설정
        // (정렬·오버레이 토글 등)이 종료 시점에 따라 유실될 수 있었다(#62).
        let _ = config::save(&self.cfg);
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
            // 단축키 치트시트(#66): 열려 있으면 뒤의 사진 단축키를 막고 오버레이가 직접 Esc/?를 받는다.
            || self.show_help
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
                        // 컬링 되돌리기(#78): Ctrl/⌘Z = 취소, Ctrl/⌘⇧Z·Ctrl/⌘Y = 재실행.
                        // cmd 가드가 있어 아래 plain Z(확대 토글)와 겹치지 않는다(위가 먼저 매칭).
                        Key::Z if cmd && modifiers.shift => self.redo(),
                        Key::Z if cmd => self.undo(),
                        Key::Y if cmd => self.redo(),
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
                        Key::O if cmd => self.pick_folder(),
                        // 전송 Enter(#63)와 단축키 치트시트 열기(#66, F1·?·⇧/)는 이 이벤트 루프에서
                        // 처리하지 않고 아래에서 consume_key로 "소비"한다 — 소비하지 않으면 같은
                        // 프레임에 새로 열린 다이얼로그/오버레이가 그 키를 다시 읽어 열림과 동시에
                        // 시작/닫힘이 일어난다. (자세한 이유는 루프 종료 뒤 주석 참조.)
                        _ => {}
                    }
                }
            }
        });

        // 모달을 여는 키(전송 Enter, 단축키 오버레이 ?·F1·⇧/)는 여기서 소비한다(#63/#66 회귀 수정).
        // 소비하지 않으면 handle_keys가 모달을 연 바로 그 프레임에, 새로 열린 전송 다이얼로그·
        // 단축키 오버레이가 같은 키를 key_pressed로 다시 읽어 "열림과 동시에 동작"이 일어난다:
        //   • Enter → 전송 다이얼로그가 즉시 시작(Copy)/이동 확인(Move)으로 직행(설정 화면 건너뜀)
        //   • ?·F1 → 오버레이의 닫기 키와 겹쳐 열리자마자 닫힘(키보드로는 절대 안 열림)
        // has_modal()이 모든 모달을 포함해 이 함수는 '여는 프레임'에만 도므로, 이후 프레임엔 소비가
        // 없어 다이얼로그 안의 Enter 시작·오버레이의 ?/F1 토글 닫기는 그대로 동작한다.
        let open_transfer = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let open_help = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::F1)
                | i.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
                | i.consume_key(egui::Modifiers::SHIFT, egui::Key::Slash)
        });
        if open_transfer {
            self.open_transfer();
        }
        if open_help {
            self.show_help = true;
        }

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
                    // 단축키 치트시트 힌트(#66): 앱이 키보드 중심임을 시작 화면에서 안내(?로 열림).
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("? = {}", tr(lang, "단축키")))
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



}

/// 로드된 EXIF에서 촬영시각(에포크 초)(#56).
fn item_capture_secs(it: &Item) -> Option<i64> {
    it.exif
        .as_ref()?
        .datetime
        .as_deref()
        .and_then(rawblow_core::cull_ext::parse_exif_datetime)
}

/// 항목의 표시 파일명(#56 정렬 키·동률 안정화용).
fn item_file_name(it: &Item) -> String {
    it.entry.display.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
}

/// 사이드카 저장 재시도 간격(#62). 평소엔 300ms(라벨링 연타를 모아 쓰는 디바운스),
/// 3연속 실패부터는 5초로 물러난다 — 읽기 전용 폴더·죽은 NAS 마운트에 매 300ms 실패 I/O를
/// 반복해 봐야 얻는 것 없이 디스크만 두드리고(느린 마운트에선 프레임 히치) 로그·토스트성
/// 소음만 늘기 때문. dirty는 유지되므로 원인이 해소되면 다음 주기에 자동 복구된다.
fn sidecar_retry_interval(fail_count: u32) -> Duration {
    if fail_count >= 3 {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(300)
    }
}

/// 평가 직후의 새 필터 index(#55). `still_visible`=방금 평가한 항목이 여전히 필터를 통과하는지,
/// `new_len`=평가 반영 후 필터 목록 길이. 항목이 남아 있으면 한 칸 전진, 빠졌으면 목록이
/// 당겨져 같은 index가 이미 다음 사진이므로 제자리(끝 넘침만 보정). 목록이 비면 None.
fn index_after_rate(index: usize, still_visible: bool, new_len: usize) -> Option<usize> {
    if new_len == 0 {
        return None;
    }
    let next = if still_visible { index + 1 } else { index };
    Some(next.min(new_len - 1))
}

#[cfg(test)]
mod tests {
    use super::index_after_rate;

    #[test]
    fn index_after_rate_filtered_out_stays_in_place() {
        // 미선택 필터에서 1번(index 0)에 평가 → 목록이 당겨져 index 0이 곧 다음 사진(#55).
        assert_eq!(index_after_rate(0, false, 4), Some(0));
        assert_eq!(index_after_rate(2, false, 4), Some(2));
    }

    #[test]
    fn index_after_rate_still_visible_advances_one() {
        // 전체 필터처럼 항목이 남아 있으면 기존대로 한 칸 전진.
        assert_eq!(index_after_rate(0, true, 5), Some(1));
        assert_eq!(index_after_rate(3, true, 5), Some(4));
    }

    #[test]
    fn index_after_rate_clamps_at_end() {
        // 마지막 항목 평가: 남아 있으면 끝에 고정, 빠졌으면 새 끝(len-1)으로 보정.
        assert_eq!(index_after_rate(4, true, 5), Some(4));
        assert_eq!(index_after_rate(4, false, 4), Some(3));
    }

    #[test]
    fn index_after_rate_empty_list_is_none() {
        // 마지막 남은 항목이 필터에서 빠져 목록이 비면 이동 없음.
        assert_eq!(index_after_rate(0, false, 0), None);
    }

    #[test]
    fn toast_kind_duration_by_severity() {
        use super::{Duration, ToastKind};
        // 정보=3초, 알림=6초, 오류=만료 없음(클릭 전까지 유지)(#61).
        assert_eq!(ToastKind::Info.duration(), Some(Duration::from_secs(3)));
        assert_eq!(ToastKind::Notice.duration(), Some(Duration::from_secs(6)));
        assert_eq!(ToastKind::Error.duration(), None);
    }

    #[test]
    fn sidecar_retry_interval_backs_off_after_three_failures() {
        use super::{sidecar_retry_interval, Duration};
        // 정상·1~2회 실패는 300ms 디바운스 유지, 3연속 실패부터 5s 백오프(#62).
        assert_eq!(sidecar_retry_interval(0), Duration::from_millis(300));
        assert_eq!(sidecar_retry_interval(1), Duration::from_millis(300));
        assert_eq!(sidecar_retry_interval(2), Duration::from_millis(300));
        assert_eq!(sidecar_retry_interval(3), Duration::from_secs(5));
        assert_eq!(sidecar_retry_interval(100), Duration::from_secs(5));
    }
}
