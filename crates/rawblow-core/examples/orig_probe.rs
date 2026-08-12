//! 진단용: ORIG(원본 보기) 디코딩 크기 확인.
//! 사용: cargo run --release -p rawblow-core --example orig_probe -- "<folder>" [count] [edge]

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::model::is_supported;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg required");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let edge: u32 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(8192);

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();

    for p in files.iter().take(count) {
        let name = p.file_name().unwrap().to_string_lossy();
        // 프리뷰(빠른) vs ORIG(원본) 크기 비교.
        let prev = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(1600) });
        let orig = decode_file(p, DecodeOptions { full_raw: true, max_edge: Some(edge) });
        let dim = |r: &Result<_, _>| match r {
            Ok(img) => {
                let i: &rawblow_core::decode::DecodedImage = img;
                format!("{}x{} full_raw={}", i.width, i.height, i.full_raw)
            }
            Err(e) => format!("ERR {e}"),
        };
        // 표시 배율(100%=실측 1:1)의 기준(#48). EXIF width/height와 갈리는 바디(NEF 등)를
        // 잡아내려고 둘 다 찍는다 — ref가 EXIF보다 크면 EXIF가 썸네일 크기라는 뜻.
        let ref_long = rawblow_core::decode::orig_long_edge(p);
        let exif_long = rawblow_core::meta::read_exif(p)
            .and_then(|e| e.display_size())
            .map(|(w, h)| w.max(h));
        println!(
            "{name}\n  preview: {}\n  ORIG:    {}\n  ref(#48): {:?}   exif: {:?}",
            dim(&prev),
            dim(&orig),
            ref_long,
            exif_long
        );
    }
}
