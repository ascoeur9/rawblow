//! RawBlow GUI 본체. 핸드오프 디자인(Studio 기본 / Cinema 풀스크린)을 egui로 구현.

use crate::theme;
use crate::widgets::{self, draw_thumb, hud_text, kbd, mono, prop, section_head, ThumbInfo, TexCache};
use crate::worker::{DecodeRequest, Worker};
use eframe::egui;
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use rawblow_core::config::{self, Config};
use rawblow_core::meta::{read_exif, ExifInfo};
use rawblow_core::transfer::{self, Action, Companions, ConflictPolicy, TransferReport, TransferRequest};
use rawblow_core::{scan, sidecar, Entry, Filter, Label, MatchMode, SortOrder, ViewMode};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const TOOLBAR_H: f32 = 40.0;
const RAIL_W: f32 = 188.0;
const FILMSTRIP_H: f32 = 92.0;
const STATUS_H: f32 = 24.0;

/// 단일/전체화면 프리뷰 최대 변(px). 속도 최우선 — DCT 축소 디코딩 타깃.
const PREVIEW_EDGE: u32 = 1600;
/// ORIG(원본 보기) 최대 변(px). GPU 텍스처 한계(보통 8192) 안에서 원본 디테일 확보.
const ORIG_EDGE: u32 = 8192;
/// 그리드·필름스트립 썸네일 최대 변(px). 작게 → 빠른 디코딩·작은 메모리.
const THUMB_EDGE: u32 = 320;
/// 선디코딩 윈도우: 컬링은 전방 진행이므로 앞쪽을 더 많이.
const PRELOAD_AHEAD: usize = 8;
const PRELOAD_BEHIND: usize = 3;
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
}

#[derive(Clone)]
struct TransferDialogState {
    labels: Vec<Label>,
    action: Action,
    companions: Companions,
    split_by_label: bool,
    conflict: ConflictPolicy,
    dest: String,
}

impl Default for TransferDialogState {
    fn default() -> Self {
        TransferDialogState {
            labels: vec![Label::Pick],
            action: Action::Copy,
            companions: Companions::Both,
            split_by_label: false,
            conflict: ConflictPolicy::AutoIncrement,
            dest: String::new(),
        }
    }
}

pub struct RawBlowApp {
    cfg: Config,
    folder: Option<PathBuf>,
    items: Vec<Item>,
    index: usize, // 필터된 목록 기준 위치
    view: ViewMode,
    fullscreen: bool,
    filter: Filter,
    show_exif: bool,
    show_hist: bool,
    full_raw: bool, // ORIG(원본 보기): 풀 RAW/최대 임베디드를 원본 크기로 디코딩
    // 단일/전체화면 줌·이동 상태.
    fit: bool,          // true = 창에 맞춤(zoom 자동), false = 명시적 배율
    zoom: f32,          // 절대 배율(화면픽셀/이미지픽셀). 1.0 = 1:1
    pan: Vec2,          // 중앙 기준 이동(화면 px)
    zoom_for: Option<usize>, // 줌 상태가 적용된 항목(real). 바뀌면 fit으로 리셋
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
    failed_preview: std::collections::HashSet<usize>,
    failed_thumb: std::collections::HashSet<usize>,
    histo: std::collections::HashMap<usize, Histo>,
    generation: u64,

    sidecar_dirty: bool,
    last_save: Instant,

    transfer: Option<TransferDialogState>,
    result: Option<TransferReport>,
    show_settings: bool,
    last_dest: Option<PathBuf>,

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
}

impl RawBlowApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        crate::fonts::install(&cc.egui_ctx);

        let cfg = config::load();
        // 워커 스레드 수 제한: 코어의 절반(2~6)으로 CPU 피크를 억제.
        // 디코딩이 DCT 축소로 가벼워져 적은 스레드로도 프리로드를 따라잡는다.
        let worker = Worker::new(
            std::thread::available_parallelism()
                .map(|n| (n.get() / 2).clamp(2, 6))
                .unwrap_or(4),
        );

        let mut app = RawBlowApp {
            view: ViewMode::Single,
            fullscreen: false,
            filter: Filter::All,
            show_exif: cfg.show_exif,
            show_hist: cfg.show_histogram,
            full_raw: false,
            fit: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            zoom_for: None,
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
            cache: TexCache::new(PREVIEW_CAP, 16),
            thumbs: TexCache::new(THUMB_CAP, 256),
            pending_preview: std::collections::HashSet::new(),
            pending_thumb: std::collections::HashSet::new(),
            pending_thumb_prio: std::collections::HashSet::new(),
            failed_preview: std::collections::HashSet::new(),
            failed_thumb: std::collections::HashSet::new(),
            histo: std::collections::HashMap::new(),
            generation: 0,
            sidecar_dirty: false,
            last_save: Instant::now(),
            transfer: None,
            result: None,
            show_settings: false,
            last_dest: None,
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
            cfg,
        };

        // 마지막 폴더 자동 복원.
        if let Some(last) = app.cfg.last_folder.clone() {
            let p = PathBuf::from(last);
            if p.is_dir() {
                app.open_folder(p);
            }
        }
        app
    }

    fn open_folder(&mut self, folder: PathBuf) {
        let entries = scan::scan_folder(&folder, self.cfg.recursive, self.sort);
        let mut items: Vec<Item> = entries
            .into_iter()
            .map(|entry| Item {
                entry,
                exif: None,
                exif_loaded: false,
            })
            .collect();

        // 사이드카 복원.
        if let Some(session) = sidecar::load(&folder) {
            let mut tmp: Vec<Entry> = items.iter().map(|i| i.entry.clone()).collect();
            sidecar::apply(&session, &mut tmp);
            for (it, e) in items.iter_mut().zip(tmp) {
                it.entry.label = e.label;
            }
        }

        self.items = items;
        self.index = 0;
        self.generation += 1;
        self.cache = TexCache::new(PREVIEW_CAP, 16);
        self.thumbs = TexCache::new(THUMB_CAP, 256);
        self.pending_preview.clear();
        self.pending_thumb.clear();
        self.pending_thumb_prio.clear();
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
        self.toast = Some((format!("{} 항목 로드", self.items.len()), Instant::now()));
    }

    /// 현재 필터를 통과하는 항목 인덱스(원본 items 기준).
    fn filtered(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| self.filter.accepts(it.entry.label))
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

    fn ensure_exif(&mut self, real: usize) {
        if let Some(it) = self.items.get_mut(real) {
            if !it.exif_loaded {
                it.exif = read_exif(&it.entry.display);
                it.exif_loaded = true;
            }
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
            // 전방 편향 윈도우 프리뷰(일반 레인).
            let lo = cur.saturating_sub(PRELOAD_BEHIND);
            let hi = (cur + PRELOAD_AHEAD).min(f.len() - 1);
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
                generation: self.generation,
            };
            if prio {
                self.worker.request_priority(req);
            } else {
                self.worker.request(req);
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
                generation: self.generation,
            };
            if prio {
                self.pending_thumb_prio.insert(real);
                self.worker.request_priority(req);
            } else {
                self.worker.request(req);
            }
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

    fn set_label(&mut self, label: Label) {
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
}

impl eframe::App for RawBlowApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 프레임 시간 측정.
        let now = Instant::now();
        self.frame_ms = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;

        self.drain_results(ctx);

        if !self.has_modal() {
            self.handle_keys(ctx);
        }

        if let Some(real) = self.current_real() {
            self.ensure_exif(real);
        }
        self.request_preload();
        self.save_sidecar_if_due();

        // 프리뷰(현재 주변)는 즉시 리페인트로 빠르게 채운다.
        // 썸네일 채우기는 이 100ms 타이머만으로 구동한다(워커는 egui를 건드리지 않음).
        // 매 틱마다 drain_results가 채널에 쌓인 결과를 한 번에 비우므로 10fps여도 처리량은
        // 충분하고, 다 끝나면(pending 비면) 타이머가 꺼져 0% CPU로 유휴.
        if !self.pending_preview.is_empty() {
            ctx.request_repaint();
        } else if !self.pending_thumb.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        // 화면 분기.
        if self.folder.is_none() {
            self.ui_open(ctx);
        } else if self.show_settings {
            self.ui_settings(ctx);
        } else if self.fullscreen {
            self.ui_fullscreen(ctx);
        } else {
            self.ui_shell(ctx);
        }

        // 모달.
        if self.transfer.is_some() {
            self.ui_transfer_dialog(ctx);
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
        self.transfer.is_some() || self.result.is_some() || self.jump_open || self.bulk_open
    }

    // ── 입력 ──────────────────────────────────────────────
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
                        Key::T => {
                            self.view = match self.view {
                                ViewMode::Single => ViewMode::Grid,
                                ViewMode::Grid => ViewMode::Single,
                            }
                        }
                        Key::I => self.show_exif = !self.show_exif,
                        Key::H => self.show_hist = !self.show_hist,
                        Key::Space | Key::Z => {
                            // 창맞춤 ↔ 1:1 토글.
                            if self.fit {
                                self.fit = false;
                                self.zoom = 1.0;
                                self.pan = Vec2::ZERO;
                            } else {
                                self.fit = true;
                                self.pan = Vec2::ZERO;
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
            self.open_folder(dir);
        }
    }

    fn open_transfer(&mut self) {
        let mut st = TransferDialogState::default();
        if let Some(folder) = &self.folder {
            st.dest = format!("{}_selected", folder.to_string_lossy());
        }
        self.transfer = Some(st);
    }

    // ── Open Folder 화면 ──────────────────────────────────
    fn ui_open(&mut self, ctx: &egui::Context) {
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
                        egui::RichText::new("FAST RAW CULLING · 사진 셀렉 뷰어")
                            .font(mono(11.0))
                            .color(theme::INK3),
                    );
                    ui.add_space(28.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("  폴더 열기 (Open Folder)  ⌘O  ").color(Color32::from_rgb(0x0a, 0x14, 0x20)),
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
    fn ui_shell(&mut self, ctx: &egui::Context) {
        self.ui_toolbar(ctx);
        self.ui_status_bar(ctx);
        self.ui_left_rail(ctx);
        if self.view == ViewMode::Single {
            self.ui_filmstrip(ctx);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG0))
            .show(ctx, |ui| match self.view {
                ViewMode::Single => self.ui_single(ui),
                ViewMode::Grid => self.ui_grid(ui),
            });
    }

    fn ui_toolbar(&mut self, ctx: &egui::Context) {
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
                    if toggle_btn(ui, "폴더 열기 ⌘O", false).clicked() {
                        self.pick_folder();
                    }
                    vsep(ui);
                    let single = self.view == ViewMode::Single;
                    if toggle_btn(ui, "Single", single).clicked() {
                        self.view = ViewMode::Single;
                    }
                    if toggle_btn(ui, "Grid", !single).clicked() {
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

                    if toggle_btn(ui, "Fit", self.fit).clicked() {
                        self.fit = true;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "1:1", !self.fit && (self.zoom - 1.0).abs() < 0.01).clicked() {
                        self.fit = false;
                        self.zoom = 1.0;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "ORIG", self.full_raw).clicked() {
                        self.full_raw = !self.full_raw;
                    }
                    vsep(ui);
                    if toggle_btn(ui, "EXIF", self.show_exif).clicked() {
                        self.show_exif = !self.show_exif;
                    }
                    if toggle_btn(ui, "Hist", self.show_hist).clicked() {
                        self.show_hist = !self.show_hist;
                    }
                    if toggle_btn(ui, &format!("Filter · {}", self.filter.ko()), false).clicked() {
                        self.filter = self.filter.next();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(egui::RichText::new(" Transfer  ⌘E ").color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT))
                            .clicked()
                        {
                            self.open_transfer();
                        }
                        if toggle_btn(ui, "Jump · G", false).clicked() {
                            self.jump_open = true;
                        }
                        // 그리드 모드 한정: 파일명으로 일괄 라벨링(#3).
                        if self.view == ViewMode::Grid
                            && toggle_btn(ui, "Bulk · B", false).clicked()
                        {
                            self.bulk_open = true;
                            self.bulk_searched = false;
                            self.bulk_hits.clear();
                        }
                        if toggle_btn(ui, "⚙", self.show_settings).clicked() {
                            self.show_settings = true;
                        }
                    });
                });
            });
    }

    fn ui_left_rail(&mut self, ctx: &egui::Context) {
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
                    p.text(Pos2::new(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, filt.ko(), prop(12.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        self.filter = filt;
                        self.index = 0;
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(8.0);
                    let saved = if self.sidecar_dirty { "● saving…" } else { "● saved" };
                    ui.label(egui::RichText::new(saved).font(mono(10.0)).color(theme::OK));
                    ui.label(egui::RichText::new("SESSION · .rawblow/session.json").font(mono(10.0)).color(theme::INK4));
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
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 맨 오른쪽: 배율(단일/전체화면에서만 의미). right_to_left이므로 먼저 추가 = 최우측.
                        if self.view == ViewMode::Single || self.fullscreen {
                            ui.label(
                                egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                                    .font(mono(10.5))
                                    .color(theme::INK2),
                            );
                            ui.label(egui::RichText::new("·").font(mono(10.5)).color(theme::INK4));
                        }
                        ui.label(egui::RichText::new(format!("{:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", self.frame_ms, fps, self.cfg.preload)).font(mono(10.5)).color(theme::INK4));
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
                        };
                        draw_thumb(ui, rect, tex, tsize, &info);
                        if resp.clicked() {
                            self.index = fi;
                        }
                    }
                });
            });
    }

    fn ui_single(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let real = match self.current_real() {
            Some(r) => r,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("표시할 항목이 없습니다").color(theme::INK3));
                });
                return;
            }
        };
        self.photo_view(ui, rect, real);
        if !self.has_modal() {
            let suffix = if self.full_raw { "ORIG · sRGB" } else { "FIT · sRGB" };
            self.paint_hud(ui, rect, real, suffix);
        }
    }

    /// 사진 영역: 줌/이동 인터랙션 + 그리기.
    /// - 클릭(드래그 아님): 창맞춤(fit) ↔ 1:1 토글
    /// - Ctrl+휠 / 터치패드 핀치: 연속 확대·축소(커서 기준)
    /// - 드래그: 확대 상태에서 이동(pan)
    /// 프리뷰가 있으면 그것을, 없으면 썸네일을 열화 표시, 둘 다 없으면 "디코딩 중".
    fn photo_view(&mut self, ui: &mut egui::Ui, area: Rect, real: usize) {
        // 표시할 항목이 바뀌면 줌 상태 리셋(fit).
        if self.zoom_for != Some(real) {
            self.fit = true;
            self.pan = Vec2::ZERO;
            self.zoom_for = Some(real);
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
                    .text(area.center(), Align2::CENTER_CENTER, "디코딩 중…", mono(12.0), theme::INK3);
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
        let max_px = ((scaled.x - area.width()) * 0.5).max(0.0);
        let max_py = ((scaled.y - area.height()) * 0.5).max(0.0);
        self.pan.x = self.pan.x.clamp(-max_px, max_px);
        self.pan.y = self.pan.y.clamp(-max_py, max_py);

        // 그리기(영역으로 클립).
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        let target = Rect::from_center_size(area.center() + self.pan, scaled);
        ui.painter().with_clip_rect(area).image(tex, target, uv, Color32::WHITE);

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
        let it = &self.items[real];
        let f = self.filtered();
        // TL: 라벨 + 파일명.
        let name = it.entry.display.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut tl = area.left_top() + Vec2::new(20.0, 22.0);
        let chip = format!("[{}]", it.entry.label.ko());
        hud_text(ui, tl, Align2::LEFT_TOP, &chip, mono(12.0), theme::label_color(it.entry.label));
        tl.x += 64.0;
        let badge = if it.entry.shows_raw_badge() { "  +RAW" } else { "" };
        hud_text(ui, tl, Align2::LEFT_TOP, &format!("{name}{badge}"), mono(12.0), theme::INK);

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
                        };
                        draw_thumb(ui, rect, tex, tsize, &info);
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

    fn ui_fullscreen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG0))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                if let Some(real) = self.current_real() {
                    self.photo_view(ui, rect, real);
                    self.paint_hud(ui, rect, real, "FULLSCREEN · ESC");
                }
            });
    }

    // ── 전송 다이얼로그 ──────────────────────────────────
    fn ui_transfer_dialog(&mut self, ctx: &egui::Context) {
        let mut st = self.transfer.clone().unwrap();
        let mut do_start = false;
        let mut do_cancel = false;
        let (pick, hold, reject, unrated) = self.counts();

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
            action: st.action,
            companions: st.companions,
            dest: PathBuf::from(&st.dest),
            split_by_label: st.split_by_label,
            conflict: st.conflict,
        });
        let raw_n = plan.iter().filter(|(p, _)| rawblow_core::model::kind_of(p) == Some(rawblow_core::model::Kind::Raw)).count();
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
                                        ui.label(egui::RichText::new("파일 전송").font(prop(15.0)).color(theme::INK));
                                        ui.label(egui::RichText::new("선택한 라벨의 파일을 복사/이동 · RAW 페어 처리").font(mono(10.5)).color(theme::INK3));
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
                        // ── BODY ──
                        egui::Frame::none()
                            .inner_margin(egui::Margin::symmetric(22.0, 18.0))
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                section_label(ui, "SOURCE LABELS");
                                ui.horizontal_wrapped(|ui| {
                                    for (label, n) in [(Label::Pick, pick), (Label::Hold, hold), (Label::Reject, reject), (Label::Unrated, unrated)] {
                                        let on = st.labels.contains(&label);
                                        if check_chip(ui, label.ko(), Some(n), theme::label_color(label), on) {
                                            if on { st.labels.retain(|l| *l != label); } else { st.labels.push(label); }
                                        }
                                    }
                                });
                                ui.add_space(8.0);
                                if check_chip(ui, "라벨별 하위폴더로 분기 (/pick, /hold …)", None, theme::ACCENT, st.split_by_label) {
                                    st.split_by_label = !st.split_by_label;
                                }
                                ui.add_space(16.0);

                                section_label(ui, "ACTION");
                                let act_sel = if st.action == Action::Copy { 0 } else { 1 };
                                if let Some(i) = segmented(ui, &[("Copy", "원본 유지"), ("Move", "원본 이동")], act_sel) {
                                    st.action = if i == 0 { Action::Copy } else { Action::Move };
                                }
                                ui.add_space(16.0);

                                section_label(ui, "COMPANIONS");
                                let comp_sel = match st.companions { Companions::Both => 0, Companions::RawOnly => 1, Companions::ImageOnly => 2 };
                                if let Some(i) = segmented(ui, &[("RAW+이미지", "페어 함께"), ("RAW만", "RAW만"), ("이미지만", "JPG만")], comp_sel) {
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
                                if let Some(i) = segmented(ui, &[("자동 일련번호", "_001 접미"), ("건너뛰기", "기존 유지")], conf_sel) {
                                    st.conflict = if i == 0 { ConflictPolicy::AutoIncrement } else { ConflictPolicy::Skip };
                                }
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
                                    ui.label(egui::RichText::new("파일").font(mono(10.0)).color(theme::INK3));
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(raw_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new("RAW").font(mono(9.5)).color(theme::INK3));
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(img_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new("이미지").font(mono(9.5)).color(theme::INK3));

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add(egui::Button::new(egui::RichText::new("  전송 시작  ").color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                                            do_start = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Enter");
                                        ui.add_space(14.0);
                                        if toggle_btn(ui, "취소", false).clicked() {
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
            let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
            let req = TransferRequest {
                entries: &entries,
                labels: st.labels.clone(),
                action: st.action,
                companions: st.companions,
                dest: PathBuf::from(&st.dest),
                split_by_label: st.split_by_label,
                conflict: st.conflict,
            };
            let report = transfer::execute(&req);
            self.last_dest = Some(PathBuf::from(&st.dest));
            self.transfer = None;
            self.result = Some(report);
        } else {
            self.transfer = Some(st);
        }
    }

    fn ui_transfer_result(&mut self, ctx: &egui::Context) {
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
                modal_header(ui, "전송 완료", "");
                ui.label(egui::RichText::new(format!("✓ {} 파일 전송 · {} 리네임 · {} 실패", report.transferred, report.renamed.len(), report.failed.len())).font(prop(13.0)).color(theme::OK));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(format!("RAW {} · 이미지 {} · {:.1} MB", report.raw_count, report.image_count, report.bytes as f64 / 1_048_576.0)).font(mono(11.0)).color(theme::INK2));
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
                        if ui.add(egui::Button::new(egui::RichText::new("  닫기  ").color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            close = true;
                        }
                        ui.add_space(8.0);
                        if has_dest && toggle_btn(ui, "대상 폴더 열기", false).clicked() {
                            open_dest = true;
                        }
                    });
                });
            });
        if open_dest {
            if let Some(dest) = self.last_dest.clone() {
                if dest.is_dir() {
                    self.open_folder(dest);
                }
            }
            self.result = None;
            return;
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.result = None;
        }
    }

    fn ui_jump(&mut self, ctx: &egui::Context) {
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
                modal_header(ui, "파일번호 점프", "줄바꿈·쉼표·탭으로 구분");
                ui.add(egui::TextEdit::multiline(&mut self.jump_text).font(mono(12.0)).desired_rows(2).desired_width(f32::INFINITY));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.jump_exact, "정확히 일치");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("  점프  ⏎  ").color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            go = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, "닫기 (Esc)", false).clicked() {
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
                self.toast = Some((format!("{} 건 매칭", hits.len()), Instant::now()));
            } else {
                self.toast = Some(("매칭 없음".into(), Instant::now()));
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
                modal_header(ui, "일괄 분류 변경", "파일명·일부 → 매칭 → 라벨 적용");
                ui.add(
                    egui::TextEdit::multiline(&mut self.bulk_text)
                        .font(mono(12.0))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("파일명 또는 일부 — 줄바꿈·쉼표·탭으로 구분"),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.bulk_exact, "정확히 일치");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("  검색  ⏎  ")
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
                        egui::RichText::new(format!("매칭 {}건", self.bulk_hits.len()))
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
                                    egui::RichText::new("매칭 결과 없음")
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
                        egui::RichText::new("적용할 라벨")
                            .font(prop(11.0))
                            .color(theme::INK3),
                    );
                    ui.horizontal(|ui| {
                        for (lbl, name) in [
                            (Label::Pick, "Q  선택"),
                            (Label::Hold, "W  보류"),
                            (Label::Reject, "E  제외"),
                            (Label::Unrated, "R  해제"),
                        ] {
                            let active = self.bulk_target == lbl;
                            if toggle_btn(ui, name, active).clicked() {
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
                            egui::RichText::new("  적용  ")
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                        )
                        .fill(theme::ACCENT);
                        if ui.add_enabled(can_apply, btn).clicked() {
                            apply = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, "닫기 (Esc)", false).clicked() {
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
                format!("{}건 → {}", self.bulk_hits.len(), target.ko()),
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
        egui::TopBottomPanel::top("settings_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button("← 돌아가기").clicked() {
                        self.show_settings = false;
                        let _ = config::save(&self.cfg);
                    }
                    ui.label(egui::RichText::new("Settings — Keyboard & General").font(prop(14.0)).color(theme::INK));
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(egui::RichText::new("GENERAL").font(prop(11.0)).color(theme::INK3));
                    ui.checkbox(&mut self.cfg.auto_advance, "라벨링 후 자동 전진");
                    ui.checkbox(&mut self.cfg.recursive, "하위 폴더 포함 스캔");
                    ui.checkbox(&mut self.cfg.show_exif, "EXIF 오버레이 기본 표시");
                    ui.checkbox(&mut self.cfg.show_histogram, "히스토그램 기본 표시");
                    ui.horizontal(|ui| {
                        ui.label("프리로드 ±");
                        ui.add(egui::DragValue::new(&mut self.cfg.preload).range(0..=10));
                    });
                    ui.horizontal(|ui| {
                        ui.label("그리드 열 수");
                        ui.add(egui::DragValue::new(&mut self.cfg.grid_cols).range(4..=12));
                    });
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("LABELS").font(prop(11.0)).color(theme::INK3));
                    let km = &self.cfg.keymap;
                    for (name, key) in [("선택 Pick", &km.pick), ("보류 Hold", &km.hold), ("제외 Reject", &km.reject), ("미선택 Clear", &km.clear)] {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kbd(ui, key);
                            });
                        });
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("단축키 재바인딩 UI는 v1.1 예정 — 현재 기본값 QWER 고정 표시").font(mono(10.0)).color(theme::INK4));
                });
            });
        self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
        self.show_exif = self.cfg.show_exif;
        self.show_hist = self.cfg.show_histogram;
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
        lines.push(dt.clone());
    }
    lines
}
