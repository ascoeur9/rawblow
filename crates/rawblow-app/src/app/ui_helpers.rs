//! 앱 공용 UI 보조(분해 1/8): 소형 위젯(토글·칩·세그먼트·모달 프레임)·표시용 포맷터·
//! AF 좌표 변환·히스토그램. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

// ── 보조 위젯 ──────────────────────────────────────────────
pub(super) fn toggle_btn(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(label).font(prop(12.0)).color(if active { theme::INK } else { theme::INK2 }))
        .fill(if active { theme::BG3 } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0_f32, if active { theme::LINE2 } else { Color32::TRANSPARENT }));
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
    ui.painter().rect(rect, Rounding::same(5.0), col, Stroke::new(1.0_f32, theme::LINE3));
    if selected {
        ui.painter().rect_stroke(rect.expand(2.0), Rounding::same(7.0), Stroke::new(2.0_f32, theme::ACCENT));
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

// ── 화면상 배율 유지(#48) ──────────────────────────────────
/// 같은 사진에서 표시 해상도가 바뀔 때(ORIG 로드/언로드) 화면상 배율을 유지하기 위한 상태.
///
/// `zoom`은 화면픽셀/**텍스처**픽셀이라 해상도가 바뀌면 같은 값이 다른 배율을 뜻한다
/// (프리뷰 1920에서의 1.0과 ORIG 8192에서의 1.0은 4배 이상 차이). 그래서 해상도·방향에
/// 불변인 **긴 변이 차지하는 화면 px**(`mag`)을 기준으로 삼는다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ViewMag {
    /// 유지하려는 화면상 긴 변 px. 창맞춤이 아닌 상태에서만 의미가 있다.
    pub mag: Option<f32>,
    /// 상·하한에 걸려 `mag`를 재현하지 못한 프레임에서 실제로 쓴 `zoom`.
    /// 이 값에서 `zoom`이 움직이면 사용자가 직접 조작한 것으로 보고 붙들기를 해제한다.
    pub pin: Option<f32>,
}

/// 사용자에게 보여줄 배율(#48). 1.0 = 실측 1:1(원본 1픽셀 = 화면 1픽셀).
///
/// `zoom`은 화면px/**텍스처**px이라 같은 화면 크기라도 프리뷰(1920)와 ORIG(8192)에서 4배 넘게
/// 다른 값이 된다 — 그대로 보여주면 D를 눌러 사진 크기가 그대로인데도 숫자만 100%↔23%로 튄다.
/// 화면상 크기(`cur_long * zoom`)를 원본 긴 변으로 나눠 해상도에 불변인 값으로 만든다.
pub(super) fn display_zoom_ratio(zoom: f32, cur_long: f32, ref_long: f32) -> Option<f32> {
    (ref_long > 0.0).then(|| cur_long * zoom / ref_long)
}

/// 이번 프레임에 쓸 `zoom`을 돌려주고 `st`를 다음 프레임 기준으로 옮긴다(#48).
///
/// - 표시 해상도가 바뀐 프레임이면 `mag`를 재현한다 — 이때 경계에 잘려도 **`mag`는 덮지 않고**
///   붙들어 둔다. 잘린 값을 기준으로 되쓰면 배율이 영구히 깎이기 때문(#48 회귀의 원인).
/// - 해상도가 그대로인 프레임의 `zoom`은 사용자가 보고 있는 값이므로 그것을 새 기준으로 삼는다.
/// - `last_long`이 `None`이면 이 항목의 첫 프레임(비교 대상 없음).
pub(super) fn sync_view_mag(
    st: &mut ViewMag,
    zoom: f32,
    cur_long: f32,
    last_long: Option<f32>,
    min_zoom: f32,
    max_zoom: f32,
) -> f32 {
    let size_changed = last_long.is_some_and(|prev| (prev - cur_long).abs() > 0.5);
    let pinned = st.pin.is_some_and(|z| (zoom - z).abs() <= 1e-4);
    if !pinned {
        st.pin = None;
    }
    match st.mag {
        Some(mag) if size_changed => {
            let want = mag / cur_long.max(1.0);
            let z = want.clamp(min_zoom, max_zoom);
            st.pin = ((z - want).abs() > 1e-4).then_some(z);
            z
        }
        _ if !pinned => {
            st.mag = Some(cur_long * zoom);
            zoom
        }
        _ => zoom,
    }
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
        .stroke(Stroke::new(1.0_f32, theme::LINE2))
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
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0_f32, theme::LINE));
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
    ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0_f32, theme::LINE));
}

/// CheckChip: 라벨색 배경(체크 시 18%)+테두리 알약. 클릭되면 true.
pub(super) fn check_chip(ui: &mut egui::Ui, label: &str, count: Option<usize>, color: Color32, checked: bool) -> bool {
    check_chip_resp(ui, label, count, color, checked).clicked()
}

/// check_chip의 Response 반환판(#70): 호출부가 툴팁(on_hover_text)을 붙일 수 있게 분리.
/// 기존 check_chip 호출부는 그대로 두고, 툴팁이 필요한 곳만 이걸 쓴다.
pub(super) fn check_chip_resp(ui: &mut egui::Ui, label: &str, count: Option<usize>, color: Color32, checked: bool) -> egui::Response {
    let fill = if checked { color.linear_multiply(0.18) } else { theme::BG1 };
    let stroke = Stroke::new(1.0_f32, if checked { color.linear_multiply(0.6) } else { theme::LINE2 });
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
                    Stroke::new(1.5_f32, if checked { color } else { theme::LINE3 }),
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
                        Stroke::new(1.7_f32, dark),
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
    resp
}

/// Segmented: BG1 트랙 + 활성 칸 BG4. 각 칸은 (라벨, 서브라벨) 2줄. 클릭된 인덱스 반환.
pub(super) fn segmented(ui: &mut egui::Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::none()
        .fill(theme::BG1)
        .stroke(Stroke::new(1.0_f32, theme::LINE2))
        .rounding(6.0)
        .inner_margin(egui::Margin::same(3.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.horizontal(|ui| {
                for (i, (label, sub)) in options.iter().enumerate() {
                    let active = i == selected;
                    let cell = egui::Frame::none()
                        .fill(if active { theme::BG4 } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.0_f32, if active { theme::LINE3 } else { Color32::TRANSPARENT }))
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
    use super::{
        af_display_coords, af_focus_center, compute_histo, exif_lines, fmt_bytes,
        display_zoom_ratio, format_capture_datetime, hex_str, parse_hex_rgb, sync_view_mag,
        ExifInfo, ViewMag,
    };
    use rawblow_core::af::{AfInfo, AfPoint};

    // ── #48: 프리뷰↔ORIG 전환 시 화면상 배율 유지 ──────────────
    //
    // photo_view의 프레임 한 컷을 재현하는 하니스. GPU 없이 D 토글 시퀀스를 그대로 돌린다.
    // 실제 상수: PREVIEW_EDGE=1920, ORIG_EDGE=8192. 사진 영역은 photo_view와 같이 -32 인셋.
    struct Sim {
        zoom: f32,
        fit: bool,
        st: ViewMag,
        last_long: Option<f32>,
        /// `view_ref_long`: 이 항목에서 지금까지 본 텍스처 중 가장 긴 변.
        seen_long: f32,
        /// `Item::orig_long`: 원본 긴 변 실측(`decode::orig_long_edge`). 백그라운드 메타가
        /// 채우기 전에는 None — 그동안은 `seen_long`으로 폴백한다(app.rs::ref_long_px와 동일).
        orig: Option<f32>,
        area: (f32, f32),
    }

    impl Sim {
        fn new(area: (f32, f32)) -> Self {
            Sim {
                zoom: 1.0,
                fit: true,
                st: ViewMag::default(),
                last_long: None,
                seen_long: 0.0,
                orig: None,
                area,
            }
        }

        /// 원본 실측 크기가 이미 도착한 상태로 만든다(#48). 실제로는 EXIF와 같은 백그라운드
        /// 패스가 채운다. EXIF가 못 믿을 바디(NEF: IFD0이 160x120 썸네일)에서도 이 값은 정확하다.
        fn with_orig(mut self, orig: f32) -> Self {
            self.orig = Some(orig);
            self
        }

        /// app.rs::ref_long_px와 같은 식 — 실측과 본 텍스처의 최댓값.
        fn ref_long(&self) -> f32 {
            self.orig.unwrap_or(0.0).max(self.seen_long)
        }

        /// 텍스처 크기 `size`로 한 프레임 그린다. 반환값 = 화면상 긴 변 px.
        fn frame(&mut self, size: (f32, f32)) -> f32 {
            let avail = ((self.area.0 - 32.0).max(1.0), (self.area.1 - 32.0).max(1.0));
            let fit_scale = (avail.0 / size.0).min(avail.1 / size.1);
            if self.fit {
                self.zoom = fit_scale;
            }
            let cur_long = size.0.max(size.1).max(1.0);
            self.seen_long = self.seen_long.max(cur_long);
            let one_to_one = (self.ref_long() / cur_long).max(1.0);
            let min_zoom = fit_scale.min(1.0);
            let max_zoom = (8.0 * one_to_one).max(fit_scale);
            if !self.fit {
                self.zoom =
                    sync_view_mag(&mut self.st, self.zoom, cur_long, self.last_long, min_zoom, max_zoom);
            }
            self.last_long = Some(cur_long);
            cur_long * self.zoom
        }

        /// 상태바에 찍히는 표시 배율(1.0 = 실측 1:1). views.rs의 zoom_pct와 같은 식.
        fn shown(&self) -> f32 {
            let cur_long = self.last_long.unwrap_or(0.0);
            display_zoom_ratio(self.zoom, cur_long, self.ref_long()).unwrap_or(0.0)
        }

        /// 사진 클릭: 창맞춤 ↔ 1:1 (views.rs의 클릭 처리와 동일).
        fn click(&mut self) {
            if self.fit {
                self.fit = false;
                self.zoom = 1.0;
            } else {
                self.fit = true;
            }
        }

        /// 실측 1:1 요청(#48): 원본 긴 변이 화면 긴 변 px가 되도록 맞춘다. 기준은 `ref_long()` —
        /// 실측이 있으면 그것, 없으면 본 텍스처. 아직 프리뷰뿐이라 상한에 걸려도 기준에는
        /// 잘리기 전 목표를 남긴다(그래야 ORIG 도착 시 제 배율이 나온다).
        fn one_to_one(&mut self, size: (f32, f32)) {
            let avail = ((self.area.0 - 32.0).max(1.0), (self.area.1 - 32.0).max(1.0));
            let fit_scale = (avail.0 / size.0).min(avail.1 / size.1);
            let cur_long = size.0.max(size.1).max(1.0);
            let ref_long = self.ref_long().max(cur_long);
            let one = (ref_long / cur_long).max(1.0);
            let want = ref_long / cur_long;
            let z = want.clamp(fit_scale.min(1.0), (8.0 * one).max(fit_scale));
            self.fit = false;
            self.zoom = z;
            self.st.mag = Some(ref_long);
            self.st.pin = ((z - want).abs() > 1e-4).then_some(z);
        }

        /// Ctrl+휠/핀치로 `target` 배율까지(경계 클램프 + 창맞춤 재판정까지 동일).
        fn wheel_to(&mut self, target: f32, size: (f32, f32)) {
            let avail = ((self.area.0 - 32.0).max(1.0), (self.area.1 - 32.0).max(1.0));
            let fit_scale = (avail.0 / size.0).min(avail.1 / size.1);
            let cur_long = size.0.max(size.1).max(1.0);
            let one_to_one = (self.ref_long().max(cur_long) / cur_long).max(1.0);
            let nz = target.clamp(fit_scale.min(1.0), (8.0 * one_to_one).max(fit_scale));
            self.zoom = nz;
            self.fit = nz <= fit_scale * 1.001;
        }
    }

    /// 1440p 창의 사진 영역. D850 프리뷰(1920 긴 변)와 ORIG(8192로 클램프).
    const AREA: (f32, f32) = (1400.0, 900.0);
    const PREV: (f32, f32) = (1920.0, 1280.0);
    const ORIG: (f32, f32) = (8192.0, 5461.0);

    fn approx(a: f32, b: f32, what: &str) {
        assert!((a - b).abs() <= b.abs() * 0.005 + 0.5, "{what}: got {a}, want {b}");
    }

    #[test]
    fn view_mag_survives_orig_toggle_at_one_to_one() {
        // 프리뷰에서 1:1로 확대한 뒤 D로 ORIG↔프리뷰를 왕복해도 화면상 배율이 그대로여야 한다.
        let mut s = Sim::new(AREA);
        s.frame(PREV); // 창맞춤
        s.click(); // 1:1
        let base = s.frame(PREV);
        approx(base, 1920.0, "프리뷰 1:1");
        approx(s.frame(ORIG), base, "D → ORIG");
        approx(s.frame(PREV), base, "D → 프리뷰");
        approx(s.frame(ORIG), base, "D → ORIG 재확인");
    }

    #[test]
    fn view_mag_survives_orig_toggle_at_high_zoom() {
        // #48 회귀의 핵심: ORIG에서 크게 확대(픽셀 확인)한 뒤 D를 누르면 배율이 줄어들고
        // 다시 D를 눌러도 원래 배율이 돌아오지 않았다. 상한이 현재 텍스처(8x 프리뷰픽셀)
        // 기준이라 ORIG의 약 1.9배 이상은 프리뷰에서 표현조차 못 했기 때문.
        for target in [1.0_f32, 1.875, 2.0, 4.0, 8.0] {
            let mut s = Sim::new(AREA);
            s.frame(PREV);
            s.frame(ORIG); // 창맞춤 상태로 ORIG 로드
            s.wheel_to(target, ORIG);
            let base = s.frame(ORIG);
            approx(base, ORIG.0 * target, &format!("ORIG {target}x"));
            approx(s.frame(PREV), base, &format!("ORIG {target}x → 프리뷰"));
            approx(s.frame(ORIG), base, &format!("ORIG {target}x → 프리뷰 → ORIG"));
        }
    }

    #[test]
    fn view_mag_is_not_destroyed_when_clamped() {
        // 사진 영역이 프리뷰 긴 변보다 넓으면(4K/5K) 프리뷰의 1:1이 창맞춤보다 **작다**.
        // ORIG에는 그만큼 작은 배율이 없어 창맞춤으로 올라붙는데, 그 잘린 값이 기준이 되면
        // 배율이 영구히 깎인다. 원래 배율은 붙들어 두고 되돌아올 때 살려야 한다.
        let mut s = Sim::new((3000.0, 1600.0));
        s.frame(PREV);
        s.click(); // 1:1 = 1920px (창맞춤 2352px보다 작다)
        let base = s.frame(PREV);
        approx(base, 1920.0, "프리뷰 1:1");
        let at_orig = s.frame(ORIG);
        assert!(at_orig > base, "ORIG에는 더 작은 배율이 없어 창맞춤으로 올라붙는다: {at_orig}");
        approx(s.frame(PREV), base, "프리뷰로 되돌아오면 원래 배율 복구");
    }

    #[test]
    fn view_mag_follows_user_zoom_after_a_clamped_frame() {
        // 경계에 걸려 붙들어 둔 상태에서도 사용자가 직접 확대하면 그 값이 새 기준이 되어야 한다
        // (붙들기가 사용자 조작을 되돌려버리면 확대가 먹지 않는 버그가 된다).
        let mut s = Sim::new((3000.0, 1600.0));
        s.frame(PREV);
        s.click();
        s.frame(PREV);
        s.frame(ORIG); // 여기서 클램프 → 붙들기 발생
        s.wheel_to(1.0, ORIG); // 사용자가 ORIG 1:1로 확대
        let base = s.frame(ORIG);
        approx(base, ORIG.0, "ORIG 1:1");
        approx(s.frame(PREV), base, "그 뒤 D → 프리뷰도 사용자 배율을 따른다");
    }

    #[test]
    fn view_mag_keeps_screen_size_when_orientation_differs() {
        // 같은 항목의 캐시된 옛 방향 텍스처 → 새로 회전된 텍스처처럼 종횡비가 뒤집혀도
        // 긴 변 기준이라 배율이 종횡비만큼 튀지 않는다.
        let mut s = Sim::new(AREA);
        s.frame((1280.0, 1920.0));
        s.click();
        let base = s.frame((1280.0, 1920.0));
        approx(s.frame((1920.0, 1280.0)), base, "세로 → 가로 텍스처");
    }

    /// D850 원본 긴 변(8256). `decode::orig_long_edge`가 임베디드 최대 해상도에서 읽어 오는 값 —
    /// 이 바디의 EXIF는 160x120(IFD0 축소 썸네일)이라 EXIF로는 절대 못 얻는다.
    const ORIG_LONG: f32 = 8256.0;

    #[test]
    fn display_zoom_is_stable_across_orig_toggle() {
        // #48 신고의 실제 정체: 사진 크기는 그대로인데 상태바 숫자만 100%→23%로 튀었다.
        // 표시 배율은 `화면상 긴 변 / 원본 긴 변`이라 텍스처가 바뀌어도 값이 같아야 한다.
        let mut s = Sim::new(AREA).with_orig(ORIG_LONG);
        s.frame(PREV);
        s.click();
        s.frame(PREV);
        let (zoom_prev, base) = (s.zoom, s.shown());
        s.frame(ORIG);
        approx(s.shown(), base, "D → ORIG 표시 배율");
        // 옛 표시식(zoom을 그대로 %로)이 왜 안 되는지 못박는다: 같은 화면 크기인데 4배 넘게 튄다.
        let jump = (zoom_prev / s.zoom).max(s.zoom / zoom_prev);
        assert!(
            jump > 3.0,
            "텍스처 기준 zoom은 해상도가 바뀌면 크게 달라진다(그래서 표시에 쓰면 안 된다): {zoom_prev} → {} ({jump:.2}배)",
            s.zoom
        );
        s.frame(PREV);
        approx(s.shown(), base, "D → 프리뷰 표시 배율");
    }

    #[test]
    fn one_to_one_reaches_true_hundred_percent_on_orig() {
        // 실측 1:1을 요청하면 ORIG가 도착했을 때 원본 1픽셀 = 화면 1픽셀(=100%)이어야 한다.
        // 프리뷰(1920)뿐인 동안은 상한에 걸릴 수 있지만 기준은 남아 ORIG에서 제 배율이 나온다.
        let mut s = Sim::new(AREA).with_orig(ORIG_LONG);
        s.frame(PREV);
        s.one_to_one(PREV);
        s.frame(PREV);
        approx(s.frame(ORIG), ORIG_LONG, "ORIG 도착 후 실측 1:1");
        approx(s.frame(PREV) / ORIG_LONG * 100.0, 100.0, "프리뷰로 돌아와도 표시 100% 유지");
    }

    // ── #48 후속: EXIF가 원본 크기를 거짓말하는 바디(니콘 NEF) ────────────
    //
    // D850 실측: EXIF display_size = 160x120(IFD0이 NewSubFileType=1 축소 썸네일),
    // 실제 원본 = 8256x5504. 기준을 "본 텍스처 중 가장 큼"으로 잡으면 프리뷰(1600) →
    // ORIG(8192)로 기준이 자라 표시 배율이 그만큼 튄다. `orig_long_edge`가 임베디드
    // 최대 해상도(8256)를 실측으로 주면 기준이 처음부터 확정되어 튀지 않는다.
    /// D850 프리뷰 실측(1600x1067). PREV(1920)과 달리 실제 앱이 받는 크기.
    const PREV_REAL: (f32, f32) = (1600.0, 1067.0);

    #[test]
    fn display_zoom_is_stable_when_exif_understates_original() {
        // 창맞춤에서 D를 눌러 ORIG를 물려도 사진 크기가 그대로면 숫자도 그대로여야 한다.
        // 실측 기준이 없으면 68% → 16%로 튀던 자리(윈도우 D850 재현).
        let mut s = Sim::new(AREA).with_orig(ORIG_LONG);
        let screen_prev = s.frame(PREV_REAL);
        let shown_prev = s.shown();
        let screen_orig = s.frame(ORIG);
        approx(screen_orig, screen_prev, "창맞춤 화면 크기는 해상도와 무관하게 동일");
        approx(s.shown(), shown_prev, "표시 배율도 동일해야 한다");
        // 기준이 실측이라 표시값은 `화면px / 8256`과 정확히 일치한다(텍스처 크기와 무관).
        approx(shown_prev, screen_prev / ORIG_LONG, "표시 배율 = 화면 긴 변 / 원본 긴 변");
    }

    #[test]
    fn one_to_one_reaches_hundred_percent_when_exif_understates_original() {
        // 프리뷰(1600)만 있는 상태에서 Space를 눌러도, 기준이 실측(8256)이라 ORIG가 도착하면
        // 실측 1:1(=100%)에 도달해야 한다. 실측이 없을 때는 1600에 묶여 23%에서 멈췄다.
        let mut s = Sim::new(AREA).with_orig(ORIG_LONG);
        s.frame(PREV_REAL);
        s.one_to_one(PREV_REAL);
        s.frame(PREV_REAL);
        approx(s.frame(ORIG), ORIG_LONG, "ORIG 도착 → 실측 1:1(원본 1px = 화면 1px)");
        approx(s.shown() * 100.0, 100.0, "상태바 100%");
    }

    #[test]
    fn display_zoom_never_exceeds_hundred_at_fit_without_orig_measure() {
        // 안전판: 실측을 못 구하는 컨테이너(CR3 등)에서도 기준은 본 텍스처의 최댓값이라
        // 창맞춤 표시가 100%를 넘지 않는다(기준이 텍스처보다 작아지는 일이 없다).
        let mut s = Sim::new(AREA); // orig = None
        s.frame(PREV_REAL);
        assert!(s.shown() <= 1.0 + 1e-3, "창맞춤인데 100% 초과: {}", s.shown());
        s.frame(ORIG);
        assert!(s.shown() <= 1.0 + 1e-3, "ORIG 창맞춤인데 100% 초과: {}", s.shown());
    }

    #[test]
    fn view_mag_recovers_after_a_clamped_carry() {
        // #85로 이월된 배율(ORIG에서 2.5배 = 20480px)은 먼저 도착하는 1920 프리뷰에서 표현할 수
        // 없어 상한(8x)에 잘린다. 그래도 기준은 잘리기 전 값이라 ORIG가 도착하면 되살아나야 한다
        // — 잘린 값을 기준으로 삼으면 그 사진부터 배율이 영구히 깎인다.
        let mut st = ViewMag { mag: Some(20480.0), pin: Some(8.0) };
        let z = sync_view_mag(&mut st, 8.0, 1920.0, Some(1920.0), 0.1, 8.0);
        approx(z, 8.0, "붙들린 프리뷰 프레임의 배율");
        assert_eq!(st.mag, Some(20480.0), "잘린 값이 기준을 덮으면 안 된다");
        let z = sync_view_mag(&mut st, 8.0, 8192.0, Some(1920.0), 0.05, 8.0);
        approx(z * 8192.0, 20480.0, "ORIG 도착 → 이월 배율 복구");
    }

    #[test]
    fn view_mag_ignores_fit_frames() {
        // 창맞춤에서는 배율이 fit_scale에서 다시 나오므로 유지 대상이 아니다 — ORIG를 물려도
        // 창맞춤 크기 그대로여야 한다(창맞춤은 해상도와 무관하게 같은 화면 크기).
        let mut s = Sim::new(AREA);
        let base = s.frame(PREV);
        approx(s.frame(ORIG), base, "창맞춤은 해상도가 바뀌어도 같은 화면 크기");
        assert!(s.st.mag.is_none(), "창맞춤 프레임은 배율을 기록하지 않는다");
    }

    /// 4-튜플 근사 비교(부동소수 오차 허용). 1-0.1 같은 뺄셈은 이진분수로 정확하지 않아
    /// epsilon 비교가 필요하다(0.25/0.75는 정확하지만 일괄로 근사 비교).
    fn approx4(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) {
        let e = 1e-9;
        assert!(
            (a.0 - b.0).abs() < e && (a.1 - b.1).abs() < e && (a.2 - b.2).abs() < e && (a.3 - b.3).abs() < e,
            "got {a:?}, want {b:?}"
        );
    }

    /// in_focus/selected만 다른 AfPoint 생성 헬퍼(좌표는 인자, 크기는 0).
    fn afp(cx: f64, cy: f64, in_focus: bool, selected: bool) -> AfPoint {
        AfPoint { cx, cy, w: 0.0, h: 0.0, in_focus, selected }
    }

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
    fn af_display_coords_all_orientations() {
        // 알려진 측거점(센서 기준). w/h는 90/270도(5~8)에서 맞바뀐다.
        let pt = AfPoint { cx: 0.25, cy: 0.1, w: 0.2, h: 0.4, in_focus: false, selected: false };
        // 1(정상)과 미정의(0·9)는 그대로.
        approx4(af_display_coords(&pt, 1), (0.25, 0.1, 0.2, 0.4));
        approx4(af_display_coords(&pt, 0), (0.25, 0.1, 0.2, 0.4));
        approx4(af_display_coords(&pt, 9), (0.25, 0.1, 0.2, 0.4));
        // 2: 좌우 미러(x→1-x).
        approx4(af_display_coords(&pt, 2), (0.75, 0.1, 0.2, 0.4));
        // 3: 180°(x,y 모두 반전).
        approx4(af_display_coords(&pt, 3), (0.75, 0.9, 0.2, 0.4));
        // 4: 상하 미러(y→1-y).
        approx4(af_display_coords(&pt, 4), (0.25, 0.9, 0.2, 0.4));
        // 5~8: 전치 계열 → w/h 스왑(0.2↔0.4).
        approx4(af_display_coords(&pt, 5), (0.1, 0.25, 0.4, 0.2)); // 전치(y,x)
        approx4(af_display_coords(&pt, 6), (0.9, 0.25, 0.4, 0.2)); // 90° CW(1-y,x)
        approx4(af_display_coords(&pt, 7), (0.9, 0.75, 0.4, 0.2)); // 전치+180°(1-y,1-x)
        approx4(af_display_coords(&pt, 8), (0.1, 0.75, 0.4, 0.2)); // 90° CCW(y,1-x)
    }

    #[test]
    fn af_focus_center_priority_fallback() {
        // (a) 합초점이 있으면 합초점만 평균. A(0.2,0.4)·B(0.6,0.8) 합초, C·D는 무시.
        let a = AfInfo {
            points: vec![
                afp(0.2, 0.4, true, false),
                afp(0.6, 0.8, true, true),
                afp(0.4, 0.2, false, true),  // selected지만 in_focus 우선이라 제외
                afp(0.9, 0.9, false, false),
            ],
            source: "t",
        };
        let (x, y) = af_focus_center(&a, 1).unwrap();
        assert!((x - 0.4).abs() < 1e-9 && (y - 0.6).abs() < 1e-9);

        // (b) 합초점 없음 + 선택점 있음 → 선택점만 평균. sel: (0.4,0.2)·(0.6,0.4).
        let b = AfInfo {
            points: vec![
                afp(0.4, 0.2, false, true),
                afp(0.6, 0.4, false, true),
                afp(0.9, 0.9, false, false),
            ],
            source: "t",
        };
        let (x, y) = af_focus_center(&b, 1).unwrap();
        assert!((x - 0.5).abs() < 1e-9 && (y - 0.3).abs() < 1e-9);

        // (c) 합초·선택 모두 없음 → 전체 평균. (0.2,0.4)·(0.4,0.6) → (0.3,0.5).
        let c = AfInfo {
            points: vec![afp(0.2, 0.4, false, false), afp(0.4, 0.6, false, false)],
            source: "t",
        };
        let (x, y) = af_focus_center(&c, 1).unwrap();
        assert!((x - 0.3).abs() < 1e-9 && (y - 0.5).abs() < 1e-9);

        // (d) 점이 없으면 None(호출부는 중앙 폴백).
        let empty = AfInfo { points: vec![], source: "t" };
        assert_eq!(af_focus_center(&empty, 1), None);
    }

    #[test]
    fn compute_histo_empty_and_known() {
        // 빈 슬라이스 → 모든 bin 0, max는 1로 클램프.
        let h = compute_histo(&[]);
        assert_eq!(h.max, 1);
        assert!(h.bins.iter().flatten().all(|&v| v == 0));

        // 3픽셀 RGBA(px<200k → step=1). 값은 >>2로 버킷.
        // px0 R0 G4 B8 / px1 R1 G5 B9 / px2 R255 G255 B255 (A는 무시).
        let rgba = [0, 4, 8, 255, 1, 5, 9, 0, 255, 255, 255, 100];
        let h = compute_histo(&rgba);
        // R: 0>>2=0, 1>>2=0 → bin0=2; 255>>2=63 → bin63=1.
        assert_eq!(h.bins[0][0], 2);
        assert_eq!(h.bins[0][63], 1);
        // G: 4>>2=1, 5>>2=1 → bin1=2; 255→bin63=1.
        assert_eq!(h.bins[1][1], 2);
        assert_eq!(h.bins[1][63], 1);
        // B: 8>>2=2, 9>>2=2 → bin2=2; 255→bin63=1.
        assert_eq!(h.bins[2][2], 2);
        assert_eq!(h.bins[2][63], 1);
        // 각 채널 총합은 픽셀 수(3)와 일치(엉뚱한 bin 없음).
        for ch in 0..3 {
            assert_eq!(h.bins[ch].iter().sum::<u32>(), 3);
        }
        // max는 가장 큰 bin(2).
        assert_eq!(h.max, 2);
    }

    #[test]
    fn exif_lines_none_and_partial() {
        // 전부 None → 빈 vec.
        let empty = ExifInfo::default();
        assert!(exif_lines(&empty).is_empty());

        // 부분 채움: 카메라·렌즈·노출(조리개/셔터/ISO/초점)·일시 순서와 조합 검증.
        let ex = ExifInfo {
            camera: Some("Canon EOS R5".into()),
            lens: Some("RF 50mm".into()),
            aperture: Some("f/1.8".into()),
            shutter: Some("1/200".into()),
            iso: Some("400".into()),
            focal_length: Some("50mm".into()),
            datetime: Some("2024:06:03 14:30:45".into()),
            ..Default::default()
        };
        assert_eq!(
            exif_lines(&ex),
            vec![
                "Canon EOS R5".to_string(),
                "RF 50mm".to_string(),
                // 노출 요소는 두 칸 공백으로 join, 셔터엔 s 접미, ISO엔 접두.
                "f/1.8  1/200s  ISO 400  50mm".to_string(),
                "2024.06.03 14:30:45".to_string(),
            ]
        );

        // ISO만 있으면 노출 한 줄만(카메라·렌즈·일시 없음).
        let iso_only = ExifInfo { iso: Some("100".into()), ..Default::default() };
        assert_eq!(exif_lines(&iso_only), vec!["ISO 100".to_string()]);
    }

    #[test]
    fn fmt_bytes_boundaries_and_rounding() {
        // B 구간(<1024).
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1023), "1023 B");
        // KB 경계(1024)와 반올림(1048575/1024≈1023.999 → 1024).
        assert_eq!(fmt_bytes(1024), "1 KB");
        assert_eq!(fmt_bytes(1_048_575), "1024 KB");
        // MB 경계(1MiB)와 소수 1자리.
        assert_eq!(fmt_bytes(1_048_576), "1.0 MB");
        assert_eq!(fmt_bytes(1_572_864), "1.5 MB"); // 1.5 * 1MiB
        assert_eq!(fmt_bytes(1_073_741_823), "1024.0 MB"); // GB 직전 반올림.
        // GB 경계(1GiB)와 소수 2자리.
        assert_eq!(fmt_bytes(1_073_741_824), "1.00 GB");
        assert_eq!(fmt_bytes(1_610_612_736), "1.50 GB"); // 1.5 * 1GiB
    }
}
