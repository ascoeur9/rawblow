//! [임시] RW2/TIFF IFD0 태그 덤프 — 임베디드 JPEG(JpgFromRaw 0x002e 등) 오프셋/길이 확인용.
//! 사용: cargo run --release -p rawblow-core --example ifd_dump -- "<file.rw2>"

use std::io::Read;

fn main() {
    let path = std::env::args().nth(1).expect("file arg");
    let mut f = std::fs::File::open(&path).unwrap();
    let flen = f.metadata().unwrap().len();
    let mut buf = vec![0u8; 1 << 20]; // 앞 1MB
    let n = f.read(&mut buf).unwrap();
    buf.truncate(n);

    let le = match &buf[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => {
            println!("not TIFF-like: {:02x} {:02x}", buf[0], buf[1]);
            return;
        }
    };
    let r16 = |o: usize| -> u16 {
        let b = &buf[o..o + 2];
        if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }
    };
    let r32 = |o: usize| -> u32 {
        let b = &buf[o..o + 4];
        if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }
    };
    println!("magic byte2={:#x} (RW2=0x55, TIFF=0x2a)  filelen={:.1}MB", r16(2) & 0xff, flen as f64 / 1e6);

    let type_size = |t: u16| -> usize {
        match t { 1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 | 11 => 4, 5 | 10 | 12 => 8, _ => 1 }
    };

    let ifd0 = r32(4) as usize;
    let count = r16(ifd0) as usize;
    println!("IFD0 @ {ifd0}, {count} entries:");
    for i in 0..count {
        let e = ifd0 + 2 + i * 12;
        if e + 12 > buf.len() { break; }
        let tag = r16(e);
        let typ = r16(e + 2);
        let cnt = r32(e + 4) as usize;
        let bytelen = cnt * type_size(typ);
        let val_or_off = r32(e + 8);
        // 큰 값(>4B)은 e+8이 오프셋. JPEG 후보: 길이가 크고 오프셋이 파일 내부.
        let is_jpeg_ish = bytelen > 4 && (val_or_off as u64) < flen && bytelen > 1000;
        let mark = if tag == 0x002e { "  <== JpgFromRaw(0x2e)" }
            else if tag == 0x0111 { "  <== StripOffsets" }
            else if tag == 0x0117 { "  <== StripByteCounts" }
            else if is_jpeg_ish { "  <== big blob" } else { "" };
        if tag == 0x002e || is_jpeg_ish || matches!(tag, 0x0111|0x0117|0x0112|0x0201|0x0202) {
            // 오프셋이 1MB 안이면 SOI 확인.
            let soi = if (val_or_off as usize) + 3 < buf.len() {
                format!(" head={:02x}{:02x}{:02x}", buf[val_or_off as usize], buf[val_or_off as usize +1], buf[val_or_off as usize +2])
            } else { String::new() };
            println!("  tag={:#06x} type={typ} count={cnt} bytelen={bytelen} off/val={val_or_off}{soi}{mark}", tag);
        }
    }
}
