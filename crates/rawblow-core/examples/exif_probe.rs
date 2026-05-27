//! 진단용: RW2의 EXIF가 어디에 있는지 조사.
//! - read_exif() 결과
//! - 파일 안의 모든 임베디드 JPEG(FF D8 FF)에 대해 kamadak로 EXIF 읽기 시도
//! - 컨테이너(TIFF IFD0) 직접 읽기 시도
//! 사용: cargo run --release -p rawblow-core --example exif_probe -- "<folder>" [count]

use rawblow_core::meta::read_exif;
use rawblow_core::model::is_supported;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg required");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();

    for p in files.iter().take(count) {
        let name = p.file_name().unwrap().to_string_lossy();
        println!("== {name} ==");
        match read_exif(p) {
            Some(ex) => println!("  read_exif: cam={:?} f={:?} ss={:?} iso={:?}", ex.camera, ex.aperture, ex.shutter, ex.iso),
            None => println!("  read_exif: None"),
        }
        let bytes = std::fs::read(p).unwrap();
        // 컨테이너(TIFF) 직접 kamadak.
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
        // 모든 임베디드 JPEG 스캔.
        let mut i = 0usize;
        let mut found = 0;
        while i + 3 < bytes.len() {
            if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
                let slice = &bytes[i..];
                let mut c = std::io::Cursor::new(slice);
                if let Ok(e) = exif::Reader::new().read_from_container(&mut c) {
                    let m = e.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|f| f.display_value().to_string());
                    let f = e.get_field(exif::Tag::FNumber, exif::In::PRIMARY).map(|f| f.display_value().to_string());
                    println!("  embedded JPEG @ {i}: EXIF OK fields={} model={:?} f={:?}", e.fields().count(), m, f);
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
