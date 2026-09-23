//! 설정·라이센스 화면(분해 4/8): ui_settings(#22 캐시·#30 언어·#36 배경색 등)·
//! ui_licenses(#39). app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

// 설정 화면 텍스트 대비 상향: 배경 BG1(#0b0d11) 위에서 기존 INK4(≈2.5:1)·INK3(≈4.5:1)는
// 특히 작은 설명 문구가 잘 안 보였다. 이 화면에 한해 잉크 톤을 한 단계 올린다
// — 설명/힌트 INK4→INK3(≈4.5:1, AA 충족), 섹션 헤더 INK3→INK2(≈8.9:1). 전역 theme는 유지.
use crate::theme::INK2 as INK_HEAD; // 섹션 헤더
use crate::theme::INK3 as INK_HELP; // 설명·힌트

/// 설정 섹션 카드 필. 셸(`theme::BG1`)과 구분되는 패널 배경 — 테스트가 이 상수를 본다.
pub(super) const SETTINGS_SECTION_FILL: Color32 = theme::BG2;
pub(super) const SETTINGS_SECTION_STROKE: Color32 = theme::LINE2;
/// 설정 액션 버튼 크롬. 비활성에서도 fill·stroke가 둘 다 불투명. `toggle_btn`과 달리 투명 금지.
pub(super) const SETTINGS_ACTION_FILL: Color32 = theme::BG3;
pub(super) const SETTINGS_ACTION_STROKE: Color32 = theme::LINE2;

const SETTINGS_COL_W: f32 = 560.0;
const SETTINGS_ROW_H: f32 = 48.0;
const SETTINGS_TOGGLE_W: f32 = 52.0;
const SETTINGS_TOGGLE_H: f32 = 30.0;
const SETTINGS_BTN_H: f32 = 36.0;
const SETTINGS_LABEL_FONT: f32 = 15.0;
const SETTINGS_HEAD_FONT: f32 = 15.0;
const SETTINGS_BTN_FONT: f32 = 15.0;

/// 설정 섹션: 헤더 + 셸 위의 카드. 폭은 바깥 칼럼을 따르고 창 전체로 늘리지 않는다.
pub(super) fn settings_section_panel(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.label(egui::RichText::new(title).font(prop(SETTINGS_HEAD_FONT)).color(INK_HEAD));
    ui.add_space(8.0);
    egui::Frame::none()
        .fill(SETTINGS_SECTION_FILL)
        .stroke(Stroke::new(1.0_f32, SETTINGS_SECTION_STROKE))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(18.0, 12.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
            add_contents(ui);
        });
    ui.add_space(22.0);
}

/// 설정 전용 액션 버튼. 비활성에서도 fill+stroke. 창 폭을 채우지 않는다(데스크톱 크기).
pub(super) fn settings_action_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    settings_action_btn_sized(ui, label, theme::INK, SETTINGS_ACTION_FILL, SETTINGS_ACTION_STROKE, Vec2::new(128.0, SETTINGS_BTN_H))
}

fn settings_action_btn_sized(
    ui: &mut egui::Ui,
    label: &str,
    fg: Color32,
    fill: Color32,
    stroke: Color32,
    min_size: Vec2,
) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).font(prop(SETTINGS_BTN_FONT)).color(fg))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, stroke))
            .rounding(Rounding::same(8.0))
            .min_size(min_size),
    )
}

#[derive(Clone, Copy)]
enum SIcon {
    Skip,
    Folder,
    Exif,
    Hist,
    Update,
    Preload,
    Grid,
    Badge,
    Sort,
    Orig,
    Send,
    Lang,
    Cache,
    Reset,
    Release,
    Issue,
    License,
    Heart,
    Cosly,
    Person,
    Key,
}

const SICON: f32 = 18.0;
const SICON_GAP: f32 = 10.0;

fn draw_sicon(p: &egui::Painter, rect: Rect, kind: SIcon) {
    let c = theme::INK2;
    let st = Stroke::new(1.35_f32, c);
    let r = rect.shrink(1.5);
    let mid = r.center();
    match kind {
        SIcon::Skip => {
            for ox in [-3.2, 2.2] {
                let a = Pos2::new(mid.x + ox - 3.0, r.top() + 4.0);
                let b = Pos2::new(mid.x + ox + 3.5, mid.y);
                let d = Pos2::new(mid.x + ox - 3.0, r.bottom() - 4.0);
                p.line_segment([a, b], st);
                p.line_segment([b, d], st);
            }
        }
        SIcon::Folder => crate::widgets::draw_folder_icon(p, r, true),
        SIcon::Exif => {
            p.circle_stroke(mid, r.width() * 0.42, st);
            p.line_segment([Pos2::new(mid.x, mid.y - 1.0), Pos2::new(mid.x, mid.y + 4.5)], st);
            p.circle_filled(Pos2::new(mid.x, mid.y - 4.2), 1.15, c);
        }
        SIcon::Hist => {
            let base = r.bottom() - 2.0;
            let xs = [0.18, 0.42, 0.66, 0.88];
            let hs = [0.35, 0.70, 0.48, 0.85];
            for (i, &xf) in xs.iter().enumerate() {
                let x = r.left() + r.width() * xf;
                p.line_segment([Pos2::new(x, base), Pos2::new(x, base - r.height() * hs[i])], st);
            }
        }
        SIcon::Update => {
            p.circle_stroke(mid, r.width() * 0.36, st);
            let tip = Pos2::new(r.right() - 2.5, mid.y - 3.5);
            p.line_segment([Pos2::new(mid.x + 3.0, r.top() + 2.5), tip], st);
            p.line_segment([tip, Pos2::new(r.right() - 2.0, mid.y + 1.5)], st);
        }
        SIcon::Preload => {
            let a = Rect::from_min_max(Pos2::new(r.left() + 1.0, r.top() + 4.0), Pos2::new(r.right() - 5.0, r.bottom() - 2.0));
            let b = Rect::from_min_max(Pos2::new(r.left() + 5.0, r.top() + 1.0), Pos2::new(r.right() - 1.0, r.bottom() - 5.0));
            p.rect_stroke(a, Rounding::same(2.0), st);
            p.rect_stroke(b, Rounding::same(2.0), st);
        }
        SIcon::Grid => {
            let q = r.width() * 0.32;
            let g = 2.2;
            for ix in 0..2 {
                for iy in 0..2 {
                    let o = Pos2::new(r.left() + 2.0 + ix as f32 * (q + g), r.top() + 2.0 + iy as f32 * (q + g));
                    p.rect_stroke(Rect::from_min_size(o, Vec2::splat(q)), Rounding::same(1.2), st);
                }
            }
        }
        SIcon::Badge => {
            p.rect_stroke(
                Rect::from_center_size(mid, Vec2::new(r.width() * 0.72, r.height() * 0.48)),
                Rounding::same(3.0),
                st,
            );
        }
        SIcon::Sort => {
            p.line_segment([Pos2::new(mid.x - 2.0, r.bottom() - 3.0), Pos2::new(mid.x - 2.0, r.top() + 3.0)], st);
            p.line_segment([Pos2::new(mid.x - 5.0, r.top() + 6.5), Pos2::new(mid.x - 2.0, r.top() + 3.0)], st);
            p.line_segment([Pos2::new(mid.x + 2.5, r.top() + 3.0), Pos2::new(mid.x + 2.5, r.bottom() - 3.0)], st);
            p.line_segment([Pos2::new(mid.x + 5.5, r.bottom() - 6.5), Pos2::new(mid.x + 2.5, r.bottom() - 3.0)], st);
        }
        SIcon::Orig => {
            let i = r.shrink(3.0);
            p.line_segment([i.left_top(), Pos2::new(i.left() + 5.0, i.top())], st);
            p.line_segment([i.left_top(), Pos2::new(i.left(), i.top() + 5.0)], st);
            p.line_segment([i.right_top(), Pos2::new(i.right() - 5.0, i.top())], st);
            p.line_segment([i.right_top(), Pos2::new(i.right(), i.top() + 5.0)], st);
            p.line_segment([i.left_bottom(), Pos2::new(i.left() + 5.0, i.bottom())], st);
            p.line_segment([i.left_bottom(), Pos2::new(i.left(), i.bottom() - 5.0)], st);
            p.line_segment([i.right_bottom(), Pos2::new(i.right() - 5.0, i.bottom())], st);
            p.line_segment([i.right_bottom(), Pos2::new(i.right(), i.bottom() - 5.0)], st);
        }
        SIcon::Send => {
            crate::widgets::draw_folder_icon(p, r, true);
        }
        SIcon::Lang => {
            p.circle_stroke(mid, r.width() * 0.40, st);
            p.line_segment([Pos2::new(r.left() + 2.0, mid.y), Pos2::new(r.right() - 2.0, mid.y)], st);
            p.line_segment([Pos2::new(mid.x, r.top() + 2.0), Pos2::new(mid.x, r.bottom() - 2.0)], st);
        }
        SIcon::Cache => {
            p.rect_stroke(
                Rect::from_center_size(mid, Vec2::new(r.width() * 0.62, r.height() * 0.70)),
                Rounding::same(2.0),
                st,
            );
            p.line_segment([Pos2::new(r.left() + 4.0, mid.y), Pos2::new(r.right() - 4.0, mid.y)], st);
        }
        SIcon::Reset => {
            p.circle_stroke(mid, r.width() * 0.34, st);
            let tip = Pos2::new(r.left() + 3.0, mid.y - 4.0);
            p.line_segment([Pos2::new(mid.x - 4.0, r.top() + 2.5), tip], st);
            p.line_segment([tip, Pos2::new(r.left() + 2.5, mid.y + 0.5)], st);
        }
        SIcon::Release => {
            p.line_segment([Pos2::new(mid.x, r.top() + 2.0), Pos2::new(mid.x, r.bottom() - 3.0)], st);
            p.line_segment([Pos2::new(mid.x - 4.0, r.bottom() - 7.0), Pos2::new(mid.x, r.bottom() - 3.0)], st);
            p.line_segment([Pos2::new(mid.x + 4.0, r.bottom() - 7.0), Pos2::new(mid.x, r.bottom() - 3.0)], st);
            p.line_segment([Pos2::new(r.left() + 2.5, r.bottom() - 2.0), Pos2::new(r.right() - 2.5, r.bottom() - 2.0)], st);
        }
        SIcon::Issue => {
            p.circle_stroke(mid, r.width() * 0.38, st);
            p.line_segment([Pos2::new(mid.x, mid.y - 3.5), Pos2::new(mid.x, mid.y + 1.5)], st);
            p.circle_filled(Pos2::new(mid.x, mid.y + 4.2), 1.1, c);
        }
        SIcon::License => {
            let doc = Rect::from_min_max(Pos2::new(r.left() + 3.5, r.top() + 2.0), Pos2::new(r.right() - 3.5, r.bottom() - 2.0));
            p.rect_stroke(doc, Rounding::same(1.5), st);
            p.line_segment([Pos2::new(doc.left() + 2.5, doc.top() + 4.0), Pos2::new(doc.right() - 2.5, doc.top() + 4.0)], st);
            p.line_segment([Pos2::new(doc.left() + 2.5, doc.top() + 7.5), Pos2::new(doc.right() - 2.5, doc.top() + 7.5)], st);
        }
        SIcon::Heart => {
            let s = r.width();
            let pts = vec![
                Pos2::new(mid.x, r.bottom() - 2.5),
                Pos2::new(r.left() + 2.0, mid.y - 0.5),
                Pos2::new(r.left() + s * 0.32, r.top() + 2.5),
                Pos2::new(mid.x, r.top() + 5.5),
                Pos2::new(r.right() - s * 0.32, r.top() + 2.5),
                Pos2::new(r.right() - 2.0, mid.y - 0.5),
            ];
            p.add(egui::Shape::closed_line(pts, st));
        }
        SIcon::Cosly => {
            p.circle_stroke(mid, r.width() * 0.28, st);
            p.circle_stroke(mid, r.width() * 0.42, Stroke::new(1.1_f32, c));
        }
        SIcon::Person => {
            p.circle_stroke(Pos2::new(mid.x, r.top() + 6.0), 3.2, st);
            p.line_segment([Pos2::new(r.left() + 3.5, r.bottom() - 2.5), Pos2::new(r.right() - 3.5, r.bottom() - 2.5)], st);
            p.line_segment([Pos2::new(r.left() + 3.5, r.bottom() - 2.5), Pos2::new(r.left() + 5.0, mid.y + 1.5)], st);
            p.line_segment([Pos2::new(r.right() - 3.5, r.bottom() - 2.5), Pos2::new(r.right() - 5.0, mid.y + 1.5)], st);
        }
        SIcon::Key => {
            p.rect_stroke(
                Rect::from_min_max(Pos2::new(r.left() + 2.0, mid.y - 3.2), Pos2::new(r.right() - 2.0, mid.y + 3.2)),
                Rounding::same(1.5),
                st,
            );
            p.line_segment([Pos2::new(r.left() + 5.5, mid.y), Pos2::new(r.left() + 5.5, mid.y + 2.0)], Stroke::new(1.2, c));
            p.line_segment([Pos2::new(mid.x, mid.y), Pos2::new(mid.x, mid.y + 2.0)], Stroke::new(1.2, c));
            p.line_segment([Pos2::new(r.right() - 5.5, mid.y), Pos2::new(r.right() - 5.5, mid.y + 2.0)], Stroke::new(1.2, c));
        }
    }
}

fn paint_settings_toggle(painter: &egui::Painter, rect: Rect, on: bool) {
    let fill = if on { theme::ACCENT } else { theme::BG4 };
    let stroke = Stroke::new(1.0_f32, if on { theme::ACCENT } else { theme::LINE2 });
    painter.rect(rect, Rounding::same(rect.height() * 0.5), fill, stroke);
    let pad = 2.0;
    let r = (rect.height() - pad * 2.0) * 0.5;
    let cx = if on {
        rect.right() - pad - r
    } else {
        rect.left() + pad + r
    };
    painter.circle_filled(Pos2::new(cx, rect.center().y), r, theme::INK);
}

/// 라벨 왼쪽 · 스위치 오른쪽. 행 전체가 클릭된다(체크박스 없음).
fn settings_toggle_row(
    ui: &mut egui::Ui,
    icon: SIcon,
    label: &str,
    on: &mut bool,
    hint: Option<&str>,
) -> bool {
    let w = ui.available_width();
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(w, SETTINGS_ROW_H), Sense::click());
    let ir = Rect::from_center_size(Pos2::new(rect.left() + SICON * 0.5, rect.center().y), Vec2::splat(SICON));
    draw_sicon(ui.painter(), ir, icon);
    ui.painter().text(
        Pos2::new(rect.left() + SICON + SICON_GAP, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        prop(SETTINGS_LABEL_FONT),
        theme::INK,
    );
    let trect = Rect::from_center_size(
        Pos2::new(rect.right() - SETTINGS_TOGGLE_W * 0.5, rect.center().y),
        Vec2::new(SETTINGS_TOGGLE_W, SETTINGS_TOGGLE_H),
    );
    paint_settings_toggle(ui.painter(), trect, *on);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if let Some(h) = hint {
        resp = resp.on_hover_text(h);
    }
    if resp.clicked() {
        *on = !*on;
        true
    } else {
        false
    }
}

/// 라벨 왼쪽 · 컨트롤 오른쪽. 같은 카드 안에서 x가 맞도록 한 줄, 접지 않음.
fn settings_field_row(ui: &mut egui::Ui, icon: SIcon, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), SETTINGS_ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            let (ir, _) = ui.allocate_exact_size(Vec2::splat(SICON), Sense::hover());
            draw_sicon(ui.painter(), ir, icon);
            ui.add_space(SICON_GAP - ui.spacing().item_spacing.x);
            ui.label(egui::RichText::new(label).font(prop(SETTINGS_LABEL_FONT)).color(theme::INK2));
            ui.with_layout(Layout::right_to_left(Align::Center), add);
        },
    );
}

/// 배타 선택: 아이콘+짧은 라벨, 세그먼트는 카드 폭을 채움.
fn settings_choice_block(
    ui: &mut egui::Ui,
    icon: SIcon,
    label: &str,
    options: &[(&str, &str)],
    selected: usize,
) -> Option<usize> {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (ir, _) = ui.allocate_exact_size(Vec2::splat(SICON), Sense::hover());
        draw_sicon(ui.painter(), ir, icon);
        ui.label(egui::RichText::new(label).font(prop(SETTINGS_LABEL_FONT)).color(theme::INK2));
    });
    ui.add_space(8.0);
    let clicked = settings_segmented(ui, options, selected);
    ui.add_space(8.0);
    clicked
}

/// 설정용 세그먼트. 칸을 균등 분할하고 비활성에도 필이 있다(글로벌 `segmented`와 별개).
fn settings_segmented(ui: &mut egui::Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let mut clicked = None;
    let n = options.len().max(1);
    let full = ui.available_width();
    let pad = 3.0;
    let gap = 3.0;
    let h = 38.0;
    let inner_w = (full - pad * 2.0).max(0.0);
    let cell_w = ((inner_w - gap * (n as f32 - 1.0)) / n as f32).max(48.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(full, h + pad * 2.0), Sense::hover());
    ui.painter().rect(
        outer,
        Rounding::same(8.0),
        theme::BG1,
        Stroke::new(1.0_f32, theme::LINE2),
    );
    for (i, (label, _sub)) in options.iter().enumerate() {
        let x = outer.left() + pad + i as f32 * (cell_w + gap);
        let rect = Rect::from_min_size(Pos2::new(x, outer.top() + pad), Vec2::new(cell_w, h));
        let id = ui.id().with(("settings_seg", i, *label));
        let resp = ui.interact(rect, id, Sense::click());
        let active = i == selected;
        ui.painter().rect(
            rect,
            Rounding::same(6.0),
            if active { theme::BG4 } else { theme::BG3 },
            Stroke::new(1.0_f32, if active { theme::LINE3 } else { theme::LINE }),
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            *label,
            prop(14.0),
            if active { theme::INK } else { theme::INK2 },
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

fn settings_inner_rule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0_f32, theme::LINE));
    ui.add_space(6.0);
}

/// 설정 맨 위 아이덴티티: 로고 + 버전 + 만든 사람. 링크/라이선스와 섞지 않는다.
fn settings_identity(ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(SETTINGS_SECTION_FILL)
        .stroke(Stroke::new(1.0_f32, SETTINGS_SECTION_STROKE))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(18.0, 16.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 6.0);
            ui.horizontal(|ui| {
                let (mark, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                crate::logo::draw_mark(ui.painter(), mark);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("RawBlow").font(prop(22.0)).color(theme::INK));
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .font(prop(14.0))
                            .color(INK_HELP),
                    );
                });
            });
            settings_inner_rule(ui);
            ui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(SICON), Sense::hover());
                draw_sicon(ui.painter(), ir, SIcon::Person);
                ui.label(egui::RichText::new("하레").font(prop(SETTINGS_LABEL_FONT)).color(theme::INK2));
                ui.add_space(8.0);
                settings_inline_link(ui, "@ascoeur9", "https://x.com/ascoeur9");
                ui.label(egui::RichText::new("·").font(prop(14.0)).color(INK_HELP));
                settings_inline_link(ui, "@hare_kig", "https://x.com/hare_kig");
            });
        });
    ui.add_space(22.0);
}

fn settings_inline_link(ui: &mut egui::Ui, text: &str, url: &str) {
    let resp = ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(prop(14.0))
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

/// 링크 전용 행. 라벨 왼쪽, 화살표 오른쪽. 버튼과 겹치지 않게 행 높이를 고정한다.
fn settings_link_row(ui: &mut egui::Ui, icon: SIcon, label: &str, url: &str) {
    if settings_nav_row(ui, icon, label) {
        open_url(url);
    }
}

fn settings_nav_row(ui: &mut egui::Ui, icon: SIcon, label: &str) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, SETTINGS_ROW_H), Sense::click());
    let ir = Rect::from_center_size(Pos2::new(rect.left() + SICON * 0.5, rect.center().y), Vec2::splat(SICON));
    draw_sicon(ui.painter(), ir, icon);
    ui.painter().text(
        Pos2::new(rect.left() + SICON + SICON_GAP, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        prop(SETTINGS_LABEL_FONT),
        theme::INK,
    );
    ui.painter().text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        "↗",
        prop(SETTINGS_LABEL_FONT),
        INK_HELP,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// 색 태그 한 줄: 점 + 고정 폭 이름 + 나머지 폭을 채우는 입력. 힌트 길이로 필드가 줄어들지 않는다.
fn settings_tag_name_row(
    ui: &mut egui::Ui,
    rgb: [u8; 3],
    default_name: &str,
    value: &mut String,
) -> bool {
    const NAME_COL: f32 = 88.0;
    const FIELD_H: f32 = 36.0;
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), SETTINGS_ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            ui.painter().circle_filled(
                dot.center(),
                7.0,
                Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
            );
            ui.add_sized(
                Vec2::new(NAME_COL, SETTINGS_ROW_H),
                egui::Label::new(
                    egui::RichText::new(default_name)
                        .font(prop(SETTINGS_LABEL_FONT))
                        .color(theme::INK2),
                ),
            );
            let rest = ui.available_width();
            ui.add_sized(
                Vec2::new(rest.max(80.0), FIELD_H),
                egui::TextEdit::singleline(value)
                    .hint_text(default_name)
                    .font(prop(SETTINGS_LABEL_FONT)),
            )
            .changed()
        },
    )
    .inner
}

impl RawBlowApp {
    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // 돌아가기(버튼·Esc 공통)와 기본값 복원은 플래그로 모아 패널을 그린 뒤 한 곳에서 실행한다 —
        // 버튼과 Esc가 서로 다른 동작으로 어긋나지 않게(#69).
        let mut go_back = false;
        let mut do_reset = false;
        egui::TopBottomPanel::top("settings_top")
            .exact_height(52.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG2)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom() - 0.5,
                    Stroke::new(1.0_f32, theme::LINE2),
                );
                ui.horizontal_centered(|ui| {
                    if settings_action_btn(ui, &format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        go_back = true;
                    }
                    // #74: 헤더 전용 캡션 키로 전체 제목을 표시한다(영어 UI에서 "Settings"로 축약되던
                    // 문제 복원). 툴바 툴팁 등에서 재사용하는 "설정"(Settings) 키와 별개.
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(tr(lang, "설정 — 키보드 · 일반"))
                            .font(prop(16.0))
                            .color(theme::INK),
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    // 스크롤 뷰포트는 패널 전체(바는 창 오른쪽). 폼은 데스크톱 폭의 왼쪽 칼럼.
                    ui.set_min_width(ui.available_width());
                    let col_w = ui.available_width().min(SETTINGS_COL_W);
                    ui.allocate_ui_with_layout(
                        Vec2::new(col_w, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(col_w);
                    settings_identity(ui);
                    // 모든 설정 컨트롤은 변경 즉시 저장(#69). persist_cfg는 작은 원자적 JSON 쓰기라
                    // 토글/드래그/텍스트 입력마다 저장해도 부담이 적다(DragValue·TextEdit의 .changed()는
                    // 드래그 틱·키 입력마다 발생). '돌아가기' 저장은 최종 catch-all로 남긴다.
                    settings_section_panel(ui, tr(lang, "일반"), |ui| {
                        if settings_toggle_row(
                            ui,
                            SIcon::Skip,
                            tr(lang, "자동 전진"),
                            &mut self.cfg.auto_advance,
                            Some(tr(lang, "라벨링 후 자동 전진")),
                        ) {
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        if settings_toggle_row(
                            ui,
                            SIcon::Folder,
                            tr(lang, "하위 폴더"),
                            &mut self.cfg.recursive,
                            Some(tr(lang, "하위 폴더 포함 스캔")),
                        ) {
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        if settings_toggle_row(
                            ui,
                            SIcon::Exif,
                            "EXIF",
                            &mut self.cfg.show_exif,
                            Some(tr(lang, "EXIF 오버레이 기본 표시")),
                        ) {
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        if settings_toggle_row(
                            ui,
                            SIcon::Hist,
                            tr(lang, "히스토그램"),
                            &mut self.cfg.show_histogram,
                            Some(tr(lang, "히스토그램 기본 표시")),
                        ) {
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        if settings_toggle_row(
                            ui,
                            SIcon::Update,
                            tr(lang, "업데이트"),
                            &mut self.cfg.check_updates,
                            Some(tr(lang, "새 버전 자동 확인")),
                        ) {
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        settings_field_row(ui, SIcon::Preload, tr(lang, "프리로드"), |ui| {
                            if ui.add(egui::DragValue::new(&mut self.cfg.preload).range(0..=10)).changed() {
                                self.persist_cfg();
                            }
                        });
                        settings_inner_rule(ui);
                        settings_field_row(ui, SIcon::Grid, tr(lang, "그리드"), |ui| {
                            if ui.add(egui::DragValue::new(&mut self.cfg.grid_cols).range(4..=12)).changed() {
                                self.persist_cfg();
                            }
                        });
                        // 스트립·그리드 표기 크기(#44): 셀 위 선택 표시·별점·색상 태그를 크게(기본)/작게.
                        if let Some(i) = settings_choice_block(
                            ui,
                            SIcon::Badge,
                            tr(lang, "배지"),
                            &[(tr(lang, "크게"), ""), (tr(lang, "작게"), "")],
                            if self.cfg.large_badges { 0 } else { 1 },
                        ) {
                            self.cfg.large_badges = i == 0;
                            self.persist_cfg();
                        }
                        // 정렬 기준(#56): 촬영시간순(기본)/파일명순. 변경 즉시 재정렬·저장.
                        if let Some(i) = settings_choice_block(
                            ui,
                            SIcon::Sort,
                            tr(lang, "정렬"),
                            &[(tr(lang, "파일명순"), ""), (tr(lang, "촬영시간순"), "")],
                            if self.cfg.sort == SortOrder::Name { 0 } else { 1 },
                        ) {
                            self.set_sort_order(if i == 0 {
                                SortOrder::Name
                            } else {
                                SortOrder::CaptureTime
                            });
                        }
                        // 사진 이동 시 원본 보기(ORIG) 유지 방식(#87).
                        if let Some(i) = settings_choice_block(
                            ui,
                            SIcon::Orig,
                            tr(lang, "ORIG"),
                            &[
                                (tr(lang, "확대 시"), ""),
                                (tr(lang, "유지"), ""),
                            ],
                            if self.cfg.view_carry == ViewCarry::Keep { 1 } else { 0 },
                        ) {
                            self.cfg.view_carry = if i == 0 {
                                ViewCarry::ZoomOnly
                            } else {
                                ViewCarry::Keep
                            };
                            self.persist_cfg();
                        }
                        settings_inner_rule(ui);
                        // 전송 dest 기본값(#113).
                        {
                            let sel = if self.cfg.transfer_dest_mode == TransferDestMode::Fixed {
                                1
                            } else {
                                0
                            };
                            if let Some(i) = settings_choice_block(
                                ui,
                                SIcon::Send,
                                tr(lang, "전송 폴더"),
                                &[(tr(lang, "원래 폴더 아래"), ""), (tr(lang, "지정된 폴더"), "")],
                                sel,
                            ) {
                                if i == 0 {
                                    self.cfg.transfer_dest_mode = TransferDestMode::CurrentFolder;
                                } else {
                                    self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                                    if self.cfg.transfer_dest_folder.trim().is_empty() {
                                        self.cfg.transfer_dest_folder =
                                            nfc_hangul(&config::pictures_dir().to_string_lossy());
                                    }
                                }
                                self.persist_cfg();
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.set_min_height(SETTINGS_BTN_H);
                            let rest = (ui.available_width() - 120.0).max(120.0);
                            let pictures = nfc_hangul(&config::pictures_dir().to_string_lossy());
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.cfg.transfer_dest_folder)
                                        .font(mono(13.0))
                                        .desired_width(rest)
                                        .hint_text(pictures)
                                        .min_size(Vec2::new(0.0, 32.0)),
                                )
                                .changed()
                            {
                                if !self.cfg.transfer_dest_folder.trim().is_empty() {
                                    self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                                }
                                self.persist_cfg();
                            }
                            if settings_action_btn(ui, tr(lang, "찾아보기…")).clicked() {
                                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                    self.cfg.transfer_dest_folder = nfc_path_label(&d);
                                    self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                                    self.persist_cfg();
                                }
                            }
                        });
                        settings_inner_rule(ui);
                        // 언어 선택(#30): 시스템(자동)/한국어/English/日本語. 변경 즉시 적용·저장.
                        {
                            let sys = tr(lang, "시스템 (자동)");
                            let opts = [
                                (sys, ""),
                                (Lang::Ko.native_name(), ""),
                                (Lang::En.native_name(), ""),
                                (Lang::Ja.native_name(), ""),
                            ];
                            let sel = match self.cfg.lang {
                                None => 0,
                                Some(Lang::Ko) => 1,
                                Some(Lang::En) => 2,
                                Some(Lang::Ja) => 3,
                            };
                            if let Some(i) = settings_choice_block(ui, SIcon::Lang, tr(lang, "언어"), &opts, sel) {
                                let new_lang = match i {
                                    1 => Some(Lang::Ko),
                                    2 => Some(Lang::En),
                                    3 => Some(Lang::Ja),
                                    _ => None,
                                };
                                if new_lang != self.cfg.lang {
                                    self.cfg.lang = new_lang;
                                    self.lang = crate::i18n::effective_lang(&self.cfg);
                                    // 폰트도 새 언어의 폰트를 primary로 교체(#32 후속: 세로 어긋남 방지).
                                    crate::fonts::install(ui.ctx(), self.lang);
                                    self.persist_cfg();
                                }
                            }
                        }
                    });

                    // ── PHOTO BACKGROUND (#36): 사진 표시 화면 배경색 — 프리셋 + HEX/RGB ──
                    settings_section_panel(ui, tr(lang, "사진 배경"), |ui| {
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
                        const BG_CELL: Vec2 = Vec2::new(84.0, 58.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(8.0, 10.0);
                            for (label, val) in presets {
                                let selected = self.cfg.photo_bg == val;
                                let swatch_rgb = val.unwrap_or(theme::BG0_RGB);
                                let cell = ui.allocate_ui_with_layout(
                                    BG_CELL,
                                    Layout::top_down(Align::Center),
                                    |ui| {
                                        ui.set_width(BG_CELL.x);
                                        ui.spacing_mut().item_spacing.y = 6.0;
                                        let clicked = bg_swatch(ui, swatch_rgb, selected);
                                        ui.label(
                                            egui::RichText::new(label)
                                                .font(prop(12.0))
                                                .color(if selected { theme::INK2 } else { INK_HELP }),
                                        );
                                        clicked
                                    },
                                );
                                if cell.inner {
                                    self.cfg.photo_bg = val;
                                    self.bg_hex = hex_str(self.photo_bg_rgb());
                                    self.persist_cfg();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.set_min_height(SETTINGS_ROW_H);
                            ui.spacing_mut().item_spacing.x = 8.0;
                            // 현재 색 미리보기.
                            let cur = self.photo_bg_rgb();
                            let (sr, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                            ui.painter().rect(
                                sr,
                                Rounding::same(4.0),
                                Color32::from_rgb(cur[0], cur[1], cur[2]),
                                Stroke::new(1.0_f32, theme::LINE3),
                            );
                            ui.label(egui::RichText::new("HEX").font(prop(14.0)).color(INK_HEAD));
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.bg_hex)
                                    .font(mono(14.0))
                                    .desired_width(96.0)
                                    .hint_text("#101010")
                                    .min_size(Vec2::new(0.0, 32.0)),
                            );
                            if resp.changed() {
                                if let Some(rgb) = parse_hex_rgb(&self.bg_hex) {
                                    self.cfg.photo_bg = Some(rgb);
                                    self.persist_cfg();
                                }
                            }
                            // HEX 무효 입력 빨간 테두리(#69): 버퍼가 비지 않았는데 파싱 실패면 필드에 REJECT 테두리.
                            if !self.bg_hex.is_empty() && parse_hex_rgb(&self.bg_hex).is_none() {
                                ui.painter().rect_stroke(
                                    resp.rect,
                                    Rounding::same(2.0),
                                    Stroke::new(1.0_f32, theme::REJECT),
                                );
                            }
                            ui.add_space(8.0);
                            let mut rgb = self.photo_bg_rgb();
                            let mut changed = false;
                            ui.label(egui::RichText::new("R").font(prop(14.0)).color(INK_HEAD));
                            changed |= ui.add(egui::DragValue::new(&mut rgb[0]).range(0..=255)).changed();
                            ui.label(egui::RichText::new("G").font(prop(14.0)).color(INK_HEAD));
                            changed |= ui.add(egui::DragValue::new(&mut rgb[1]).range(0..=255)).changed();
                            ui.label(egui::RichText::new("B").font(prop(14.0)).color(INK_HEAD));
                            changed |= ui.add(egui::DragValue::new(&mut rgb[2]).range(0..=255)).changed();
                            if changed {
                                self.cfg.photo_bg = Some(rgb);
                                self.bg_hex = hex_str(rgb);
                                self.persist_cfg();
                            }
                        });
                    });

                    settings_section_panel(ui, tr(lang, "라벨"), |ui| {
                        let km = &self.cfg.keymap;
                        let keys = [
                            (Label::Pick, &km.pick),
                            (Label::Hold, &km.hold),
                            (Label::Reject, &km.reject),
                            (Label::Unrated, &km.clear),
                        ];
                        for (i, (lbl, key)) in keys.into_iter().enumerate() {
                            if i > 0 {
                                settings_inner_rule(ui);
                            }
                            settings_field_row(ui, SIcon::Key, lbl.name(lang), |ui| {
                                kbd(ui, key);
                            });
                        }
                    });

                    // ── COLOR TAGS (#27): 색별 커스텀 이름. 비우면 기본 색 이름 표시 ──
                    settings_section_panel(ui, tr(lang, "색 태그 이름"), |ui| {
                        for (i, tag) in ColorTag::ALL.iter().enumerate() {
                            if i > 0 {
                                settings_inner_rule(ui);
                            }
                            if settings_tag_name_row(
                                ui,
                                tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]),
                                tag.default_name(lang),
                                &mut self.cfg.tag_names[i],
                            ) {
                                self.persist_cfg(); // 태그 이름은 키 입력마다 즉시 저장(#69).
                            }
                        }
                    });

                    // ── CACHE (#22): 썸네일 디스크 캐시 사용량 + 비우기 ──
                    settings_section_panel(ui, tr(lang, "캐시"), |ui| {
                        if self.cache_size.is_none() {
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                        }
                        let size = self.cache_size.unwrap_or(0);
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(trf(lang, "썸네일 캐시 사용량 · {}", &[&fmt_bytes(size)]))
                                .font(prop(15.0))
                                .color(theme::INK2),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if settings_action_btn(ui, tr(lang, "캐시 비우기")).clicked() {
                                let _ = cache::clear(&config::cache_dir());
                                self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                                self.toast_info(tr(lang, "썸네일 캐시를 비웠습니다").into());
                            }
                            if settings_action_btn(ui, tr(lang, "새로고침")).clicked() {
                                self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                            }
                        });
                        settings_field_row(ui, SIcon::Cache, tr(lang, "자동 상한"), |ui| {
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.cfg.cache_limit_mb)
                                        .speed(64.0)
                                        .range(0..=1_048_576)
                                        .suffix(" MB"),
                                )
                                .changed()
                            {
                                self.persist_cfg(); // 캐시 상한 변경 즉시 저장(#69).
                            }
                            ui.label(
                                egui::RichText::new(tr(lang, "(0 = 무제한)"))
                                    .font(prop(14.0))
                                    .color(INK_HELP),
                            );
                        });
                        // 캐시 경로: 클릭하면 OS 파일 관리자에서 캐시 폴더를 연다(#69). hover 시 밝게 + 손가락 커서.
                        let cache_path = config::cache_dir();
                        let cache_path_str = cache_path.to_string_lossy().to_string();
                        let cache_font = mono(9.5);
                        let galley =
                            ui.painter()
                                .layout_no_wrap(cache_path_str.clone(), cache_font.clone(), INK_HELP);
                        let (cp_rect, cp_resp) = ui.allocate_exact_size(galley.size(), Sense::click());
                        let cp_col = if cp_resp.hovered() { theme::INK2 } else { INK_HELP };
                        ui.painter().text(
                            cp_rect.left_top(),
                            Align2::LEFT_TOP,
                            &cache_path_str,
                            cache_font,
                            cp_col,
                        );
                        if cp_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if cp_resp.clicked() {
                            reveal_in_file_manager(&cache_path);
                        }
                    });

                    // ── RESET (#69): 모든 설정을 기본값으로 — 2단 인라인 확인(모달 없이) ──
                    settings_section_panel(ui, tr(lang, "초기화"), |ui| {
                        if !self.settings_reset_armed {
                            settings_field_row(ui, SIcon::Reset, tr(lang, "기본값"), |ui| {
                                if settings_action_btn(ui, tr(lang, "복원")).clicked() {
                                    self.settings_reset_armed = true;
                                }
                            });
                        } else {
                            // '복원'은 경고색(WARN) 테두리. 실행은 패널을 그린 뒤 do_reset에서.
                            settings_field_row(ui, SIcon::Reset, tr(lang, "기본값"), |ui| {
                                // right_to_left: 먼저 넣은 복원이 오른쪽, 취소가 그 왼쪽.
                                if settings_action_btn_sized(
                                    ui,
                                    tr(lang, "복원"),
                                    theme::WARN,
                                    SETTINGS_ACTION_FILL,
                                    theme::WARN,
                                    Vec2::new(128.0, SETTINGS_BTN_H),
                                )
                                .clicked()
                                {
                                    do_reset = true;
                                }
                                if settings_action_btn(ui, tr(lang, "취소")).clicked() {
                                    self.settings_reset_armed = false;
                                }
                            });
                        }
                    });

                    // ── LINKS (#18/#39): 버전·제작자는 맨 위 아이덴티티. 여기는 링크만 행으로.
                    settings_section_panel(ui, tr(lang, "정보"), |ui| {
                        settings_link_row(
                            ui,
                            SIcon::Release,
                            tr(lang, "릴리스"),
                            "https://github.com/ascoeur9/rawblow/releases",
                        );
                        settings_inner_rule(ui);
                        settings_link_row(
                            ui,
                            SIcon::Issue,
                            tr(lang, "이슈"),
                            "https://github.com/ascoeur9/rawblow/issues",
                        );
                        settings_inner_rule(ui);
                        if settings_nav_row(ui, SIcon::License, tr(lang, "라이선스")) {
                            self.licenses = Some(crate::licenses::LicensesPage::new());
                        }
                        settings_inner_rule(ui);
                        settings_link_row(
                            ui,
                            SIcon::Heart,
                            tr(lang, "후원"),
                            "https://toon.at/donate/hare",
                        );
                        settings_inner_rule(ui);
                        settings_link_row(ui, SIcon::Cosly, "cosly", "https://cosly.link");
                    });
                    }); // column
                });
            });
        // Esc = 돌아가기(#69). 라이센스 페이지가 떠 있으면 그 페이지가 Esc를 처리한다(실제로 licenses가
        // Some이면 ui_settings가 호출되지 않지만, 방어적으로 가드). has_modal이 전역 키를 막으므로 여기가
        // 설정 화면에서 Esc의 유일한 소비처다.
        if self.licenses.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            go_back = true;
        }
        // 기본값 복원 실행(#69): 사용자 데이터(폴더 히스토리·다이얼로그 마지막 사용 옵션)는 보존하고
        // 나머지 설정만 Config::default()로. 보존 필드는 self.cfg를 덮어쓰기 前에 clone out한다.
        if do_reset {
            let last_folder = self.cfg.last_folder.clone();
            let recent_folders = self.cfg.recent_folders.clone();
            let transfer_defaults = self.cfg.transfer_defaults.clone();
            let organize_defaults = self.cfg.organize_defaults.clone();
            let prev_lang = self.lang;
            self.cfg = Config::default();
            self.cfg.last_folder = last_folder;
            self.cfg.recent_folders = recent_folders;
            self.cfg.transfer_defaults = transfer_defaults;
            self.cfg.organize_defaults = organize_defaults;
            // 라이브 미러 재동기화(설정 컨트롤이 하던 것과 동일하게).
            self.show_exif = self.cfg.show_exif;
            self.show_hist = self.cfg.show_histogram;
            self.show_map = self.cfg.show_map;
            self.show_af = self.cfg.show_af;
            self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
            self.lang = crate::i18n::effective_lang(&self.cfg);
            if self.lang != prev_lang {
                // 언어가 바뀌면 폰트 primary도 교체(#32 후속: 세로 어긋남 방지).
                crate::fonts::install(ctx, self.lang);
            }
            self.bg_hex = hex_str(self.photo_bg_rgb());
            // 정렬도 기본값(촬영시간순)으로 즉시 반영: 다른 미러와 달리 정렬은 self.sort 재설정 +
            // 재정렬이 필요하다. set_sort_order로 self.sort·cfg.sort·화면 순서를 함께 맞춘다
            // (안 하면 설정 UI는 기본값을, 화면은 이전 정렬을 보여 다음 폴더 열기 전까지 어긋남).
            let target_sort = self.cfg.sort;
            self.set_sort_order(target_sort);
            self.settings_reset_armed = false;
            self.persist_cfg();
            self.schedule_cache_trim(); // 기본 상한으로 캐시 정리.
            self.toast_info(tr(self.lang, "설정을 기본값으로 되돌렸습니다").into());
        }
        if go_back {
            self.settings_back();
        }
        self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
        self.show_exif = self.cfg.show_exif;
        self.show_hist = self.cfg.show_histogram;
    }

    /// 설정 화면 닫기(#69): '돌아가기' 버튼과 Esc가 공유하는 단일 동작 — 저장 + 변경된 상한으로
    /// 캐시 정리 후 닫는다. 한 곳에 모아 버튼과 Esc 동작이 어긋나지 않게 한다.
    fn settings_back(&mut self) {
        self.show_settings = false;
        self.persist_cfg();
        self.schedule_cache_trim(); // 변경된 상한으로 캐시 정리.
    }

    /// 오픈소스 라이센스 페이지(#39): 좌측 구성요소 목록(검색 가능) + 우측 라이센스 전문.
    /// 설정의 ABOUT에서 열며, 돌아가기/Esc로 설정 화면에 복귀한다.
    pub(super) fn ui_licenses(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let (total, generated) = self
            .licenses
            .as_ref()
            .map(|p| (p.doc.crates.len(), p.doc.generated.clone()))
            .unwrap_or((0, String::new()));
        egui::TopBottomPanel::top("licenses_top")
            .exact_height(52.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG2)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom() - 0.5,
                    Stroke::new(1.0_f32, theme::LINE2),
                );
                ui.horizontal_centered(|ui| {
                    if settings_action_btn(ui, &format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        close = true;
                    }
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "오픈소스 라이센스")).font(prop(16.0)).color(theme::INK));
                    ui.label(egui::RichText::new(trf(lang, "{} 구성요소", &[&total.to_string()])).font(mono(10.5)).color(INK_HEAD));
                    if !generated.is_empty() {
                        ui.label(egui::RichText::new(format!("· {}", generated)).font(mono(10.5)).color(INK_HELP));
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
                        .color(INK_HELP),
                );
                ui.add_space(12.0);
                if let Some(c) = page.doc.crates.get(page.selected) {
                    ui.label(egui::RichText::new(format!("{} v{}", c.n, c.v)).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(c.l.as_str()).font(mono(11.0)).color(INK_HEAD));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    /// 도형 트리를 훑어 사각형 fill+stroke를 모은다(`Shape::Vec` 중첩 대응). theme.rs와 같은 방식.
    fn rect_chrome(shapes: &[egui::Shape], out: &mut Vec<(Color32, Stroke)>) {
        for s in shapes {
            match s {
                egui::Shape::Rect(r) => out.push((r.fill, r.stroke)),
                egui::Shape::Vec(v) => rect_chrome(v, out),
                _ => {}
            }
        }
    }

    fn painted_chrome(shapes: &[egui::epaint::ClippedShape]) -> Vec<(Color32, Stroke)> {
        let mut out = Vec::new();
        rect_chrome(
            &shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
            &mut out,
        );
        out
    }

    fn opaque_fill_or_stroke(fill: Color32, stroke: Stroke) -> bool {
        fill.a() != 0 || (stroke.width > 0.0 && stroke.color.a() != 0)
    }

    fn is_action_button_chrome(fill: Color32, stroke: Stroke) -> bool {
        fill == SETTINGS_ACTION_FILL
            && stroke.color == SETTINGS_ACTION_STROKE
            && stroke.width > 0.0
            && fill.a() != 0
            && stroke.color.a() != 0
    }

    fn run_settings_chrome(mut paint: impl FnMut(&mut egui::Ui)) -> Vec<(Color32, Stroke)> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::BG1))
                .show(ctx, |ui| paint(ui));
        });
        painted_chrome(&out.shapes)
    }

    /// 설정 크롬 헬퍼(섹션 패널·액션 버튼·세그먼트)를 실제 함수로 그리고,
    /// (a) 섹션 바디 필이 셸 BG1과 다르고, (b) 비활성 액션 버튼이 불투명 fill 또는 stroke를 가지며,
    /// (c) 같은 패스의 일반 라벨은 그 버튼 크롬을 쓰지 않는지 본다.
    #[test]
    fn settings_chrome_helpers_paint_visible_panels_and_buttons() {
        assert_ne!(
            SETTINGS_SECTION_FILL, theme::BG1,
            "섹션 필 상수가 셸 BG1이면 패널이 구분되지 않는다"
        );

        let chrome = run_settings_chrome(|ui| {
            settings_section_panel(ui, "일반", |ui| {
                ui.label("help text that must not look like a button");
                let _ = settings_action_btn(ui, "캐시 비우기");
                let _ = segmented(ui, &[("파일명순", ""), ("촬영시간순", "")], 0);
            });
        });
        assert!(!chrome.is_empty(), "그려진 사각형이 없다 — 테스트 전제 실패");

        // (a) 섹션 바디에 셸 BG1이 아닌 필이 있다.
        assert!(
            chrome.iter().any(|(fill, _)| *fill == SETTINGS_SECTION_FILL && *fill != theme::BG1),
            "섹션 패널 필({SETTINGS_SECTION_FILL:?})이 셸 BG1과 구분되어 칠해져야 한다 (chrome={chrome:?})"
        );

        // (b) 비활성 액션 버튼: 완전 투명 fill+stroke가 아니다.
        let btn: Vec<_> = chrome
            .iter()
            .copied()
            .filter(|(fill, stroke)| is_action_button_chrome(*fill, *stroke))
            .collect();
        assert!(
            !btn.is_empty(),
            "비활성 액션 버튼 크롬(fill={SETTINGS_ACTION_FILL:?}, stroke={SETTINGS_ACTION_STROKE:?})이 없다 (chrome={chrome:?})"
        );
        assert!(
            btn.iter().any(|(fill, stroke)| opaque_fill_or_stroke(*fill, *stroke)),
            "비활성 액션 버튼이 완전 투명 fill+stroke로 칠해졌다 (btn={btn:?})"
        );

        // (c) 같은 런의 일반 라벨은 버튼 크롬을 만들지 않는다 — 버튼 1개분만 그 조합.
        assert_eq!(
            btn.len(),
            1,
            "라벨·세그먼트·섹션이 액션 버튼 크롬을 복제하면 안 된다 (btn={btn:?}, chrome={chrome:?})"
        );
    }

    /// 액션 버튼만 그렸을 때도 비활성 크롬이 보이고, 라벨만 그렸을 때는 그 크롬이 없다.
    #[test]
    fn settings_action_button_inactive_chrome_unlike_label() {
        let with_btn = run_settings_chrome(|ui| {
            let _ = settings_action_btn(ui, "새로고침");
        });
        assert!(
            with_btn
                .iter()
                .any(|(fill, stroke)| is_action_button_chrome(*fill, *stroke)
                    && opaque_fill_or_stroke(*fill, *stroke)),
            "단독 액션 버튼이 불투명 크롬을 칠해야 한다 (chrome={with_btn:?})"
        );

        let label_only = run_settings_chrome(|ui| {
            ui.label("정적 설명 — 버튼이 아님");
        });
        assert!(
            label_only
                .iter()
                .all(|(fill, stroke)| !is_action_button_chrome(*fill, *stroke)),
            "라벨만 있는 패스에 액션 버튼 크롬이 생기면 안 된다 (chrome={label_only:?})"
        );
    }

    fn collect_text(shapes: &[egui::Shape], out: &mut Vec<(String, egui::Rect)>) {
        for s in shapes {
            match s {
                egui::Shape::Text(t) => {
                    out.push((t.galley.text().to_string(), egui::Rect::from_min_size(t.pos, t.galley.size())));
                }
                egui::Shape::Vec(v) => collect_text(v, out),
                _ => {}
            }
        }
    }

    fn texts_from(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        let mut v = Vec::new();
        collect_text(
            &out.shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
            &mut v,
        );
        v
    }

    /// 실제 `ui_settings`에 포인터/키를 넣는 하니스. 세그먼트 인덱스를 테스트에 복제하지 않는다.
    struct SettingsQa {
        ctx: egui::Context,
        app: RawBlowApp,
        time: f64,
    }

    impl SettingsQa {
        fn new() -> Self {
            RawBlowApp::settings_qa_begin();
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            crate::fonts::install(&ctx, Lang::Ko);
            Self {
                ctx,
                app: RawBlowApp::for_settings_qa(),
                time: 0.0,
            }
        }

        fn screen() -> egui::Rect {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 4200.0))
        }

        fn raw(&self, events: Vec<egui::Event>) -> egui::RawInput {
            egui::RawInput {
                screen_rect: Some(Self::screen()),
                time: Some(self.time),
                events,
                max_texture_side: Some(4096),
                ..Default::default()
            }
        }

        fn paint(&mut self) -> Vec<(String, egui::Rect)> {
            self.time += 1.0 / 60.0;
            let input = self.raw(Vec::new());
            let out = self.ctx.run(input, |ctx| self.app.ui_settings(ctx));
            texts_from(&out)
        }

        fn click_pos(&mut self, pos: egui::Pos2) {
            let click = |pressed: bool| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            self.time += 1.0 / 60.0;
            let _ = self.ctx.run(self.raw(vec![egui::Event::PointerMoved(pos)]), |ctx| {
                self.app.ui_settings(ctx);
            });
            self.time += 1.0 / 60.0;
            let _ = self.ctx.run(
                self.raw(vec![egui::Event::PointerMoved(pos), click(true)]),
                |ctx| self.app.ui_settings(ctx),
            );
            self.time += 1.0 / 60.0;
            let _ = self.ctx.run(
                self.raw(vec![egui::Event::PointerMoved(pos), click(false)]),
                |ctx| self.app.ui_settings(ctx),
            );
        }

        fn click_exact(&mut self, label: &str) {
            let texts = self.paint();
            let rect = texts
                .iter()
                .find(|(t, _)| t == label)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| {
                    let shown: Vec<&str> = texts.iter().map(|(t, _)| t.as_str()).collect();
                    panic!("설정 화면에 {label:?} 텍스트가 없다. painted={shown:?}");
                });
            self.click_pos(rect.center());
        }

        fn press_esc(&mut self) {
            self.time += 1.0 / 60.0;
            let _ = self.ctx.run(
                self.raw(vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }]),
                |ctx| self.app.ui_settings(ctx),
            );
        }
    }

    /// 보이는 라벨을 클릭하면 그 라벨이 뜻하는 설정값이 저장된다(인덱스 테이블 비복제).
    #[test]
    fn settings_qa_exclusive_clicks_write_matching_fields() {
        let mut qa = SettingsQa::new();
        assert_eq!(qa.app.cfg.sort, SortOrder::CaptureTime);
        assert!(qa.app.cfg.large_badges);
        assert_eq!(qa.app.cfg.view_carry, ViewCarry::ZoomOnly);
        assert_eq!(qa.app.cfg.transfer_dest_mode, TransferDestMode::CurrentFolder);
        assert_eq!(qa.app.cfg.lang, None);

        qa.click_exact("파일명순");
        assert_eq!(qa.app.cfg.sort, SortOrder::Name, "파일명순 클릭이 Name이어야 한다");
        assert_eq!(qa.app.sort, SortOrder::Name);
        qa.click_exact("촬영시간순");
        assert_eq!(qa.app.cfg.sort, SortOrder::CaptureTime, "촬영시간순 클릭이 CaptureTime이어야 한다");

        qa.click_exact("작게");
        assert!(!qa.app.cfg.large_badges, "작게 클릭이 large_badges=false");
        qa.click_exact("크게");
        assert!(qa.app.cfg.large_badges, "크게 클릭이 large_badges=true");

        qa.click_exact("유지");
        assert_eq!(qa.app.cfg.view_carry, ViewCarry::Keep);
        qa.click_exact("확대 시");
        assert_eq!(qa.app.cfg.view_carry, ViewCarry::ZoomOnly);

        qa.click_exact("지정된 폴더");
        assert_eq!(qa.app.cfg.transfer_dest_mode, TransferDestMode::Fixed);
        assert!(
            !qa.app.cfg.transfer_dest_folder.trim().is_empty(),
            "지정된 폴더 클릭 시 빈 경로는 사진 폴더로 채운다"
        );
        qa.click_exact("원래 폴더 아래");
        assert_eq!(qa.app.cfg.transfer_dest_mode, TransferDestMode::CurrentFolder);

        // 시스템 (자동)은 tr()이라 UI 언어가 바뀌면 라벨도 바뀐다. 한국어 UI에서 먼저 왕복한다.
        qa.click_exact("한국어");
        assert_eq!(qa.app.cfg.lang, Some(Lang::Ko));
        qa.click_exact("시스템 (자동)");
        assert_eq!(qa.app.cfg.lang, None);
        qa.click_exact("한국어");
        assert_eq!(qa.app.cfg.lang, Some(Lang::Ko));
        qa.click_exact("English");
        assert_eq!(qa.app.cfg.lang, Some(Lang::En));
        qa.click_exact("日本語");
        assert_eq!(qa.app.cfg.lang, Some(Lang::Ja));

        assert!(
            RawBlowApp::settings_qa_persist_count() > 0,
            "배타 선택 변경은 즉시 persist_cfg를 호출해야 한다"
        );
    }

    /// 체크박스·사진 배경 프리셋이 같은 필드를 즉시 갱신한다.
    #[test]
    fn settings_qa_persistable_clicks_update_fields() {
        let mut qa = SettingsQa::new();
        assert!(qa.app.cfg.auto_advance);
        assert!(!qa.app.cfg.recursive);
        assert!(qa.app.cfg.show_exif);
        assert!(qa.app.cfg.show_histogram);
        assert!(qa.app.cfg.check_updates);
        assert_eq!(qa.app.cfg.photo_bg, None);

        qa.click_exact("자동 전진");
        assert!(!qa.app.cfg.auto_advance);
        qa.click_exact("하위 폴더");
        assert!(qa.app.cfg.recursive);
        qa.click_exact("EXIF");
        assert!(!qa.app.cfg.show_exif);
        assert!(!qa.app.show_exif, "ui_settings 끝에서 show_exif 미러가 따라가야 한다");
        qa.click_exact("히스토그램");
        assert!(!qa.app.cfg.show_histogram);
        assert!(!qa.app.show_hist);
        qa.click_exact("업데이트");
        assert!(!qa.app.cfg.check_updates);

        let texts = qa.paint();
        let _ = texts
            .iter()
            .find(|(t, _)| t == "중간 회색")
            .expect("중간 회색 프리셋");
        let mut swatches = Vec::new();
        fn collect_rects(shapes: &[egui::Shape], out: &mut Vec<(Color32, egui::Rect)>) {
            for s in shapes {
                match s {
                    egui::Shape::Rect(r) => out.push((r.fill, r.rect)),
                    egui::Shape::Vec(v) => collect_rects(v, out),
                    _ => {}
                }
            }
        }
        {
            qa.time += 1.0 / 60.0;
            let input = qa.raw(Vec::new());
            let out = qa.ctx.run(input, |ctx| qa.app.ui_settings(ctx));
            collect_rects(
                &out.shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
                &mut swatches,
            );
        }
        let mid = Color32::from_rgb(0x80, 0x80, 0x80);
        let swatch = swatches
            .iter()
            .find(|(f, r)| *f == mid && (r.width() - 26.0).abs() < 2.0)
            .map(|(_, r)| *r)
            .expect("중간 회색 색견본 rect");
        qa.click_pos(swatch.center());
        assert_eq!(qa.app.cfg.photo_bg, Some([0x80, 0x80, 0x80]));

        qa.click_exact("새로고침");
        assert!(qa.app.cache_size.is_some());

        qa.click_exact("라이선스");
        assert!(qa.app.licenses.is_some(), "라이센스 버튼이 페이지를 열어야 한다");
    }

    #[test]
    fn settings_qa_reset_is_two_step_and_preserves_history() {
        let mut qa = SettingsQa::new();
        qa.app.cfg.last_folder = Some("/qa/last".into());
        qa.app.cfg.recent_folders = vec!["/qa/a".into(), "/qa/b".into()];
        qa.app.cfg.transfer_defaults.scope_all = false;
        qa.app.cfg.organize_defaults.action = Action::Copy;
        qa.app.cfg.auto_advance = false;
        qa.app.cfg.large_badges = false;
        qa.app.cfg.sort = SortOrder::Name;
        qa.app.sort = SortOrder::Name;

        qa.click_exact("복원");
        assert!(qa.app.settings_reset_armed, "첫 클릭은 arm만");
        assert!(!qa.app.cfg.auto_advance, "arm 단계에서는 값을 바꾸지 않는다");

        qa.click_exact("취소");
        assert!(!qa.app.settings_reset_armed);
        assert!(!qa.app.cfg.auto_advance, "취소는 설정을 되돌리지 않는다");

        qa.click_exact("복원");
        assert!(qa.app.settings_reset_armed);
        qa.click_exact("복원");
        assert!(!qa.app.settings_reset_armed);
        assert!(qa.app.cfg.auto_advance, "복원 후 기본값");
        assert!(qa.app.cfg.large_badges);
        assert_eq!(qa.app.cfg.sort, SortOrder::CaptureTime);
        assert_eq!(qa.app.cfg.last_folder.as_deref(), Some("/qa/last"));
        assert_eq!(qa.app.cfg.recent_folders, vec!["/qa/a".to_string(), "/qa/b".to_string()]);
        assert!(!qa.app.cfg.transfer_defaults.scope_all);
        assert_eq!(qa.app.cfg.organize_defaults.action, Action::Copy);
    }

    #[test]
    fn settings_qa_back_and_esc_close_and_persist() {
        let mut qa = SettingsQa::new();
        qa.app.cfg.auto_advance = false;
        let before = RawBlowApp::settings_qa_persist_count();
        qa.click_exact("← 돌아가기");
        assert!(!qa.app.show_settings, "돌아가기가 설정을 닫는다");
        assert!(RawBlowApp::settings_qa_persist_count() > before);

        let mut qa = SettingsQa::new();
        qa.app.show_settings = true;
        qa.press_esc();
        assert!(!qa.app.show_settings, "Esc가 설정을 닫는다");
    }

    /// 리스타일 이후에도 모든 옵션/액션 문자열이 설정 경로에 남아 있는지.
    #[test]
    fn settings_qa_source_still_has_every_named_control() {
        let src = include_str!("settings_ui.rs");
        let src = src.split("#[cfg(test)]").next().unwrap_or(src);
        for needle in [
            "자동 전진",
            "하위 폴더",
            "EXIF",
            "히스토그램",
            "업데이트",
            "프리로드",
            "그리드",
            "배지",
            "크게",
            "작게",
            "정렬",
            "파일명순",
            "촬영시간순",
            "ORIG",
            "확대 시",
            "유지",
            "전송 폴더",
            "원래 폴더 아래",
            "지정된 폴더",
            "찾아보기…",
            "언어",
            "시스템 (자동)",
            "사진 배경",
            "색 태그 이름",
            "캐시 비우기",
            "새로고침",
            "기본값",
            "라이선스",
            "이슈",
            "릴리스",
            "후원",
            "draw_sicon",
            "돌아가기",
            "auto_advance",
            "recursive",
            "show_exif",
            "show_histogram",
            "check_updates",
            "preload",
            "grid_cols",
            "large_badges",
            "view_carry",
            "transfer_dest_mode",
            "tag_names",
            "photo_bg",
            "cache_limit_mb",
            "persist_cfg",
            "set_sort_order",
            "settings_back",
            "reveal_in_file_manager",
            "cache::clear",
            "rfd::FileDialog",
            "LicensesPage",
            "settings_segmented",
            "settings_toggle_row",
            "settings_tag_name_row",
            "settings_identity",
            "settings_link_row",
            "draw_mark",
        ] {
            assert!(src.contains(needle), "설정 경로에 {needle:?}가 없다");
        }
        assert!(
            !src.contains("ui.checkbox"),
            "설정 화면 불린은 토글이어야 한다 (checkbox 금지)"
        );
        assert!(
            src.contains("SETTINGS_COL_W"),
            "설정 폼은 데스크톱 고정 폭 칼럼이어야 한다"
        );
    }

    /// 디자인 QA: 카드는 왼쪽 고정 폭 칼럼, 라벨 x는 맞고, 창 전체로 늘어나지 않는다.
    #[test]
    fn settings_qa_layout_full_width_and_aligned_labels() {
        RawBlowApp::settings_qa_begin();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        crate::fonts::install(&ctx, Lang::Ko);
        let mut app = RawBlowApp::for_settings_qa();
        let mut input = egui::RawInput::default();
        let screen_w = 1440.0;
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen_w, 900.0),
        ));
        input.max_texture_side = Some(4096);
        let out = ctx.run(input, |ctx| app.ui_settings(ctx));

        fn rects(shapes: &[egui::Shape], out: &mut Vec<(Color32, egui::Rect)>) {
            for s in shapes {
                match s {
                    egui::Shape::Rect(r) => out.push((r.fill, r.rect)),
                    egui::Shape::Vec(v) => rects(v, out),
                    _ => {}
                }
            }
        }
        let mut painted = Vec::new();
        rects(
            &out.shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
            &mut painted,
        );
        let sections: Vec<egui::Rect> = painted
            .iter()
            .filter(|(f, r)| *f == SETTINGS_SECTION_FILL && r.width() > 200.0)
            .map(|(_, r)| *r)
            .collect();
        assert!(!sections.is_empty(), "섹션 카드가 그려지지 않았다");
        let left = sections.iter().map(|r| r.left()).fold(f32::MAX, f32::min);
        let width = sections.iter().map(|r| r.width()).fold(0.0_f32, f32::max);
        assert!(
            left < 80.0,
            "카드가 왼쪽 여백에 붙어야 한다(웹처럼 가운데 정렬이면 안 됨). left={left:.0}"
        );
        assert!(
            width <= SETTINGS_COL_W + 48.0,
            "카드가 창 전체로 늘어나면 모바일웹처럼 보인다. width={width:.0} col={SETTINGS_COL_W}"
        );
        assert!(
            width > 400.0,
            "카드가 너무 좁다 width={width:.0}"
        );

        let texts = texts_from(&out);
        let l1 = texts.iter().find(|(t, _)| t == "자동 전진").map(|(_, r)| r.min.x);
        let l2 = texts.iter().find(|(t, _)| t == "하위 폴더").map(|(_, r)| r.min.x);
        let l3 = texts.iter().find(|(t, _)| t == "프리로드").map(|(_, r)| r.min.x);
        match (l1, l2, l3) {
            (Some(a), Some(b), Some(c)) => {
                assert!((a - b).abs() < 1.5, "토글 라벨 x 불일치 {a} vs {b}");
                assert!((a - c).abs() < 1.5, "토글/필드 라벨 x 불일치 {a} vs {c}");
            }
            _ => panic!("일반 섹션 라벨이 화면에 없다: {texts:?}"),
        }
    }

    /// 색 태그 입력칸은 힌트 글자 길이와 무관하게 같은 왼쪽·오른쪽 가장자리를 공유한다.
    #[test]
    fn settings_qa_color_tag_fields_share_edges() {
        RawBlowApp::settings_qa_begin();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        crate::fonts::install(&ctx, Lang::Ko);
        let mut app = RawBlowApp::for_settings_qa();
        // 힌트 길이가 제각각이어도 필드 폭이 같아야 한다.
        app.cfg.tag_names = [
            String::new(),
            "a".into(),
            "긴사용자이름입니다".into(),
            String::new(),
            "x".into(),
        ];
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 4200.0),
            )),
            max_texture_side: Some(4096),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.ui_settings(ctx));
        let texts = texts_from(&out);
        let y0 = texts
            .iter()
            .find(|(t, _)| t == "색 태그 이름")
            .map(|(_, r)| r.bottom())
            .expect("색 태그 이름 헤더");
        let y1 = texts
            .iter()
            .find(|(t, _)| t == "캐시")
            .map(|(_, r)| r.top())
            .expect("캐시 헤더");

        fn rects(shapes: &[egui::Shape], out: &mut Vec<(Color32, egui::Rect)>) {
            for s in shapes {
                match s {
                    egui::Shape::Rect(r) => out.push((r.fill, r.rect)),
                    egui::Shape::Vec(v) => rects(v, out),
                    _ => {}
                }
            }
        }
        let mut painted = Vec::new();
        rects(
            &out.shapes.iter().map(|c| c.shape.clone()).collect::<Vec<_>>(),
            &mut painted,
        );
        let mut fields: Vec<egui::Rect> = painted
            .iter()
            .filter(|(f, r)| {
                // TextEdit는 extreme_bg_color(BG0)로 칠한다.
                *f == theme::BG0
                    && r.width() > 180.0
                    && (24.0..52.0).contains(&r.height())
                    && r.top() > y0
                    && r.bottom() < y1
            })
            .map(|(_, r)| *r)
            .collect();
        fields.sort_by(|a, b| a.top().partial_cmp(&b.top()).unwrap());
        assert!(
            fields.len() >= 5,
            "색 태그 입력 필드 5개가 필요 (got {} rects, y0={y0} y1={y1})",
            fields.len()
        );
        let fields = &fields[..5];
        let left0 = fields[0].left();
        let right0 = fields[0].right();
        for (i, r) in fields.iter().enumerate() {
            assert!(
                (r.left() - left0).abs() < 1.5,
                "색 태그 필드 {i} left {} vs {left0}",
                r.left()
            );
            assert!(
                (r.right() - right0).abs() < 1.5,
                "색 태그 필드 {i} right {} vs {right0}",
                r.right()
            );
        }
        let names: Vec<f32> = ["주황", "분홍", "청록", "파랑", "보라"]
            .iter()
            .filter_map(|n| texts.iter().find(|(t, _)| t == n).map(|(_, r)| r.min.x))
            .collect();
        assert_eq!(names.len(), 5, "기본 색 이름이 다 보여야 한다");
        let nx = names[0];
        for (i, x) in names.iter().enumerate() {
            assert!((x - nx).abs() < 1.5, "색 이름 라벨 {i} x {x} vs {nx}");
        }
    }

    /// 버그 제보 링크와 오픈소스 라이선스 행이 겹치지 않는다.
    #[test]
    fn settings_qa_bug_report_not_covered_by_licenses() {
        let mut qa = SettingsQa::new();
        let texts = qa.paint();
        let bug = texts
            .iter()
            .find(|(t, _)| t == "이슈")
            .map(|(_, r)| *r)
            .expect("이슈");
        let lic = texts
            .iter()
            .find(|(t, _)| t == "라이선스")
            .map(|(_, r)| *r)
            .expect("라이선스");
        assert!(
            !bug.intersects(lic),
            "버그 제보 {bug:?}가 라이선스 {lic:?}에 가려진다"
        );
        assert!(
            lic.top() >= bug.bottom() - 1.0,
            "라이선스 행이 버그 제보 아래에 있어야 한다 (bug.bottom={} lic.top={})",
            bug.bottom(),
            lic.top()
        );
        assert!(
            texts.iter().any(|(t, _)| t == "RawBlow"),
            "아이덴티티에 RawBlow가 있어야 한다"
        );
    }
}
