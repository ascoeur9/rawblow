//! 진단용: MakerNote 안에서 **렌즈명**을 담을 수 있는 태그를 찾는다.
//! ASCII 태그는 값을 그대로 찍고, 숫자 태그(LensType 등)는 원시값을 보여준다.
//!
//! 사용: cargo run --release -p rawblow-core --example lens_probe -- "<file-or-folder>"

use rawblow_core::model::is_supported;
use std::path::{Path, PathBuf};

struct T<'a> {
    b: &'a [u8],
    le: bool,
}
impl<'a> T<'a> {
    fn new(b: &'a [u8]) -> Option<Self> {
        let le = match b.get(0..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        Some(T { b, le })
    }
    fn u16(&self, o: usize) -> Option<u16> {
        let s = self.b.get(o..o + 2)?;
        Some(if self.le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let s = self.b.get(o..o + 4)?;
        Some(if self.le {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    }
    fn unit(typ: u16) -> usize {
        match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => 1,
        }
    }
    /// IFD 엔트리를 (tag, typ, count, 데이터 오프셋) 목록으로.
    fn entries(&self, ifd: usize, base: usize) -> Vec<(u16, u16, usize, usize)> {
        let mut out = Vec::new();
        let Some(n) = self.u16(ifd) else { return out };
        for i in 0..(n as usize).min(512) {
            let e = ifd + 2 + i * 12;
            let (Some(tag), Some(typ), Some(c), Some(v)) =
                (self.u16(e), self.u16(e + 2), self.u32(e + 4), self.u32(e + 8))
            else {
                break;
            };
            let len = c as usize * Self::unit(typ);
            let off = if len <= 4 { e + 8 } else { base + v as usize };
            out.push((tag, typ, c as usize, off));
        }
        out
    }
    fn ascii(&self, off: usize, len: usize) -> String {
        // 첫 NUL에서 자른다 — CR3의 0x0095처럼 종단 뒤에 쓰레기가 붙는 필드가 있다.
        let s = self.b.get(off..off + len).unwrap_or(&[]);
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        String::from_utf8_lossy(&s[..end]).trim().to_string()
    }
}

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
        println!("== {} ==", p.file_name().unwrap().to_string_lossy());
        match rawblow_core::meta::read_exif(p) {
            Some(e) => println!("  read_exif lens = {:?}  camera={:?}", e.lens, e.camera),
            None => println!("  read_exif: None"),
        }
        probe(p);
    }
}

fn probe(path: &Path) {
    // CR3는 CMT3가 곧 MakerNote IFD.
    if let Some((cmt3, model)) = rawblow_core::bmff::cr3_makernote(path) {
        println!("  [CR3] model={model}");
        if let Some(t) = T::new(&cmt3) {
            if let Some(ifd) = t.u32(4) {
                dump(&t, ifd as usize, 0, "CMT3");
            }
        }
        return;
    }
    let Ok(buf) = read_prefix(path, 4 * 1024 * 1024) else { return };
    // 컨테이너 TIFF(비표준 매직 허용) 또는 임베디드 JPEG의 APP1.
    let mut tiffs: Vec<(usize, Vec<u8>)> = Vec::new();
    if matches!(buf.get(0..2), Some(b"II") | Some(b"MM")) {
        let mut fixed = buf.clone();
        // ORF의 'RO'/'RS' 버전을 42로 표준화(meta.rs와 동일 트릭).
        if fixed.len() > 4 {
            let ver = if fixed[0] == b'I' {
                u16::from_le_bytes([fixed[2], fixed[3]])
            } else {
                u16::from_be_bytes([fixed[2], fixed[3]])
            };
            if ver == 0x4F52 || ver == 0x5352 {
                if fixed[0] == b'I' {
                    fixed[2] = 0x2A;
                    fixed[3] = 0x00;
                } else {
                    fixed[2] = 0x00;
                    fixed[3] = 0x2A;
                }
            }
        }
        tiffs.push((0, fixed));
    }
    let mut i = 0usize;
    while i + 3 < buf.len() && tiffs.len() < 4 {
        if buf[i] == 0xFF && buf[i + 1] == 0xD8 && buf[i + 2] == 0xFF {
            if let Some(off) = app1_tiff_off(&buf[i..]) {
                tiffs.push((0, buf[i + off..].to_vec()));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    for (_, tiff) in &tiffs {
        let Some(t) = T::new(tiff) else { continue };
        let Some(ifd0) = t.u32(4).map(|v| v as usize) else { continue };
        let e0 = t.entries(ifd0, 0);
        let make = e0
            .iter()
            .find(|x| x.0 == 0x010F)
            .map(|x| t.ascii(x.3, x.2))
            .unwrap_or_default();
        let Some(exif_ifd) = e0.iter().find(|x| x.0 == 0x8769).and_then(|x| t.u32(x.3)) else {
            continue;
        };
        let ee = t.entries(exif_ifd as usize, 0);
        // 표준 렌즈 태그: LensMake/LensModel/LensSerial(ASCII), LensSpecification(RATIONAL×4).
        for (tag, name) in [(0xA434u16, "Exif.LensModel"), (0xA433, "Exif.LensMake"), (0xA435, "Exif.LensSerial")] {
            if let Some(x) = ee.iter().find(|x| x.0 == tag) {
                println!("    {name} = {:?}", t.ascii(x.3, x.2));
            }
        }
        if let Some(x) = ee.iter().find(|x| x.0 == 0xA432) {
            let r: Vec<String> = (0..x.2.min(4))
                .map(|i| {
                    let n = t.u32(x.3 + i * 8).unwrap_or(0) as f64;
                    let d = t.u32(x.3 + i * 8 + 4).unwrap_or(1).max(1) as f64;
                    format!("{:.1}", n / d)
                })
                .collect();
            println!("    Exif.LensSpecification = [{}]  (minFocal,maxFocal,minF,maxF)", r.join(", "));
        }
        let Some(mn) = ee.iter().find(|x| x.0 == 0x927C) else { continue };
        println!("  make={make:?} MakerNote @{} len={}", mn.3, mn.2);
        dump_makernote(&t, mn.3, &make);
        return;
    }
    println!("  (MakerNote를 못 찾음)");
}

fn dump_makernote(t: &T, mn: usize, make: &str) {
    let head = t.b.get(mn..mn + 12.min(t.b.len() - mn)).unwrap_or(&[]);
    if head.starts_with(b"OLYMP\0") {
        // 구형(E-300 등): "OLYMP\0"+2바이트 뒤부터 IFD, 값 오프셋은 파일 TIFF 베이스(0) 기준.
        dump(t, mn + 8, 0, "Olympus MN (old OLYMP\\0)");
        return;
    }
    if head.starts_with(b"OLYMPUS\0") || head.starts_with(b"OM SYSTEM\0") {
        let sig = if head.starts_with(b"OLYMPUS\0") { 8 } else { 12 };
        let ifd = mn + sig + 4;
        dump(t, ifd, mn, "Olympus MN");
        // Equipment 서브IFD(0x2010)
        for (tag, _, _, off) in t.entries(ifd, mn) {
            if tag == 0x2010 {
                if let Some(sub) = t.u32(off) {
                    dump(t, mn + sub as usize, mn, "Olympus Equipment(0x2010)");
                }
            }
        }
        return;
    }
    if head.starts_with(b"PENTAX \0") {
        let le = matches!(t.b.get(mn + 8..mn + 10), Some(b"II"));
        let p = T { b: t.b, le };
        dump(&p, mn + 10, mn, "Pentax MN");
        return;
    }
    if make.starts_with("Canon") {
        dump(t, mn, 0, "Canon MN");
        return;
    }
    dump(t, mn, 0, "MN");
}

/// IFD 엔트리 덤프 — ASCII는 값까지, 숫자는 앞 몇 개만.
fn dump(t: &T, ifd: usize, base: usize, label: &str) {
    let es = t.entries(ifd, base);
    println!("    [{label}] {} entries", es.len());
    for (tag, typ, cnt, off) in es {
        if typ == 2 && cnt > 1 {
            let s = t.ascii(off, cnt);
            if !s.is_empty() {
                println!("      tag=0x{tag:04X} ASCII[{cnt}] = {s:?}");
            }
        } else if matches!(typ, 1 | 3 | 4 | 7 | 8) && cnt <= 64 {
            let vals: Vec<i64> = (0..cnt.min(40))
                .filter_map(|i| match typ {
                    3 | 8 => t.u16(off + i * 2).map(|v| v as i64),
                    4 => t.u32(off + i * 4).map(|v| v as i64),
                    _ => t.b.get(off + i).map(|v| *v as i64),
                })
                .collect();
            // 렌즈 관련 후보 태그만 강조.
            let hot = matches!(tag, 0x0095 | 0x0016 | 0x003F | 0x0207 | 0x0201 | 0x001D | 0x0202);
            if hot || cnt <= 8 {
                println!(
                    "      tag=0x{tag:04X} type={typ} cnt={cnt} {}{vals:?}",
                    if hot { "★ " } else { "" }
                );
            }
        } else if typ == 3 && cnt > 64 {
            // CameraSettings(0x0001) 같은 긴 int16 배열은 앞 32개만.
            let vals: Vec<u16> = (0..32).filter_map(|i| t.u16(off + i * 2)).collect();
            println!("      tag=0x{tag:04X} type=3 cnt={cnt} head32={vals:?}");
        }
    }
}

fn app1_tiff_off(jpeg: &[u8]) -> Option<usize> {
    let mut i = 2usize;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let marker = jpeg[i + 1];
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = ((jpeg[i + 2] as usize) << 8) | jpeg[i + 3] as usize;
        if marker == 0xE1 && len >= 8 && jpeg.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            return Some(i + 10);
        }
        i += 2 + len;
    }
    None
}

fn read_prefix(p: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut b = Vec::new();
    std::fs::File::open(p)?.take(max as u64).read_to_end(&mut b)?;
    Ok(b)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
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
