//! EXIF 추출 (F8 오버레이·촬영시간순 정렬·Orientation). kamadak-exif 사용.
//!
//! RW2 등 TIFF 기반 RAW는 컨테이너에서 직접 읽고, 캐논 CR3(ISO BMFF)는
//! [`crate::bmff`]로 `moov` 안의 CMT 블록을 표준 TIFF로 합성해 읽으며,
//! 그래도 안 되면 임베디드 JPEG의 EXIF로 폴백한다.

use crate::model::{kind_of, Kind};
use exif::{Exif, In, Tag, Value};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ExifInfo {
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub focal_length: Option<String>,
    pub aperture: Option<String>,
    pub shutter: Option<String>,
    pub iso: Option<String>,
    pub exposure_bias: Option<String>,
    pub datetime: Option<String>,
    pub white_balance: Option<String>,
    /// EXIF에 기록된 **미회전(센서 기준)** 픽셀 크기. 세로 컷도 가로 숫자로 들어온다.
    /// 화면에 보이는 방향으로 판단하려면 `orientation`과 함께 봐야 한다(`display_size`).
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// EXIF Orientation(1..8). 읽지 못하면 1.
    pub orientation: u16,
    /// 촬영 위치(#38). 표준 EXIF GPS IFD에서 추출. 없으면 None(대부분의 RAW).
    pub gps: Option<GpsCoord>,
}

/// GPS 좌표(#38). 위경도는 십진수(deg), 남위/서경은 음수.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpsCoord {
    pub lat: f64,
    pub lon: f64,
    /// 고도(m). GPSAltitudeRef=1(해수면 아래)이면 음수.
    pub alt: Option<f64>,
}

/// `orientation`만 1(정상)로, 나머지는 비어 있는 기본값. 파생 Default는 0을 주는데
/// 0은 유효한 Orientation이 아니다.
impl Default for ExifInfo {
    fn default() -> Self {
        ExifInfo {
            camera: None,
            lens: None,
            focal_length: None,
            aperture: None,
            shutter: None,
            iso: None,
            exposure_bias: None,
            datetime: None,
            white_balance: None,
            width: None,
            height: None,
            orientation: 1,
            gps: None,
        }
    }
}

impl ExifInfo {
    /// 정보가 하나도 없으면 true.
    pub fn is_empty(&self) -> bool {
        self.camera.is_none()
            && self.lens.is_none()
            && self.aperture.is_none()
            && self.shutter.is_none()
            && self.iso.is_none()
            && self.datetime.is_none()
            && self.width.is_none()
            && self.gps.is_none()
    }

    /// 화면에 보이는 방향의 (가로, 세로). EXIF의 width/height는 센서 기준(미회전)이라
    /// 세로 컷도 가로 숫자로 들어온다 — Orientation 5..8이면 맞바꾼다.
    /// 디코딩 결과(`decode::finish` → `apply_orientation`)와 같은 방향을 가리킨다.
    pub fn display_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.width?, self.height?);
        Some(if (5..=8).contains(&self.orientation) { (h, w) } else { (w, h) })
    }
}

/// 경로에서 EXIF를 읽는다.
///
/// 렌즈명은 표준 태그(`LensModel` 0xA434)가 없는 바디가 많아, 비면 MakerNote에서
/// 한 번 더 찾는다([`crate::af::parse_lens`] — 캐논 0x0095 / 올림푸스 Equipment 0x0203).
pub fn read_exif(path: &Path) -> Option<ExifInfo> {
    let mut info = read_exif_inner(path)?;
    if info.lens.is_none() {
        info.lens = crate::af::parse_lens(path);
    }
    Some(info)
}

fn read_exif_inner(path: &Path) -> Option<ExifInfo> {
    if let Some(info) = read_container(path) {
        if !info.is_empty() {
            return Some(info);
        }
    }
    if kind_of(path) == Some(Kind::Raw) {
        // 캐논 CR3: TIFF가 아니라 ISO BMFF라 kamadak도 IFD0 직독도 못 쓴다. `moov` 안의
        // CMT1(IFD0)·CMT2(Exif IFD)·CMT4(GPS)를 표준 EXIF TIFF로 합성해 읽는다.
        // (이게 없으면 CR3는 EXIF가 통째로 비어 F8 오버레이·촬영시간순 정렬이 죽는다.)
        if let Some(info) = read_bmff_exif(path) {
            if !info.is_empty() {
                return Some(info);
            }
        }
        // 올림푸스/OM SYSTEM ORF 등 "매직만 비표준"인 TIFF 계열 RAW: 버전 필드를
        // 표준값(42)으로 고쳐 컨테이너를 직접 읽는다(임베디드 JPEG에 EXIF가 없음).
        if let Some(info) = read_nonstandard_tiff(path) {
            return Some(info);
        }
    }
    // RAW(RW2 등): kamadak가 컨테이너(매직 0x55)를 못 읽으므로 임베디드 JPEG의 EXIF로 폴백.
    // 가장 큰 임베디드(파일 끝의 거대한 프리뷰)에는 EXIF가 없고, 앞쪽 1920급 프리뷰
    // (보통 오프셋 ~6KB)에 APP1 EXIF가 들어있다 → 임베디드 JPEG들을 차례로 시도한다.
    if kind_of(path) == Some(Kind::Raw) {
        // 앞 2MB만 읽어도 EXIF 품은 프리뷰를 보통 포함(56MB 전체 읽기 회피).
        if let Ok(prefix) = read_prefix(path, 2 * 1024 * 1024) {
            if let Some(info) = scan_embedded_exif(&prefix) {
                return Some(info);
            }
        }
        // 드물게 앞부분에 없으면 전체에서 재스캔.
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(info) = scan_embedded_exif(&bytes) {
                return Some(info);
            }
        }
    }
    None
}

/// 파일 앞부분 최대 `max` 바이트만 읽는다.
fn read_prefix(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?
        .take(max as u64)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

/// 바이트 안의 임베디드 JPEG(SOI=FF D8 FF)들을 차례로 시도해, EXIF(APP1)를 담은
/// 첫 번째 것의 정보를 반환한다.
fn scan_embedded_exif(bytes: &[u8]) -> Option<ExifInfo> {
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            if let Some(info) = read_bytes(&bytes[i..]) {
                if !info.is_empty() {
                    return Some(info);
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// Orientation 검출에 쓰는 헤더 프리픽스(IFD0는 항상 파일 선두 근처에 있다).
const ORIENT_HEADER: usize = 64 * 1024;
/// 임베디드 JPEG EXIF 폴백 스캔 범위. read_exif와 같은 값 — EXIF를 품은 프리뷰는
/// 파일 앞쪽(보통 수십 KB)에 있다.
const ORIENT_SCAN_PREFIX: usize = 2 * 1024 * 1024;

/// EXIF Orientation(1..8)을 읽는다. 없으면 1(정상).
///
/// **디코딩마다(썸네일·프리뷰·원본) 호출되는 핫패스**라 읽는 양을 유계로 묶는다.
/// 순서가 곧 비용 순서다:
/// 1. TIFF 계열 헤더(II/MM) IFD0 직독 — 앞 64KB. CR2·ARW·NEF·DNG·PEF·SRW·TIF는
///    물론 매직만 다른 RW2(0x55)·ORF('RO'/'RS')까지 여기서 끝난다. kamadak의
///    `read_from_container`는 TIFF로 판정되면 **파일 전체**를 메모리로 읽어버려
///    (kamadak-exif reader.rs: `if tiff::is_tiff(..) { reader.read_to_end(..) }`)
///    85MB ARW 한 장에 24ms가 든다 — 그래서 kamadak보다 **앞에** 둔다.
/// 2. ISO BMFF(캐논 CR3) — `moov` 안 CMT1. TIFF도 kamadak도 못 읽던 구멍이라
///    세로 사진이 가로로 표시됐다(EOS R6 Mark III 제보).
/// 3. kamadak 컨테이너 — JPEG(APP1만 읽음)·PNG·WebP·HEIC/HEIF, 그리고 IFD0에
///    태그가 없던 TIFF 계열의 보험.
/// 4. 마지막 폴백: 앞 2MB 안의 임베디드 JPEG APP1(EXIF). 완전한 SOI..EOI가
///    아니어도 APP1만 있으면 읽히므로 잘린 프리뷰에도 강하다(RAF 등).
pub fn orientation(path: &Path) -> u16 {
    fn from_exif(exif: &Exif) -> Option<u16> {
        let v = exif
            .get_field(Tag::Orientation, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))? as u16;
        (1..=8).contains(&v).then_some(v)
    }
    let head = read_prefix(path, ORIENT_HEADER).unwrap_or_default();
    // 1) TIFF 계열(표준 0x2A + RW2 0x55 + ORF 'RO'/'RS') IFD0 직독.
    if let Some(o) = tiff_ifd0_orientation(&head) {
        return o;
    }
    // 2) ISO BMFF(CR3).
    if crate::bmff::is_bmff(&head) {
        if let Some(o) = crate::bmff::cr3_orientation(path) {
            return o;
        }
    }
    // 3) 표준 컨테이너: kamadak.
    if let Ok(file) = std::fs::File::open(path) {
        let mut reader = std::io::BufReader::new(file);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) {
            if let Some(o) = from_exif(&exif) {
                return o;
            }
        }
    }
    // 4) RAW의 임베디드 JPEG EXIF(앞 2MB).
    if kind_of(path) == Some(Kind::Raw) {
        let prefix = if head.len() >= ORIENT_SCAN_PREFIX {
            head
        } else {
            read_prefix(path, ORIENT_SCAN_PREFIX).unwrap_or(head)
        };
        if let Some(o) = scan_embedded_orientation(&prefix) {
            return o;
        }
    }
    1
}

/// 프리픽스 안의 임베디드 JPEG(SOI)들을 차례로 시도해 첫 Orientation을 찾는다.
/// kamadak의 JPEG 경로는 APP1까지만 읽으므로 뒤가 잘려 있어도 동작한다.
fn scan_embedded_orientation(bytes: &[u8]) -> Option<u16> {
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            let mut c = std::io::Cursor::new(&bytes[i..]);
            if let Ok(exif) = exif::Reader::new().read_from_container(&mut c) {
                let v = exif
                    .get_field(Tag::Orientation, In::PRIMARY)
                    .and_then(|f| f.value.get_uint(0))
                    .map(|v| v as u16);
                if let Some(v) = v.filter(|v| (1..=8).contains(v)) {
                    return Some(v);
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// TIFF 계열 헤더 바이트의 IFD0에서 Orientation(0x0112)을 직접 읽는다.
/// 표준 TIFF(매직 0x2A)·파나소닉 RW2(0x55)·올림푸스 ORF('RO'/'RS')를 모두 처리한다
/// (매직 검사는 생략하고 II/MM + IFD 구조 유효성으로만 판단).
fn tiff_ifd0_orientation(buf: &[u8]) -> Option<u16> {
    if buf.len() < 8 {
        return None;
    }
    let le = match &buf[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r16 = |o: usize| -> Option<u16> {
        let b = buf.get(o..o + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let r32 = |o: usize| -> Option<u32> {
        let b = buf.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let ifd0 = r32(4)? as usize;
    let count = r16(ifd0)? as usize;
    for i in 0..count.min(512) {
        let e = ifd0 + 2 + i * 12;
        let tag = match r16(e) {
            Some(t) => t,
            None => break,
        };
        if tag == 0x0112 {
            // SHORT 값은 value 필드 선두(e+8)에 인라인 저장.
            let v = r16(e + 8)?;
            if (1..=8).contains(&v) {
                return Some(v);
            }
        }
    }
    None
}

fn read_container(path: &Path) -> Option<ExifInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    Some(build(&exif))
}

fn read_bytes(bytes: &[u8]) -> Option<ExifInfo> {
    let mut cursor = std::io::Cursor::new(bytes);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    Some(build(&exif))
}

/// ISO BMFF(캐논 CR3)의 EXIF. 합성된 표준 TIFF를 kamadak로 파싱한다.
fn read_bmff_exif(path: &Path) -> Option<ExifInfo> {
    let raw = crate::bmff::cr3_exif_tiff(path)?;
    read_raw_exif(raw)
}

/// 올림푸스/OM SYSTEM ORF처럼 표준 TIFF 구조인데 버전 필드만 비표준인 RAW를 읽는다.
///
/// ORF는 IFD0/EXIF IFD 레이아웃이 표준 TIFF와 같지만, 헤더의 버전 필드가 42(0x2A)가
/// 아니라 'RO'(0x4F52) 또는 'RS'(0x5352)라서 kamadak가 "Unknown image format"으로
/// 거부한다. 버전 필드만 42로 고쳐 read_raw로 넘기면 그대로 파싱된다.
///
/// EXIF IFD와 그 값들은 파일 앞쪽(거대한 센서 데이터 앞)에 모여 있어 2MB 프리픽스로
/// 충분하다. 드물게 값 오프셋이 프리픽스를 넘어가면 전체를 읽어 재시도한다.
fn read_nonstandard_tiff(path: &Path) -> Option<ExifInfo> {
    let mut data = read_prefix(path, 2 * 1024 * 1024).ok()?;
    if !patch_tiff_version(&mut data) {
        return None;
    }
    if let Some(info) = read_raw_exif(data) {
        if !info.is_empty() {
            return Some(info);
        }
    }
    let mut all = std::fs::read(path).ok()?;
    if !patch_tiff_version(&mut all) {
        return None;
    }
    read_raw_exif(all).filter(|i| !i.is_empty())
}

/// TIFF 헤더(II/MM + 버전)를 검사해, 올림푸스 ORF 계열의 비표준 버전('RO'/'RS')이면
/// 표준 TIFF 버전(42)으로 바꾼다. 표준 TIFF나 다른 포맷이면 건드리지 않고 false.
fn patch_tiff_version(data: &mut [u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    let le = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return false,
    };
    let ver = if le {
        u16::from_le_bytes([data[2], data[3]])
    } else {
        u16::from_be_bytes([data[2], data[3]])
    };
    // ORF 시그니처: IIRO/MMOR = 0x4F52('RO'), IIRS = 0x5352('RS'). 이것만 표준화한다.
    if ver != 0x4F52 && ver != 0x5352 {
        return false;
    }
    if le {
        data[2] = 0x2A;
        data[3] = 0x00;
    } else {
        data[2] = 0x00;
        data[3] = 0x2A;
    }
    true
}

fn read_raw_exif(data: Vec<u8>) -> Option<ExifInfo> {
    let exif = exif::Reader::new().read_raw(data).ok()?;
    Some(build(&exif))
}

fn disp(exif: &Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    // ASCII 필드는 display_value가 ASCII 배열을 `"a", "b", "c"`로 직렬화하기
    // 때문에, 니콘처럼 LensModel 뒤에 빈 항목이 붙는 카메라에서 따옴표 꼬리가
    // 그대로 노출된다. Value::Ascii를 직접 처리해 첫 비어있지 않은 항목만 쓴다.
    if let Value::Ascii(parts) = &field.value {
        for raw in parts {
            let s = String::from_utf8_lossy(raw).trim().trim_end_matches('\0').trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
        return None;
    }
    // 그 외 타입은 kamadak 표시값에서 양끝 따옴표만 제거.
    Some(
        field
            .display_value()
            .with_unit(exif)
            .to_string()
            .trim_matches('"')
            .to_string(),
    )
}

fn uint(exif: &Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
}

fn build(exif: &Exif) -> ExifInfo {
    let camera = disp(exif, Tag::Model).or_else(|| disp(exif, Tag::Make));
    ExifInfo {
        camera,
        lens: disp(exif, Tag::LensModel),
        focal_length: disp(exif, Tag::FocalLength),
        aperture: disp(exif, Tag::FNumber),
        shutter: disp(exif, Tag::ExposureTime),
        iso: disp(exif, Tag::PhotographicSensitivity)
            .or_else(|| disp(exif, Tag::ISOSpeed)),
        exposure_bias: disp(exif, Tag::ExposureBiasValue),
        datetime: disp(exif, Tag::DateTimeOriginal).or_else(|| disp(exif, Tag::DateTime)),
        white_balance: disp(exif, Tag::WhiteBalance),
        width: uint(exif, Tag::PixelXDimension).or_else(|| uint(exif, Tag::ImageWidth)),
        height: uint(exif, Tag::PixelYDimension).or_else(|| uint(exif, Tag::ImageLength)),
        // width/height가 센서 기준(미회전)이라, 이걸로 가로/세로를 판정하려면 필수다.
        // 표시 방향(`decode`의 apply_orientation)과 같은 태그를 본다.
        orientation: uint(exif, Tag::Orientation)
            .map(|v| v as u16)
            .filter(|v| (1..=8).contains(v))
            .unwrap_or(1),
        gps: gps_coord(exif),
    }
}

/// GPS IFD에서 위경도/고도를 십진수로 추출(#38). 위경도는 도/분/초 rational ×3 +
/// 반구 기호(N/S, E/W). 어느 하나라도 깨져 있으면 None(조용히 미표시).
fn gps_coord(exif: &Exif) -> Option<GpsCoord> {
    fn dms(exif: &Exif, tag: Tag) -> Option<f64> {
        let f = exif.get_field(tag, In::PRIMARY)?;
        if let Value::Rational(r) = &f.value {
            let d = r.first()?.to_f64();
            let m = r.get(1).map(|v| v.to_f64()).unwrap_or(0.0);
            let s = r.get(2).map(|v| v.to_f64()).unwrap_or(0.0);
            let v = d + m / 60.0 + s / 3600.0;
            return v.is_finite().then_some(v);
        }
        None
    }
    fn hemi(exif: &Exif, tag: Tag) -> Option<char> {
        let f = exif.get_field(tag, In::PRIMARY)?;
        if let Value::Ascii(parts) = &f.value {
            return parts.first().and_then(|p| p.first()).map(|b| (*b as char).to_ascii_uppercase());
        }
        None
    }
    let mut lat = dms(exif, Tag::GPSLatitude)?;
    let mut lon = dms(exif, Tag::GPSLongitude)?;
    if hemi(exif, Tag::GPSLatitudeRef) == Some('S') {
        lat = -lat;
    }
    if hemi(exif, Tag::GPSLongitudeRef) == Some('W') {
        lon = -lon;
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    // (0,0)은 대서양 한복판 — 실촬영이 아니라 GPS 미수신 기본값일 확률이 압도적이라 버린다.
    if lat == 0.0 && lon == 0.0 {
        return None;
    }
    let alt = exif.get_field(Tag::GPSAltitude, In::PRIMARY).and_then(|f| {
        if let Value::Rational(r) = &f.value {
            let v = r.first()?.to_f64();
            let below = exif
                .get_field(Tag::GPSAltitudeRef, In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                == Some(1);
            v.is_finite().then_some(if below { -v } else { v })
        } else {
            None
        }
    });
    Some(GpsCoord { lat, lon, alt })
}
