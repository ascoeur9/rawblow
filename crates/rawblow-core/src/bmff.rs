//! ISO BMFF 컨테이너(캐논 CR3)의 EXIF 메타 추출.
//!
//! CR3는 TIFF가 아니라 ISO BMFF(MP4 계열)다 — 파일 선두가 `ftyp`/`crx `라서
//! `meta::read_tiff_ifd0_orientation`(II/MM 전제)도, kamadak의
//! `read_from_container`(TIFF·JPEG·PNG·WebP·HEIF만 인식, HEIF는 호환 브랜드
//! `mif1`/`msf1` 필수)도 읽지 못한다. 그 결과 CR3는 **Orientation이 항상 1**로
//! 떨어져 세로 사진이 가로로 표시됐다(EOS R6 Mark III 제보).
//!
//! 캐논은 EXIF를 `moov` 안의 uuid 박스(85c0b687-820f-11e0-8111-f4ce462b6a48)에
//! **통짜 TIFF 블록**으로 넣는다:
//! - `CMT1` = IFD0 (Make/Model/**Orientation(0x0112)**)
//! - `CMT2` = Exif IFD (노출·조리개·ISO·렌즈·촬영시각)
//! - `CMT3` = 캐논 메이커노트 (여기선 쓰지 않음 — 내부 오프셋이 절대값이라 재배치 불가)
//! - `CMT4` = GPS IFD
//!
//! 각 블록은 그 자체로 완결된 TIFF(`II*\0` + IFD 오프셋)지만 **서로 연결돼 있지
//! 않아** kamadak에 그대로 먹이면 Exif IFD 태그가 TIFF 컨텍스트로 잘못 해석된다.
//! 그래서 [`cr3_exif_tiff`]가 세 블록을 한 버퍼에 이어 붙이고 내부 오프셋을 전역
//! 좌표로 재배치한 뒤, IFD0에 ExifIFD(0x8769)·GPSIFD(0x8825) 포인터를 더한
//! **표준 EXIF TIFF**를 합성한다 → 기존 `meta::build()`를 그대로 재사용.
//!
//! I/O는 항상 유계다: 최상위 박스 헤더만 훑어 `moov` 구간을 찾고 그 구간만 읽는다
//! (R6 Mark III 47MB 파일에서 실제로 읽는 양 ≈ 40KB).

use std::path::Path;

/// 캐논 CR3 메타 uuid(`moov` 하위). CMT1..CMT4·THMB를 담는다.
const CANON_META_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];

/// `moov` 박스를 통째로 읽을 때의 상한(정상 CR3는 40~200KB).
const MAX_MOOV: u64 = 16 * 1024 * 1024;
/// 최상위 박스 순회 상한(손상 파일 방어).
const MAX_TOP_BOXES: usize = 64;
/// IFD 엔트리 수 상한(손상 파일 방어).
const MAX_IFD_ENTRIES: usize = 512;

/// 파일 선두 바이트가 ISO BMFF(`....ftyp`)인지. CR3·HEIF·MP4 공통.
pub fn is_bmff(head: &[u8]) -> bool {
    head.len() >= 12 && &head[4..8] == b"ftyp"
}

/// CR3(ISO BMFF)의 Orientation(1..8). CMT1만 훑는 싼 경로 — 디코딩마다 호출되는
/// `meta::orientation`용. BMFF가 아니거나 태그가 없으면 `None`.
pub fn cr3_orientation(path: &Path) -> Option<u16> {
    let blocks = canon_meta_blocks(path)?;
    tiff_ifd0_short(&blocks.cmt1?, 0x0112).filter(|v| (1..=8).contains(v))
}

/// CR3의 CMT1+CMT2+CMT4를 하나의 **표준 EXIF TIFF** 바이트열로 합성한다.
/// kamadak `Reader::read_raw`에 그대로 넘길 수 있다. BMFF가 아니거나 CMT1이
/// 없으면 `None`.
pub fn cr3_exif_tiff(path: &Path) -> Option<Vec<u8>> {
    let b = canon_meta_blocks(path)?;
    assemble(b.cmt1?, b.cmt2, b.cmt4)
}

/// CR3의 **캐논 MakerNote**(CMT3)와 Model 문자열. AF 측거점(#37)용.
///
/// CMT3는 그 자체로 완결된 TIFF 블록이고 **IFD0이 곧 캐논 MakerNote IFD**다(캐논은
/// MakerNote를 헤더 없이 바로 IFD로 쓴다). 값 오프셋도 블록 기준이라 `af.rs`의
/// 기존 캐논 파서에 그대로 먹일 수 있다 — [`cr3_exif_tiff`]가 CMT3를 빼고 합성하는
/// 이유(내부 오프셋 절대값)와 달리, 블록을 **옮기지 않고** 통째로 쓰면 문제없다.
pub fn cr3_makernote(path: &Path) -> Option<(Vec<u8>, String)> {
    let b = canon_meta_blocks(path)?;
    let model = b.cmt1.as_deref().and_then(|c| tiff_ifd0_ascii(c, 0x0110)).unwrap_or_default();
    Some((b.cmt3?, model))
}

/// `moov` 안 캐논 uuid 박스에서 뽑아낸 TIFF 블록들.
struct CanonBlocks {
    cmt1: Option<Vec<u8>>,
    cmt2: Option<Vec<u8>>,
    cmt3: Option<Vec<u8>>,
    cmt4: Option<Vec<u8>>,
}

/// 파일에서 `moov` → 캐논 uuid → CMT1/CMT2/CMT4를 뽑는다. `moov` 구간만 읽는다.
fn canon_meta_blocks(path: &Path) -> Option<CanonBlocks> {
    let file_len = std::fs::metadata(path).ok()?.len();
    let head = read_at(path, 0, 12).ok()?;
    if !is_bmff(&head) {
        return None;
    }
    let (moov_off, moov_len) = find_top_box(path, file_len, b"moov")?;
    if moov_len == 0 || moov_len > MAX_MOOV {
        return None;
    }
    let moov = read_at(path, moov_off, moov_len as usize).ok()?;
    let uuid_body = find_uuid_box(&moov, &CANON_META_UUID)?;
    Some(CanonBlocks {
        cmt1: find_box(uuid_body, b"CMT1").map(|s| s.to_vec()),
        cmt2: find_box(uuid_body, b"CMT2").map(|s| s.to_vec()),
        cmt3: find_box(uuid_body, b"CMT3").map(|s| s.to_vec()),
        cmt4: find_box(uuid_body, b"CMT4").map(|s| s.to_vec()),
    })
}

/// 파일의 [off, off+len) 구간만 읽는다.
fn read_at(path: &Path, off: u64, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    if off > 0 {
        f.seek(SeekFrom::Start(off))?;
    }
    let mut buf = Vec::new();
    f.take(len as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

/// 최상위 박스만 헤더(≤16B)씩 건너뛰며 찾는다 → 거대한 `mdat`를 읽지 않는다.
/// 반환은 **페이로드**의 (오프셋, 길이).
fn find_top_box(path: &Path, file_len: u64, want: &[u8; 4]) -> Option<(u64, u64)> {
    let mut off = 0u64;
    for _ in 0..MAX_TOP_BOXES {
        if off + 8 > file_len {
            return None;
        }
        let h = read_at(path, off, 16).ok()?;
        if h.len() < 8 {
            return None;
        }
        let (size, hdr) = box_size(&h, file_len - off)?;
        if &h[4..8] == want {
            return Some((off + hdr as u64, size - hdr as u64));
        }
        off = off.checked_add(size)?;
    }
    None
}

/// 박스 헤더에서 (전체 크기, 헤더 길이)를 구한다. size==1이면 64비트 확장,
/// size==0이면 "끝까지".
fn box_size(h: &[u8], remaining: u64) -> Option<(u64, usize)> {
    let s32 = u32::from_be_bytes(h.get(0..4)?.try_into().ok()?) as u64;
    let (size, hdr) = match s32 {
        1 => (u64::from_be_bytes(h.get(8..16)?.try_into().ok()?), 16usize),
        0 => (remaining, 8usize),
        n => (n, 8usize),
    };
    (size >= hdr as u64 && size <= remaining).then_some((size, hdr))
}

/// 메모리 버퍼 안에서 같은 레벨의 박스를 찾아 페이로드 슬라이스를 반환.
fn find_box<'a>(buf: &'a [u8], want: &[u8; 4]) -> Option<&'a [u8]> {
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let (size, hdr) = box_size(&buf[i..], (buf.len() - i) as u64)?;
        if &buf[i + 4..i + 8] == want {
            return buf.get(i + hdr..i + size as usize);
        }
        i += size as usize;
    }
    None
}

/// 메모리 버퍼 안에서 지정 uuid의 `uuid` 박스를 찾아, uuid 16B를 뺀 본문을 반환.
fn find_uuid_box<'a>(buf: &'a [u8], want: &[u8; 16]) -> Option<&'a [u8]> {
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let (size, hdr) = box_size(&buf[i..], (buf.len() - i) as u64)?;
        if &buf[i + 4..i + 8] == b"uuid" {
            if let Some(u) = buf.get(i + hdr..i + hdr + 16) {
                if u == want {
                    return buf.get(i + hdr + 16..i + size as usize);
                }
            }
        }
        i += size as usize;
    }
    None
}

// ── TIFF 블록 다루기 ────────────────────────────────────────────────────────

/// TIFF 헤더에서 엔디언(true=LE)을 읽는다.
fn tiff_endian(b: &[u8]) -> Option<bool> {
    match b.get(0..2)? {
        b"II" => Some(true),
        b"MM" => Some(false),
        _ => None,
    }
}

fn rd16(b: &[u8], o: usize, le: bool) -> Option<u16> {
    let x: [u8; 2] = b.get(o..o + 2)?.try_into().ok()?;
    Some(if le { u16::from_le_bytes(x) } else { u16::from_be_bytes(x) })
}

fn rd32(b: &[u8], o: usize, le: bool) -> Option<u32> {
    let x: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
    Some(if le { u32::from_le_bytes(x) } else { u32::from_be_bytes(x) })
}

fn wr32(b: &mut [u8], o: usize, v: u32, le: bool) {
    let x = if le { v.to_le_bytes() } else { v.to_be_bytes() };
    if let Some(s) = b.get_mut(o..o + 4) {
        s.copy_from_slice(&x);
    }
}

/// TIFF 필드 타입별 바이트 크기. 미지의 타입은 `None`.
fn type_size(t: u16) -> Option<usize> {
    Some(match t {
        1 | 2 | 6 | 7 => 1,  // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,          // SHORT, SSHORT
        4 | 9 | 11 | 13 => 4, // LONG, SLONG, FLOAT, IFD
        5 | 10 | 12 => 8,    // RATIONAL, SRATIONAL, DOUBLE
        _ => return None,
    })
}

/// 서브 IFD 포인터 태그(값 자체가 오프셋).
const SUB_IFD_TAGS: [u16; 3] = [0x8769, 0x8825, 0xA005]; // Exif, GPS, Interoperability

/// 통짜 TIFF 블록의 IFD0에서 ASCII 태그 하나를 읽는다(값은 오프셋 저장).
fn tiff_ifd0_ascii(block: &[u8], want: u16) -> Option<String> {
    let le = tiff_endian(block)?;
    let ifd0 = rd32(block, 4, le)? as usize;
    let cnt = (rd16(block, ifd0, le)? as usize).min(MAX_IFD_ENTRIES);
    for i in 0..cnt {
        let e = ifd0 + 2 + i * 12;
        if rd16(block, e, le)? != want {
            continue;
        }
        let n = rd32(block, e + 4, le)? as usize;
        // ASCII는 1바이트/개 — 4바이트를 넘으면 값 필드가 오프셋이다.
        let bytes = if n <= 4 {
            block.get(e + 8..e + 8 + n)?
        } else {
            let off = rd32(block, e + 8, le)? as usize;
            block.get(off..off + n)?
        };
        let s = String::from_utf8_lossy(bytes).trim_end_matches('\0').trim().to_string();
        return (!s.is_empty()).then_some(s);
    }
    None
}

/// 통짜 TIFF 블록의 IFD0에서 SHORT 태그 하나를 읽는다(값은 인라인 저장).
fn tiff_ifd0_short(block: &[u8], want: u16) -> Option<u16> {
    let le = tiff_endian(block)?;
    let ifd0 = rd32(block, 4, le)? as usize;
    let cnt = (rd16(block, ifd0, le)? as usize).min(MAX_IFD_ENTRIES);
    for i in 0..cnt {
        let e = ifd0 + 2 + i * 12;
        if rd16(block, e, le)? == want {
            return rd16(block, e + 8, le);
        }
    }
    None
}

/// CMT1/CMT2/CMT4를 한 버퍼에 이어 붙여 표준 EXIF TIFF를 만든다.
///
/// 각 블록은 그대로 복사한 뒤 [`relocate`]로 내부 오프셋을 `+base` 보정하고,
/// 마지막에 CMT1의 IFD0 엔트리를 복사해 ExifIFD/GPSIFD 포인터를 덧붙인 **새 IFD0**를
/// 버퍼 끝에 쓴다(원본 IFD0 테이블은 참조되지 않는 사표가 되지만 무해).
fn assemble(cmt1: Vec<u8>, cmt2: Option<Vec<u8>>, cmt4: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let le = tiff_endian(&cmt1)?;
    // 엔디언이 섞이면 하나의 TIFF로 합칠 수 없다(실제 CR3는 전부 II).
    let same_endian = |b: &Option<Vec<u8>>| b.as_ref().is_none_or(|x| tiff_endian(x) == Some(le));
    if !same_endian(&cmt2) || !same_endian(&cmt4) {
        return None;
    }

    let mut out: Vec<u8> = Vec::with_capacity(cmt1.len() + 4096);
    out.extend_from_slice(if le { b"II\x2a\x00" } else { b"MM\x00\x2a" });
    out.extend_from_slice(&[0u8; 4]); // IFD0 포인터 — 마지막에 패치.

    let place = |out: &mut Vec<u8>, blk: &[u8]| -> Option<(usize, usize)> {
        if out.len() % 2 == 1 {
            out.push(0); // TIFF 오프셋은 짝수 정렬이 관례.
        }
        let base = out.len();
        let ifd0 = rd32(blk, 4, le)? as usize;
        out.extend_from_slice(blk);
        relocate(out, base, blk.len(), le)?;
        Some((base, base + ifd0))
    };

    let (_, ifd0_abs) = place(&mut out, &cmt1)?;
    let exif_ifd = cmt2.as_deref().and_then(|b| place(&mut out, b)).map(|(_, i)| i);
    let gps_ifd = cmt4.as_deref().and_then(|b| place(&mut out, b)).map(|(_, i)| i);

    // 새 IFD0 = (재배치된) CMT1 엔트리 + ExifIFD/GPSIFD 포인터.
    let cnt = (rd16(&out, ifd0_abs, le)? as usize).min(MAX_IFD_ENTRIES);
    let entries = out.get(ifd0_abs + 2..ifd0_abs + 2 + cnt * 12)?.to_vec();
    // 이미 포인터가 있으면 중복 추가하지 않는다(캐논 CMT1에는 없지만 방어).
    let has = |tag: u16| {
        (0..cnt).any(|i| rd16(&entries, i * 12, le) == Some(tag))
    };
    let extras: Vec<(u16, u32)> = [(0x8769u16, exif_ifd), (0x8825u16, gps_ifd)]
        .into_iter()
        .filter_map(|(tag, off)| off.map(|o| (tag, o as u32)))
        .filter(|(tag, _)| !has(*tag))
        .collect();

    if out.len() % 2 == 1 {
        out.push(0);
    }
    let new_ifd0 = out.len();
    let total = u16::try_from(cnt + extras.len()).ok()?;
    out.extend_from_slice(&if le { total.to_le_bytes() } else { total.to_be_bytes() });
    out.extend_from_slice(&entries);
    for (tag, off) in extras {
        // 태그 오름차순 유지: CMT1의 마지막 태그(0x8298 Copyright) < 0x8769 < 0x8825.
        out.extend_from_slice(&if le { tag.to_le_bytes() } else { tag.to_be_bytes() });
        out.extend_from_slice(&if le { 4u16.to_le_bytes() } else { 4u16.to_be_bytes() }); // LONG
        out.extend_from_slice(&if le { 1u32.to_le_bytes() } else { 1u32.to_be_bytes() }); // count
        out.extend_from_slice(&if le { off.to_le_bytes() } else { off.to_be_bytes() });
    }
    out.extend_from_slice(&[0u8; 4]); // next-IFD = 0

    wr32(&mut out, 4, u32::try_from(new_ifd0).ok()?, le);
    Some(out)
}

/// `out[base..base+len)`에 통째로 복사된 TIFF 블록의 내부 오프셋을 `+base` 보정한다.
///
/// IFD 체인과 서브 IFD(Exif/GPS/Interop)를 따라가며 세 가지를 모두 이동시킨다:
/// 값이 4바이트를 넘어 **오프셋으로 저장된** 엔트리(count×타입크기 > 4), 서브 IFD 포인터,
/// next-IFD 포인터.
///
/// 인라인 값(≤4B)과 미지 타입은 건드리지 않는다 — StripOffsets처럼 블록 밖 이미지 데이터를
/// 가리키는 LONG도 인라인이라 그대로 남는다(kamadak는 값만 읽고 따라가지 않으므로 무해).
fn relocate(out: &mut [u8], base: usize, len: usize, le: bool) -> Option<()> {
    let ifd0 = rd32(out, base + 4, le)? as usize;
    let mut queue = vec![ifd0];
    let mut qi = 0usize;
    let mut visited: Vec<usize> = Vec::new();
    while qi < queue.len() && visited.len() < 8 {
        let ifd = queue[qi];
        qi += 1;
        if visited.contains(&ifd) {
            continue;
        }
        visited.push(ifd);
        let cnt = match rd16(out, base + ifd, le) {
            Some(c) => c as usize,
            None => continue,
        };
        if cnt > MAX_IFD_ENTRIES || ifd + 2 + cnt * 12 + 4 > len {
            continue; // 손상/절단 — 이 IFD는 건너뛴다.
        }
        for i in 0..cnt {
            let e = base + ifd + 2 + i * 12;
            let (tag, typ, c) = (rd16(out, e, le)?, rd16(out, e + 2, le)?, rd32(out, e + 4, le)?);
            let v = rd32(out, e + 8, le)?;
            if SUB_IFD_TAGS.contains(&tag) && typ == 4 && c == 1 {
                queue.push(v as usize);
                wr32(out, e + 8, v.checked_add(base as u32)?, le);
                continue;
            }
            let Some(ts) = type_size(typ) else { continue };
            if (c as usize).saturating_mul(ts) > 4 {
                wr32(out, e + 8, v.checked_add(base as u32)?, le);
            }
        }
        let nx = base + ifd + 2 + cnt * 12;
        if let Some(n) = rd32(out, nx, le) {
            if n != 0 {
                queue.push(n as usize);
                wr32(out, nx, n.checked_add(base as u32)?, le);
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 IFD 엔트리: SHORT(인라인) 또는 ASCII(오프셋 저장).
    enum E<'a> {
        Short(u16, u16),
        Ascii(u16, &'a str),
    }
    impl E<'_> {
        fn tag(&self) -> u16 {
            match self {
                E::Short(t, _) | E::Ascii(t, _) => *t,
            }
        }
    }

    /// 최소 TIFF 블록(CMTn 모사). 엔트리는 태그 오름차순으로 정렬해 쓴다.
    fn tiff_block(entries: &[E]) -> Vec<u8> {
        let mut es: Vec<&E> = entries.iter().collect();
        es.sort_by_key(|e| e.tag());
        let n = es.len();
        let mut b = Vec::new();
        b.extend_from_slice(b"II\x2a\x00");
        b.extend_from_slice(&8u32.to_le_bytes()); // IFD0 @ 8
        b.extend_from_slice(&(n as u16).to_le_bytes());
        // 값 영역은 엔트리 테이블 + next-IFD 포인터 뒤에서 시작.
        let mut value_at = 8 + 2 + n * 12 + 4;
        for e in &es {
            match e {
                E::Short(tag, val) => {
                    b.extend_from_slice(&tag.to_le_bytes());
                    b.extend_from_slice(&3u16.to_le_bytes()); // SHORT
                    b.extend_from_slice(&1u32.to_le_bytes());
                    b.extend_from_slice(&(*val as u32).to_le_bytes()); // 인라인
                }
                E::Ascii(tag, s) => {
                    b.extend_from_slice(&tag.to_le_bytes());
                    b.extend_from_slice(&2u16.to_le_bytes()); // ASCII
                    b.extend_from_slice(&((s.len() + 1) as u32).to_le_bytes());
                    b.extend_from_slice(&(value_at as u32).to_le_bytes()); // 오프셋
                    value_at += s.len() + 1;
                }
            }
        }
        b.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        for e in &es {
            if let E::Ascii(_, s) = e {
                b.extend_from_slice(s.as_bytes());
                b.push(0);
            }
        }
        b
    }

    /// CR3와 같은 박스 구조의 최소 BMFF 파일을 만든다.
    fn cr3_file(cmt1: &[u8], cmt2: Option<&[u8]>) -> Vec<u8> {
        cr3_file_full(cmt1, cmt2, None)
    }

    fn cr3_file_full(cmt1: &[u8], cmt2: Option<&[u8]>, cmt4: Option<&[u8]>) -> Vec<u8> {
        fn boxed(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            v.extend_from_slice(typ);
            v.extend_from_slice(body);
            v
        }
        let mut uuid_body = CANON_META_UUID.to_vec();
        uuid_body.extend(boxed(b"CNCV", b"CanonCR3_001"));
        uuid_body.extend(boxed(b"CMT1", cmt1));
        if let Some(c2) = cmt2 {
            uuid_body.extend(boxed(b"CMT2", c2));
        }
        if let Some(c4) = cmt4 {
            uuid_body.extend(boxed(b"CMT4", c4));
        }
        let moov = boxed(b"moov", &boxed(b"uuid", &uuid_body));
        let mut f = boxed(b"ftyp", b"crx isom");
        f.extend(moov);
        f.extend(boxed(b"mdat", &[0u8; 64])); // 본데이터 자리(읽히면 안 됨)
        f
    }

    /// GPS IFD 블록(CMT4 모사): 위도/경도 각 RATIONAL×3 + 반구 ASCII.
    /// 값이 전부 오프셋 저장이라 [`relocate`]의 재배치가 안 되면 좌표가 깨진다.
    fn gps_block(lat: (u32, u32, u32), lat_ref: &str, lon: (u32, u32, u32), lon_ref: &str) -> Vec<u8> {
        let entries: Vec<(u16, u16, u32, Vec<u8>)> = vec![
            (0x0001, 2, 2, format!("{lat_ref}\0").into_bytes()),
            (0x0002, 5, 3, rationals(&[lat.0, lat.1, lat.2])),
            (0x0003, 2, 2, format!("{lon_ref}\0").into_bytes()),
            (0x0004, 5, 3, rationals(&[lon.0, lon.1, lon.2])),
        ];
        let n = entries.len();
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        b.extend_from_slice(&0x002au16.to_le_bytes());
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&(n as u16).to_le_bytes());
        let mut value_at = 8 + 2 + n * 12 + 4;
        let mut values = Vec::new();
        for (tag, typ, cnt, data) in &entries {
            b.extend_from_slice(&tag.to_le_bytes());
            b.extend_from_slice(&typ.to_le_bytes());
            b.extend_from_slice(&cnt.to_le_bytes());
            if data.len() <= 4 {
                let mut inline = data.clone();
                inline.resize(4, 0);
                b.extend_from_slice(&inline);
            } else {
                b.extend_from_slice(&(value_at as u32).to_le_bytes());
                value_at += data.len();
                values.extend_from_slice(data);
            }
        }
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&values);
        b
    }

    fn rationals(vals: &[u32]) -> Vec<u8> {
        let mut v = Vec::new();
        for x in vals {
            v.extend_from_slice(&x.to_le_bytes());
            v.extend_from_slice(&1u32.to_le_bytes());
        }
        v
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn detects_bmff_header() {
        assert!(is_bmff(b"\0\0\0\x18ftypcrx \0\0\0\x01"));
        assert!(!is_bmff(b"II\x2a\0\x08\0\0\0\0\0\0\0"));
        assert!(!is_bmff(b"short"));
    }

    #[test]
    fn reads_cr3_orientation_from_cmt1() {
        for want in 1..=8u16 {
            let cmt1 = tiff_block(&[E::Ascii(0x0110, "EOS R6m2"), E::Short(0x0112, want)]);
            let p = write_tmp(&format!("rb_cr3_orient_{want}.CR3"), &cr3_file(&cmt1, None));
            assert_eq!(cr3_orientation(&p), Some(want), "orientation {want}");
            // meta::orientation()도 같은 값을 내야 한다(통합 경로).
            assert_eq!(crate::meta::orientation(&p), want);
            let _ = std::fs::remove_file(&p);
        }
    }

    #[test]
    fn rejects_non_bmff_and_out_of_range() {
        let p = write_tmp("rb_cr3_bad.CR3", b"II\x2a\x00\x08\x00\x00\x00\x00\x00");
        assert_eq!(cr3_orientation(&p), None);
        let _ = std::fs::remove_file(&p);
        // 범위 밖(9)은 무시하고 meta::orientation은 1로 폴백.
        let cmt1 = tiff_block(&[E::Short(0x0112, 9)]);
        let p = write_tmp("rb_cr3_range.CR3", &cr3_file(&cmt1, None));
        assert_eq!(cr3_orientation(&p), None);
        assert_eq!(crate::meta::orientation(&p), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn assembles_standard_exif_tiff_readable_by_kamadak() {
        // CMT1: Model(오프셋 저장 ASCII) + Orientation, CMT2: Exif IFD의 ISO·렌즈.
        let cmt1 = tiff_block(&[E::Ascii(0x0110, "Canon EOS R6m2"), E::Short(0x0112, 6)]);
        let cmt2 = tiff_block(&[E::Short(0x8827, 400), E::Ascii(0xA434, "RF24-105mm F4 L IS USM")]);
        let p = write_tmp("rb_cr3_exif.CR3", &cr3_file(&cmt1, Some(&cmt2)));
        let raw = cr3_exif_tiff(&p).expect("assembled tiff");
        let exif = exif::Reader::new().read_raw(raw).expect("kamadak parses assembled tiff");

        let get = |t: exif::Tag| {
            exif.get_field(t, exif::In::PRIMARY).map(|f| f.display_value().to_string())
        };
        assert_eq!(
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0)),
            Some(6)
        );
        // IFD0의 오프셋 저장 ASCII가 재배치 후에도 온전해야 한다.
        assert!(get(exif::Tag::Model).unwrap().contains("Canon EOS R6m2"));
        // Exif IFD(=CMT2)의 태그가 **Exif 컨텍스트**로 읽혀야 한다.
        assert_eq!(
            exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0)),
            Some(400)
        );
        assert!(get(exif::Tag::LensModel).unwrap().contains("RF24-105mm"));
        let _ = std::fs::remove_file(&p);
    }

    /// CMT4(GPS IFD)까지 합성 대상에 포함되는지. GPS 값은 전부 오프셋 저장(RATIONAL×3)이라
    /// [`relocate`]가 블록 이동분을 안 더하면 좌표가 통째로 깨진다 — 실제 좌표로 확인한다.
    #[test]
    fn cr3_gps_survives_block_relocation() {
        let cmt1 = tiff_block(&[E::Ascii(0x0110, "Canon EOS R6 Mark III"), E::Short(0x0112, 1)]);
        let cmt2 = tiff_block(&[E::Short(0x8827, 200)]);
        // 서울 시청 부근: N 37°33'58", E 126°58'41".
        let cmt4 = gps_block((37, 33, 58), "N", (126, 58, 41), "E");
        let p = write_tmp("rb_cr3_gps.CR3", &cr3_file_full(&cmt1, Some(&cmt2), Some(&cmt4)));
        let info = crate::meta::read_exif(&p).expect("cr3 exif");
        let g = info.gps.expect("CMT4 GPS가 합성 TIFF에 연결돼야 한다");
        assert!((g.lat - (37.0 + 33.0 / 60.0 + 58.0 / 3600.0)).abs() < 1e-6, "lat={}", g.lat);
        assert!((g.lon - (126.0 + 58.0 / 60.0 + 41.0 / 3600.0)).abs() < 1e-6, "lon={}", g.lon);
        // Exif IFD(CMT2)도 여전히 살아 있어야 한다(포인터 두 개 동시 추가 회귀 가드).
        assert_eq!(info.iso.as_deref(), Some("200"));
        let _ = std::fs::remove_file(&p);
    }

    /// 남반구·서경(음수 변환)도 정상인지.
    #[test]
    fn cr3_gps_southern_western_hemisphere() {
        let cmt1 = tiff_block(&[E::Short(0x0112, 1)]);
        let cmt4 = gps_block((33, 51, 54), "S", (151, 12, 36), "W");
        let p = write_tmp("rb_cr3_gps_sw.CR3", &cr3_file_full(&cmt1, None, Some(&cmt4)));
        let g = crate::meta::read_exif(&p).expect("cr3 exif").gps.expect("gps");
        assert!(g.lat < 0.0 && g.lon < 0.0, "S/W는 음수여야 한다: {g:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn read_exif_uses_cr3_path() {
        let cmt1 = tiff_block(&[E::Ascii(0x0110, "Canon EOS R6m2"), E::Short(0x0112, 8)]);
        let cmt2 =
            tiff_block(&[E::Short(0x8827, 1600), E::Ascii(0x9003, "2026:07:26 10:11:12")]);
        let p = write_tmp("rb_cr3_readexif.CR3", &cr3_file(&cmt1, Some(&cmt2)));
        let info = crate::meta::read_exif(&p).expect("cr3 exif");
        assert_eq!(info.camera.as_deref(), Some("Canon EOS R6m2"));
        assert_eq!(info.iso.as_deref(), Some("1600"));
        assert_eq!(info.datetime.as_deref(), Some("2026:07:26 10:11:12"));
        let _ = std::fs::remove_file(&p);
    }
}
