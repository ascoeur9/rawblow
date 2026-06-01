//! 공용 그리기 헬퍼: HUD 그림자 텍스트, 키캡, 라벨 칩, 썸네일, LRU 텍스처 캐시.
//! 디자인 핸드오프 §Component specs / §HUD overlay rule을 따른다.

use crate::theme;
use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Ui, Vec2};
use rawblow_core::Label;
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
    pub selected: bool, // 다중 선택됨
    pub stars: u8,      // 별점(0~5, #23)
}

/// 주어진 rect에 썸네일을 그린다. 텍스처가 있으면 이미지를, 없으면 플레이스홀더.
pub fn draw_thumb(ui: &Ui, rect: Rect, tex: Option<egui::TextureId>, size: Vec2, info: &ThumbInfo) {
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
    }
    // 다중 선택 표시: 강조색 반투명 오버레이.
    if info.selected {
        p.rect_filled(rect, Rounding::same(3.0), with_alpha(theme::ACCENT, 64));
    }
    // 테두리(active/selected > focused > 기본).
    let stroke = if info.active || info.selected {
        Stroke::new(2.0, theme::ACCENT)
    } else if info.focused {
        Stroke::new(1.5, theme::LINE3)
    } else {
        Stroke::new(1.0, theme::LINE)
    };
    p.rect_stroke(rect, Rounding::same(3.0), stroke);
    // 라벨 점(TL).
    if !matches!(info.label, Label::Unrated) {
        p.circle_filled(
            Pos2::new(rect.left() + 7.0, rect.top() + 7.0),
            3.0,
            theme::label_color(info.label),
        );
    }
    // RAW 배지(TR).
    if info.raw_badge || info.raw_only {
        let txt = if info.raw_only { "RAW" } else { "RAW+" };
        let font = mono(8.0);
        let g = p.layout_no_wrap(txt.to_string(), font.clone(), Color32::WHITE);
        let bw = g.size().x + 6.0;
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - bw - 4.0, rect.top() + 4.0),
            Vec2::new(bw, 12.0),
        );
        let (bg, fg) = if info.raw_only {
            (theme::HOLD, Color32::from_rgb(0x20, 0x18, 0x00))
        } else {
            (Color32::from_black_alpha(180), Color32::WHITE)
        };
        p.rect_filled(badge, Rounding::same(2.0), bg);
        p.text(badge.center(), Align2::CENTER_CENTER, txt, font, fg);
    }
    // 별점 배지(BL, #23): ★N. 라벨 점(TL)·RAW 배지(TR)와 위치가 겹치지 않는다.
    if info.stars > 0 {
        let txt = format!("★{}", info.stars);
        let font = mono(8.5);
        let g = p.layout_no_wrap(txt.clone(), font.clone(), theme::HOLD);
        let bw = g.size().x + 6.0;
        let badge = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.bottom() - 14.0),
            Vec2::new(bw, 11.0),
        );
        p.rect_filled(badge, Rounding::same(2.0), Color32::from_black_alpha(170));
        p.text(badge.center(), Align2::CENTER_CENTER, &txt, font, theme::HOLD);
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
        for x in 0..n {
            let v = bins[c][x] as f32 / maxf;
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
    // 한 프레임에 참조되는 텍스처 수에 맞춰 크기를 정한다(썸네일은 화면에 수십 장 → 크게,
    // 프리뷰는 1~2장 + 텍스처가 거대 → 작게: 큰 텍스처를 과다 보관하면 VRAM 낭비).
    retired: VecDeque<egui::TextureHandle>,
    retire_keep: usize,
}

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

    /// 밀려난/교체된 핸들을 유예 큐에 넣고, 충분히 오래된 것만 실제로 드롭한다.
    /// egui/wgpu는 핸들 드롭 시 프레임 경계에서 GPU 텍스처를 파괴하므로, in-flight
    /// 프레임이 참조할 수 있는 동안은 살려둬야 "Texture has been destroyed" 크래시를 막는다.
    fn retire(&mut self, handle: egui::TextureHandle) {
        self.retired.push_back(handle);
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
