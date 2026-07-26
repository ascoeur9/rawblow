//! 진단용: EXIF가 어디서 어떻게 읽히는지 조사.
//! - `read_exif()`가 채운 **모든 필드** 덤프(빈 필드는 `-`로 표시)
//! - 컨테이너(kamadak) 직접 읽기 시도 결과
//! - 파일 안의 모든 임베디드 JPEG(FF D8 FF)에 대해 EXIF 읽기 시도
//!
//! 사용: cargo run --release -p rawblow-core --example exif_probe -- "<folder>" [count]
//!       cargo run --release -p rawblow-core --example exif_probe -- "<file>"

use rawblow_core::meta::read_exif;
use rawblow_core::model::is_supported;
use std::path::PathBuf;

fn main() {
    let arg = std::env::args().nth(1).expect("path arg required");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let root = PathBuf::from(&arg);

    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root);
    } else {
        collect(&root, &mut files);
        files.sort();
        files.truncate(count);
    }

    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy();
        println!("== {name} ==");
        match read_exif(p) {
            Some(ex) => {
                let s = |o: &Option<String>| o.clone().unwrap_or_else(|| "-".into());
                let n = |o: &Option<u32>| o.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
                println!("  camera       : {}", s(&ex.camera));
                println!("  lens         : {}", s(&ex.lens));
                println!("  focal_length : {}", s(&ex.focal_length));
                println!("  aperture     : {}", s(&ex.aperture));
                println!("  shutter      : {}", s(&ex.shutter));
                println!("  iso          : {}", s(&ex.iso));
                println!("  exposure_bias: {}", s(&ex.exposure_bias));
                println!("  white_balance: {}", s(&ex.white_balance));
                println!("  datetime     : {}", s(&ex.datetime));
                println!("  orientation  : {}", ex.orientation);
                println!(
                    "  size(exif)   : {}x{}   display: {}",
                    n(&ex.width),
                    n(&ex.height),
                    ex.display_size().map(|(w, h)| format!("{w}x{h}")).unwrap_or("-".into())
                );
                match ex.gps {
                    Some(g) => println!(
                        "  gps          : {:.6}, {:.6}  alt={}",
                        g.lat,
                        g.lon,
                        g.alt.map(|a| format!("{a:.1}m")).unwrap_or("-".into())
                    ),
                    None => println!("  gps          : -"),
                }
                // 비어 있는 필드를 한 줄로 요약(누락 감사용).
                let mut missing = Vec::new();
                for (k, v) in [
                    ("camera", ex.camera.is_none()),
                    ("lens", ex.lens.is_none()),
                    ("focal_length", ex.focal_length.is_none()),
                    ("aperture", ex.aperture.is_none()),
                    ("shutter", ex.shutter.is_none()),
                    ("iso", ex.iso.is_none()),
                    ("exposure_bias", ex.exposure_bias.is_none()),
                    ("white_balance", ex.white_balance.is_none()),
                    ("datetime", ex.datetime.is_none()),
                    ("width", ex.width.is_none()),
                    ("gps", ex.gps.is_none()),
                ] {
                    if v {
                        missing.push(k);
                    }
                }
                println!("  MISSING      : {}", if missing.is_empty() { "(none)".into() } else { missing.join(", ") });
            }
            None => println!("  read_exif: None  ← EXIF를 전혀 못 읽음"),
        }
        let bytes = std::fs::read(p).unwrap();
        {
            let mut c = std::io::Cursor::new(&bytes[..]);
            match exif::Reader::new().read_from_container(&mut c) {
                Ok(e) => {
                    let m = e.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|f| f.display_value().to_string());
                    println!("  container kamadak: OK model={:?} fields={}", m, e.fields().count());
                }
                Err(e) => println!("  container kamadak: Err({e})"),
            }
        }
        let mut i = 0usize;
        let mut found = 0;
        while i + 3 < bytes.len() {
            if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
                let mut c = std::io::Cursor::new(&bytes[i..]);
                if let Ok(e) = exif::Reader::new().read_from_container(&mut c) {
                    println!("  embedded JPEG @ {i}: EXIF OK fields={}", e.fields().count());
                    found += 1;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        if found == 0 {
            println!("  (no embedded JPEG carried readable EXIF)");
        }
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
