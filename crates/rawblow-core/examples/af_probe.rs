//! 진단용: AF 측거점 추출 결과 + 캐논 AFInfo2 원시 헤더 덤프.
//!
//! 사용: cargo run --release -p rawblow-core --example af_probe -- "<folder-or-file>"

use rawblow_core::af::parse_af;
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
        match parse_af(p) {
            Some(af) => {
                let focus = af.points.iter().filter(|q| q.in_focus).count();
                let sel = af.points.iter().filter(|q| q.selected).count();
                println!(
                    "{name}: src={} points={} in_focus={focus} selected={sel}",
                    af.source,
                    af.points.len()
                );
                for q in af.points.iter().filter(|q| q.in_focus || q.selected).take(6) {
                    println!(
                        "    cx={:.3} cy={:.3} w={:.3} h={:.3} focus={} sel={}",
                        q.cx, q.cy, q.w, q.h, q.in_focus, q.selected
                    );
                }
            }
            None => println!("{name}: (no AF)"),
        }
        // CR3면 CMT3의 AFInfo2 원시 헤더도 덤프.
        if let Some((cmt3, model)) = rawblow_core::bmff::cr3_makernote(p) {
            println!("    [CR3] model={model:?} CMT3={}B", cmt3.len());
            dump_afinfo2(&cmt3);
        }
    }
}

/// CMT3(=캐논 MakerNote IFD) 안의 0x0026/0x0012 원시 스트림 앞부분을 덤프.
fn dump_afinfo2(b: &[u8]) {
    let le = matches!(b.get(0..2), Some(b"II"));
    let r16 = |o: usize| -> Option<u16> {
        let x: [u8; 2] = b.get(o..o + 2)?.try_into().ok()?;
        Some(if le { u16::from_le_bytes(x) } else { u16::from_be_bytes(x) })
    };
    let r32 = |o: usize| -> Option<u32> {
        let x: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(x) } else { u32::from_be_bytes(x) })
    };
    let Some(ifd) = r32(4).map(|v| v as usize) else { return };
    let Some(cnt) = r16(ifd) else { return };
    for i in 0..cnt as usize {
        let e = ifd + 2 + i * 12;
        let (Some(tag), Some(typ), Some(c), Some(val)) = (r16(e), r16(e + 2), r32(e + 4), r32(e + 8))
        else {
            break;
        };
        if tag != 0x0026 && tag != 0x0012 {
            continue;
        }
        let off = val as usize;
        let words = c as usize;
        println!("    tag=0x{tag:04X} type={typ} count={c} off={off}");
        let g = |i: usize| r16(off + i * 2).unwrap_or(0);
        if tag == 0x0026 {
            let (num, valid) = (g(2) as usize, g(3) as usize);
            println!(
                "      AFInfo2: size={} mode={} NumAFPoints={num} ValidAFPoints={valid} \
                 CanonImg={}x{} AFImg={}x{}",
                g(0), g(1), g(4), g(5), g(6), g(7)
            );
            let mask = num.div_ceil(16);
            let need = 8 + 4 * num + 2 * mask;
            println!("      필요 워드={need}, 실제 count={words} (충분={})", words >= need);
            let (afw, afh) = (g(6) as f64, g(7) as f64);
            // 배열: W[num] H[num] X[num] Y[num] InFocus[mask] Selected[mask]
            for i in 0..4.min(num) {
                let (w, h) = (g(8 + i) as i16, g(8 + num + i) as i16);
                let (x, y) = (g(8 + 2 * num + i) as i16, g(8 + 3 * num + i) as i16);
                println!(
                    "      [{i}] w={w} h={h} x={x} y={y}  -> cx={:.3} cy={:.3}",
                    0.5 + x as f64 / afw,
                    0.5 - y as f64 / afh
                );
            }
            let pc = |base: usize| -> (usize, Vec<usize>) {
                let mut n = 0;
                let mut idx = Vec::new();
                for i in 0..num {
                    if (g(base + i / 16) >> (i % 16)) & 1 == 1 {
                        n += 1;
                        if idx.len() < 6 {
                            idx.push(i);
                        }
                    }
                }
                (n, idx)
            };
            let (nf, fi) = pc(8 + 4 * num);
            let (ns, si) = pc(8 + 4 * num + mask);
            println!("      InFocus bits={nf} at {fi:?} / Selected bits={ns} at {si:?}");
        } else {
            println!("      AFInfo: NumAFPoints={} Valid={}", g(0), g(1));
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
