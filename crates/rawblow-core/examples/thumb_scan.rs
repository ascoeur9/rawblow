//! [임시] 썸네일 디코딩이 prefix를 넘어 **전체파일(수십 MB) 폴백**을 얼마나 하는지 스캔.
//! 전체파일 폴백이 많으면 그게 "썸네일 3분"의 진짜 원인.
//! 사용: cargo run --release -p rawblow-core --example thumb_scan -- "<folder>" [count] [offset]

use rawblow_core::decode::{self, decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let offset: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok().map(|e| e.into_path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    let files: Vec<PathBuf> = files.into_iter().skip(offset).take(count).collect();

    let mut fallbacks = 0usize;
    let mut total_bytes = 0u64;
    let mut max_bytes = 0u64;
    let mut sum_ms = 0.0;
    let mut worst: Vec<(String, u64, f64)> = Vec::new();

    let mut errors = 0usize;
    let mut gray = 0usize; // 분산이 낮은(회색/균일) 손상 의심 썸네일 수
    let mut min_var = f64::MAX;
    let mut min_var_file = String::new();
    let t_all = Instant::now();
    for p in &files {
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let r = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(320) });
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let b = decode::take_bytes_read();
        total_bytes += b;
        max_bytes = max_bytes.max(b);
        sum_ms += ms;
        match &r {
            Ok(img) => {
                // 루마 분산(거친) — 0에 가까우면 회색/균일(역사적 false-EOI 손상).
                let mut s = 0.0f64;
                let mut s2 = 0.0f64;
                let mut k = 0.0f64;
                for px in img.rgba.as_chunks::<4>().0.iter().step_by(11) {
                    let y = 0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64;
                    s += y;
                    s2 += y * y;
                    k += 1.0;
                }
                let var = if k > 0.0 { (s2 / k) - (s / k).powi(2) } else { 0.0 };
                if var < min_var {
                    min_var = var;
                    min_var_file = p.file_name().unwrap().to_string_lossy().into();
                }
                if var < 30.0 {
                    gray += 1;
                }
            }
            Err(_) => errors += 1,
        }
        if b > 2 * 1024 * 1024 {
            fallbacks += 1;
            if worst.len() < 12 {
                worst.push((p.file_name().unwrap().to_string_lossy().into(), b, ms));
            }
        }
    }
    let total_s = t_all.elapsed().as_secs_f64();
    let n = files.len().max(1);
    println!("scanned {} files (offset {offset})", files.len());
    println!("whole-file FALLBACKS (>2MB read): {fallbacks}  ({:.0}%)", fallbacks as f64 * 100.0 / n as f64);
    println!("avg bytes/thumb = {:.2}MB   max = {:.1}MB", total_bytes as f64 / 1e6 / n as f64, max_bytes as f64 / 1e6);
    println!("avg time/thumb = {:.1}ms   total wall = {:.1}s", sum_ms / n as f64, total_s);
    println!("decode ERRORS = {errors}   GRAY(var<30) = {gray}   min luma-var = {:.0} ({min_var_file})", if min_var == f64::MAX { 0.0 } else { min_var });
    println!("→ 3562개 전체 추정 시간 = {:.0}s", total_s / files.len() as f64 * 3562.0);
    for (name, b, ms) in &worst {
        println!("   FALLBACK {name}  {:.1}MB  {:.0}ms", *b as f64 / 1e6, ms);
    }
}
