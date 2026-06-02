//! [임시] 제조사별 RAW 한 파일씩 — 썸네일(320)·프리뷰(2048)·ORIG 각각의 바이트/시간/출력크기.
//! 모든 경로가 정상 디코딩되는지(회귀 없이) 교차검증. 사용:
//!   cargo run --release -p rawblow-core --example fmt_check -- <file1> <file2> ...
//! 또는 폴더를 주면 그 폴더의 첫 RAW 한 개만.

use rawblow_core::decode::{self, decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn one(p: &PathBuf, full_raw: bool, edge: Option<u32>) -> (f64, f64, String) {
    let _ = decode::take_bytes_read();
    let t = Instant::now();
    let r = decode_file(p, DecodeOptions { full_raw, max_edge: edge });
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let by = decode::take_bytes_read() as f64 / 1e6;
    let dims = match &r {
        Ok(i) => format!("{}x{}", i.width, i.height),
        Err(e) => format!("ERR {e}"),
    };
    (ms, by, dims)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files: Vec<PathBuf> = Vec::new();
    for a in &args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            if let Some(f) = WalkDir::new(&p)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok().map(|e| e.into_path()))
                .filter(|p| p.is_file() && is_supported(p))
                .min()
            {
                files.push(f);
            }
        } else if p.is_file() {
            files.push(p);
        }
    }
    println!(
        "{:<14} {:>22} {:>22} {:>22}",
        "file", "thumb320", "preview2048", "ORIG"
    );
    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy();
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        let (tm, tb, td) = one(p, false, Some(320));
        let (pm, pb, pd) = one(p, false, Some(2048));
        let (om, ob, od) = one(p, true, None);
        println!(
            "{:<14} {:>6.0}ms {:>5.2}MB {:>8} {:>6.0}ms {:>5.2}MB {:>8} {:>6.0}ms {:>5.2}MB {:>8}",
            format!("{ext}/{}", &name[..name.len().min(6)]),
            tm, tb, td, pm, pb, pd, om, ob, od
        );
    }
}
