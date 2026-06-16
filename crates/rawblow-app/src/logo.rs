//! RawBlow 로고 마크: 어두운 둥근 사각 박스 + 3겹 셰브론(»), 점점 밝아지는 블루.
//! 원본 로고 SVG와 동일한 기하를 egui로 직접 그리고(벡터, 의존성 없음),
//! 창/작업표시줄 아이콘용 RGBA 래스터도 같은 기하로 생성한다.

use eframe::egui::{self, Color32, Rect, Rounding, Shape, Stroke, Vec2};

// 64px viewBox 기준 색/좌표(SVG와 동일).
const BOX_FILL: Color32 = Color32::from_rgb(0x0e, 0x12, 0x18);
const BOX_STROKE: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2c);
// 셰브론 3겹(어두운→밝은 블루로 깊이감). 알파 대신 색 단계로 표현(겹침 블렌딩 artifact 회피).
const CHEV: [Color32; 3] = [
    Color32::from_rgb(0x3a, 0x7b, 0xd5),
    Color32::from_rgb(0x6a, 0xb8, 0xff),
    Color32::from_rgb(0x8e, 0xc9, 0xff),
];
const CHEV_X: [f32; 3] = [16.0, 28.0, 40.0];

/// egui painter로 마크를 `rect`(정사각 가정)에 그린다.
pub fn draw_mark(painter: &egui::Painter, rect: Rect) {
    let s = rect.width() / 64.0;
    let p = |x: f32, y: f32| rect.min + Vec2::new(x * s, y * s);
    painter.rect_filled(rect, Rounding::same(14.0 * s), BOX_FILL);
    painter.rect_stroke(rect, Rounding::same(14.0 * s), Stroke::new(s.max(1.0), BOX_STROKE));
    let w = 4.0 * s;
    for (i, &x) in CHEV_X.iter().enumerate() {
        let a = p(x, 20.0);
        let b = p(x + 12.0, 32.0);
        let c = p(x, 44.0);
        painter.add(Shape::line(vec![a, b, c], Stroke::new(w, CHEV[i])));
        // 둥근 캡/조인(egui 라인은 평평한 끝이라 점 위에 원을 덧그림).
        for pt in [a, b, c] {
            painter.circle_filled(pt, w * 0.5, CHEV[i]);
        }
    }
}

/// 창 아이콘용 RGBA8(straight alpha) 버퍼. SDF 기반 안티에일리어싱.
/// macOS 앱 아이콘 표준 여백(각 변, 캔버스 대비 비율). Apple 그리드 기준
/// 1024 캔버스에서 본체 824 → 가장자리 100px. 이 여백이 없으면 cmd+Tab·Dock에서
/// 다른 앱보다 아이콘이 커 보인다.
pub const MACOS_ICON_MARGIN: f32 = 100.0 / 1024.0;

/// 창 아이콘용 RGBA8(straight alpha) 버퍼. 마크를 캔버스에 꽉 채운다(풀블리드).
pub fn icon_rgba(size: u32) -> Vec<u8> {
    icon_rgba_inset(size, 0.0)
}

/// 마크를 `margin`(각 변, 캔버스 대비 비율)만큼 안쪽으로 그린 RGBA8(SDF AA).
/// `margin == 0.0` 이면 풀블리드(창/작업표시줄 아이콘용), macOS `.icns` 는
/// [`MACOS_ICON_MARGIN`] 을 써서 표준 여백을 둔다.
pub fn icon_rgba_inset(size: u32, margin: f32) -> Vec<u8> {
    let n = size as usize;
    let mut buf = vec![0u8; n * n * 4];
    let off = size as f32 * margin; // 안쪽 박스 좌상단 오프셋(px).
    let inner = size as f32 * (1.0 - 2.0 * margin); // 마크가 차지하는 안쪽 정사각 변(px).
    let s = inner / 64.0;
    let r_box = 14.0 * s;
    let stroke_r = 2.0 * s; // 4px 스트로크의 반.
    // 셰브론 세그먼트(픽셀 좌표) 2개씩 — 안쪽 박스 기준으로 off 만큼 이동.
    let seg = |x: f32| {
        [
            (off + x * s, off + 20.0 * s, off + (x + 12.0) * s, off + 32.0 * s),
            (off + (x + 12.0) * s, off + 32.0 * s, off + x * s, off + 44.0 * s),
        ]
    };
    let chevs = [seg(CHEV_X[0]), seg(CHEV_X[1]), seg(CHEV_X[2])];

    for py in 0..n {
        for px in 0..n {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            // 둥근 사각 박스(채움) — 안쪽 정사각 로컬 좌표로 SDF 평가.
            let cov_box = (0.5 - sdf_round_rect(fx - off, fy - off, inner, inner, r_box)).clamp(0.0, 1.0);
            let (mut r, mut g, mut b) = (BOX_FILL.r() as f32, BOX_FILL.g() as f32, BOX_FILL.b() as f32);
            let mut a = cov_box;
            // 셰브론(위에 알파 over 블렌딩).
            for (ci, segs) in chevs.iter().enumerate() {
                let mut d = f32::MAX;
                for &(x0, y0, x1, y1) in segs.iter() {
                    d = d.min(sdf_segment(fx, fy, x0, y0, x1, y1));
                }
                let cov = (stroke_r - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let col = CHEV[ci];
                    let out_a = cov + a * (1.0 - cov);
                    if out_a > 0.0 {
                        r = (col.r() as f32 * cov + r * a * (1.0 - cov)) / out_a;
                        g = (col.g() as f32 * cov + g * a * (1.0 - cov)) / out_a;
                        b = (col.b() as f32 * cov + b * a * (1.0 - cov)) / out_a;
                    }
                    a = out_a;
                }
            }
            let o = (py * n + px) * 4;
            buf[o] = r.round() as u8;
            buf[o + 1] = g.round() as u8;
            buf[o + 2] = b.round() as u8;
            buf[o + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    buf
}

/// 둥근 사각형 SDF(좌상단 0,0 기준 w×h, 모서리 반지름 r). 음수=내부.
fn sdf_round_rect(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let (cx, cy) = (w * 0.5, h * 0.5);
    let qx = (px - cx).abs() - (cx - r);
    let qy = (py - cy).abs() - (cy - r);
    let (ax, ay) = (qx.max(0.0), qy.max(0.0));
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

/// 점에서 선분까지 거리.
fn sdf_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (pax, pay) = (px - x0, py - y0);
    let (bax, bay) = (x1 - x0, y1 - y0);
    let denom = bax * bax + bay * bay;
    let h = if denom > 0.0 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (pax - bax * h, pay - bay * h);
    (dx * dx + dy * dy).sqrt()
}
