//! HEIF 컨테이너 항목 색인 — JPEG·썸네일 항목을 골라 낸다(#114).
//!
//! `heif-oxide`는 primary HEVC/`grid`만 풀고 JPEG-in-HEIF는 거절한다. iPhone HEIC는
//! 보통 작은 `thmb`(hvc1 한 장)와 가끔 `jpeg` 항목을 들고 있는데, 그걸 쓰면 그리드·컬링이
//! 12MP 타일 HEVC(~1s)를 피할 수 있다. 박스가 깨져 있으면 `None` — 호출부가 본 이미지로 폴백.

use std::collections::HashMap;

/// 악성·손상 파일 방어 상한(#114). 실제 iPhone/카메라 HEIC는 항목 수십~수백, 항목당 조각
/// 수 개 수준이다. 넘으면 색인을 포기하고(None) 호출부가 본 이미지 디코드로 폴백한다.
const MAX_ITEMS: u32 = 4096;
const MAX_EXTENTS_PER_ITEM: u16 = 64;
const MAX_TOTAL_EXTENTS: usize = 16 * 1024;
const MAX_REFS: usize = 4096;
const MAX_PROPS: usize = 4096;
const MAX_ASSOC_PER_ITEM: u8 = 64;

/// HEIF 변환 속성(표시 전에 적용). `irot`는 반시계 90° 단위. `imir` 축은 ISO/IEC 23008-12:2022
/// §6.5.12 기준 **0 = 위아래 교환(상하 반전, EXIF 4), 1 = 좌우 교환(좌우 반전, EXIF 2)** —
/// libheif·libavif와 같은 해석. ipma에 나열된 순서대로 적용한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    RotCcw(u8),
    Mirror(u8),
}

#[derive(Clone, Debug)]
struct ItemLoc {
    construction_method: u8,
    data_reference_index: u16,
    base_offset: u64,
    extents: Vec<(u64, u64)>,
}

#[derive(Clone, Copy, Debug)]
struct ItemInfo {
    typ: [u8; 4],
    protection: u16,
}

/// 파일에서 읽은 HEIF 항목 목록. JPEG 바이트와 `pitm` 패치에 쓴다.
#[derive(Debug)]
pub struct HeifIndex {
    pub primary: u32,
    pitm_id_offset: usize,
    pitm_id_len: u8,
    items: HashMap<u32, ItemInfo>,
    locations: HashMap<u32, ItemLoc>,
    idat: Option<(usize, usize)>,
    pub jpeg_ids: Vec<u32>,
    pub thumb_ids: Vec<u32>,
    /// 항목별 변환 속성(ipma 순서). 없으면 회전 없음.
    transforms: HashMap<u32, Vec<Transform>>,
}

impl HeifIndex {
    pub fn is_jpeg(&self, id: u32) -> bool {
        self.items
            .get(&id)
            .is_some_and(|i| i.protection == 0 && (i.typ == *b"jpeg" || i.typ == *b"jpg "))
    }

    /// 항목의 irot/imir를 합성한 EXIF Orientation(1..8). `decode_jpeg_scaled`의 orient로 넘기면
    /// heif-oxide가 HEVC에 해 주는 것과 같은 방향으로 선다.
    pub fn exif_orientation(&self, id: u32) -> u16 {
        self.transforms.get(&id).map(|t| compose_to_exif(t)).unwrap_or(1)
    }

    /// 이 항목을 heif-oxide로 풀었을 때 규격 방향으로 맞추는 보정(imir 축 해석 차이, [`hevc_mirror_fix`]).
    pub fn hevc_orientation_fix(&self, id: u32) -> u16 {
        self.transforms.get(&id).map(|t| hevc_mirror_fix(t)).unwrap_or(1)
    }

    pub fn is_hevc_image(&self, id: u32) -> bool {
        self.items
            .get(&id)
            .is_some_and(|i| i.protection == 0 && (i.typ == *b"hvc1" || i.typ == *b"grid"))
    }

    pub fn item_bytes(&self, data: &[u8], id: u32) -> Option<Vec<u8>> {
        let info = self.items.get(&id)?;
        if info.protection != 0 {
            return None;
        }
        let loc = self.locations.get(&id)?;
        if loc.data_reference_index != 0 {
            return None;
        }
        let source: &[u8] = match loc.construction_method {
            0 => data,
            1 => {
                let (s, e) = self.idat?;
                data.get(s..e)?
            }
            _ => return None,
        };
        let mut out = Vec::new();
        for &(off, len) in &loc.extents {
            let start = usize::try_from(loc.base_offset.checked_add(off)?).ok()?;
            let end = if len == 0 {
                source.len()
            } else {
                start.checked_add(usize::try_from(len).ok()?)?
            };
            let part = source.get(start..end)?;
            // 같은 구간을 여러 번 가리키는 조각으로 파일보다 큰 출력을 만들지 못하게(메모리 폭발 방지).
            if out.len() + part.len() > source.len() {
                return None;
            }
            out.extend_from_slice(part);
        }
        Some(out)
    }

    /// `pitm`의 항목 id를 `new_id`로 덮어쓴다. heif-oxide가 그 항목을 primary로 풀게.
    pub fn patch_pitm(&self, data: &mut [u8], new_id: u32) -> bool {
        let off = self.pitm_id_offset;
        match self.pitm_id_len {
            2 => {
                if new_id > u16::MAX as u32 {
                    return false;
                }
                if off + 2 > data.len() {
                    return false;
                }
                data[off..off + 2].copy_from_slice(&(new_id as u16).to_be_bytes());
                true
            }
            4 => {
                if off + 4 > data.len() {
                    return false;
                }
                data[off..off + 4].copy_from_slice(&new_id.to_be_bytes());
                true
            }
            _ => false,
        }
    }
}

/// `ftyp`가 HEIF 계열이고 `meta`가 있으면 항목을 읽는다. 아니면 `None`.
pub fn index(data: &[u8]) -> Option<HeifIndex> {
    if data.len() < 12 || &data[4..8] != b"ftyp" {
        return None;
    }
    let mut primary = 0u32;
    let mut pitm_id_offset = 0usize;
    let mut pitm_id_len = 0u8;
    let mut saw_pitm = false;
    let mut items: HashMap<u32, ItemInfo> = HashMap::new();
    let mut locations: HashMap<u32, ItemLoc> = HashMap::new();
    let mut idat = None;
    let mut refs: Vec<([u8; 4], u32, Vec<u32>)> = Vec::new();
    let mut transforms: HashMap<u32, Vec<Transform>> = HashMap::new();

    let mut pos = 0usize;
    while let Some((_, fourcc, bs, be)) = next_box(data, pos, data.len()) {
        if &fourcc == b"meta" {
            if be.saturating_sub(bs) < 4 {
                return None;
            }
            parse_meta_children(
                data,
                bs + 4,
                be,
                &mut primary,
                &mut pitm_id_offset,
                &mut pitm_id_len,
                &mut saw_pitm,
                &mut items,
                &mut locations,
                &mut idat,
                &mut refs,
                &mut transforms,
            )?;
        }
        // next_box는 be > pos(최소 8바이트 헤더)를 보장하므로 항상 전진한다. 빈 박스(be==bs)도
        // 형제 박스 파싱을 끊지 않는다.
        pos = be;
    }
    if !saw_pitm || items.is_empty() {
        return None;
    }

    let jpeg_ids: Vec<u32> = items
        .iter()
        .filter(|(_, i)| i.protection == 0 && (i.typ == *b"jpeg" || i.typ == *b"jpg "))
        .map(|(&id, _)| id)
        .collect();

    let mut thumb_ids = Vec::new();
    for (kind, from, to) in &refs {
        if kind != b"thmb" {
            continue;
        }
        if *from == primary {
            for id in to {
                if !thumb_ids.contains(id) {
                    thumb_ids.push(*id);
                }
            }
        }
        if to.contains(&primary) && !thumb_ids.contains(from) {
            thumb_ids.push(*from);
        }
    }

    Some(HeifIndex {
        primary,
        pitm_id_offset,
        pitm_id_len,
        items,
        locations,
        idat,
        jpeg_ids,
        thumb_ids,
        transforms,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_meta_children(
    data: &[u8],
    start: usize,
    end: usize,
    primary: &mut u32,
    pitm_off: &mut usize,
    pitm_len: &mut u8,
    saw_pitm: &mut bool,
    items: &mut HashMap<u32, ItemInfo>,
    locations: &mut HashMap<u32, ItemLoc>,
    idat: &mut Option<(usize, usize)>,
    refs: &mut Vec<([u8; 4], u32, Vec<u32>)>,
    transforms: &mut HashMap<u32, Vec<Transform>>,
) -> Option<()> {
    let mut pos = start;
    while let Some((_, fourcc, bs, be)) = next_box(data, pos, end) {
        match &fourcc {
            b"pitm" => {
                let mut c = Cur::new(data, bs, be);
                let ver = c.fullbox()?;
                *pitm_off = c.pos;
                if ver == 0 {
                    *primary = c.u16()? as u32;
                    *pitm_len = 2;
                } else {
                    *primary = c.u32()?;
                    *pitm_len = 4;
                }
                *saw_pitm = true;
            }
            b"iinf" => parse_iinf(data, bs, be, items)?,
            b"iloc" => parse_iloc(data, bs, be, locations)?,
            b"iref" => parse_iref(data, bs, be, refs)?,
            b"idat" => *idat = Some((bs, be)),
            // 속성은 회전 표시용 부가정보 — 깨져 있어도 색인 자체는 살린다(회전 없음으로).
            b"iprp" => {
                if let Some(t) = parse_iprp(data, bs, be) {
                    *transforms = t;
                }
            }
            _ => {}
        }
        // next_box는 be > pos(최소 8바이트 헤더)를 보장하므로 항상 전진한다. 빈 박스(be==bs)도
        // 형제 박스 파싱을 끊지 않는다.
        pos = be;
    }
    Some(())
}

fn parse_iinf(data: &[u8], start: usize, end: usize, items: &mut HashMap<u32, ItemInfo>) -> Option<()> {
    let mut c = Cur::new(data, start, end);
    let ver = c.fullbox()?;
    let _count = if ver == 0 { c.u16()? as u32 } else { c.u32()? };
    let mut pos = c.pos;
    while let Some((_, fourcc, bs, be)) = next_box(data, pos, end) {
        if &fourcc == b"infe" {
            let mut r = Cur::new(data, bs, be);
            let ver = r.fullbox()?;
            if ver >= 2 {
                let id = if ver == 2 { r.u16()? as u32 } else { r.u32()? };
                let protection = r.u16()?;
                let typ = r.fourcc()?;
                items.insert(id, ItemInfo { typ, protection });
                if items.len() > MAX_ITEMS as usize {
                    return None;
                }
            }
        }
        // next_box는 be > pos(최소 8바이트 헤더)를 보장하므로 항상 전진한다. 빈 박스(be==bs)도
        // 형제 박스 파싱을 끊지 않는다.
        pos = be;
    }
    Some(())
}

fn parse_iloc(
    data: &[u8],
    start: usize,
    end: usize,
    locations: &mut HashMap<u32, ItemLoc>,
) -> Option<()> {
    let mut c = Cur::new(data, start, end);
    let ver = c.fullbox()?;
    if ver > 2 {
        return None;
    }
    let b = c.u8()?;
    let offset_size = b >> 4;
    let length_size = b & 0xF;
    let b = c.u8()?;
    let base_offset_size = b >> 4;
    let index_size = if ver >= 1 { b & 0xF } else { 0 };
    let item_count = if ver < 2 { c.u16()? as u32 } else { c.u32()? };
    if item_count > MAX_ITEMS {
        return None;
    }
    let mut total_extents = 0usize;
    for _ in 0..item_count {
        let item_id = if ver < 2 { c.u16()? as u32 } else { c.u32()? };
        let construction_method = if ver >= 1 { (c.u16()? & 0xF) as u8 } else { 0 };
        let data_reference_index = c.u16()?;
        let base_offset = c.uint_sized(base_offset_size)?;
        let extent_count = c.u16()?;
        total_extents += extent_count as usize;
        if extent_count > MAX_EXTENTS_PER_ITEM || total_extents > MAX_TOTAL_EXTENTS {
            return None;
        }
        let mut extents = Vec::new();
        for _ in 0..extent_count {
            if index_size > 0 {
                let _ = c.uint_sized(index_size)?;
            }
            let off = c.uint_sized(offset_size)?;
            let len = c.uint_sized(length_size)?;
            extents.push((off, len));
        }
        locations.insert(
            item_id,
            ItemLoc {
                construction_method,
                data_reference_index,
                base_offset,
                extents,
            },
        );
    }
    Some(())
}

fn parse_iref(
    data: &[u8],
    start: usize,
    end: usize,
    refs: &mut Vec<([u8; 4], u32, Vec<u32>)>,
) -> Option<()> {
    let mut c = Cur::new(data, start, end);
    let ver = c.fullbox()?;
    let mut pos = c.pos;
    while let Some((_, fourcc, bs, be)) = next_box(data, pos, end) {
        let mut r = Cur::new(data, bs, be);
        let from = if ver == 0 { r.u16()? as u32 } else { r.u32()? };
        let count = r.u16()?;
        let mut to = Vec::new();
        for _ in 0..count {
            to.push(if ver == 0 { r.u16()? as u32 } else { r.u32()? });
        }
        refs.push((fourcc, from, to));
        if refs.len() > MAX_REFS {
            return None;
        }
        // next_box는 be > pos(최소 8바이트 헤더)를 보장하므로 항상 전진한다. 빈 박스(be==bs)도
        // 형제 박스 파싱을 끊지 않는다.
        pos = be;
    }
    Some(())
}

/// `iprp` → `ipco`(속성 목록, 1부터 번호) + `ipma`(항목 → 속성 번호들)에서 항목별 변환만 뽑는다.
fn parse_iprp(data: &[u8], start: usize, end: usize) -> Option<HashMap<u32, Vec<Transform>>> {
    let mut props: Vec<Option<Transform>> = Vec::new();
    let mut assoc: Vec<(u32, Vec<u16>)> = Vec::new();
    let mut pos = start;
    while let Some((_, fourcc, bs, be)) = next_box(data, pos, end) {
        match &fourcc {
            b"ipco" => {
                let mut p = bs;
                while let Some((_, pc, pbs, pbe)) = next_box(data, p, be) {
                    let t = match &pc {
                        // 본문 없는 빈 박스는 다음 박스 바이트를 읽지 않게 건너뛴다.
                        b"irot" if pbs < pbe => Some(Transform::RotCcw(data[pbs] & 3)),
                        b"imir" if pbs < pbe => Some(Transform::Mirror(data[pbs] & 1)),
                        _ => None,
                    };
                    props.push(t);
                    if props.len() > MAX_PROPS {
                        return None;
                    }
                    p = pbe;
                }
            }
            b"ipma" => {
                let mut c = Cur::new(data, bs, be);
                let ver = c.u8()?;
                let flags = u32::from_be_bytes([0, c.u8()?, c.u8()?, c.u8()?]);
                let count = c.u32()?;
                if count > MAX_ITEMS {
                    return None;
                }
                for _ in 0..count {
                    let id = if ver < 1 { c.u16()? as u32 } else { c.u32()? };
                    let n = c.u8()?;
                    if n > MAX_ASSOC_PER_ITEM {
                        return None;
                    }
                    let mut idx = Vec::new();
                    for _ in 0..n {
                        let v = if flags & 1 != 0 { c.u16()? & 0x7FFF } else { (c.u8()? & 0x7F) as u16 };
                        idx.push(v);
                    }
                    assoc.push((id, idx));
                }
            }
            _ => {}
        }
        pos = be;
    }
    let mut out: HashMap<u32, Vec<Transform>> = HashMap::new();
    for (id, idx) in assoc {
        let t: Vec<Transform> = idx
            .iter()
            .filter_map(|&i| props.get((i as usize).checked_sub(1)?).copied().flatten())
            .collect();
        if !t.is_empty() {
            out.insert(id, t);
        }
    }
    Some(out)
}

type Grid = (usize, usize, Vec<u8>); // (w, h, row-major) 표식 격자

fn grid_at(g: &Grid, x: usize, y: usize) -> u8 {
    g.2[y * g.0 + x]
}
fn grid_rot_cw(g: &Grid) -> Grid {
    let (w, h) = (g.0, g.1);
    let mut v = Vec::with_capacity(w * h);
    for y in 0..w {
        for x in 0..h {
            v.push(grid_at(g, y, h - 1 - x));
        }
    }
    (h, w, v)
}
fn grid_fliph(g: &Grid) -> Grid {
    let mut v = Vec::with_capacity(g.2.len());
    for y in 0..g.1 {
        for x in 0..g.0 {
            v.push(grid_at(g, g.0 - 1 - x, y));
        }
    }
    (g.0, g.1, v)
}
fn grid_flipv(g: &Grid) -> Grid {
    let mut v = Vec::with_capacity(g.2.len());
    for y in 0..g.1 {
        for x in 0..g.0 {
            v.push(grid_at(g, x, g.1 - 1 - y));
        }
    }
    (g.0, g.1, v)
}
/// decode.rs `apply_orientation`과 같은 EXIF 정의(image `rotate90` = 시계 방향).
fn grid_exif(g: &Grid, o: u16) -> Grid {
    match o {
        2 => grid_fliph(g),
        3 => grid_rot_cw(&grid_rot_cw(g)),
        4 => grid_flipv(g),
        5 => grid_fliph(&grid_rot_cw(g)),
        6 => grid_rot_cw(g),
        7 => grid_fliph(&grid_rot_cw(&grid_rot_cw(&grid_rot_cw(g)))),
        8 => grid_rot_cw(&grid_rot_cw(&grid_rot_cw(g))),
        _ => g.clone(),
    }
}
fn grid_base() -> Grid {
    (3, 2, vec![0, 1, 2, 3, 4, 5])
}
/// 변환을 차례로 적용. `spec_mirror`면 규격(imir 0=상하), 아니면 heif-oxide 0.1 해석(imir 0=좌우).
fn grid_apply(ts: &[Transform], spec_mirror: bool) -> Grid {
    ts.iter().fold(grid_base(), |g, t| match *t {
        // 반시계 k번 = 시계 (4-k)번.
        Transform::RotCcw(k) => (0..(4 - k as usize % 4) % 4).fold(g, |acc, _| grid_rot_cw(&acc)),
        Transform::Mirror(axis) => {
            if (axis == 0) == spec_mirror {
                grid_flipv(&g)
            } else {
                grid_fliph(&g)
            }
        }
    })
}

/// 변환 목록을 EXIF Orientation 하나로 합성한다. 3×2 표식 격자에 변환을 차례로 적용한 결과를
/// EXIF 8가지와 대조 — 합성 규칙을 손으로 외우지 않는다.
pub fn compose_to_exif(ts: &[Transform]) -> u16 {
    let want = grid_apply(ts, true);
    (1..=8).find(|&o| grid_exif(&grid_base(), o) == want).unwrap_or(1)
}

/// heif-oxide 0.1은 imir 축을 규격과 반대로(0=좌우) 적용한다. 그 결과에 추가로 걸면 규격대로
/// 서게 되는 EXIF 보정값. imir가 없으면 1(보정 없음).
pub fn hevc_mirror_fix(ts: &[Transform]) -> u16 {
    if !ts.iter().any(|t| matches!(t, Transform::Mirror(_))) {
        return 1;
    }
    let got = grid_apply(ts, false);
    let want = grid_apply(ts, true);
    (1..=8).find(|&o| grid_exif(&got, o) == want).unwrap_or(1)
}

fn next_box(data: &[u8], pos: usize, limit: usize) -> Option<(usize, [u8; 4], usize, usize)> {
    if pos + 8 > limit || pos + 8 > data.len() {
        return None;
    }
    let s32 = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?);
    let fourcc = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
    let (hdr, size) = match s32 {
        1 => {
            if pos + 16 > limit || pos + 16 > data.len() {
                return None;
            }
            let sz = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?);
            (16usize, sz)
        }
        0 => (8usize, (limit - pos) as u64),
        n => (8usize, n as u64),
    };
    if size < hdr as u64 {
        return None;
    }
    let end = pos.checked_add(size as usize)?;
    if end > limit || end > data.len() {
        return None;
    }
    Some((pos, fourcc, pos + hdr, end))
}

struct Cur<'a> {
    data: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Cur<'a> {
    fn new(data: &'a [u8], pos: usize, end: usize) -> Self {
        Cur { data, pos, end }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.pos;
        let e = s.checked_add(n)?;
        if e > self.end || e > self.data.len() {
            return None;
        }
        self.pos = e;
        Some(&self.data[s..e])
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    fn fourcc(&mut self) -> Option<[u8; 4]> {
        self.take(4)?.try_into().ok()
    }
    fn fullbox(&mut self) -> Option<u8> {
        let b = self.take(4)?;
        Some(b[0])
    }
    fn uint_sized(&mut self, sz: u8) -> Option<u64> {
        match sz {
            0 => Some(0),
            1 => self.u8().map(|v| v as u64),
            2 => self.u16().map(|v| v as u64),
            4 => self.u32().map(|v| v as u64),
            8 => self.u64(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(body);
        v
    }
    fn full(ver: u8, rest: &[u8]) -> Vec<u8> {
        let mut b = vec![ver, 0, 0, 0];
        b.extend_from_slice(rest);
        b
    }

    /// primary=`hvc1`(쓰레기) + `thmb`→JPEG. HEVC를 안 풀어도 JPEG를 꺼낼 수 있어야 한다.
    fn build_hvc1_with_jpeg_thumb(jpeg: &[u8]) -> Vec<u8> {
        let garbage = b"not-hevc";
        let mut infe1 = full(2, &[]);
        infe1.extend_from_slice(&1u16.to_be_bytes());
        infe1.extend_from_slice(&0u16.to_be_bytes());
        infe1.extend_from_slice(b"hvc1");
        infe1.push(0);
        let mut infe2 = full(2, &[]);
        infe2.extend_from_slice(&2u16.to_be_bytes());
        infe2.extend_from_slice(&0u16.to_be_bytes());
        infe2.extend_from_slice(b"jpeg");
        infe2.push(0);
        let mut iinf_body = full(0, &2u16.to_be_bytes());
        iinf_body.extend_from_slice(&bx(b"infe", &infe1));
        iinf_body.extend_from_slice(&bx(b"infe", &infe2));
        let iinf = bx(b"iinf", &iinf_body);

        let pitm = bx(b"pitm", &full(0, &1u16.to_be_bytes()));

        let mut thmb = Vec::new();
        thmb.extend_from_slice(&1u16.to_be_bytes());
        thmb.extend_from_slice(&1u16.to_be_bytes());
        thmb.extend_from_slice(&2u16.to_be_bytes());
        let mut iref_body = full(0, &[]);
        iref_body.extend_from_slice(&bx(b"thmb", &thmb));
        let iref = bx(b"iref", &iref_body);

        // iloc placeholder — 오프셋은 조립 후 패치.
        let mut iloc_rest = vec![0x44, 0x00]; // offset/length 4, base 0
        iloc_rest.extend_from_slice(&2u16.to_be_bytes());
        // item 1
        iloc_rest.extend_from_slice(&1u16.to_be_bytes());
        iloc_rest.extend_from_slice(&0u16.to_be_bytes());
        iloc_rest.extend_from_slice(&1u16.to_be_bytes());
        let off1_at = iloc_rest.len();
        iloc_rest.extend_from_slice(&0u32.to_be_bytes());
        iloc_rest.extend_from_slice(&(garbage.len() as u32).to_be_bytes());
        // item 2
        iloc_rest.extend_from_slice(&2u16.to_be_bytes());
        iloc_rest.extend_from_slice(&0u16.to_be_bytes());
        iloc_rest.extend_from_slice(&1u16.to_be_bytes());
        let off2_at = iloc_rest.len();
        iloc_rest.extend_from_slice(&0u32.to_be_bytes());
        iloc_rest.extend_from_slice(&(jpeg.len() as u32).to_be_bytes());
        let iloc_body = full(0, &iloc_rest);
        let iloc = bx(b"iloc", &iloc_body);

        let mut meta_body = full(0, &[]);
        meta_body.extend_from_slice(&pitm);
        meta_body.extend_from_slice(&iinf);
        let iloc_file_off_in_meta = meta_body.len();
        meta_body.extend_from_slice(&iloc);
        meta_body.extend_from_slice(&iref);
        let meta = bx(b"meta", &meta_body);
        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        let ftyp_len = ftyp.len();

        let mut mdat_body = Vec::new();
        mdat_body.extend_from_slice(garbage);
        let jpeg_rel = mdat_body.len();
        mdat_body.extend_from_slice(jpeg);
        let mdat = bx(b"mdat", &mdat_body);

        let mut file = ftyp;
        file.extend_from_slice(&meta);
        let mdat_at = file.len();
        file.extend_from_slice(&mdat);

        let g_off = (mdat_at + 8) as u32;
        let j_off = (mdat_at + 8 + jpeg_rel) as u32;
        // meta 박스: [8 헤더][meta_body]. iloc는 meta_body[iloc_file_off_in_meta]에서 시작.
        let iloc_box = ftyp_len + 8 + iloc_file_off_in_meta;
        // iloc 박스: [8 헤더][4 fullbox][iloc_rest]
        let rest_at = iloc_box + 8 + 4;
        let o1 = rest_at + off1_at;
        let o2 = rest_at + off2_at;
        file[o1..o1 + 4].copy_from_slice(&g_off.to_be_bytes());
        file[o2..o2 + 4].copy_from_slice(&j_off.to_be_bytes());
        file
    }

    fn tiny_jpeg() -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let buf = ImageBuffer::from_fn(32, 24, |x, y| Rgb([x as u8 * 8, y as u8 * 10, 80]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn index_finds_jpeg_thumb_and_payload() {
        let jpeg = tiny_jpeg();
        let file = build_hvc1_with_jpeg_thumb(&jpeg);
        let idx = index(&file).expect("heif index");
        assert_eq!(idx.primary, 1);
        assert!(idx.is_hevc_image(1));
        assert!(idx.is_jpeg(2));
        assert_eq!(idx.thumb_ids, vec![2]);
        let bytes = idx.item_bytes(&file, 2).expect("jpeg bytes");
        assert_eq!(bytes, jpeg);
    }

    #[test]
    fn patch_pitm_rewrites_primary_id() {
        let jpeg = tiny_jpeg();
        let mut file = build_hvc1_with_jpeg_thumb(&jpeg);
        let idx = index(&file).unwrap();
        assert!(idx.patch_pitm(&mut file, 2));
        let idx2 = index(&file).unwrap();
        assert_eq!(idx2.primary, 2);
    }

    /// iloc를 직접 조립한다: 항목 1개(id 2 = jpeg), `extents`개 조각이 모두 (0, 0) = "파일 끝까지".
    fn heif_with_iloc(extent_count: u16, offset_size_nibbles: u8) -> Vec<u8> {
        let mut infe = full(2, &[]);
        infe.extend_from_slice(&2u16.to_be_bytes());
        infe.extend_from_slice(&0u16.to_be_bytes());
        infe.extend_from_slice(b"jpeg");
        infe.push(0);
        let mut iinf_body = full(0, &1u16.to_be_bytes());
        iinf_body.extend_from_slice(&bx(b"infe", &infe));
        let mut iloc_rest = vec![offset_size_nibbles, 0x00];
        iloc_rest.extend_from_slice(&1u16.to_be_bytes()); // item_count
        iloc_rest.extend_from_slice(&2u16.to_be_bytes()); // item id
        iloc_rest.extend_from_slice(&0u16.to_be_bytes()); // data ref
        iloc_rest.extend_from_slice(&extent_count.to_be_bytes());
        let mut meta_body = full(0, &[]);
        meta_body.extend_from_slice(&bx(b"pitm", &full(0, &2u16.to_be_bytes())));
        meta_body.extend_from_slice(&bx(b"iinf", &iinf_body));
        meta_body.extend_from_slice(&bx(b"iloc", &full(0, &iloc_rest)));
        let mut file = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        file.extend_from_slice(&bx(b"meta", &meta_body));
        file.extend_from_slice(&bx(b"mdat", &[0xFF; 64]));
        file
    }

    #[test]
    fn hostile_extent_count_is_rejected_without_allocating() {
        // offset/length 크기 0 → 조각 하나가 0바이트라 헤더 몇 바이트로 65535 조각을 선언할 수 있다.
        // 상한에 걸려 색인을 포기해야 한다(수 GB 할당·반복 방지).
        let file = heif_with_iloc(u16::MAX, 0x00);
        assert!(index(&file).is_none());
    }

    #[test]
    fn repeated_whole_file_extents_cannot_exceed_file_size() {
        // 상한 안(40조각)이라도 모두 "파일 끝까지"면 출력이 파일×40이 된다 — None이어야 한다.
        let file = heif_with_iloc(40, 0x00);
        let idx = index(&file).expect("index within limits");
        assert!(idx.item_bytes(&file, 2).is_none());
    }

    #[test]
    fn empty_sibling_box_does_not_stop_parsing() {
        // 헤더만 있는 빈 박스(size 8) 뒤의 형제 박스도 읽어야 한다.
        let jpeg = tiny_jpeg();
        let file = build_hvc1_with_jpeg_thumb(&jpeg);
        let mut with_free = file[..24].to_vec(); // ftyp(24바이트) 뒤에
        with_free.extend_from_slice(&bx(b"free", &[]));
        with_free.extend_from_slice(&file[24..]);
        // iloc 절대 오프셋이 8바이트 밀렸으니 색인만 확인한다.
        let idx = index(&with_free).expect("parse past empty box");
        assert!(idx.is_jpeg(2));
    }

    #[test]
    fn compose_to_exif_matches_known_cases() {
        use Transform::*;
        assert_eq!(compose_to_exif(&[]), 1);
        assert_eq!(compose_to_exif(&[RotCcw(0)]), 1);
        assert_eq!(compose_to_exif(&[RotCcw(1)]), 8, "반시계 90 = EXIF 8");
        assert_eq!(compose_to_exif(&[RotCcw(2)]), 3);
        assert_eq!(compose_to_exif(&[RotCcw(3)]), 6, "반시계 270 = 시계 90 = EXIF 6");
        // 규격(23008-12:2022): axis 0 = 상하(EXIF 4), axis 1 = 좌우(EXIF 2).
        assert_eq!(compose_to_exif(&[Mirror(0)]), 4);
        assert_eq!(compose_to_exif(&[Mirror(1)]), 2);
        // 시계 90 후 좌우 반전 = EXIF 5, 반시계 90 후 좌우 반전 = EXIF 7.
        assert_eq!(compose_to_exif(&[RotCcw(3), Mirror(1)]), 5);
        assert_eq!(compose_to_exif(&[RotCcw(1), Mirror(1)]), 7);
    }

    #[test]
    fn hevc_mirror_fix_turns_heif_oxide_result_into_spec_result() {
        use Transform::*;
        assert_eq!(hevc_mirror_fix(&[]), 1);
        assert_eq!(hevc_mirror_fix(&[RotCcw(1)]), 1, "회전만 있으면 보정 없음");
        for ts in [vec![Mirror(0)], vec![Mirror(1)], vec![RotCcw(1), Mirror(0)], vec![Mirror(1), RotCcw(3)]] {
            let fixed = grid_exif(&grid_apply(&ts, false), hevc_mirror_fix(&ts));
            assert_eq!(fixed, grid_apply(&ts, true), "{ts:?}");
        }
        // 단일 imir는 좌우↔상하 바꿈 = 180° 회전(EXIF 3).
        assert_eq!(hevc_mirror_fix(&[Mirror(0)]), 3);
    }

    /// ipma 한 항목(id 2)에 `assoc` 번호들을 붙이고 ipco에 `props`를 넣은 iprp 본문.
    fn iprp(props: &[Vec<u8>], ver: u8, flags: u8, assoc: &[u16]) -> Vec<u8> {
        let mut ipma = vec![ver, 0, 0, flags];
        ipma.extend_from_slice(&1u32.to_be_bytes());
        if ver < 1 {
            ipma.extend_from_slice(&2u16.to_be_bytes());
        } else {
            ipma.extend_from_slice(&2u32.to_be_bytes());
        }
        ipma.push(assoc.len() as u8);
        for &a in assoc {
            if flags & 1 != 0 {
                ipma.extend_from_slice(&(0x8000 | a).to_be_bytes());
            } else {
                ipma.push(0x80 | a as u8);
            }
        }
        let mut body = bx(b"ipco", &props.concat());
        body.extend_from_slice(&bx(b"ipma", &ipma));
        body
    }

    fn transforms_of(body: &[u8]) -> Option<Vec<Transform>> {
        parse_iprp(body, 0, body.len()).map(|m| m.get(&2).cloned().unwrap_or_default())
    }

    #[test]
    fn ipma_variants_and_bad_indices() {
        use Transform::*;
        let rot = bx(b"irot", &[1]);
        let mir = bx(b"imir", &[1]);
        let other = bx(b"pixi", &[0, 0, 0, 0, 3, 8, 8, 8]);
        // 기본(ver 0, 1바이트 번호), 속성 순서 유지, 비변환 속성은 무시.
        assert_eq!(transforms_of(&iprp(&[other.clone(), rot.clone(), mir.clone()], 0, 0, &[1, 2, 3])), Some(vec![RotCcw(1), Mirror(1)]));
        // ver 1(u32 id) + flags&1(2바이트 번호).
        assert_eq!(transforms_of(&iprp(std::slice::from_ref(&rot), 1, 1, &[1])), Some(vec![RotCcw(1)]));
        // 번호 0(= 속성 없음)·범위 밖 번호는 건너뛴다.
        assert_eq!(transforms_of(&iprp(std::slice::from_ref(&rot), 0, 0, &[0, 9, 1])), Some(vec![RotCcw(1)]));
        // 본문 없는 irot 박스는 다음 박스 바이트를 각도로 읽지 않는다.
        assert_eq!(transforms_of(&iprp(&[bx(b"irot", &[]), bx(b"irot", &[3])], 0, 0, &[1, 2])), Some(vec![RotCcw(3)]));
        // 항목 수만 있고 내용이 잘린 ipma는 None → 호출부는 회전 없음으로 색인을 유지.
        let mut cut = bx(b"ipco", &rot);
        cut.extend_from_slice(&bx(b"ipma", &[0, 0, 0, 0, 0, 0, 0, 1]));
        assert_eq!(transforms_of(&cut), None);
    }

    #[test]
    fn junk_is_not_heif() {
        assert!(index(b"not a heif file").is_none());
    }
}
