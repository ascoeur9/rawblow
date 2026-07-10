//! 공용 그리기 헬퍼: HUD 그림자 텍스트, 키캡, 라벨 칩, 썸네일, LRU 텍스처 캐시.
//! 디자인 핸드오프 §Component specs / §HUD overlay rule을 따른다.

use crate::theme;
use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Ui, Vec2};
use rawblow_core::{ColorTag, Label};
use std::collections::{HashMap, VecDeque};

/// 사진 위 HUD 텍스트: 카드 배경 없이 그림자를 흉내내려 두 번 그린다(핸드오프 CRITICAL).
pub fn hud_text(ui: &Ui, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let p = ui.painter();
    p.text(
        pos + Vec2::new(0.0, 1.0),
        anchor,
        text,
        font.clone(),
        Color32::from_black_alpha(230),
    );
    p.text(pos, anchor, text, font, color)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}
pub fn prop(size: f32) -> FontId {
    FontId::proportional(size)
}

/// 키캡 칩(인라인). 폭은 글자에 맞춰 최소 18.
pub fn kbd(ui: &mut Ui, key: &str) {
    let font = mono(10.0);
    let galley = ui.painter().layout_no_wrap(key.to_string(), font.clone(), theme::INK2);
    let w = galley.size().x.max(8.0) + 10.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 18.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect(rect, Rounding::same(3.0), theme::BG3, Stroke::new(1.0, theme::LINE2));
    // 키캡 느낌의 아래쪽 2px 강조선.
    p.line_segment(
        [
            Pos2::new(rect.left() + 1.0, rect.bottom() - 1.0),
            Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
        ],
        Stroke::new(1.5, theme::LINE3),
    );
    p.text(rect.center(), Align2::CENTER_CENTER, key, font, theme::INK2);
}

/// 라벨 칩(점 + 이름). big=true면 약간 크게. (1:1 줌 화면 등에서 사용 예정 컴포넌트)
#[allow(dead_code)]
pub fn label_chip(ui: &mut Ui, label: Label, big: bool) {
    let color = theme::label_color(label);
    let name = match label {
        Label::Pick => "PICK",
        Label::Hold => "HOLD",
        Label::Reject => "REJECT",
        Label::Unrated => "UNRATED",
    };
    let fs = if big { 11.0 } else { 9.0 };
    let font = mono(fs);
    let galley = ui.painter().layout_no_wrap(name.to_string(), font.clone(), color);
    let pad = if big { 8.0 } else { 5.0 };
    let dot = if matches!(label, Label::Unrated) { 0.0 } else { 6.0 };
    let w = galley.size().x + pad * 2.0 + dot + if dot > 0.0 { 6.0 } else { 0.0 };
    let h = if big { 20.0 } else { 16.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let p = ui.painter();
    if matches!(label, Label::Unrated) {
        p.rect(rect, Rounding::same(3.0), Color32::TRANSPARENT, Stroke::new(1.0, theme::LINE2));
        p.text(rect.center(), Align2::CENTER_CENTER, name, font, theme::INK3);
    } else {
        let fill = color.linear_multiply(0.18).to_opaque(); // 근사 18% 틴트
        p.rect(rect, Rounding::same(3.0), with_alpha(color, 46), Stroke::new(1.0, with_alpha(color, 64)));
        let _ = fill;
        let cy = rect.center().y;
        p.circle_filled(Pos2::new(rect.left() + pad + 3.0, cy), dot / 2.0, color);
        p.text(
            Pos2::new(rect.left() + pad + dot + 6.0, cy),
            Align2::LEFT_CENTER,
            name,
            font,
            color,
        );
    }
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// 폴더 아이콘(벡터, 컬러). 이모지 폰트(🗂)가 두부(□)로 깨지는 걸 피하려 직접 그린다(#33 후속).
/// 뒤 탭 + 앞 몸통 두 색으로 입체감. `enabled=false`면 회색.
pub fn draw_folder_icon(p: &egui::Painter, rect: Rect, enabled: bool) {
    let (body, tab) = if enabled {
        (Color32::from_rgb(0xf2, 0xb1, 0x3c), Color32::from_rgb(0xd9, 0x93, 0x1f)) // 앰버
    } else {
        (Color32::from_rgb(0x6b, 0x72, 0x80), Color32::from_rgb(0x55, 0x5d, 0x68)) // 회색(비활성)
    };
    let (w, h) = (rect.width(), rect.height());
    let r = (w * 0.13).max(1.5);
    // 뒤 탭(좌상단에서 살짝 보임).
    let tab_r = Rect::from_min_size(
        Pos2::new(rect.left() + w * 0.06, rect.top() + h * 0.12),
        Vec2::new(w * 0.5, h * 0.30),
    );
    p.rect_filled(tab_r, Rounding::same(r), tab);
    // 앞 몸통.
    let body_r = Rect::from_min_max(
        Pos2::new(rect.left() + w * 0.05, rect.top() + h * 0.26),
        Pos2::new(rect.right() - w * 0.05, rect.bottom() - h * 0.10),
    );
    p.rect_filled(body_r, Rounding::same(r), body);
}

/// 4갈래 반짝임(스파클) 하나 — 세로·가로 얇은 마름모 두 개(둘 다 볼록 → convex_polygon 안전).
fn star4(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let k = r * 0.28; // 갈래 두께(작을수록 뾰족).
    let vert = vec![
        Pos2::new(c.x, c.y - r),
        Pos2::new(c.x + k, c.y),
        Pos2::new(c.x, c.y + r),
        Pos2::new(c.x - k, c.y),
    ];
    let horz = vec![
        Pos2::new(c.x - r, c.y),
        Pos2::new(c.x, c.y - k),
        Pos2::new(c.x + r, c.y),
        Pos2::new(c.x, c.y + k),
    ];
    p.add(egui::Shape::convex_polygon(vert, color, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(horz, color, Stroke::NONE));
}

/// 축하 스파클 아이콘(벡터, 컬러). 이모지(🎉) 두부 회피용(#33). 따뜻한 3색이라 파랑 accent
/// 배경 위에서도 잘 보인다. `rect`는 정사각 가정.
pub fn draw_sparkles(p: &egui::Painter, rect: Rect) {
    let gold = Color32::from_rgb(0xff, 0xcf, 0x3a);
    let pink = Color32::from_rgb(0xff, 0x5c, 0x8a);
    let orange = Color32::from_rgb(0xff, 0x8a, 0x3d);
    let s = rect.height();
    let c = rect.center();
    star4(p, Pos2::new(c.x - s * 0.06, c.y + s * 0.04), s * 0.42, gold); // 큰 별(메인)
    star4(p, Pos2::new(rect.left() + s * 0.85, rect.top() + s * 0.18), s * 0.20, pink); // 우상단 작은 별
    star4(p, Pos2::new(rect.left() + s * 0.15, rect.top() + s * 0.76), s * 0.15, orange); // 좌하단 더 작은 별
}

/// 섹션 헤더(좌측 레일): 작은 대문자 라벨 + 우측 힌트.
pub fn section_head(ui: &mut Ui, label: &str, hint: Option<&str>) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .font(prop(10.0))
                .color(theme::INK3),
        );
        if let Some(h) = hint {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(h).font(prop(9.5)).color(theme::INK4));
            });
        }
    });
    ui.add_space(4.0);
}

/// 썸네일 칩 정보(그리기 입력).
pub struct ThumbInfo {
    pub label: Label,
    pub raw_badge: bool, // RAW+ (둘 다)
    pub raw_only: bool,  // RAW 단독
    pub active: bool,
    pub focused: bool,
    pub selected: bool,  // 다중 선택됨
    pub stars: u8,       // 별점(0~5, #23)
    pub tag: ColorTag,   // 컬러 태그(#27)
    pub failed: bool,    // 디코딩 영구 실패(#64) — 텍스처 없으면 placeholder에 ⚠ 표시
}

/// 주어진 rect에 썸네일을 그린다. 텍스처가 있으면 이미지를, 없으면 플레이스홀더.
/// `scale`(#44): 셀 위 표기(선택 테두리·라벨 점·RAW/별점 배지·색상 태그)의 크기 배율. 1.0=이전 크기(작게),
/// 1.8 등=크게. 셀 자체/이미지는 영향받지 않고 오버레이만 비례 확대된다.
pub fn draw_thumb(ui: &Ui, rect: Rect, tex: Option<egui::TextureId>, size: Vec2, info: &ThumbInfo, scale: f32) {
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(3.0), theme::BG1);
    if let Some(id) = tex {
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        // reject는 흐리게.
        let tint = if matches!(info.label, Label::Reject) {
            Color32::from_rgba_unmultiplied(140, 140, 140, 140)
        } else {
            Color32::WHITE
        };
        // 비율 유지(fit): 세로/가로 사진을 늘리지 않고 셀 안에 맞추고 남는 영역은 패딩.
        let target = if size.x > 0.0 && size.y > 0.0 {
            let s = (rect.width() / size.x).min(rect.height() / size.y);
            Rect::from_center_size(rect.center(), size * s)
        } else {
            rect
        };
        p.with_clip_rect(rect).image(id, target, uv, tint);
    } else if info.failed {
        // 디코딩 영구 실패(#64): 빈 회색 placeholder 대신 경고 아이콘을 보여준다.
        p.text(rect.center(), Align2::CENTER_CENTER, "⚠", mono(16.0 * scale), theme::WARN);
    }
    // 다중 선택 표시: 강조색 반투명 오버레이.
    if info.selected {
        p.rect_filled(rect, Rounding::same(3.0), with_alpha(theme::ACCENT, 64));
    }
    // 테두리(active/selected > focused > 기본). 선택/활성 강조는 표기 크기 설정에 따라 굵게(#44).
    let stroke = if info.active || info.selected {
        Stroke::new(2.0 * scale, theme::ACCENT)
    } else if info.focused {
        Stroke::new(1.5, theme::LINE3)
    } else {
        Stroke::new(1.0, theme::LINE)
    };
    // 스트로크는 경로 중심 기준이라 절반이 rect 밖으로 나간다 — 그리드 좌/우 끝 셀에서는
    // 스크롤 영역 클립 경계에 걸려 그 절반이 잘려 보인다. 절반폭만큼 안쪽으로 들여 그려
    // 어느 가장자리에서도 테두리가 온전히 보이게 한다.
    let inset = stroke.width / 2.0;
    p.rect_stroke(rect.shrink(inset), Rounding::same(3.0), stroke);
    // 라벨 점(TL).
    if !matches!(info.label, Label::Unrated) {
        p.circle_filled(
            Pos2::new(rect.left() + 7.0 * scale, rect.top() + 7.0 * scale),
            3.0 * scale,
            theme::label_color(info.label),
        );
    }
    // RAW 배지(TR).
    if info.raw_badge || info.raw_only {
        let txt = if info.raw_only { "RAW" } else { "RAW+" };
        let font = mono(8.0 * scale);
        let g = p.layout_no_wrap(txt.to_string(), font.clone(), Color32::WHITE);
        let bw = g.size().x + 6.0 * scale;
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - bw - 4.0 * scale, rect.top() + 4.0 * scale),
            Vec2::new(bw, 12.0 * scale),
        );
        let (bg, fg) = if info.raw_only {
            (theme::HOLD, Color32::from_rgb(0x20, 0x18, 0x00))
        } else {
            (Color32::from_black_alpha(180), Color32::WHITE)
        };
        p.rect_filled(badge, Rounding::same(2.0 * scale), bg);
        p.text(badge.center(), Align2::CENTER_CENTER, txt, font, fg);
    }
    // 별점 배지(BL, #23): ★N. 라벨 점(TL)·RAW 배지(TR)와 위치가 겹치지 않는다.
    if info.stars > 0 {
        let txt = format!("★{}", info.stars);
        let font = mono(8.5 * scale);
        let g = p.layout_no_wrap(txt.clone(), font.clone(), theme::HOLD);
        let bw = g.size().x + 6.0 * scale;
        let badge = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0 * scale, rect.bottom() - 14.0 * scale),
            Vec2::new(bw, 11.0 * scale),
        );
        p.rect_filled(badge, Rounding::same(2.0 * scale), Color32::from_black_alpha(170));
        p.text(badge.center(), Align2::CENTER_CENTER, &txt, font, theme::HOLD);
    }
    // 컬러 태그 점(BR, #27): 라벨(TL)·RAW(TR)·별점(BL)과 위치가 겹치지 않는다.
    if let Some(rgb) = info.tag.color_rgb() {
        let c = Pos2::new(rect.right() - 8.0 * scale, rect.bottom() - 8.0 * scale);
        p.circle_filled(c, 4.0 * scale, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        p.circle_stroke(c, 4.0 * scale, Stroke::new(1.0, Color32::from_black_alpha(120)));
    }
}

/// RGB 히스토그램(64 bins/채널)을 사진 위 BR에 그린다(디자인 핸드오프 HUD).
pub fn draw_histogram(ui: &Ui, rect: Rect, bins: &[[u32; 64]; 3], max: u32) {
    let p = ui.painter();
    let colors = [theme::REJECT, theme::PICK, Color32::from_rgb(0x60, 0xa5, 0xfa)]; // R/G/B
    let n = 64usize;
    let bw = rect.width() / n as f32;
    let maxf = max.max(1) as f32;
    // 라벨 헤더.
    hud_text(ui, rect.left_top() + Vec2::new(0.0, -2.0), Align2::LEFT_BOTTOM, "RGB", mono(9.5), theme::INK3);
    for c in 0..3 {
        for (x, &bin) in bins[c].iter().enumerate().take(n) {
            let v = bin as f32 / maxf;
            let bh = (v.sqrt()) * rect.height(); // sqrt로 낮은 빈도도 보이게
            let bar = Rect::from_min_max(
                Pos2::new(rect.left() + x as f32 * bw, rect.bottom() - bh),
                Pos2::new(rect.left() + (x as f32 + 1.0) * bw, rect.bottom()),
            );
            p.rect_filled(bar, Rounding::ZERO, with_alpha(colors[c], 110));
        }
    }
}

/// 간단한 LRU 텍스처 캐시. 키 = 항목 인덱스. 원본 픽셀 크기를 함께 보관(1:1 줌용).
pub struct TexCache {
    map: HashMap<usize, (egui::TextureHandle, bool, [usize; 2])>, // (handle, is_full_raw, size)
    order: VecDeque<usize>,
    cap: usize,
    // eviction된 핸들을 바로 버리지 않고 잠시 살려두는 유예(은퇴) 큐.
    // egui/wgpu는 핸들이 드롭되면 프레임 경계에서 GPU 텍스처를 파괴하는데, 빠른 스크롤 중
    // 방금 그린 셀이 곧바로 eviction되면 제출(submit) 중인 프레임이 파괴된 텍스처를 참조해
    // "Texture ... has been destroyed" 크래시가 난다. 유예 보관으로 in-flight 참조를 안전하게.
    //
    // 해제 시점은 **프레임 수(TTL)** 기준이다. 개수 기준만으로는 빠른 스크롤 한 프레임에
    // 수십 장이 은퇴하면 방금 그린 핸들이 같은 프레임 안에서 밀려나 드롭→크래시할 수 있다.
    // 각 핸들은 RETIRE_TTL_FRAMES 프레임 동안 보존되어 in-flight GPU 제출이 끝날 때까지 산다.
    // retire_keep은 병적인 churn에 대비한 VRAM 안전 상한일 뿐이며, 가장 최근(=현재 프레임에
    // 그려졌을 가능성이 큰) 핸들이 남도록 오래된 것부터 버린다.
    retired: VecDeque<(egui::TextureHandle, u8)>,
    retire_keep: usize,
}

/// 은퇴한 텍스처 핸들을 드롭하기 전 살려두는 프레임 수.
/// eframe/wgpu의 in-flight 프레임(스왑체인 + 명령버퍼)이 파괴된 텍스처를 참조하지 않도록,
/// 최대 프레임 지연보다 넉넉히 잡는다.
const RETIRE_TTL_FRAMES: u8 = 3;

impl TexCache {
    pub fn new(cap: usize, retire_keep: usize) -> Self {
        TexCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
            retired: VecDeque::new(),
            retire_keep,
        }
    }

    /// (텍스처 ID, 풀RAW 여부, 원본 픽셀 크기).
    pub fn get(&mut self, id: usize) -> Option<(egui::TextureId, bool, egui::Vec2)> {
        if let Some((h, full, size)) = self.map.get(&id) {
            let tid = h.id();
            let full = *full;
            let sz = egui::Vec2::new(size[0] as f32, size[1] as f32);
            self.touch(id);
            Some((tid, full, sz))
        } else {
            None
        }
    }

    pub fn contains_full(&self, id: usize, want_full: bool) -> bool {
        match self.map.get(&id) {
            Some((_, full, _)) => *full == want_full,
            None => false,
        }
    }

    /// 크기/종류 무관하게 해당 id 텍스처가 있는지.
    pub fn contains(&self, id: usize) -> bool {
        self.map.contains_key(&id)
    }

    /// 현재 캐시된 텍스처 수(계측·벤치용). 릴리즈에선 벤치가 제외돼 미사용일 수 있다.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn insert(&mut self, id: usize, handle: egui::TextureHandle, full: bool) {
        let size = handle.size();
        // 같은 id 교체 시 반환되는 이전 핸들을 **즉시 드롭하지 말고** 유예한다.
        // (그냥 두면 map.insert가 반환한 옛 핸들이 이 자리에서 드롭→텍스처 파괴되어,
        //  제출 중인 프레임이 참조하면 "Texture ... destroyed"로 크래시.)
        if let Some((old, _, _)) = self.map.insert(id, (handle, full, size)) {
            self.retire(old);
            self.touch(id);
        } else {
            self.order.push_back(id);
        }
        // LRU: 가장 오래 안 쓰인 것부터 제거(현재 항목은 매 프레임 touch되어 뒤쪽이라 보호됨).
        while self.order.len() > self.cap {
            if let Some(old_id) = self.order.pop_front() {
                if old_id != id {
                    if let Some((handle, _, _)) = self.map.remove(&old_id) {
                        self.retire(handle);
                    }
                }
            }
        }
    }

    /// 밀려난/교체된 핸들을 유예 큐에 넣는다(TTL 부여). 실제 드롭은 tick()이 프레임 경계에서
    /// 한다. egui/wgpu는 핸들 드롭 시 프레임 경계에서 GPU 텍스처를 파괴하므로, in-flight
    /// 프레임이 참조할 수 있는 동안은 살려둬야 "Texture has been destroyed" 크래시를 막는다.
    fn retire(&mut self, handle: egui::TextureHandle) {
        self.retired.push_back((handle, RETIRE_TTL_FRAMES));
        // VRAM 안전 상한(오래된 것부터). 정상 동작에선 TTL이 먼저 비우므로 거의 닿지 않는다.
        while self.retired.len() > self.retire_keep {
            self.retired.pop_front();
        }
    }

    /// 매 프레임 호출. 은퇴 핸들의 TTL을 깎고 0이 된 것만 실제로 드롭(=GPU 텍스처 파괴)한다.
    /// 프레임 단위로 살리므로 churn 양과 무관하게 in-flight 참조가 끝난 뒤에만 파괴된다.
    pub fn tick(&mut self) {
        self.retired.retain_mut(|(_, ttl)| {
            *ttl = ttl.saturating_sub(1);
            *ttl > 0
        });
    }

    /// 폴더 전환 등으로 캐시를 통째로 비울 때 사용. 핸들을 **즉시 드롭하지 않고** 유예 큐로
    /// 옮겨 TTL 동안 살려둔다(통째 교체 시 in-flight 프레임이 참조 중인 텍스처가 파괴되는
    /// 크래시 방지). order(LRU) 앞쪽=오래된 것부터 넣어, 상한에 걸리면 현재 프레임에 그려졌을
    /// 가능성이 큰 최신 핸들이 남도록 한다.
    pub fn retire_all(&mut self) {
        while let Some(id) = self.order.pop_front() {
            if let Some((handle, _, _)) = self.map.remove(&id) {
                self.retired.push_back((handle, RETIRE_TTL_FRAMES));
            }
        }
        // order에 없던 잔여 핸들도 안전하게 유예.
        for (_, (handle, _, _)) in self.map.drain() {
            self.retired.push_back((handle, RETIRE_TTL_FRAMES));
        }
        while self.retired.len() > self.retire_keep {
            self.retired.pop_front();
        }
    }

    fn touch(&mut self, id: usize) {
        if let Some(pos) = self.order.iter().position(|&x| x == id) {
            self.order.remove(pos);
        }
        self.order.push_back(id);
    }
}
