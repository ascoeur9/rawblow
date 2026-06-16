//! macOS `.icns` 용 아이콘셋(PNG 10종) 생성기.
//!
//! cmd+Tab·Dock에서 다른 앱과 같은 크기로 보이도록, `logo.rs` 와 **동일한**
//! 셰브론 기하를 macOS 표준 여백([`logo::MACOS_ICON_MARGIN`])으로 렌더한다.
//! 각 픽셀 크기로 네이티브 렌더(SDF AA)해 작은 크기도 또렷하다.
//!
//! 사용:
//!   cargo run -q -p rawblow-app --example gen_macos_icon -- <out.iconset 디렉터리>
//! 이후 `iconutil -c icns <out.iconset> -o RawBlow.icns` 로 합친다
//! (scripts/gen-macos-icon.sh 가 두 단계를 한 번에 수행).

#![allow(dead_code)] // logo.rs 의 draw_mark 등 일부는 이 생성기에서 안 쓴다.

#[path = "../src/logo.rs"]
mod logo;

use std::path::Path;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: gen_macos_icon <iconset_dir>");
    let dir = Path::new(&out);
    std::fs::create_dir_all(dir).expect("create iconset dir");

    // (파일명, 픽셀 크기) — Apple iconset 표준 10종.
    let specs: [(&str, u32); 10] = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ];

    for (name, size) in specs {
        let rgba = logo::icon_rgba_inset(size, logo::MACOS_ICON_MARGIN);
        let img = image::RgbaImage::from_raw(size, size, rgba).expect("rgba buffer size");
        img.save(dir.join(name)).expect("write png");
        println!("wrote {name} ({size}px)");
    }
    println!("iconset → {}", dir.display());
}
