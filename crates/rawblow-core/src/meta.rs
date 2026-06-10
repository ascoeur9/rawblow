//! EXIF 추출 (F8 오버레이용). kamadak-exif 사용.
//!
//! RW2 등 TIFF 기반 RAW는 컨테이너에서 직접 읽고, 실패하면 임베디드 JPEG의
//! EXIF로 폴백한다.

use crate::model::{kind_of, Kind};
use exif::{Exif, In, Tag, Value};
use std::path::Path;

#[derive(Clone, Debug, Default)]
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
    pub width: Option<u32>,
    pub height: Option<u32>,
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
}

/// 경로에서 EXIF를 읽는다.
pub fn read_exif(path: &Path) -> Option<ExifInfo> {
    if let Some(info) = read_container(path) {
        if !info.is_empty() {
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

/// EXIF Orientation(1..8)을 읽는다. 없으면 1(정상).
///
/// 일반 이미지·TIFF 기반 RAW(RW2 등)는 컨테이너 IFD0에서 직접 읽고,
/// 실패하면 RAW의 임베디드 JPEG EXIF로 폴백한다.
pub fn orientation(path: &Path) -> u16 {
    fn from_exif(exif: &Exif) -> Option<u16> {
        let v = exif
            .get_field(Tag::Orientation, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))? as u16;
        (1..=8).contains(&v).then_some(v)
    }
    // 1) 표준 컨테이너(JPG/TIFF/표준 DNG): kamadak.
    if let Ok(file) = std::fs::File::open(path) {
        let mut reader = std::io::BufReader::new(file);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) {
            if let Some(o) = from_exif(&exif) {
                return o;
            }
        }
    }
    if kind_of(path) == Some(Kind::Raw) {
        // 2) RW2(파나소닉 매직 0x55) 등은 kamadak이 못 읽으므로 IFD0를 직접 파싱.
        if let Some(o) = read_tiff_ifd0_orientation(path) {
            return o;
        }
        // 3) 마지막 폴백: 임베디드 JPEG의 EXIF.
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(jpeg) = crate::decode::extract_embedded_jpeg(&bytes) {
                let mut cursor = std::io::Cursor::new(jpeg);
                if let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) {
                    if let Some(o) = from_exif(&exif) {
                        return o;
                    }
                }
            }
        }
    }
    1
}

/// TIFF 계열 헤더의 IFD0에서 Orientation(0x0112)을 직접 읽는다.
/// 표준 TIFF(매직 0x2A)와 파나소닉 RW2(매직 0x55) 모두 처리(매직 검사 생략).
fn read_tiff_ifd0_orientation(path: &Path) -> Option<u16> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
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
    for i in 0..count {
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
