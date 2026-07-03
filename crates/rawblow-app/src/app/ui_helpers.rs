//! 앱 공용 UI 보조(분해 1/8): 소형 위젯(토글·칩·세그먼트·모달 프레임)·표시용 포맷터·
//! AF 좌표 변환·히스토그램. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

// ── 보조 위젯 ──────────────────────────────────────────────
pub(super) fn toggle_btn(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(label).font(prop(12.0)).color(if active { theme::INK } else { theme::INK2 }))
        .fill(if active { theme::BG3 } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0, if active { theme::LINE2 } else { Color32::TRANSPARENT }));
    ui.add(btn)
}

pub(super) fn vsep(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 18.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, theme::LINE);
    ui.add_space(4.0);
}

/// 사진 배경 색견본(클릭 가능, #36). 선택 시 accent 링을 두른다.
pub(super) fn bg_swatch(ui: &mut egui::Ui, rgb: [u8; 3], selected: bool) -> bool {
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


/// AF 정규화 좌표(센서 기준, 원점 좌상단)를 EXIF orientation이 적용된 표시
/// 좌표계로 변환한다(#37). 90/270도 회전은 폭/높이도 맞바꾼다.
pub(super) fn af_display_coords(pt: &rawblow_core::af::AfPoint, orient: u16) -> (f64, f64, f64, f64) {
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
pub(super) fn af_focus_center(af: &rawblow_core::af::AfInfo, orient: u16) -> Option<(f64, f64)> {
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
pub(super) fn parse_hex_rgb(s: &str) -> Option<[u8; 3]> {
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
pub(super) fn hex_str(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// 모달 다이얼로그용 공통 프레임(앱 디자인에 맞춘 패널: 어두운 배경 + 테두리 + 둥근 모서리 + 여백).
/// egui 기본 윈도우 크롬 대신 이걸 쓰고 제목줄은 끈다(title_bar(false)).
pub(super) fn modal_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::BG2)
        .stroke(Stroke::new(1.0, theme::LINE2))
        .rounding(12.0)
        .inner_margin(egui::Margin::same(22.0))
}

/// 모달 헤더(제목 + 부제 + 구분선).
pub(super) fn modal_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
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
pub(super) fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).font(prop(10.0)).color(theme::INK3));
    ui.add_space(5.0);
}

/// 전체 너비 1px 구분선.
pub(super) fn hline_full(ui: &mut egui::Ui) {
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
}

/// CheckChip: 라벨색 배경(체크 시 18%)+테두리 알약. 클릭되면 true.
pub(super) fn check_chip(ui: &mut egui::Ui, label: &str, count: Option<usize>, color: Color32, checked: bool) -> bool {
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
pub(super) fn segmented(ui: &mut egui::Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
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
pub(super) fn send_icon(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
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
pub(super) struct Histo {
    pub(super) bins: [[u32; 64]; 3],
    pub(super) max: u32,
}

/// 디코딩된 RGBA에서 히스토그램을 계산(대용량은 서브샘플로 비용 제한).
pub(super) fn compute_histo(rgba: &[u8]) -> Histo {
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

pub(super) fn exif_lines(ex: &ExifInfo) -> Vec<String> {
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
pub(super) fn format_capture_datetime(dt: &str) -> String {
    match dt.split_once(' ') {
        Some((date, time)) => format!("{} {}", date.replace(':', "."), time),
        None => dt.replace(':', "."),
    }
}

/// 플랫폼별 Command/Ctrl 표기(#32). macOS는 `⌘`, 그 외(Windows/Linux)는 `Ctrl+`.
/// Windows에서 `⌘` 글리프가 폴백 폰트로 그려지며 세로로 떠 보이는 문제 + 실제 키도 Ctrl이라
/// 텍스트로 대체한다(예: `{cmd}O` → mac "⌘O", win "Ctrl+O").
pub(super) fn cmd_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// 외부 URL을 기본 브라우저로 연다(#18). reveal_in_file_manager와 같은 무음 spawn 패턴.
pub(super) fn open_url(url: &str) {
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
pub(super) fn link_label(ui: &mut egui::Ui, text: &str, url: &str) {
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
pub(super) fn fmt_bytes(n: u64) -> String {
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
pub(super) fn reveal_in_file_manager(path: &std::path::Path) {
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
    use super::{format_capture_datetime, hex_str, parse_hex_rgb};

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
}
