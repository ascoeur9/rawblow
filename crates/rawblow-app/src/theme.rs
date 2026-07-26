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
    v.window_stroke = egui::Stroke::new(1.0_f32, LINE2);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = ACCENT;

    let widgets = &mut v.widgets;
    // `bg_fill`뿐 아니라 `weak_bg_fill`도 반드시 함께 덮는다(#83). egui 0.29에서 기본 스타일의
    // Button·ComboBox·SelectableLabel은 bg_fill이 아니라 **weak_bg_fill**로 칠해지는데,
    // 여기 값을 안 주면 egui 기본 dark의 gray(60)/gray(70)/gray(55)가 그대로 남아
    // near-black 셸(BG1=11,13,17) 위에 밝은 회색 덩어리로 뜬다.
    widgets.noninteractive.bg_fill = BG1;
    widgets.noninteractive.weak_bg_fill = BG1;
    widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, INK2);
    widgets.inactive.bg_fill = BG3;
    widgets.inactive.weak_bg_fill = BG3;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, INK2);
    widgets.hovered.bg_fill = BG4;
    widgets.hovered.weak_bg_fill = BG4;
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, LINE2);
    widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, INK);
    widgets.active.bg_fill = BG4;
    widgets.active.weak_bg_fill = BG4;
    widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, LINE3);
    widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, INK);
    // 열린 ComboBox/CollapsingHeader 팝업도 같은 팔레트로(기본은 gray(27)/gray(45) + gray(210) 텍스트).
    widgets.open.bg_fill = BG3;
    widgets.open.weak_bg_fill = BG3;
    widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, LINE2);
    widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, INK2);

    // 이 앱은 전용 다크 테마다. egui는 기본적으로 OS 테마를 따르므로(ThemePreference::System),
    // OS가 "라이트 모드"인 PC에서는 egui가 light 슬롯의 기본값을 써서 패널 구분선(SidePanel/
    // TopBottomPanel separator)이 밝은 회색(from_gray(190))으로 그려진다 → "요소 사이 흰 줄"(#83).
    // 다크 모드 PC에서는 from_gray(60)이라 near-black 패널에 묻혀 보이지 않아 "특정 PC에서만" 재현됐다.
    // 테마를 다크로 고정하고, 라이트 슬롯에도 동일 비주얼을 넣어 OS 모드와 무관하게 동일 렌더링을 보장한다.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 도형 트리를 훑어 칠해진 사각형 색을 모은다(`Shape::Vec` 중첩 대응).
    fn rect_fills(shapes: &[egui::Shape], out: &mut Vec<Color32>) {
        for s in shapes {
            match s {
                egui::Shape::Rect(r) => out.push(r.fill),
                egui::Shape::Vec(v) => rect_fills(v, out),
                _ => {}
            }
        }
    }

    /// #83: egui 기본 스타일의 Button·ComboBox·SelectableLabel은 `bg_fill`이 아니라
    /// **`weak_bg_fill`**로 칠해진다(egui 0.29 `widgets/button.rs`,
    /// `containers/combo_box.rs`, `widgets/selected_label.rs`). 여기를 안 덮으면
    /// egui 기본 dark의 회색(gray 27/45/55/60/70)이 near-black 셸 위에 그대로 뜬다.
    ///
    /// 스타일 필드만 확인하면 "어떤 위젯이 그 필드를 읽는가"를 놓치므로,
    /// 실제로 **그려진 사각형 색**을 검사해 팔레트 밖 색이 안 나오는지 본다.
    #[test]
    fn default_widgets_use_theme_palette_not_egui_gray() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(BG1))
                .show(ctx, |ui| {
                    let _ = ui.button("button");
                    let _ = ui.selectable_label(false, "selectable");
                    egui::ComboBox::from_id_salt("cb")
                        .selected_text("combo")
                        .show_ui(ui, |_| {});
                });
        });
        let mut fills = Vec::new();
        rect_fills(
            &out.shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
            &mut fills,
        );
        assert!(!fills.is_empty(), "그려진 사각형이 없다 — 테스트 전제 실패");

        // egui 기본 dark의 weak_bg_fill 값들(style.rs Widgets::dark)이 새어 나오면 안 된다.
        for g in [27u8, 45, 55, 60, 70] {
            let c = Color32::from_gray(g);
            assert!(
                !fills.contains(&c),
                "egui 기본 회색 gray({g})이 그대로 칠해졌다 — weak_bg_fill 누락 (fills={fills:?})"
            );
        }
        // 팔레트 색으로 칠해졌는지(inactive.weak_bg_fill = BG3).
        assert!(
            fills.contains(&BG3),
            "기본 위젯이 팔레트 색(BG3)으로 칠해져야 한다 (fills={fills:?})"
        );
    }

    /// #83의 실제 원인: OS가 라이트 모드면 egui가 **light 슬롯**을 쓰는데 앱이 그쪽을
    /// 안 덮어 구분선이 gray(190)으로 그려졌다. 두 슬롯 모두 같은 값이어야 한다.
    #[test]
    fn both_theme_slots_get_the_dark_palette() {
        let ctx = egui::Context::default();
        apply(&ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let v = ctx.style_of(theme).visuals.clone();
            assert_eq!(v.widgets.noninteractive.bg_stroke.color, LINE, "{theme:?} 패널 구분선");
            assert_eq!(v.widgets.inactive.weak_bg_fill, BG3, "{theme:?} 기본 위젯 배경");
            assert_eq!(v.panel_fill, BG1, "{theme:?} 패널 배경");
        }
    }
}
