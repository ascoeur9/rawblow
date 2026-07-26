//! 진단용: ISO BMFF(CR3/HEIF) 박스 트리 덤프 + Canon CMT1..4 TIFF 블록 조사.
//!
//! 사용: cargo run --release -p rawblow-core --example bmff_dump -- "<file>"

use std::io::Read;

fn main() {
    let path = std::env::args().nth(1).expect("file arg required");
    let mut buf = Vec::new();
    // 앞 4MB만: moov/uuid 메타는 보통 파일 선두에 있다.
    std::fs::File::open(&path)
        .expect("open")
        .take(4 * 1024 * 1024)
        .read_to_end(&mut buf)
        .expect("read");
    walk(&buf, 0, 0, 0);
}

fn walk(b: &[u8], base: u64, depth: usize, _n: usize) {
    let mut i = 0usize;
    while i + 8 <= b.len() {
        let size32 = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as u64;
        let typ = String::from_utf8_lossy(&b[i + 4..i + 8]).to_string();
        let (size, hdr) = if size32 == 1 {
            if i + 16 > b.len() {
                return;
            }
            let s = u64::from_be_bytes(b[i + 8..i + 16].try_into().unwrap());
            (s, 16usize)
        } else if size32 == 0 {
            ((b.len() - i) as u64, 8usize)
        } else {
            (size32, 8usize)
        };
        if size < hdr as u64 {
            return;
        }
        let pad = "  ".repeat(depth);
        let mut extra = String::new();
        if typ == "uuid" && i + hdr + 16 <= b.len() {
            let u = &b[i + hdr..i + hdr + 16];
            extra = format!(" uuid={}", u.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(""));
        }
        println!("{pad}{typ} @{} size={size}{extra}", base + i as u64);

        let body_start = i + hdr;
        let body_end = ((i as u64 + size) as usize).min(b.len());
        if body_end <= body_start {
            i = (i as u64 + size) as usize;
            continue;
        }
        let body = &b[body_start..body_end];

        // Canon CMTn: 통짜 TIFF 블록 → IFD0 엔트리 덤프.
        if typ.starts_with("CMT") {
            dump_tiff(body, depth + 1);
        }
        // 컨테이너 박스는 재귀.
        let container = matches!(
            typ.as_str(),
            "moov" | "trak" | "mdia" | "minf" | "stbl" | "meta" | "dinf" | "udta" | "CRAW" | "stsd"
        );
        if container {
            walk(body, base + body_start as u64, depth + 1, 0);
        } else if typ == "uuid" && body.len() > 16 {
            walk(&body[16..], base + body_start as u64 + 16, depth + 1, 0);
        }
        i = (i as u64 + size) as usize;
    }
}

fn dump_tiff(b: &[u8], depth: usize) {
    let pad = "  ".repeat(depth);
    if b.len() < 8 {
        return;
    }
    let le = match &b[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => {
            println!("{pad}(not tiff: {:02X} {:02X})", b[0], b[1]);
            return;
        }
    };
    let r16 = |o: usize| -> Option<u16> {
        let x = b.get(o..o + 2)?;
        Some(if le { u16::from_le_bytes([x[0], x[1]]) } else { u16::from_be_bytes([x[0], x[1]]) })
    };
    let r32 = |o: usize| -> Option<u32> {
        let x = b.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([x[0], x[1], x[2], x[3]])
        } else {
            u32::from_be_bytes([x[0], x[1], x[2], x[3]])
        })
    };
    let ver = r16(2).unwrap_or(0);
    let ifd0 = r32(4).unwrap_or(0) as usize;
    println!("{pad}TIFF le={le} ver={ver} ifd0={ifd0} len={}", b.len());
    let cnt = r16(ifd0).unwrap_or(0) as usize;
    for k in 0..cnt.min(64) {
        let e = ifd0 + 2 + k * 12;
        let (Some(tag), Some(typ), Some(c), Some(v)) = (r16(e), r16(e + 2), r32(e + 4), r32(e + 8)) else {
            break;
        };
        let inline = r16(e + 8).unwrap_or(0);
        println!("{pad}  tag=0x{tag:04X} type={typ} count={c} val={v} (short={inline})");
    }
}
