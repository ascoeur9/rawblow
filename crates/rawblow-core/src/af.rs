//! AF 포인트 추출(#37). 측거점 좌표는 표준 EXIF가 아니라 제조사 MakerNote에 있어
//! 직접 파싱한다(kamadak-exif는 MakerNote 내부를 풀지 않음). 레이아웃은 ExifTool의
//! Canon.pm / Panasonic.pm / Sony.pm 정의를 따르며, Windows 실파일 검증 결과는
//! `docs/plan-af-points.md` §4-3 참조. 지원: Canon CR2/JPG(AFInfo·AFInfo2),
//! Panasonic RW2/JPG(AFPointPosition·AFAreaSize), Sony ARW(FocusLocation),
//! Nikon Z8/Z9 NEF(AFInfo2 V0400 — 오토에어리어 측거 존 비트마스크/단일점, #45),
//! Nikon Z 미러리스 NEF(AFInfo2 V04xx — 단일점 픽셀좌표는 그리드 무관해 미검증 바디도
//! 표시; 존 비트마스크는 0400만, #50).
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
    if head.starts_with(b"Nikon\x00") {
        // Nikon Type3 MakerNote: "Nikon\0"+버전(2)+"\0\0" 뒤(오프셋 10)부터 자체 TIFF 헤더.
        // 내부 오프셋이 그 TIFF 베이스 기준이라 해당 지점부터 서브슬라이스로 파싱한다.
        return nikon_af(tiff.get(mn_off + 10..)?);
    }
    if head.starts_with(b"OM SYSTEM\0") || head.starts_with(b"OLYMPUS\0") {
        // 올림푸스/OM SYSTEM 신형: 내부 오프셋은 MakerNote 시작(mn_off) 기준.
        return olympus_af(&t, mn_off);
    }
    if head.starts_with(b"PENTAX \0") {
        // Pentax(RICOH): "PENTAX \0"(8) + 바이트오더(II/MM) 뒤 IFD. 오프셋은 mn_off 기준.
        return pentax_af(&t, mn_off);
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
    let num = *v.first()? as usize;
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

// ── Olympus / OM SYSTEM: CameraSettings(0x2020) → AFPointSelected(0x0305) (#81) ──
// MakerNote는 "OM SYSTEM\0\0\0"(12) 또는 "OLYMPUS\0"(8) 시그니처 + 내부 TIFF(II/MM +
// 버전 2바이트) 뒤 IFD가 바로 온다. 내부 값 오프셋은 MakerNote 시작 기준(mn).
// AF점은 CameraSettings 서브IFD의 AFPointSelected(0x0305, srational64s[5])에 있고,
// [flag, x0, y0, x1, y1]로 박스 좌표가 **0..1 정규화**되어 저장된다(ExifTool Olympus.pm:
// "coordinates expressed as a percent"). 선택 AF점 하나를 그 박스로 표시한다.
// 구형 "OLYMP\0"(E-300 등)은 이 태그가 없어 미지원(잘못 찍느니 미표시).
fn olympus_af(t: &Tiff, mn: usize) -> Option<AfInfo> {
    let sig_len = if t.b.get(mn..mn + 8)? == b"OLYMPUS\0" { 8 } else { 12 };
    // 시그니처 뒤 II/MM(2) + 버전(2), 그다음 MakerNote IFD가 바로.
    let mn_ifd = mn + sig_len + 4;
    // CameraSettings(0x2020) 서브IFD(오프셋은 mn 기준).
    let cs = mn + t.u32(t.find(mn_ifd, 0x2020)?.val_field)? as usize;
    // AFPointSelected(0x0305): srational64s[5]. 5×8=40바이트라 항상 포인터.
    let e = t.find(cs, 0x0305)?;
    if e.typ != 10 || e.count < 5 {
        return None;
    }
    let off = mn + t.u32(e.val_field)? as usize;
    // srational: 분모 0(="undef")이면 무효. [0]은 항상 undef 플래그, [1..5]가 좌표.
    let srat = |o: usize| -> Option<f64> {
        let n = t.u32(o)? as i32 as f64;
        let d = t.u32(o + 4)? as i32 as f64;
        (d != 0.0).then(|| n / d)
    };
    let (x0, y0, x1, y1) = (srat(off + 8)?, srat(off + 16)?, srat(off + 24)?, srat(off + 32)?);
    let inside = |v: f64| (0.0..=1.0).contains(&v);
    if !(inside(x0) && inside(y0) && inside(x1) && inside(y1)) {
        return None; // 범위 밖(레이아웃 불일치·n/a) → 미표시.
    }
    let pt = AfPoint {
        cx: (x0 + x1) / 2.0,
        cy: (y0 + y1) / 2.0,
        w: (x1 - x0).abs(),
        h: (y1 - y0).abs(),
        in_focus: true,
        selected: true,
    };
    Some(AfInfo { points: vec![pt], source: "olympus" })
}

// ── Pentax K-1: AFPointInfo(0x0245) — SAFOX 12 33점 고정 그리드 (#82) ────────────
// Pentax MakerNote는 "PENTAX \0"(8) + 내부 바이트오더(II/MM, 2) 뒤 IFD가 오고, 값 오프셋은
// MakerNote 시작(mn) 기준이다(내부 바이트오더는 외부 TIFF와 독립 → 따로 읽는다).
// K-1의 33개 측거점은 AFPointInfo(0x0245, ExifTool Pentax.pm)에 담긴다:
//   off 0  version(int16u)=1.   off 2  NumAFPoints(int16u)=33.
//   off 4  int8u[ceil(num/4)]=int8u[9]: 점당 2비트(4점/바이트, MSB first, 점 1..33).
//          비트0(0x01)=선택, 비트1(0x02)=합초. (ExifTool DecodeAFPoints)
// 점 번호(1..33)→프레임 정규화 좌표는 아래 표(Focus-Points 실측, 7360×4912 기준).
// num≠33(KP/K-70 등 다른 그리드)은 좌표표가 달라 미표시(잘못 찍느니 안 그림).
fn pentax_af(t: &Tiff, mn: usize) -> Option<AfInfo> {
    let le = match t.b.get(mn + 8..mn + 10)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    // 내부 바이트오더로 MakerNote를 읽는 뷰(오프셋은 mn 상대라 data()는 못 쓰고 직접 계산).
    let p = Tiff { b: t.b, le };
    let ifd = mn + 10;
    let n = p.u16(ifd)? as usize;
    if n > 1024 {
        return None;
    }
    let mut data = None;
    for i in 0..n {
        let e = ifd + 2 + i * 12;
        if p.u16(e)? == 0x0245 {
            let cnt = p.u32(e + 4)? as usize; // UNDEF(int8u) → 바이트 길이.
            let off = if cnt <= 4 { e + 8 } else { mn + p.u32(e + 8)? as usize };
            data = Some((off, cnt));
            break;
        }
    }
    let (off, len) = data?;
    let num = p.u16(off + 2)? as usize;
    if num != 33 {
        return None; // K-1(33점) 전용 좌표표.
    }
    let nbytes = num.div_ceil(4);
    if len < 4 + nbytes {
        return None;
    }
    let mut points = Vec::new();
    for (i, &(px, py)) in PENTAX_K1_POINTS.iter().enumerate() {
        let byte = *p.b.get(off + 4 + i / 4)?;
        let field = (byte >> (6 - 2 * (i % 4))) & 0x03; // MSB first, 점당 2비트.
        if field & 0x03 == 0 {
            continue; // 미선택 점.
        }
        points.push(AfPoint {
            cx: px as f64 / PENTAX_K1_W,
            cy: py as f64 / PENTAX_K1_H,
            w: 150.0 / PENTAX_K1_W, // 측거점 마커 실측 150px.
            h: 150.0 / PENTAX_K1_H,
            in_focus: field & 0x02 != 0,
            selected: true,
        });
    }
    (!points.is_empty()).then_some(AfInfo { points, source: "pentax" })
}

// K-1 33점 측거점 중심 픽셀좌표(풀프레임 7360×4912 기준, Focus-Points 실측). 점 1..33 =
// 인덱스 0..32. 다이아몬드 배열(5/7/9/7/5행), 중심=점17=(3680,2456)=(0.5,0.5).
const PENTAX_K1_W: f64 = 7360.0;
const PENTAX_K1_H: f64 = 4912.0;
const PENTAX_K1_POINTS: [(u16, u16); 33] = [
    (2797, 1818), (3238, 1818), (3680, 1818), (4121, 1818), (4563, 1818),
    (2355, 2137), (2797, 2137), (3238, 2137), (3680, 2137), (4121, 2137), (4563, 2137), (5005, 2137),
    (1914, 2456), (2350, 2456), (2797, 2456), (3238, 2456), (3680, 2456), (4121, 2456), (4563, 2456), (5005, 2456), (5446, 2456),
    (2355, 2775), (2797, 2775), (3238, 2775), (3680, 2775), (4121, 2775), (4563, 2775), (5005, 2775),
    (2797, 3094), (3238, 3094), (3680, 3094), (4121, 3094), (4563, 3094),
];

// ── Nikon: AFInfo2 V04xx — 미러리스 Z 계열(Expeed 6/7) (#45, #50) ────────────
// 인자는 Nikon Type3 MakerNote의 임베드 TIFF(오프셋 10부터)다. 레이아웃은 ExifTool
// Nikon.pm `AFInfo2V0400`:
//   off 0   AFInfo2Version (ASCII[4]) — "04xx"가 Z 미러리스(0400=Z8/Z9, 0402=Z5II …).
//   off 5   AFAreaMode (197=Auto, 207=3D-tracking … 카메라가 측거 존 선택).
//   off 7   AFCoordinatesAvailable (0=AFPointsUsed 비트마스크 사용, 1=AFAreaX/YPosition 픽셀).
//   off 10  AFPointsUsed: 51바이트(408비트) 비트마스크, **LSB-first**, afPoints405 순서
//           (15행 A–O × 27열 1–27). 켜진 비트 = 활성 측거점(존).
//   off 0x3e/0x40  AFImageWidth/Height(int16u).  off 0x42/0x44  AFAreaX/YPosition(좌상단 원점, px).
// 두 경로의 성격이 다르다:
//   · 단일점(coords=1)은 픽셀좌표를 이미지 크기로 나누는 거라 **바디·그리드와 무관**하고,
//     좌표가 이미지 범위 안인지로 자체 검증된다 → V04xx면 미검증 모델이라도 표시(#50).
//   · 존 비트마스크(coords=0)는 아래 그리드 기하가 Z8/Z9(493점) 전용이라 0400만 허용한다.
// 그리드→이미지 매핑: ExifTool 분리값(가로 260px·세로 286px)과 전체 PDAF 29×17,
// 8256×5504 기준으로 도출 → 405점은 그 안쪽 27×15(가장자리 1줄 제외), 중심=프레임 중앙.
fn nikon_af(nikon_tiff: &[u8]) -> Option<AfInfo> {
    let t = Tiff::new(nikon_tiff)?;
    let ifd0 = t.u32(4)? as usize;
    let (off, len) = t.data(&t.find(ifd0, 0x00b7)?)?; // AFInfo2(0x00b7)
    let ver = nikon_tiff.get(off..off + 4)?;
    // DSLR 153점(D5/D500/D850): AFInfo2 V0101 + FocusPointSchema=7 (#80). 위상차(뷰파인더)
    // 측거점은 미러리스와 달리 픽셀좌표가 없고, 센서에 고정된 물리 그리드(9행 A–I × 17열 1–17)를
    // 20바이트 비트마스크로 지목한다. 아래 V04xx 경로와 레이아웃이 전혀 달라 별도 함수로 뺀다.
    if ver == b"0101" {
        return nikon_af_dslr153(nikon_tiff, off, len);
    }
    // V04xx(미러리스)만 이 레이아웃을 쓴다 — 그 밖의 DSLR(01xx~03xx)은 구조가 전혀 다르므로 제외.
    if len < 0x46 || &ver[..2] != b"04" {
        return None;
    }
    let coords_avail = *nikon_tiff.get(off + 7)?;

    // 단일점(좌표 사용 가능): AFAreaX/YPosition(px, 좌상단 원점) ÷ AFImageWidth/Height.
    // 그리드와 무관 + 범위 검증으로 자체 정합 → 버전 화이트리스트 없이 V04xx 전체 처리.
    if coords_avail == 1 {
        let (iw, ih) = (t.u16(off + 0x3e)? as f64, t.u16(off + 0x40)? as f64);
        let (ax, ay) = (t.u16(off + 0x42)? as f64, t.u16(off + 0x44)? as f64);
        // 크기가 유효하고 측거점이 이미지 안에 있어야 한다. 벗어나면 레이아웃 불일치로 보고
        // 조용히 미표시(clamp로 가짜 점을 그리지 않는다 — 미검증 바디 오표시 방지).
        if iw < 1.0 || ih < 1.0 || ax > iw || ay > ih {
            return None;
        }
        let pt = AfPoint {
            cx: ax / iw,
            cy: ay / ih,
            w: (260.0 / iw).min(0.1), // 박스 ≈ 인접 측거점 간격.
            h: (286.0 / ih).min(0.1),
            in_focus: true,
            selected: true,
        };
        return Some(AfInfo { points: vec![pt], source: "nikon-point" });
    }

    // 오토에어리어/3D 추적: 51바이트 비트마스크 → afPoints405(15행×27열) 활성 점들(존).
    // 그리드 좌표는 Z8/Z9(493점, 0400) 기준이라 다른 V04xx 바디는 None으로 둔다
    // (그리드가 달라 잘못 찍느니 미표시 — 단일점만 위에서 범용 처리).
    if ver != b"0400" {
        return None;
    }
    let mask = nikon_tiff.get(off + 10..off + 10 + 51)?;
    const COLS: usize = 27;
    const ROWS: usize = 15;
    // 전체 PDAF(29열×17행)가 프레임 가로 91.3%·세로 88.3% 커버 → 한 칸 간격/여백.
    let (step_x, step_y) = (0.913 / 29.0, 0.883 / 17.0);
    let (x0, y0) = ((1.0 - 0.913) / 2.0, (1.0 - 0.883) / 2.0);
    let mut points = Vec::new();
    for r in 0..ROWS {
        for c in 0..COLS {
            let i = r * COLS + c;
            if mask[i / 8] >> (i % 8) & 1 == 0 {
                continue;
            }
            // 405 그리드는 전체 29×17의 안쪽(열·행 각 +1) → 중심점이 정확히 0.5.
            points.push(AfPoint {
                cx: x0 + (c as f64 + 1.5) * step_x,
                cy: y0 + (r as f64 + 1.5) * step_y,
                w: step_x * 0.82,
                h: step_y * 0.82,
                in_focus: true,
                selected: false,
            });
        }
    }
    (!points.is_empty()).then_some(AfInfo { points, source: "nikon-zone" })
}

// ── Nikon DSLR 153점: AFInfo2 V0101 + FocusPointSchema=7 — D5/D500/D850 (#80) ────────
// 위상차(뷰파인더) AF는 미러리스처럼 픽셀좌표를 주지 않는다. 대신 센서에 고정된 물리
// 측거점 그리드(9행 A–I × 17열 1–17 = 153점, 중심 E9)를 20바이트 비트마스크로 지목한다.
// 레이아웃(ExifTool Nikon.pm `AFInfo2V0101`, 태그 0x00b7 데이터 선두 기준):
//   off 4  AFDetectionMethod(0=위상차/뷰파인더, 1=콘트라스트/라이브뷰).
//   off 6  FocusPointSchema(7=153점 D5/D500/D850). 다른 값=점 수·기하가 달라 미지원.
//   off 8  AFPointsUsed(20바이트, LSB-first, 153비트).  off 28 AFPointsSelected.  off 48 AFPointsInFocus.
//   off 68 PrimaryAFPoint(int8, 1-based; 0=없음, 1=E9 중심).
// 비트 i(0-based) → ExifTool 키 (i+1) → 점 이름. 키→(행,열)은 센서 스캔 순서를 역산한다:
//   9점씩 한 열, 열 순서 = 중앙에서 바깥(9,10,11,8,7,12,13,14,15,16,17,6,5,4,3,2,1),
//   열 내 행 순서 = E,D,C,B,A,F,G,H,I. 좌표는 실측(8256×5504) 정규화값(Focus-Points 플러그인).
const N153_COL_X: [f64; 17] = [
    0.2235, 0.2489, 0.2743, 0.3352, 0.3609, 0.3867, 0.4484, 0.4742, 0.5000, 0.5258, 0.5516, 0.6133,
    0.6391, 0.6648, 0.7257, 0.7511, 0.7765,
];
// 내측 11열(4~14)과 외측 6열(1,2,3,15,16,17)의 세로 스케일이 미세하게 다르다(실측).
const N153_ROW_Y_INNER: [f64; 9] =
    [0.3503, 0.3877, 0.4251, 0.4626, 0.5000, 0.5374, 0.5749, 0.6123, 0.6497];
const N153_ROW_Y_OUTER: [f64; 9] =
    [0.3612, 0.3959, 0.4306, 0.4653, 0.5000, 0.5347, 0.5694, 0.6041, 0.6388];
// 키(1-based)를 9열 블록으로 나눈 열 순서와 열 내 행 순서(0=A..8=I).
const N153_COL_ORDER: [usize; 17] = [9, 10, 11, 8, 7, 12, 13, 14, 15, 16, 17, 6, 5, 4, 3, 2, 1];
const N153_ROW_ORDER: [usize; 9] = [4, 3, 2, 1, 0, 5, 6, 7, 8];

/// ExifTool 키(1..=153)를 정규화 중심좌표(cx,cy)로. 범위 밖이면 None.
fn nikon_153_center(key: usize) -> Option<(f64, f64)> {
    if !(1..=153).contains(&key) {
        return None;
    }
    let g = (key - 1) / 9; // 열 블록 인덱스(0..16)
    let r = (key - 1) % 9; // 열 내 행 인덱스(0..8)
    let col = N153_COL_ORDER[g]; // 1..=17
    let row = N153_ROW_ORDER[r]; // 0..=8 (A..I)
    let cx = N153_COL_X[col - 1];
    let outer = matches!(col, 1 | 2 | 3 | 15 | 16 | 17);
    let cy = if outer { N153_ROW_Y_OUTER[row] } else { N153_ROW_Y_INNER[row] };
    Some((cx, cy))
}

/// Nikon DSLR 153점(D850 등)의 AFInfo2 V0101을 파싱한다(#80). `off`/`len`은 nikon_tiff 안에서의
/// 0x00b7 데이터 위치·길이. FocusPointSchema≠7(다른 점 수 바디)이나 길이 부족은 None.
fn nikon_af_dslr153(nikon_tiff: &[u8], off: usize, len: usize) -> Option<AfInfo> {
    // PrimaryAFPoint(off 68)까지 읽어야 하므로 최소 69바이트. schema 7만 이 그리드를 쓴다.
    if len < 69 || *nikon_tiff.get(off + 6)? != 7 {
        return None;
    }
    let used = nikon_tiff.get(off + 8..off + 28)?; // AFPointsUsed
    let infocus = nikon_tiff.get(off + 48..off + 68)?; // AFPointsInFocus
    let primary = *nikon_tiff.get(off + 68)? as usize; // 1-based, 0=없음
    let bit = |m: &[u8], i: usize| m[i / 8] >> (i % 8) & 1 == 1;

    // 그릴 대상 = Used ∪ InFocus ∪ {primary}. 각 점의 in_focus=InFocus 소속(또는 primary),
    // selected=primary. 오토에어리어는 여러 점, 단일/그룹은 소수 점이 켜진다.
    let mut points = Vec::new();
    for i in 0..153 {
        let in_used = bit(used, i);
        let in_focus = bit(infocus, i);
        let is_primary = primary != 0 && primary - 1 == i;
        if !in_used && !in_focus && !is_primary {
            continue;
        }
        let Some((cx, cy)) = nikon_153_center(i + 1) else { continue };
        points.push(AfPoint {
            cx,
            cy,
            w: 157.0 / 8256.0, // 실측 측거점 폭·높이(정규화).
            h: 145.0 / 5504.0,
            in_focus: in_focus || is_primary,
            selected: is_primary,
        });
    }
    (!points.is_empty()).then_some(AfInfo { points, source: "nikon-dslr153" })
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

    /// 합성 TIFF에 OM SYSTEM MakerNote(시그니처+내부 IFD+CameraSettings→0x0305 srational[5])를
    /// 넣어 좌표 박스 파싱·범위 검증을 확인한다(#81). 값 오프셋은 MakerNote 시작 기준.
    #[test]
    fn olympus_synthetic() {
        // 선택 AF점 박스 (0.30,0.20)-(0.40,0.28) → 중심(0.35,0.24), 크기(0.10,0.08).
        let info = build_tiff_olympus(0.30, 0.20, 0.40, 0.28);
        let af = parse_tiff(&info).expect("olympus parse");
        assert_eq!(af.source, "olympus");
        assert_eq!(af.points.len(), 1);
        let p = af.points[0];
        assert!((p.cx - 0.35).abs() < 1e-6 && (p.cy - 0.24).abs() < 1e-6, "cx={} cy={}", p.cx, p.cy);
        assert!((p.w - 0.10).abs() < 1e-6 && (p.h - 0.08).abs() < 1e-6);
        assert!(p.in_focus && p.selected);
        // 범위 밖(1.5) 좌표 → 미표시.
        assert!(parse_tiff(&build_tiff_olympus(0.3, 0.2, 1.5, 0.28)).is_none());
    }

    /// 합성 TIFF에 Pentax MakerNote(AFPointInfo 0x0245: ver+num=33+2비트/점 비트필드)를 넣어
    /// K-1 33점 디코딩·좌표 매핑·선택/합초 비트를 검증한다(#82).
    #[test]
    fn pentax_k1_synthetic() {
        // 점17(Center, i=16) 필드=3(선택+합초), 점1(Top-left, i=0) 필드=1(선택만).
        let info = build_tiff_pentax(&[(17, 3), (1, 1)]);
        let af = parse_tiff(&info).expect("pentax parse");
        assert_eq!(af.source, "pentax");
        assert_eq!(af.points.len(), 2);
        // 점17 = 프레임 정중앙, 합초.
        let c = af.points.iter().find(|p| p.in_focus).expect("in-focus point");
        assert!((c.cx - 0.5).abs() < 1e-4 && (c.cy - 0.5).abs() < 1e-4, "cx={} cy={}", c.cx, c.cy);
        assert!(c.selected);
        // 점1 = 좌상단(2797/7360, 1818/4912), 선택만(합초 아님).
        let tl = af.points.iter().find(|p| !p.in_focus).expect("selected-only point");
        assert!((tl.cx - 2797.0 / 7360.0).abs() < 1e-4 && (tl.cy - 1818.0 / 4912.0).abs() < 1e-4);
        // 아무 점도 선택 안 됨 → None.
        assert!(parse_tiff(&build_tiff_pentax(&[])).is_none());
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

    /// LE TIFF + OM SYSTEM MakerNote. 구조: IFD0[Make,Model,ExifIFD] → ExifIFD[MakerNote]
    /// → MakerNote["OM SYSTEM\0\0\0"(12)+"II"+ver(4)+IFD(0x2020)] → CameraSettings[0x0305].
    /// MakerNote 내부 오프셋은 모두 MakerNote 시작(mn_off) 기준.
    fn build_tiff_olympus(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        put16(&mut b, 42);
        put32(&mut b, 8);
        let (make, model): (&[u8], &[u8]) = (b"OM SYSTEM", b"OM-1");
        let make_n = make.len() + 1;
        let model_n = model.len() + 1;
        // IFD0: 3엔트리 = 2+36+4 = 42 → 8..50. 문자열 데이터 뒤 ExifIFD.
        let exif_ifd = 50 + make_n + model_n;
        let mn_off = exif_ifd + 18; // ExifIFD 1엔트리.
        // MakerNote 내부(모두 mn_off 상대):
        let mn_ifd = 16; // 시그니처12 + "II"+ver 4.
        let cs_ifd = mn_ifd + 18; // MakerNote IFD 1엔트리.
        let data = cs_ifd + 18; // CameraSettings 1엔트리.
        let mn_len = data + 40; // srational[5].
        // IFD0.
        put16(&mut b, 3);
        put16(&mut b, 0x010F); put16(&mut b, 2); put32(&mut b, make_n as u32); put32(&mut b, 50);
        put16(&mut b, 0x0110); put16(&mut b, 2); put32(&mut b, model_n as u32); put32(&mut b, (50 + make_n) as u32);
        put16(&mut b, 0x8769); put16(&mut b, 4); put32(&mut b, 1); put32(&mut b, exif_ifd as u32);
        put32(&mut b, 0);
        b.extend_from_slice(make); b.push(0);
        b.extend_from_slice(model); b.push(0);
        // ExifIFD.
        assert_eq!(b.len(), exif_ifd);
        put16(&mut b, 1);
        put16(&mut b, 0x927C); put16(&mut b, 7); put32(&mut b, mn_len as u32); put32(&mut b, mn_off as u32);
        put32(&mut b, 0);
        // MakerNote.
        assert_eq!(b.len(), mn_off);
        b.extend_from_slice(b"OM SYSTEM\0\0\0");
        b.extend_from_slice(b"II");
        put16(&mut b, 4); // 버전.
        // MakerNote IFD: 0x2020 CameraSettings(LONG, 오프셋 mn 상대).
        put16(&mut b, 1);
        put16(&mut b, 0x2020); put16(&mut b, 4); put32(&mut b, 1); put32(&mut b, cs_ifd as u32);
        put32(&mut b, 0);
        // CameraSettings IFD: 0x0305 AFPointSelected(SRATIONAL, count5).
        assert_eq!(b.len(), mn_off + cs_ifd);
        put16(&mut b, 1);
        put16(&mut b, 0x0305); put16(&mut b, 10); put32(&mut b, 5); put32(&mut b, data as u32);
        put32(&mut b, 0);
        // 데이터: [undef, x0, y0, x1, y1] (srational, 분모 1e6; undef은 0/0).
        assert_eq!(b.len(), mn_off + data);
        put32(&mut b, 0); put32(&mut b, 0); // undef 플래그.
        for v in [x0, y0, x1, y1] {
            put32(&mut b, (v * 1_000_000.0).round() as i32 as u32);
            put32(&mut b, 1_000_000);
        }
        b
    }

    /// LE TIFF + Pentax MakerNote. IFD0[Make,Model,ExifIFD] → ExifIFD[MakerNote] →
    /// MakerNote["PENTAX \0"(8)+"II"(2)+IFD(0x0245)] → AFPointInfo[ver=1,num=33,int8u[9]].
    /// 내부 오프셋은 MakerNote 시작(mn_off) 기준. `pts`=(점번호1..33, 2비트필드값).
    fn build_tiff_pentax(pts: &[(usize, u8)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        put16(&mut b, 42);
        put32(&mut b, 8);
        let (make, model): (&[u8], &[u8]) = (b"RICOH", b"PENTAX K-1");
        let make_n = make.len() + 1;
        let model_n = model.len() + 1;
        let exif_ifd = 50 + make_n + model_n; // IFD0 3엔트리 = 42 → 8..50.
        let mn_off = exif_ifd + 18;
        // MakerNote 내부(mn 상대): "PENTAX \0"(8)+"II"(2)=10, IFD 1엔트리=18 → data@28.
        let data_rel = 10 + 18;
        let mn_len = data_rel + 13; // ver2 + num2 + int8u[9].
        // IFD0.
        put16(&mut b, 3);
        put16(&mut b, 0x010F); put16(&mut b, 2); put32(&mut b, make_n as u32); put32(&mut b, 50);
        put16(&mut b, 0x0110); put16(&mut b, 2); put32(&mut b, model_n as u32); put32(&mut b, (50 + make_n) as u32);
        put16(&mut b, 0x8769); put16(&mut b, 4); put32(&mut b, 1); put32(&mut b, exif_ifd as u32);
        put32(&mut b, 0);
        b.extend_from_slice(make); b.push(0);
        b.extend_from_slice(model); b.push(0);
        // ExifIFD.
        assert_eq!(b.len(), exif_ifd);
        put16(&mut b, 1);
        put16(&mut b, 0x927C); put16(&mut b, 7); put32(&mut b, mn_len as u32); put32(&mut b, mn_off as u32);
        put32(&mut b, 0);
        // MakerNote.
        assert_eq!(b.len(), mn_off);
        b.extend_from_slice(b"PENTAX \0");
        b.extend_from_slice(b"II");
        // MakerNote IFD: 0x0245 AFPointInfo(UNDEF, count=13, 오프셋 mn 상대).
        put16(&mut b, 1);
        put16(&mut b, 0x0245); put16(&mut b, 7); put32(&mut b, 13); put32(&mut b, data_rel as u32);
        put32(&mut b, 0);
        // AFPointInfo 데이터.
        assert_eq!(b.len(), mn_off + data_rel);
        put16(&mut b, 1); // version
        put16(&mut b, 33); // NumAFPoints
        let mut field = [0u8; 9];
        for &(pt, f) in pts {
            let i = pt - 1;
            field[i / 4] |= (f & 0x03) << (6 - 2 * (i % 4));
        }
        b.extend_from_slice(&field);
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

    /// Nikon 임베드 TIFF(LE): IFD0에 AFInfo2(0x00b7=undef[blob]) 하나만 둔 최소 구조.
    /// nikon_af의 입력(= Type3 MakerNote의 오프셋 10부터)과 동일한 형태.
    fn nikon_embedded_tiff(blob: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II*\0");
        b.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
        b.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        let val_off = 8 + 2 + 12 + 4; // header+count+entry+next-ifd
        b.extend_from_slice(&0x00b7u16.to_le_bytes()); // AFInfo2
        b.extend_from_slice(&7u16.to_le_bytes()); // undef
        b.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        b.extend_from_slice(&(val_off as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        b.extend_from_slice(blob);
        b
    }

    /// Nikon Z8 AFInfo2 V0400 — 오토에어리어 비트마스크(존)와 단일점·버전 게이트 검증(#45).
    #[test]
    fn nikon_v0400_zone_and_point() {
        // 오토에어리어: 측거점 (r0,c0)·(r1,c1) 두 개를 LSB-first 비트마스크로 세팅.
        let mut blob = vec![0u8; 80];
        blob[0..4].copy_from_slice(b"0400");
        blob[5] = 197; // AFAreaMode=Auto
        blob[7] = 0; // AFCoordinatesAvailable=0 → 비트마스크 사용
        for i in [0usize, 27 + 1] {
            blob[10 + i / 8] |= 1 << (i % 8);
        }
        let info = nikon_af(&nikon_embedded_tiff(&blob)).expect("nikon zone");
        assert_eq!(info.source, "nikon-zone");
        assert_eq!(info.points.len(), 2);
        let (sx, sy) = (0.913 / 29.0, 0.883 / 17.0);
        let (x0, y0) = ((1.0 - 0.913) / 2.0, (1.0 - 0.883) / 2.0);
        let p0 = info.points[0];
        assert!((p0.cx - (x0 + 1.5 * sx)).abs() < 1e-9 && (p0.cy - (y0 + 1.5 * sy)).abs() < 1e-9);
        assert!(p0.in_focus);

        // 단일점: 좌표 사용 가능 → AFAreaX/YPosition ÷ AFImageWidth/Height.
        let mut blob = vec![0u8; 80];
        blob[0..4].copy_from_slice(b"0400");
        blob[7] = 1; // coords available
        let put = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        put(&mut blob, 0x3e, 8256); // AFImageWidth
        put(&mut blob, 0x40, 5504); // AFImageHeight
        put(&mut blob, 0x42, 4128); // X = 중앙
        put(&mut blob, 0x44, 2752); // Y = 중앙
        let info = nikon_af(&nikon_embedded_tiff(&blob)).expect("nikon point");
        assert_eq!(info.source, "nikon-point");
        assert_eq!(info.points.len(), 1);
        assert!((info.points[0].cx - 0.5).abs() < 1e-6 && (info.points[0].cy - 0.5).abs() < 1e-6);

        // 버전 게이트: 0401(Z6III/Zf 등)은 그리드가 달라 None.
        let mut other = vec![0u8; 80];
        other[0..4].copy_from_slice(b"0401");
        assert!(nikon_af(&nikon_embedded_tiff(&other)).is_none());
    }

    /// Nikon Z5II AFInfo2 V0402 — 단일점(픽셀좌표)은 그리드와 무관해 0400과 동일 처리(#50).
    /// 실제 z5ii_sample/DSC_0040.NEF 값(6048×4032, X=3274,Y=2176)으로 회귀 고정.
    #[test]
    fn nikon_v0402_z5ii_point() {
        let mut blob = vec![0u8; 80];
        blob[0..4].copy_from_slice(b"0402");
        blob[5] = 207; // AFAreaMode=3D-tracking
        blob[7] = 1; // AFCoordinatesAvailable=1 → 단일점 픽셀좌표
        let put = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        put(&mut blob, 0x3e, 6048); // AFImageWidth
        put(&mut blob, 0x40, 4032); // AFImageHeight
        put(&mut blob, 0x42, 3274); // AFAreaXPosition
        put(&mut blob, 0x44, 2176); // AFAreaYPosition
        let info = nikon_af(&nikon_embedded_tiff(&blob)).expect("z5ii point");
        assert_eq!(info.source, "nikon-point");
        assert_eq!(info.points.len(), 1);
        let p = info.points[0];
        assert!((p.cx - 3274.0 / 6048.0).abs() < 1e-6 && (p.cy - 2176.0 / 4032.0).abs() < 1e-6);
        assert!(p.in_focus && p.selected);

        // 0402의 존 비트마스크 경로는 그리드 미검증 → None(잘못 찍느니 미표시).
        let mut zone = vec![0u8; 80];
        zone[0..4].copy_from_slice(b"0402");
        zone[7] = 0; // 비트마스크 사용
        zone[10] |= 1; // 측거점 하나 켬
        assert!(nikon_af(&nikon_embedded_tiff(&zone)).is_none());
    }

    /// 미검증 V04xx 바디라도 단일점(픽셀좌표)은 그리드 무관 + 범위검증으로 안전 표시(#50).
    #[test]
    fn nikon_v04xx_unknown_body_point() {
        let put = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let mk = |ver: &[u8], iw: u16, ih: u16, x: u16, y: u16| {
            let mut blob = vec![0u8; 80];
            blob[0..4].copy_from_slice(ver);
            blob[7] = 1; // 단일점
            put(&mut blob, 0x3e, iw);
            put(&mut blob, 0x40, ih);
            put(&mut blob, 0x42, x);
            put(&mut blob, 0x44, y);
            blob
        };

        // 화이트리스트에 없는 가상의 미래 버전 — 단일점이면 표시된다.
        let info = nikon_af(&nikon_embedded_tiff(&mk(b"0408", 8256, 5504, 4128, 2752)))
            .expect("unknown 04xx point");
        assert_eq!(info.source, "nikon-point");
        assert!((info.points[0].cx - 0.5).abs() < 1e-6 && (info.points[0].cy - 0.5).abs() < 1e-6);

        // 범위 밖 좌표(레이아웃 불일치 신호) → clamp로 가짜 점 그리지 않고 None.
        assert!(nikon_af(&nikon_embedded_tiff(&mk(b"0408", 8256, 5504, 9000, 2752))).is_none());

        // DSLR 계열(0300 등)은 레이아웃 자체가 달라 단일점이라도 제외(0101=153점만 별도 처리).
        assert!(nikon_af(&nikon_embedded_tiff(&mk(b"0300", 6000, 4000, 3000, 2000))).is_none());
    }

    /// Nikon DSLR 153점(D850 등) AFInfo2 V0101 — 물리 그리드 매핑·primary·schema 게이트 검증(#80).
    /// 실제 d850 sample 구조(schema=7, 20바이트 마스크 3개 @8/28/48 + PrimaryAFPoint @68)를 합성 재현.
    #[test]
    fn nikon_v0101_d850_153point() {
        let mk = |set_bits: &[usize], primary: u8, schema: u8| {
            let mut blob = vec![0u8; 84];
            blob[0..4].copy_from_slice(b"0101");
            blob[4] = 0; // AFDetectionMethod = 위상차(뷰파인더)
            blob[5] = 8; // AFAreaMode = Auto-area
            blob[6] = schema; // FocusPointSchema
            for &b in set_bits {
                blob[8 + b / 8] |= 1 << (b % 8); // AFPointsUsed
                blob[48 + b / 8] |= 1 << (b % 8); // AFPointsInFocus
            }
            blob[68] = primary; // PrimaryAFPoint(1-based)
            blob
        };

        // 중심 E9(키 1 → 비트 0) + primary=1 → 정확히 (0.5, 0.5), 합초·선택.
        let info = nikon_af(&nikon_embedded_tiff(&mk(&[0], 1, 7))).expect("d850 153 center");
        assert_eq!(info.source, "nikon-dslr153");
        let c = *info.points.iter().find(|p| p.selected).expect("primary point");
        assert!((c.cx - 0.5).abs() < 1e-6 && (c.cy - 0.5).abs() < 1e-6);
        assert!(c.in_focus && c.selected);

        // 좌상단 코너 A1(키 149 → 비트 148): 외측 열 y·좌측 열 x = (0.2235, 0.3612).
        let info = nikon_af(&nikon_embedded_tiff(&mk(&[148], 0, 7))).expect("d850 153 corner");
        assert_eq!(info.points.len(), 1);
        let p = info.points[0];
        assert!((p.cx - 0.2235).abs() < 1e-6 && (p.cy - 0.3612).abs() < 1e-6);

        // FocusPointSchema≠7(51점 D7500 등 다른 바디)은 그리드가 달라 미표시(잘못 찍느니 안 그림).
        assert!(nikon_af(&nikon_embedded_tiff(&mk(&[0], 1, 1))).is_none());
    }
}
