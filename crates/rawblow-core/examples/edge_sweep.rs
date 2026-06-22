//! [임시] 디코드 타깃 변(max_edge) 스윕: 썸네일/프리뷰 타깃별 시간·바이트·출력크기.
//! decode_file의 max_edge 인자를 한 실행에서 여러 값으로 — 각 값이 1 사이클.
//! 사용: cargo run --release -p rawblow-core --example edge_sweep -- "<folder>" [count]

use rawblow_core::decode::{self, decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn avg_for(files: &[PathBuf], edge: u32, full_raw: bool) -> (f64, f64, String) {
    let (mut ms, mut by) = (0.0, 0u64);
    let mut dims = String::new();
    for p in files {
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let r = decode_file(p, DecodeOptions { full_raw, max_edge: Some(edge) });
        ms += t.elapsed().as_secs_f64() * 1000.0;
        by += decode::take_bytes_read();
        if dims.is_empty() {
            if let Ok(i) = &r {
                dims = format!("{}x{}", i.width, i.height);
            }
        }
    }
    let n = files.len().max(1) as f64;
    (ms / n, by as f64 / 1e6 / n, dims)
}

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok().map(|e| e.into_path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    files.truncate(count);
    println!("files={}", files.len());

    println!("\n== THUMBNAIL target sweep (full_raw=false) ==");
    for edge in [128u32, 192, 256, 288, 320, 384, 448, 512] {
        let (ms, mb, dims) = avg_for(&files, edge, false);
        println!("  edge={edge:4} -> {ms:5.1}ms  {mb:.2}MB  out~{dims}");
    }
    println!("\n== PREVIEW target sweep (full_raw=false) ==");
    for edge in [1280u32, 1440, 1600, 1920, 2048, 2560, 3200, 4096] {
        let (ms, mb, dims) = avg_for(&files, edge, false);
        println!("  edge={edge:4} -> {ms:5.1}ms  {mb:.2}MB  out~{dims}");
    }
}
