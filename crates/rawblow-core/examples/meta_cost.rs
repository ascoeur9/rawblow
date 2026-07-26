//! 진단용: meta::orientation()/read_exif()의 실제 I/O 비용 측정.
//!
//! 사용: cargo run --release -p rawblow-core --example meta_cost -- "<folder-or-file>"

use rawblow_core::meta::{orientation, read_exif};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let arg = std::env::args().nth(1).expect("path arg required");
    let root = PathBuf::from(&arg);
    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root);
    } else {
        collect(&root, &mut files);
    }
    files.sort();
    // 확장자별로 첫 파일만.
    let mut seen = std::collections::HashSet::new();
    for p in &files {
        let ext = p.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        if !seen.insert(ext.clone()) {
            continue;
        }
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let t = Instant::now();
        let o = orientation(p);
        let d_or = t.elapsed();
        let t = Instant::now();
        let ex = read_exif(p);
        let d_ex = t.elapsed();
        let (eo, dims) = match &ex {
            Some(e) => (
                e.orientation.to_string(),
                e.display_size().map(|(w, h)| format!("{w}x{h}")).unwrap_or("-".into()),
            ),
            None => ("-".into(), "-".into()),
        };
        let flag = if ex.is_some() && eo != o.to_string() { " <<< MISMATCH" } else { "" };
        println!(
            "{ext:5} {:>6.1}MB  orient={o} in {:>6.1}ms | exif={} exif_orient={eo} disp={dims} in {:>6.1}ms  {}{flag}",
            size as f64 / 1e6,
            d_or.as_secs_f64() * 1e3,
            ex.is_some(),
            d_ex.as_secs_f64() * 1e3,
            p.file_name().unwrap().to_string_lossy()
        );
    }
}

fn collect(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if is_supported(&p) {
            out.push(p);
        }
    }
}
