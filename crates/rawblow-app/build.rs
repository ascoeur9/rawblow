//! 빌드 스크립트: Windows에서 RawBlow 로고를 멀티사이즈 .ico로 만들어 .exe 파일
//! 아이콘(탐색기/작업표시줄)으로 임베드한다. logo.rs와 동일한 셰브론 기하(SDF 래스터).
//! 리소스 컴파일러(rc.exe)가 없으면 조용히 건너뛰어 빌드는 계속된다.

fn main() {
    #[cfg(windows)]
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ico_path = std::path::Path::new(&out_dir).join("rawblow.ico");

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for &size in &[16u32, 24, 32, 48, 64, 128, 256] {
        let rgba = render_mark(size);
        let img = ico::IconImage::from_rgba_data(size, size, rgba);
        match ico::IconDirEntry::encode(&img) {
            Ok(e) => dir.add_entry(e),
            Err(_) => return,
        }
    }
    let f = match std::fs::File::create(&ico_path) {
        Ok(f) => f,
        Err(_) => return,
    };
    if dir.write(f).is_err() {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico_path.to_str().unwrap());
    // rc.exe(Windows SDK)가 없으면 실패할 수 있는데, 아이콘만 빠질 뿐 빌드는 계속되게 무시.
    let _ = res.compile();
}

/// 로고 마크(어두운 둥근 사각 박스 + 3겹 셰브론)를 `size`×`size` RGBA8로 렌더(SDF AA).
#[cfg(windows)]
fn render_mark(size: u32) -> Vec<u8> {
    let n = size as usize;
    let mut buf = vec![0u8; n * n * 4];
    let s = size as f32 / 64.0;
    let r_box = 14.0 * s;
    let stroke_r = 2.0 * s;
    let sz = size as f32;
    let box_fill = [14u8, 18, 24];
    let chev = [[58u8, 123, 213], [106, 184, 255], [142, 201, 255]];
    let chev_x = [16.0f32, 28.0, 40.0];
    let seg = |x: f32| {
        [
            (x * s, 20.0 * s, (x + 12.0) * s, 32.0 * s),
            ((x + 12.0) * s, 32.0 * s, x * s, 44.0 * s),
        ]
    };
    let chevs = [seg(chev_x[0]), seg(chev_x[1]), seg(chev_x[2])];

    for py in 0..n {
        for px in 0..n {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let cov_box = (0.5 - sdf_round_rect(fx, fy, sz, sz, r_box)).clamp(0.0, 1.0);
            let (mut r, mut g, mut b) = (box_fill[0] as f32, box_fill[1] as f32, box_fill[2] as f32);
            let mut a = cov_box;
            for (ci, segs) in chevs.iter().enumerate() {
                let mut d = f32::MAX;
                for &(x0, y0, x1, y1) in segs.iter() {
                    d = d.min(sdf_segment(fx, fy, x0, y0, x1, y1));
                }
                let cov = (stroke_r - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let c = chev[ci];
                    let out_a = cov + a * (1.0 - cov);
                    if out_a > 0.0 {
                        r = (c[0] as f32 * cov + r * a * (1.0 - cov)) / out_a;
                        g = (c[1] as f32 * cov + g * a * (1.0 - cov)) / out_a;
                        b = (c[2] as f32 * cov + b * a * (1.0 - cov)) / out_a;
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

#[cfg(windows)]
fn sdf_round_rect(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let (cx, cy) = (w * 0.5, h * 0.5);
    let qx = (px - cx).abs() - (cx - r);
    let qy = (py - cy).abs() - (cy - r);
    let (ax, ay) = (qx.max(0.0), qy.max(0.0));
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

#[cfg(windows)]
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
