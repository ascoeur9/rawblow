//! Windows MSIX(스토어) 패키지용 로고 PNG 자산 생성기.
//!
//! `logo.rs` 와 **동일한** 셰브론 마크를 MSIX 매니페스트가 요구하는 타일 크기로
//! 각각 네이티브 렌더(SDF AA)해, 작은 크기(44px 작업표시줄)도 또렷하게 만든다.
//! 풀블리드(여백 0)라 둥근 사각 박스가 그대로 앱 아이콘이 된다(.exe .ico 와 동일 룩).
//!
//! 사용:
//!   cargo run -q -p rawblow-app --example gen_msix_assets -- <out Assets 디렉터리>
//! (scripts/build-msix-windows.ps1 가 스테이징의 Assets\ 로 이 명령을 호출한다.)

#![allow(dead_code)] // logo.rs 의 draw_mark 등 일부는 이 생성기에서 안 쓴다.

#[path = "../src/logo.rs"]
mod logo;

use std::path::Path;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: gen_msix_assets <assets_dir>");
    let dir = Path::new(&out);
    std::fs::create_dir_all(dir).expect("create assets dir");

    // (파일명, 픽셀 크기) — MSIX 필수/권장 자산.
    //   Square44x44Logo   : VisualElements 필수(앱 목록·작업표시줄)
    //   Square150x150Logo : VisualElements 필수(중간 타일)
    //   Square71x71Logo   : DefaultTile 권장(작은 타일)
    //   StoreLogo(50px)   : Properties/Logo 필수(스토어 목록·설정)
    // (Square310x310 대형 타일은 Wide310x150 짝이 있어야만 유효 — 생략.)
    let specs: [(&str, u32); 4] = [
        ("Square44x44Logo.png", 44),
        ("Square71x71Logo.png", 71),
        ("Square150x150Logo.png", 150),
        ("StoreLogo.png", 50),
    ];

    for (name, size) in specs {
        let rgba = logo::icon_rgba(size); // 풀블리드(여백 0).
        let img = image::RgbaImage::from_raw(size, size, rgba).expect("rgba buffer size");
        img.save(dir.join(name)).expect("write png");
        println!("wrote {name} ({size}px)");
    }
    println!("assets → {}", dir.display());
}
