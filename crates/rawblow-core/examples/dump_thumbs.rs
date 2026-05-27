//! 진단용: 폴더 앞쪽 N개 파일을 썸네일 설정으로 디코딩해 PNG로 저장.
//! 디코딩 결과 픽셀이 실제로 회색/깨짐인지 눈으로 확인하기 위함.
//! 사용: cargo run --release -p rawblow-core --example dump_thumbs -- "<folder>" [count] [out_dir]

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::model::is_supported;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg required");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(16);
    let out = std::env::args()
        .nth(3)
        .unwrap_or_else(|| std::env::temp_dir().join("rawblow_thumbs").to_string_lossy().to_string());
    std::fs::create_dir_all(&out).ok();

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    println!("files={} dumping first {} to {}", files.len(), count, out);

    for (i, p) in files.iter().take(count).enumerate() {
        match decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(320) }) {
            Ok(img) => {
                // 평균/최소/최대 밝기로 회색 여부 정량 판단.
                let n = img.rgba.len() / 4;
                let mut sum = 0u64;
                let mut mn = 255u8;
                let mut mx = 0u8;
                for px in img.rgba.chunks_exact(4) {
                    let l = ((px[0] as u32 + px[1] as u32 + px[2] as u32) / 3) as u8;
                    sum += l as u64;
                    mn = mn.min(l);
                    mx = mx.max(l);
                }
                let avg = if n > 0 { sum / n as u64 } else { 0 };
                let range = mx - mn;
                let name = p.file_stem().unwrap().to_string_lossy();
                // 의심(회색/저대비)만 저장·출력해 전체 폴더 스캔을 빠르게.
                if range < 50 {
                    let png = format!("{}/{:02}_{}.png", out, i, name);
                    if let Some(buf) =
                        image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
                    {
                        buf.save(&png).ok();
                    }
                    println!(
                        "SUSPECT {:04} {:>4}x{:<4} avg={:3} min={:3} max={:3} range={:3}  {}",
                        i, img.width, img.height, avg, mn, mx, range, name
                    );
                }
            }
            Err(e) => println!("{:02} ERR {}  {:?}", i, e, p.file_name().unwrap()),
        }
    }
    println!("done → {}", out);
}
