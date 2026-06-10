//! AF 포인트 추출(#37). 측거점 좌표는 표준 EXIF가 아니라 제조사 MakerNote에 있어
//! 직접 파싱한다(kamadak-exif는 MakerNote 내부를 풀지 않음). 레이아웃은 ExifTool의
//! Canon.pm / Panasonic.pm / Sony.pm 정의를 따르며, Windows 실파일 검증 결과는
//! `docs/plan-af-points.md` §4-3 참조. 지원: Canon CR2/JPG(AFInfo·AFInfo2),
//! Panasonic RW2/JPG(AFPointPosition·AFAreaSize), Sony ARW(FocusLocation).
//! 미지원 바디·태그 없음·파싱 실패는 전부 None — 호출부는 조용히 미표시한다.

use std::path::Path;

/// AF 측거점 하나. 좌표는 **센서(미회전) 이미지 기준 0..1 정규화**, 원점 좌상단.
/// `w`/`h`가 0.0이면 크기 정보가 없는 것 — 표시 측이 고정 크기 박스로 폴백한다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AfPoint {
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    /// 합초(초점이 실제로 맞은) 측거점.
    pub in_focus: bool,
    /// 사용자가 선택한 측거점.
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AfInfo {
    pub points: Vec<AfPoint>,
    /// 어느 제조사 경로로 파싱됐는지(디버그·테스트용).
    pub source: &'static str,
}

/// 파일에서 AF 정보를 추출한다. 앞 2MB만 읽는다(EXIF/MakerNote는 파일 선두에 있음 —
/// meta.rs의 EXIF 폴백과 동일한 가정). 못 찾으면 None.
pub fn parse_af(path: &Path) -> Option<AfInfo> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(2 * 1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    parse_af_bytes(&buf)
}

/// 바이트(파일 prefix)에서 AF 정보를 추출한다. JPEG이면 APP1의 EXIF TIFF를,
/// TIFF 계열(CR2/ARW/RW2)이면 컨테이너를 직접 걷고, 실패하면 내부 임베디드
/// JPEG들(RW2의 프리뷰 등)의 EXIF로 폴백한다.
pub fn parse_af_bytes(bytes: &[u8]) -> Option<AfInfo> {
    // 1) 컨테이너 직접.
    if bytes.len() >= 4 {
        if bytes[0] == 0xFF && bytes[1] == 0xD8 {
            if let Some(tiff) = jpeg_exif_tiff(bytes) {
                if let Some(af) = parse_tiff(tiff) {
                    return Some(af);
                }
            }
        } else if &bytes[0..2] == b"II" || &bytes[0..2] == b"MM" {
            if let Some(af) = parse_tiff(bytes) {
                return Some(af);
            }
        }
    }
    // 2) 임베디드 JPEG 폴백(RW2: MakerNote가 프리뷰 JPEG의 EXIF에 있음).
    let mut i = 1usize; // 0은 위에서 시도했으므로 1부터.
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            if let Some(tiff) = jpeg_exif_tiff(&bytes[i..]) {
                if let Some(af) = parse_tiff(tiff) {
                    return Some(af);
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// JPEG 바이트에서 APP1 "Exif\0\0" 페이로드(TIFF 블록)를 찾는다.
fn jpeg_exif_tiff(jpeg: &[u8]) -> Option<&[u8]> {
    let mut i = 2usize; // SOI 다음.
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let marker = jpeg[i + 1];
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => {
                i += 2; // 길이 없는 마커.
                continue;
            }
            0xDA | 0xD9 => return None, // SOS/EOI — 이후엔 APP1 없음.
            _ => {}
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            return None;
        }
        if marker == 0xE1 && len >= 8 && &jpeg[i + 4..i + 10] == b"Exif\0\0" {
            return Some(&jpeg[i + 10..i + 2 + len]);
        }
        i += 2 + len;
    }
    None
}

// ── TIFF 워커 ──────────────────────────────────────────────

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

/// IFD 엔트리 하나: (타입, 개수, 값 필드 오프셋[4바이트 인라인 또는 포인터]).
struct Entry {
    typ: u16,
    count: u32,
    val_field: usize,
}

impl<'a> Tiff<'a> {
    fn new(b: &'a [u8]) -> Option<Self> {
        if b.len() < 8 {
            return None;
        }
        let le = match &b[0..2] {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        Some(Tiff { b, le })
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
    /// IFD에서 태그를 찾아 엔트리를 반환.
    fn find(&self, ifd: usize, tag: u16) -> Option<Entry> {
        let count = self.u16(ifd)? as usize;
        if count > 1024 {
            return None; // 손상 가드.
        }
        for i in 0..count {
            let e = ifd + 2 + i * 12;
            if self.u16(e)? == tag {
                return Some(Entry {
                    typ: self.u16(e + 2)?,
                    count: self.u32(e + 4)?,
                    val_field: e + 8,
                });
            }
        }
        None
    }
    /// 엔트리의 실데이터 오프셋과 바이트 길이. 4바이트 이하면 값 필드 인라인.
    fn data(&self, e: &Entry) -> Option<(usize, usize)> {
        let unit = match e.typ {
            1 | 2 | 6 | 7 => 1, // BYTE/ASCII/SBYTE/UNDEFINED
            3 | 8 => 2,         // SHORT/SSHORT
            4 | 9 | 11 => 4,    // LONG/SLONG/FLOAT
            5 | 10 | 12 => 8,   // RATIONAL/SRATIONAL/DOUBLE
            _ => return None,
        };
        let len = (e.count as usize).checked_mul(unit)?;
        let off = if len <= 4 { e.val_field } else { self.u32(e.val_field)? as usize };
        (off.checked_add(len)? <= self.b.len()).then_some((off, len))
    }
    /// SHORT 배열을 읽는다(부호는 호출부에서 캐스팅).
    fn shorts(&self, off: usize, n: usize) -> Option<Vec<u16>> {
        (0..n).map(|i| self.u16(off + i * 2)).collect()
    }
    /// RATIONAL(u32/u32) 하나를 f64로.
    fn rational(&self, off: usize) -> Option<f64> {
        let num = self.u32(off)? as f64;
        let den = self.u32(off + 4)? as f64;
        (den != 0.0).then(|| num / den)
    }
    /// ASCII 값 문자열.
    fn ascii(&self, e: &Entry) -> Option<String> {
        let (off, len) = self.data(e)?;
        let s = &self.b[off..off + len];
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        Some(String::from_utf8_lossy(&s[..end]).trim().to_string())
    }
}

/// EXIF TIFF 블록에서 IFD0 → ExifIFD → MakerNote를 걷고 제조사별 파서로 분기.
fn parse_tiff(tiff: &[u8]) -> Option<AfInfo> {
    let t = Tiff::new(tiff)?;
    let ifd0 = t.u32(4)? as usize;
    let make = t.find(ifd0, 0x010F).and_then(|e| t.ascii(&e)).unwrap_or_default();
    let model = t.find(ifd0, 0x0110).and_then(|e| t.ascii(&e)).unwrap_or_default();
    let exif_ifd = t.u32(t.find(ifd0, 0x8769)?.val_field)? as usize;
    let mn = t.find(exif_ifd, 0x927C)?;
    let (mn_off, mn_len) = t.data(&mn)?;
    if mn_len < 14 {
        return None;
    }
    let head = &tiff[mn_off..mn_off + 12.min(mn_len)];

    // 제조사 식별: MakerNote 헤더 시그니처 우선, 없으면 IFD0 Make.
    if head.starts_with(b"Panasonic\0") {
        // "Panasonic\0\0\0" 12바이트 헤더 뒤 표준 IFD. 값 오프셋은 TIFF 베이스 기준.
        return panasonic_af(&t, mn_off + 12);
    }
    if head.starts_with(b"SONY DSC") || head.starts_with(b"SONY CAM") || head.starts_with(b"SONY MOBILE") {
        return sony_af(&t, mn_off + 12);
    }
    if make.starts_with("Canon") {
        // Canon MakerNote는 헤더 없이 바로 IFD. 값 오프셋은 TIFF 베이스 기준.
        return canon_af(&t, mn_off, &model);
    }
    if make.to_ascii_uppercase().starts_with("SONY") {
        // 헤더 없는 소니 변형: 바로 IFD로 시도.
        return sony_af(&t, mn_off);
    }
    if make.starts_with("Panasonic") {
        return panasonic_af(&t, mn_off + 12);
    }
    None
}

// ── Canon: AFInfo2(0x26) / AFInfo(0x12) — int16u 연속 스트림 ──────────────
// 좌표계: 이미지 중심 원점, AFImageWidth/Height 기준, EOS는 Y가 위로 양수(ExifTool
// Canon.pm NOTES; PowerShot은 아래로 양수). 화면 좌표로는 y를 뒤집는다.

fn canon_af(t: &Tiff, ifd: usize, model: &str) -> Option<AfInfo> {
    let y_up = !model.contains("PowerShot"); // EOS = Y 위로 양수.
    if let Some(e) = t.find(ifd, 0x0026) {
        let (off, len) = t.data(&e)?;
        return canon_af_info2(t, off, len / 2, y_up);
    }
    if let Some(e) = t.find(ifd, 0x0012) {
        let (off, len) = t.data(&e)?;
        return canon_af_info1(t, off, len / 2, y_up);
    }
    None
}

/// AFInfo2: [size, mode, num, valid, cw, ch, afw, afh, W[num], H[num], X[num], Y[num],
/// InFocus[ceil], Selected[ceil]] (모두 16비트).
fn canon_af_info2(t: &Tiff, off: usize, words: usize, y_up: bool) -> Option<AfInfo> {
    let v = t.shorts(off, words.min(4096))?;
    let num = *v.get(2)? as usize;
    let valid = (*v.get(3)? as usize).min(num);
    let (afw, afh) = (*v.get(6)? as f64, *v.get(7)? as f64);
    if num == 0 || num > 128 || afw < 1.0 || afh < 1.0 {
        return None;
    }
    let mask_words = num.div_ceil(16);
    // 배열 시작: 8.
    if v.len() < 8 + 4 * num + 2 * mask_words {
        return None;
    }
    let w = &v[8..8 + num];
    let h = &v[8 + num..8 + 2 * num];
    let x = &v[8 + 2 * num..8 + 3 * num];
    let y = &v[8 + 3 * num..8 + 4 * num];
    let focus_bits = &v[8 + 4 * num..8 + 4 * num + mask_words];
    let sel_bits = v.get(8 + 4 * num + mask_words..8 + 4 * num + 2 * mask_words);
    let bit = |bits: &[u16], i: usize| (bits[i / 16] >> (i % 16)) & 1 == 1;
    let points = (0..valid)
        .map(|i| {
            let (xi, yi) = (x[i] as i16 as f64, y[i] as i16 as f64);
            AfPoint {
                cx: 0.5 + xi / afw,
                cy: if y_up { 0.5 - yi / afh } else { 0.5 + yi / afh },
                w: (w[i] as i16 as f64).abs() / afw,
                h: (h[i] as i16 as f64).abs() / afh,
                in_focus: bit(focus_bits, i),
                selected: sel_bits.map(|b| bit(b, i)).unwrap_or(false),
            }
        })
        .collect();
    Some(AfInfo { points, source: "canon-afinfo2" })
}

/// AFInfo(구형): [num, valid, cw, ch, afw, afh, aw, ah, X[num], Y[num], InFocus[ceil]].
/// 측거점 크기는 단일값(aw/ah)이 전 측거점 공통.
fn canon_af_info1(t: &Tiff, off: usize, words: usize, y_up: bool) -> Option<AfInfo> {
    let v = t.shorts(off, words.min(4096))?;
    let num = *v.get(0)? as usize;
    let valid = (*v.get(1)? as usize).min(num);
    let (afw, afh) = (*v.get(4)? as f64, *v.get(5)? as f64);
    if num == 0 || num > 128 || afw < 1.0 || afh < 1.0 {
        return None;
    }
    let (aw, ah) = (*v.get(6)? as f64, *v.get(7)? as f64);
    let mask_words = num.div_ceil(16);
    if v.len() < 8 + 2 * num + mask_words {
        return None;
    }
    let x = &v[8..8 + num];
    let y = &v[8 + num..8 + 2 * num];
    let focus_bits = &v[8 + 2 * num..8 + 2 * num + mask_words];
    let bit = |bits: &[u16], i: usize| (bits[i / 16] >> (i % 16)) & 1 == 1;
    let points = (0..valid)
        .map(|i| {
            let (xi, yi) = (x[i] as i16 as f64, y[i] as i16 as f64);
            AfPoint {
                cx: 0.5 + xi / afw,
                cy: if y_up { 0.5 - yi / afh } else { 0.5 + yi / afh },
                w: aw / afw,
                h: ah / afh,
                in_focus: bit(focus_bits, i),
                selected: false,
            }
        })
        .collect();
    Some(AfInfo { points, source: "canon-afinfo1" })
}

// ── Panasonic: AFPointPosition(0x4d) + AFAreaSize(0xde) — rational ──────────
// 0~1 정규화 중심/크기. "none"=16777216, n/a=4194303.9…(ExifTool Panasonic.pm).

fn panasonic_af(t: &Tiff, ifd: usize) -> Option<AfInfo> {
    let pos = t.find(ifd, 0x004d)?;
    if pos.typ != 5 || pos.count < 2 {
        return None;
    }
    let (off, _) = t.data(&pos)?;
    let cx = t.rational(off)?;
    let cy = t.rational(off + 8)?;
    if !(0.0..=1.0).contains(&cx) || !(0.0..=1.0).contains(&cy) {
        return None; // "none"(16777216) 포함 무효값.
    }
    let (mut w, mut h) = (0.0, 0.0);
    if let Some(sz) = t.find(ifd, 0x00de) {
        if sz.typ == 5 && sz.count >= 2 {
            if let Some((soff, _)) = t.data(&sz) {
                let sw = t.rational(soff).unwrap_or(0.0);
                let sh = t.rational(soff + 8).unwrap_or(0.0);
                // n/a = 4194303.9…(4294967295/1024) — 정상값은 0~1.
                if (0.0..=1.0).contains(&sw) && (0.0..=1.0).contains(&sh) {
                    (w, h) = (sw, sh);
                }
            }
        }
    }
    Some(AfInfo {
        points: vec![AfPoint { cx, cy, w, h, in_focus: true, selected: true }],
        source: "panasonic",
    })
}

// ── Sony: FocusLocation(0x2027) — int16u[4] = (imgW, imgH, X, Y) ───────────

fn sony_af(t: &Tiff, ifd: usize) -> Option<AfInfo> {
    let e = t.find(ifd, 0x2027)?;
    if e.count < 4 {
        return None;
    }
    let (off, _) = t.data(&e)?;
    let v = t.shorts(off, 4)?;
    let (w, h, x, y) = (v[0] as f64, v[1] as f64, v[2] as f64, v[3] as f64);
    if w < 1.0 || h < 1.0 || x > w || y > h {
        return None;
    }
    Some(AfInfo {
        points: vec![AfPoint { cx: x / w, cy: y / h, w: 0.0, h: 0.0, in_focus: true, selected: true }],
        source: "sony",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 합성 TIFF(LE): IFD0(Make/Model/ExifIFD) → ExifIFD(MakerNote=Canon AFInfo2)로
    /// 파서의 워킹·비트마스크·좌표 변환을 결정적으로 검증한다(샘플 불필요).
    #[test]
    fn canon_afinfo2_synthetic() {
        // AFInfo2 스트림: size, mode, num=2, valid=2, cw, ch, afw=1000, afh=500,
        // W[2]=100,100  H[2]=80,80  X[2]=-250,+250  Y[2]=+125,-125,
        // InFocus=0b01(point0), Selected=0b10(point1).
        let af: Vec<u16> = vec![
            0, 2, 2, 2, 1000, 500, 1000, 500, 100, 100, 80, 80, (-250i16) as u16, 250, 125,
            (-125i16) as u16, 0b01, 0b10,
        ];
        let mut afb = Vec::new();
        for v in &af {
            afb.extend_from_slice(&v.to_le_bytes());
        }
        let tiff = build_tiff_with_makernote(b"Canon", b"Canon EOS TEST", &afb, 0x0026);
        let info = parse_tiff(&tiff).expect("canon afinfo2 parse");
        assert_eq!(info.source, "canon-afinfo2");
        assert_eq!(info.points.len(), 2);
        let p0 = info.points[0];
        // x=-250/1000 → cx=0.25; y=+125/500, Y 위로 양수 → cy=0.5-0.25=0.25.
        assert!((p0.cx - 0.25).abs() < 1e-9 && (p0.cy - 0.25).abs() < 1e-9);
        assert!((p0.w - 0.1).abs() < 1e-9 && (p0.h - 0.16).abs() < 1e-9);
        assert!(p0.in_focus && !p0.selected);
        let p1 = info.points[1];
        assert!((p1.cx - 0.75).abs() < 1e-9 && (p1.cy - 0.75).abs() < 1e-9);
        assert!(!p1.in_focus && p1.selected);
    }

    /// 합성 TIFF에 Panasonic MakerNote(헤더+IFD, rational 값)를 넣어 검증.
    #[test]
    fn panasonic_synthetic() {
        // MakerNote 페이로드는 "Panasonic\0\0\0" + IFD인데, IFD 값 오프셋이 TIFF 베이스
        // 기준이라 빌더가 배치 후 오프셋을 채워야 한다. 빌더의 vendor IFD 모드 사용.
        let tiff = build_tiff_panasonic(0.65, 0.46, Some((0.05, 0.07)));
        let info = parse_tiff(&tiff).expect("panasonic parse");
        assert_eq!(info.source, "panasonic");
        let p = info.points[0];
        assert!((p.cx - 0.65).abs() < 1e-6 && (p.cy - 0.46).abs() < 1e-6);
        assert!((p.w - 0.05).abs() < 1e-6 && (p.h - 0.07).abs() < 1e-6);
        // 무효 좌표("none"=16777216)는 None.
        let bad = build_tiff_panasonic(16777216.0, 16777216.0, None);
        assert!(parse_tiff(&bad).is_none());
    }

    // ── 테스트용 TIFF 빌더 ──────────────────────────────────────

    fn put16(b: &mut Vec<u8>, v: u16) {
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn put32(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_le_bytes());
    }

    /// LE TIFF: IFD0[Make, Model, ExifIFD] → ExifIFD[MakerNote(tag 0x927C)].
    /// MakerNote 페이로드는 그대로 박고, Canon식(헤더 없는 IFD)을 흉내내기 위해
    /// 페이로드 앞에 엔트리 1개짜리 IFD(태그 `af_tag`, 데이터는 페이로드 뒤)를 합성한다.
    fn build_tiff_with_makernote(make: &[u8], model: &[u8], af_payload: &[u8], af_tag: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        put16(&mut b, 42);
        put32(&mut b, 8); // IFD0 at 8.
        // 레이아웃(오프셋 선계산):
        // IFD0: 8 .. 8+2+3*12+4 = 50
        // make data: 50, model data: 50+make_n
        let make_n = make.len() + 1;
        let model_n = model.len() + 1;
        let exif_ifd = 50 + make_n + model_n;
        // ExifIFD: 엔트리 1개(MakerNote) = 2+12+4 = 18바이트.
        let mn_off = exif_ifd + 18;
        // MakerNote 본체 = Canon식 IFD: 엔트리 1개 = 2+12+4 = 18바이트, 그 뒤 af 데이터.
        let af_data_off = mn_off + 18;
        let mn_len = 18 + af_payload.len();

        // IFD0.
        put16(&mut b, 3);
        put16(&mut b, 0x010F); // Make
        put16(&mut b, 2);
        put32(&mut b, make_n as u32);
        if make_n <= 4 {
            let mut v = [0u8; 4];
            v[..make.len()].copy_from_slice(make);
            b.extend_from_slice(&v);
        } else {
            put32(&mut b, 50);
        }
        put16(&mut b, 0x0110); // Model
        put16(&mut b, 2);
        put32(&mut b, model_n as u32);
        put32(&mut b, (50 + make_n) as u32);
        put16(&mut b, 0x8769); // ExifIFD
        put16(&mut b, 4);
        put32(&mut b, 1);
        put32(&mut b, exif_ifd as u32);
        put32(&mut b, 0); // next IFD.
        // 문자열 데이터.
        b.extend_from_slice(make);
        b.push(0);
        b.extend_from_slice(model);
        b.push(0);
        // ExifIFD.
        assert_eq!(b.len(), exif_ifd);
        put16(&mut b, 1);
        put16(&mut b, 0x927C); // MakerNote
        put16(&mut b, 7); // UNDEFINED
        put32(&mut b, mn_len as u32);
        put32(&mut b, mn_off as u32);
        put32(&mut b, 0);
        // MakerNote(Canon식 IFD).
        assert_eq!(b.len(), mn_off);
        put16(&mut b, 1);
        put16(&mut b, af_tag);
        put16(&mut b, 3); // SHORT
        put32(&mut b, (af_payload.len() / 2) as u32);
        put32(&mut b, af_data_off as u32);
        put32(&mut b, 0);
        b.extend_from_slice(af_payload);
        b
    }

    /// LE TIFF + Panasonic MakerNote("Panasonic\0\0\0" 헤더 + IFD + rational 데이터).
    fn build_tiff_panasonic(cx: f64, cy: f64, size: Option<(f64, f64)>) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        put16(&mut b, 42);
        put32(&mut b, 8);
        let make = b"Panasonic";
        let make_n = make.len() + 1; // 10
        let exif_ifd = 8 + 2 + 2 * 12 + 4 + make_n; // IFD0: Make+ExifIFD 2엔트리.
        let mn_off = exif_ifd + 18;
        let n_entries = if size.is_some() { 2 } else { 1 };
        let ifd_inner = mn_off + 12; // 헤더 12바이트 뒤 IFD.
        let data_off = ifd_inner + 2 + n_entries * 12 + 4;
        let mn_len = 12 + 2 + n_entries * 12 + 4 + n_entries * 16;
        // IFD0.
        put16(&mut b, 2);
        put16(&mut b, 0x010F);
        put16(&mut b, 2);
        put32(&mut b, make_n as u32);
        put32(&mut b, (8 + 2 + 2 * 12 + 4) as u32);
        put16(&mut b, 0x8769);
        put16(&mut b, 4);
        put32(&mut b, 1);
        put32(&mut b, exif_ifd as u32);
        put32(&mut b, 0);
        b.extend_from_slice(make);
        b.push(0);
        // ExifIFD.
        assert_eq!(b.len(), exif_ifd);
        put16(&mut b, 1);
        put16(&mut b, 0x927C);
        put16(&mut b, 7);
        put32(&mut b, mn_len as u32);
        put32(&mut b, mn_off as u32);
        put32(&mut b, 0);
        // MakerNote.
        assert_eq!(b.len(), mn_off);
        b.extend_from_slice(b"Panasonic\0\0\0");
        put16(&mut b, n_entries as u16);
        let rat = |v: f64| -> (u32, u32) { ((v * 1_000_000.0).round() as u32, 1_000_000) };
        // 0x4d AFPointPosition.
        put16(&mut b, 0x004d);
        put16(&mut b, 5);
        put32(&mut b, 2);
        put32(&mut b, data_off as u32);
        if size.is_some() {
            put16(&mut b, 0x00de);
            put16(&mut b, 5);
            put32(&mut b, 2);
            put32(&mut b, (data_off + 16) as u32);
        }
        put32(&mut b, 0);
        // rational 데이터.
        assert_eq!(b.len(), data_off);
        for v in [cx, cy] {
            let (n, d) = rat(v);
            put32(&mut b, n);
            put32(&mut b, d);
        }
        if let Some((w, h)) = size {
            for v in [w, h] {
                let (n, d) = rat(v);
                put32(&mut b, n);
                put32(&mut b, d);
            }
        }
        b
    }
}
