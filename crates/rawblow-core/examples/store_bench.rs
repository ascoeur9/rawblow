//! [임시] 썸네일 디스크 캐시 store 비용 측정 — 실제 캐시 디렉토리(Defender 스캔 대상) vs temp.
//! "썸네일 3분"이 디코딩이 아니라 캐시 쓰기(파일 생성+Defender) 때문인지 확인.
//! 사용: cargo run --release -p rawblow-core --example store_bench -- "<folder>" [count]

use rawblow_core::cache;
use rawblow_core::config;
use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);

    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok().map(|e| e.into_path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    files.truncate(count);

    // 미리 디코딩(저장 비용만 따로 재기 위해).
    let imgs: Vec<_> = files
        .iter()
        .filter_map(|p| decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(320) }).ok().map(|i| (p.clone(), i)))
        .collect();
    println!("decoded {} thumbs", imgs.len());

    let real_dir = config::cache_dir();
    let tmp = tempfile::tempdir().unwrap();
    println!("real cache dir = {}", real_dir.display());

    // 1) 실제 캐시 디렉토리(LOCALAPPDATA, Defender 스캔)로 store.
    let _ = std::fs::create_dir_all(&real_dir);
    let t = Instant::now();
    for (p, img) in &imgs {
        let key = cache::thumb_key(p, 320).unwrap_or_else(|| "x".into());
        cache::store(&real_dir, &format!("bench_{key}"), img);
    }
    let real_ms = t.elapsed().as_secs_f64() * 1000.0;

    // 2) temp 디렉토리로 store(같은 디스크일 수 있으나 Defender 예외일 수도).
    let t = Instant::now();
    for (p, img) in &imgs {
        let key = cache::thumb_key(p, 320).unwrap_or_else(|| "x".into());
        cache::store(tmp.path(), &format!("bench_{key}"), img);
    }
    let tmp_ms = t.elapsed().as_secs_f64() * 1000.0;

    let n = imgs.len().max(1) as f64;
    println!("store→real-cache  avg {:.2}ms/thumb  (3562개 추정 {:.0}s)", real_ms / n, real_ms / n * 3562.0 / 1000.0);
    println!("store→temp        avg {:.2}ms/thumb  (3562개 추정 {:.0}s)", tmp_ms / n, tmp_ms / n * 3562.0 / 1000.0);

    // 정리: 벤치 파일 삭제.
    if let Ok(rd) = std::fs::read_dir(&real_dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("bench_") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
