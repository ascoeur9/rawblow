//! 디자인 핸드오프 색 토큰 → egui. near-black 쿨톤 다크 테마.

use egui::Color32;

/// 사진 표시 배경(photo void) 기본 RGB. 설정에서 photo_bg 미지정 시 사용(#36).
pub const BG0_RGB: [u8; 3] = [0x06, 0x07, 0x0a];
pub const BG0: Color32 = Color32::from_rgb(BG0_RGB[0], BG0_RGB[1], BG0_RGB[2]); // photo void
pub const BG1: Color32 = Color32::from_rgb(0x0b, 0x0d, 0x11); // app shell
pub const BG2: Color32 = Color32::from_rgb(0x14, 0x17, 0x1c); // panels
pub const BG3: Color32 = Color32::from_rgb(0x1b, 0x1f, 0x26); // elevated
pub const BG4: Color32 = Color32::from_rgb(0x26, 0x2c, 0x35); // hover/active

pub const LINE: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2c);
pub const LINE2: Color32 = Color32::from_rgb(0x2c, 0x33, 0x3d);
pub const LINE3: Color32 = Color32::from_rgb(0x3b, 0x43, 0x4f);

pub const INK: Color32 = Color32::from_rgb(0xe8, 0xea, 0xed);
pub const INK2: Color32 = Color32::from_rgb(0xaa, 0xb0, 0xba);
pub const INK3: Color32 = Color32::from_rgb(0x73, 0x7a, 0x85);
pub const INK4: Color32 = Color32::from_rgb(0x4d, 0x53, 0x5d);

pub const ACCENT: Color32 = Color32::from_rgb(0x6a, 0xb8, 0xff);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x24, 0x45, 0x61);

#[allow(dead_code)]
pub const PICK: Color32 = Color32::from_rgb(0x4a, 0xde, 0x80);
pub const HOLD: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const REJECT: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
#[allow(dead_code)]
pub const UNRATED: Color32 = Color32::from_rgb(0x6b, 0x72, 0x80);

pub const WARN: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
pub const OK: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);

/// 코어 라벨 → 테마 색.
pub fn label_color(label: rawblow_core::Label) -> Color32 {
    let [r, g, b] = label.color_rgb();
    Color32::from_rgb(r, g, b)
}

/// egui 전역 비주얼을 핸드오프 토큰으로 설정한다.
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(INK);
    v.panel_fill = BG1;
    v.window_fill = BG2;
    v.extreme_bg_color = BG0;
    v.faint_bg_color = BG2;
    v.window_stroke = egui::Stroke::new(1.0, LINE2);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    let widgets = &mut v.widgets;
    widgets.noninteractive.bg_fill = BG1;
    widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, INK2);
    widgets.inactive.bg_fill = BG3;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0, LINE);
    widgets.inactive.fg_stroke = egui::Stroke::new(1.0, INK2);
    widgets.hovered.bg_fill = BG4;
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0, LINE2);
    widgets.hovered.fg_stroke = egui::Stroke::new(1.0, INK);
    widgets.active.bg_fill = BG4;
    widgets.active.bg_stroke = egui::Stroke::new(1.0, LINE3);
    widgets.active.fg_stroke = egui::Stroke::new(1.0, INK);

    ctx.set_visuals(v);
}
