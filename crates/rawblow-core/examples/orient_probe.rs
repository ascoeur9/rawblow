//! 진단용: 파일별 EXIF Orientation 검출 + 디코딩 결과 방향 확인.
//!
//! 사용: cargo run --release -p rawblow-core --example orient_probe -- "<folder-or-file>"

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::meta::orientation;
use rawblow_core::model::is_supported;
use std::path::PathBuf;

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

    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy();
        let o = orientation(p);
        // 컨테이너 헤더 앞 16바이트(포맷 식별용).
        let head = {
            use std::io::Read;
            let mut b = [0u8; 16];
            let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
            b[..n].iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
        };
        let thumb = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(320) })
            .map(|d| format!("{}x{}", d.width, d.height))
            .unwrap_or_else(|e| format!("ERR({e})"));
        let prev = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(1600) })
            .map(|d| format!("{}x{}", d.width, d.height))
            .unwrap_or_else(|e| format!("ERR({e})"));
        let orig = decode_file(p, DecodeOptions { full_raw: true, max_edge: Some(2400) })
            .map(|d| format!("{}x{}", d.width, d.height))
            .unwrap_or_else(|e| format!("ERR({e})"));
        println!("{name}");
        println!("  head   : {head}");
        println!("  orient : {o}");
        println!("  thumb  : {thumb}");
        println!("  preview: {prev}");
        println!("  orig   : {orig}");
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
