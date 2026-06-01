//! 이미지/RAW 디코딩 (F3 표시 + F9 컬러 + 풀 RAW).
//!
//! 속도 최우선(PRD §F7) 설계:
//! - JPEG(일반 .jpg 및 RAW 내장 프리뷰)는 `jpeg-decoder`의 **DCT 축소 디코딩**
//!   (`scale`, 1/2·1/4·1/8)으로 화면 크기에 맞춰 **적게** 디코딩한다. 풀해상도를
//!   통째로 푸는 비용(IDCT)을 피하는 것이 핵심.
//! - PNG/TIFF/WEBP 등은 `image` 크레이트로 디코딩 후 다운스케일.
//! - RAW 풀 디코딩은 `fullraw` 피처(`imagepipe`)에서 1:1·D키 시점에만.
//!
//! 모든 경로는 `finish()`로 모여 EXIF Orientation 회전 → 다운스케일 → ICC→sRGB
//! 변환을 일관 적용한다.

use crate::model::{ext_lower, kind_of, Kind};
use image::{DynamicImage, GenericImageView, ImageDecoder};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// 디코드 경로가 디스크에서 읽은 누적 바이트 수(프로파일링·회귀 가드용). 느린 드라이브에서
/// 핵심 비용은 "읽은 바이트"이므로, 전체파일(수십 MB) 폴백을 정량 추적한다. 거의 0비용.
pub static BYTES_READ: AtomicU64 = AtomicU64::new(0);
#[inline]
fn count_read(n: usize) {
    BYTES_READ.fetch_add(n as u64, Ordering::Relaxed);
}
/// 카운터를 0으로 리셋하고 이전 값을 반환(프로파일러가 디코드 단위로 측정).
pub fn take_bytes_read() -> u64 {
    BYTES_READ.swap(0, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub color_managed: bool,
    pub full_raw: bool,
}

#[derive(Debug)]
pub enum DecodeError {
    Io(String),
    Unsupported,
    NoEmbeddedPreview,
    Decode(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "io: {e}"),
            DecodeError::Unsupported => write!(f, "unsupported file type"),
            DecodeError::NoEmbeddedPreview => write!(f, "no embedded preview found in raw"),
            DecodeError::Decode(e) => write!(f, "decode: {e}"),
        }
    }
}
impl std::error::Error for DecodeError {}
impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        DecodeError::Io(e.to_string())
    }
}

#[derive(Clone, Copy, Default)]
pub struct DecodeOptions {
    /// RAW를 풀 디코딩할지(아니면 임베디드 프리뷰).
    pub full_raw: bool,
    /// 결과의 최대 변 길이(px) 제한. `Some(n)`이면 큰 변을 n 이하로(JPEG은 DCT 축소).
    /// `None`이면 원본 크기 그대로.
    pub max_edge: Option<u32>,
}

/// 경로를 종류에 따라 디코딩한다.
pub fn decode_file(path: &Path, opts: DecodeOptions) -> Result<DecodedImage, DecodeError> {
    let orient = crate::meta::orientation(path);
    match kind_of(path) {
        Some(Kind::Image) => {
            let is_jpeg = matches!(ext_lower(path).as_deref(), Some("jpg") | Some("jpeg"));
            if is_jpeg {
                // 썸네일 크기 요청이면 전체(수 MB)를 읽지 않고, 앞부분의 임베디드 EXIF 썸네일을
                // 먼저 시도해 I/O를 크게 줄인다(예: 8MB JPG → 512KB). find_eoi 마커워킹이 본
                // 이미지의 가짜 EOI에 안 속으므로 완전한 임베디드 썸네일만 잡힌다. 없으면 전체 폴백.
                let thumb = matches!(opts.max_edge, Some(e) if e <= 384);
                if thumb {
                    if let Ok(prefix) = read_prefix(path, 512 * 1024) {
                        if let Some(img) = decode_best_embedded(&prefix, true, orient, opts.max_edge) {
                            return Ok(img);
                        }
                    }
                }
                let bytes = read_whole(path)?;
                decode_jpeg_scaled(&bytes, orient, opts.max_edge)
            } else {
                decode_other_image(path, orient, opts.max_edge)
            }
        }
        Some(Kind::Raw) => {
            if opts.full_raw {
                // ORIG(원본 보기): 먼저 IFD가 가리키는 **풀해상도 임베디드 JPEG**를 그 구간만
                // 읽어 디코딩한다(예: RW2 0x0127의 8144px). 전체파일(수십 MB) 읽기와 rawloader
                // 패닉을 모두 피해 가장 빠르고, 카메라 풀해상도 JPEG라 컬링에 충분한 디테일.
                if let Some(img) = decode_largest_ifd_embedded(path, orient, opts.max_edge, 3000) {
                    return Ok(img);
                }
                // 큰 임베디드가 없을 때만 풀 RAW 현상 시도(미지원 카메라는 패닉 → 폴백).
                let full = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_full_raw(path, orient, opts.max_edge)
                }));
                match full {
                    Ok(Ok(img)) => Ok(img),
                    _ => decode_largest_embedded(path, orient, opts.max_edge),
                }
            } else {
                decode_raw_embedded(path, orient, opts.max_edge)
            }
        }
        None => Err(DecodeError::Unsupported),
    }
}

/// RAW의 임베디드 JPEG를 추출해 축소 디코딩.
///
/// 성능 핵심: RW2는 파일 앞쪽(보통 수십 KB 이내)에 1920급 완전 프리뷰를 담으므로
/// **앞 1MB만 읽어** 추출한다(56MB 전체를 읽지 않음 → 디스크 부하·지연 대폭 감소).
/// 앞부분에서 못 찾거나 손상된 드문 경우에만 전체를 읽어 재시도한다.
fn decode_raw_embedded(
    path: &Path,
    orient: u16,
    max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    let thumb = matches!(max_edge, Some(e) if e <= 384);
    // 썸네일: 320으로 축소. 프리뷰: 1920급 임베디드를 줄이지 않고(≤2048 → 그대로) 선명하게,
    // 단 드물게 거대한 임베디드는 2048로 상한.
    let decode_edge = if thumb { max_edge } else { Some(2048) };

    // 프리픽스 크기: 썸네일은 512KB. 너무 작으면(예: 64KB) 작은 썸네일의 완전한 SOI..EOI가
    // 안 들어오고, 더 큰 임베디드가 가짜(이른) EOI로 "완전"해 보여 잘린 채 디코딩 → 회색.
    // 512KB면 완전한 작은 썸네일을 거의 항상 포함. 프리뷰는 1MB(완전한 1920급 확보).
    let prefix_size = if thumb { 512 * 1024 } else { 1024 * 1024 };
    let prefix = read_prefix(path, prefix_size)?;
    if let Some(img) = decode_best_embedded(&prefix, thumb, orient, decode_edge) {
        // 프리뷰가 충분히 크면 채택. 너무 작으면(1920을 못 찾음) 전체를 읽어 재시도.
        if thumb || img.width.max(img.height) >= 1200 {
            return Ok(img);
        }
    }
    // 폴백: 전체 읽기.
    let full = read_whole(path)?;
    decode_best_embedded(&full, thumb, orient, decode_edge).ok_or(DecodeError::NoEmbeddedPreview)
}

/// ORIG 폴백: RAW의 **가장 큰** 임베디드 JPEG를 요청 크기(`max_edge`)로 디코딩한다.
/// 풀 RAW 현상이 안 되는 카메라(예: 일부 RW2)에서도 원본 사이즈 프리뷰(예: 8144급)를
/// 보여주기 위함. 전체 파일을 읽어야 가장 큰 임베디드를 찾을 수 있어 다소 느리다(의도된 비용).
fn decode_largest_embedded(
    path: &Path,
    orient: u16,
    max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    let bytes = read_whole(path)?;
    let jpeg = extract_embedded_jpeg(&bytes).ok_or(DecodeError::NoEmbeddedPreview)?;
    decode_jpeg_scaled(jpeg, orient, max_edge)
}

/// 후보 임베디드 JPEG들을 선호 순서로 디코딩 시도해, 처음 성공한 것을 반환.
/// 잘린/손상 후보는 자동으로 건너뛴다(회색 화면 방지).
/// - 썸네일: 작은 것부터(빠른 디코딩).
/// - 프리뷰: 화면 크기(≥1600) 이상 중 작은 것부터, 없으면 큰 것부터.
fn decode_best_embedded(
    bytes: &[u8],
    thumb: bool,
    orient: u16,
    decode_edge: Option<u32>,
) -> Option<DecodedImage> {
    let cands = embedded_candidates(bytes); // (start, len, max_edge) 오름차순
    if cands.is_empty() {
        return None;
    }
    let mut order: Vec<usize> = (0..cands.len()).collect();
    if !thumb {
        order.sort_by_key(|&i| {
            let e = cands[i].2;
            if e >= 1600 {
                (0u8, e) // ≥1600 중 작은 것 우선
            } else {
                (1u8, u32::MAX - e) // 그다음 큰 것부터(폴백)
            }
        });
    }
    for i in order {
        let (s, l, _) = cands[i];
        if let Ok(img) = decode_jpeg_scaled(&bytes[s..s + l], orient, decode_edge) {
            return Some(img);
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
    count_read(buf.len());
    Ok(buf)
}

/// 전체 파일 읽기(폴백). BYTES_READ에 집계해 느린 드라이브에서의 전체파일 폴백을 추적한다.
fn read_whole(path: &Path) -> std::io::Result<Vec<u8>> {
    let buf = std::fs::read(path)?;
    count_read(buf.len());
    Ok(buf)
}

/// 파일의 [offset, offset+len) 구간만 읽는다(seek + read). 임베디드 JPEG를 전체파일 읽기 없이
/// 정확히 가져오는 데 쓴다. EOF를 넘으면 가능한 만큼만 읽는다.
fn read_range(path: &Path, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.take(len as u64).read_to_end(&mut buf)?;
    count_read(buf.len());
    Ok(buf)
}

/// TIFF/RW2 IFD0를 헤더(앞 64KB)만 읽어 파싱하고, 임베디드 JPEG 블롭들의 (offset, len)을
/// 길이 내림차순으로 반환한다. Panasonic RW2는 0x002e(JpgFromRaw, ~1920 프리뷰)와
/// 0x0127(풀해상도 8144 JPEG)에 JPEG를 통째로 담는다(type=7 UNDEFINED, count=바이트수).
/// 표준 TIFF(0x2a)·RW2(0x55) 모두 처리(매직 검사 생략). 못 읽으면 빈 벡터.
fn tiff_ifd0_jpeg_blobs(path: &Path) -> Vec<(u64, usize)> {
    use std::io::Read;
    let mut header = vec![0u8; 64 * 1024];
    let n = match std::fs::File::open(path).and_then(|mut f| f.read(&mut header)) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    header.truncate(n);
    count_read(n);
    if header.len() < 16 {
        return Vec::new();
    }
    let le = match &header[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Vec::new(),
    };
    let r16 = |o: usize| -> Option<u16> {
        let b = header.get(o..o + 2)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    };
    let r32 = |o: usize| -> Option<u32> {
        let b = header.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let ifd0 = match r32(4) {
        Some(v) => v as usize,
        None => return Vec::new(),
    };
    let count = match r16(ifd0) {
        Some(v) => v as usize,
        None => return Vec::new(),
    };
    // 오프셋이 파일 범위를 벗어난 손상 IFD 엔트리를 거르기 위한 파일 크기(못 읽으면 무제한).
    let file_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    let mut blobs: Vec<(u64, usize)> = Vec::new();
    for i in 0..count.min(512) {
        let e = ifd0 + 2 + i * 12;
        let (typ, cnt, val) = match (r16(e + 2), r32(e + 4), r32(e + 8)) {
            (Some(t), Some(c), Some(v)) => (t, c as usize, v as u64),
            _ => break,
        };
        // type=7(UNDEFINED, 1바이트) && 4바이트 초과 → val은 오프셋, cnt는 바이트 길이.
        // JPEG로 보이는 충분히 큰 블롭만(>1KB), 오프셋이 파일 내부인 것만 후보로(잘못된 오프셋
        // 방어). 길이 과대선언은 read_range가 EOF까지만 읽어 안전하므로 허용.
        if typ == 7 && cnt > 1024 && val < file_len {
            blobs.push((val, cnt));
        }
    }
    blobs.sort_by(|a, b| b.1.cmp(&a.1)); // 길이 내림차순
    blobs
}

/// ORIG용: RW2 IFD가 가리키는 **가장 큰** 임베디드 JPEG를 그 구간만 읽어 디코딩한다.
/// 전체파일(수십 MB) 읽기와 rawloader 패닉을 모두 피한다. JPEG가 충분히 크면(`min_long_edge`
/// 이상) 원본 디테일로 채택, 작으면 None(풀 RAW 현상으로 폴백 유도).
fn decode_largest_ifd_embedded(
    path: &Path,
    orient: u16,
    max_edge: Option<u32>,
    min_long_edge: u32,
) -> Option<DecodedImage> {
    for (off, len) in tiff_ifd0_jpeg_blobs(path) {
        let bytes = match read_range(path, off, len) {
            Ok(b) if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 => b,
            _ => continue,
        };
        // 디코딩 가능 SOF에서 크기 확인 — 너무 작으면(프리뷰만) 건너뛴다.
        if let Some((w, h)) = jpeg_dimensions(&bytes) {
            if (w.max(h) as u32) < min_long_edge {
                continue;
            }
        }
        if let Ok(img) = decode_jpeg_scaled(&bytes, orient, max_edge) {
            return Some(img);
        }
    }
    None
}

/// JPEG 바이트를 디코딩한다. 빠른 경로는 jpeg-decoder의 DCT 축소(`scale`),
/// 어떤 이유로든 실패/이상하면 견고한 `image` 크레이트로 폴백(검은 화면 방지).
fn decode_jpeg_scaled(
    bytes: &[u8],
    orient: u16,
    max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    if let Some((img, icc)) = try_jpeg_decoder_scaled(bytes, max_edge) {
        return Ok(finish(img, icc, false, orient, max_edge));
    }
    // 폴백: image 크레이트 JPEG 디코더(축소 디코딩은 없지만 견고).
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    let icc = decoder.icc_profile().ok().flatten();
    let img = DynamicImage::from_decoder(decoder).map_err(|e| DecodeError::Decode(e.to_string()))?;
    Ok(finish(img, icc, false, orient, max_edge))
}

/// jpeg-decoder로 (가능하면 DCT 축소) 디코딩 시도. 실패·이상값이면 None → 폴백 유도.
/// `scale()`를 `decode()` 전에만 호출하고 `read_info()`는 별도로 부르지 않는다
/// (중복 헤더 읽기로 인한 깨진 출력 회피).
fn try_jpeg_decoder_scaled(
    bytes: &[u8],
    max_edge: Option<u32>,
) -> Option<(DynamicImage, Option<Vec<u8>>)> {
    let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    // DCT 축소 디코딩: 요청 변(max_edge)으로 scale()을 호출하면 jpeg-decoder가
    // 1/1·1/2·1/4·1/8 중 **결과 긴 변이 요청 이상**이 되는 가장 큰 축소를 골라
    // IDCT 비용을 최대 64배까지 줄인다(예: 6000px JPEG → 320 썸네일은 750px만 IDCT,
    // 1920 임베디드 → 320 썸네일은 480px만 IDCT). choose_idct_size가 `>= 요청`을
    // 보장하므로 finish()의 고품질(Triangle) 재축소에 업스케일이 없고 디테일 손실이 없다.
    // (과거 "1920→240" 화질저하는 scale 미사용 탓이 아니라 요청값을 작게 넘긴 탓이며,
    //  scale(max_edge,max_edge)는 긴 변이 항상 max_edge 이상이라 안전하다. 프리뷰(1600)는
    //  1920 임베디드에서 어떤 축소도 1600에 못 미쳐 풀디코딩으로 떨어진다 → 기존과 동일.)
    // max_edge=0은 "축소 없음"(downscale도 0을 그렇게 본다)이므로 scale을 건너뛴다.
    if let Some(me) = max_edge.filter(|&e| e > 0) {
        let me16 = me.min(u16::MAX as u32) as u16;
        if d.scale(me16, me16).is_err() {
            return None; // 헤더 파싱 실패 → image 크레이트 폴백
        }
    }
    let pixels = d.decode().ok()?;
    let info = d.info()?;
    let icc = d.icc_profile();
    let (w, h) = (info.width as u32, info.height as u32);
    if w == 0 || h == 0 {
        return None;
    }
    let rgba = pixels_to_rgba(&pixels, w, h, info.pixel_format).ok()?;
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    Some((DynamicImage::ImageRgba8(img), icc))
}

/// jpeg-decoder 픽셀 포맷 → RGBA8.
fn pixels_to_rgba(
    px: &[u8],
    w: u32,
    h: u32,
    fmt: jpeg_decoder::PixelFormat,
) -> Result<Vec<u8>, DecodeError> {
    use jpeg_decoder::PixelFormat::*;
    let n = (w as usize) * (h as usize);
    let mut out = vec![0u8; n * 4];
    match fmt {
        RGB24 => {
            if px.len() < n * 3 {
                return Err(DecodeError::Decode("jpeg: short rgb".into()));
            }
            for i in 0..n {
                out[i * 4] = px[i * 3];
                out[i * 4 + 1] = px[i * 3 + 1];
                out[i * 4 + 2] = px[i * 3 + 2];
                out[i * 4 + 3] = 255;
            }
        }
        L8 => {
            if px.len() < n {
                return Err(DecodeError::Decode("jpeg: short l8".into()));
            }
            for i in 0..n {
                let v = px[i];
                out[i * 4] = v;
                out[i * 4 + 1] = v;
                out[i * 4 + 2] = v;
                out[i * 4 + 3] = 255;
            }
        }
        L16 => {
            if px.len() < n * 2 {
                return Err(DecodeError::Decode("jpeg: short l16".into()));
            }
            for i in 0..n {
                let v = px[i * 2 + 1]; // 상위 바이트(LE)
                out[i * 4] = v;
                out[i * 4 + 1] = v;
                out[i * 4 + 2] = v;
                out[i * 4 + 3] = 255;
            }
        }
        CMYK32 => {
            if px.len() < n * 4 {
                return Err(DecodeError::Decode("jpeg: short cmyk".into()));
            }
            // Adobe JPEG의 CMYK는 보통 반전 저장 → (255-x)로 되돌린 뒤 RGB로.
            for i in 0..n {
                let c = 255 - px[i * 4];
                let m = 255 - px[i * 4 + 1];
                let y = 255 - px[i * 4 + 2];
                let k = 255 - px[i * 4 + 3];
                out[i * 4] = ((255 - c) as u16 * (255 - k) as u16 / 255) as u8;
                out[i * 4 + 1] = ((255 - m) as u16 * (255 - k) as u16 / 255) as u8;
                out[i * 4 + 2] = ((255 - y) as u16 * (255 - k) as u16 / 255) as u8;
                out[i * 4 + 3] = 255;
            }
        }
    }
    Ok(out)
}

/// PNG/TIFF/WEBP 등(JPEG 외) — image 크레이트로 디코딩 후 다운스케일.
fn decode_other_image(
    path: &Path,
    orient: u16,
    max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    let icc = decoder.icc_profile().ok().flatten();
    let img = DynamicImage::from_decoder(decoder).map_err(|e| DecodeError::Decode(e.to_string()))?;
    Ok(finish(img, icc, false, orient, max_edge))
}

/// DynamicImage → (orientation 회전) → (다운스케일) → RGBA8 + ICC→sRGB.
fn finish(
    img: DynamicImage,
    icc: Option<Vec<u8>>,
    full_raw: bool,
    orient: u16,
    max_edge: Option<u32>,
) -> DecodedImage {
    let img = apply_orientation(img, orient);
    let img = match max_edge {
        Some(me) => downscale(img, me),
        None => img,
    };
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut buf = rgba.into_raw();
    let color_managed = match icc {
        Some(profile) if !profile.is_empty() => crate::color::convert_to_srgb(&mut buf, &profile),
        _ => full_raw,
    };
    DecodedImage {
        width,
        height,
        rgba: buf,
        color_managed,
        full_raw,
    }
}

/// EXIF Orientation(1..8)에 따라 회전/반전.
fn apply_orientation(img: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// 큰 변이 `max_edge`를 넘으면 비율 유지하며 축소.
fn downscale(img: DynamicImage, max_edge: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let m = w.max(h);
    if m <= max_edge || max_edge == 0 {
        return img;
    }
    let s = max_edge as f32 / m as f32;
    let nw = ((w as f32 * s).round() as u32).max(1);
    let nh = ((h as f32 * s).round() as u32).max(1);
    img.resize(nw, nh, image::imageops::FilterType::Triangle)
}

/// 풀 RAW 현상(imagepipe → sRGB RGB8). `fullraw` 피처에서만.
///
/// 회전 주의: imagepipe의 transform 옵코드가 rawloader가 읽은
/// `Orientation`에 따라 이미 회전을 적용한다(소스: imagepipe ops/transform.rs).
/// 따라서 `finish()`의 `apply_orientation`을 또 적용하면 **이중 회전**이 되어
/// 세로 사진이 가로로 표시된다(이슈 #11, 소니 A7R3 ARW). orient=1로 넘겨 스킵.
#[cfg(feature = "fullraw")]
pub fn decode_full_raw(
    path: &Path,
    _orient: u16,
    max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    let mut pipeline = imagepipe::Pipeline::new_from_file(path)
        .map_err(|e| DecodeError::Decode(format!("imagepipe open: {e:?}")))?;
    let out = pipeline
        .output_8bit(None)
        .map_err(|e| DecodeError::Decode(format!("imagepipe develop: {e:?}")))?;
    let (w, h) = (out.width as u32, out.height as u32);
    let rgb = image::RgbImage::from_raw(w, h, out.data)
        .ok_or_else(|| DecodeError::Decode("imagepipe: rgb buffer size mismatch".into()))?;
    Ok(finish(DynamicImage::ImageRgb8(rgb), None, true, 1, max_edge))
}

#[cfg(not(feature = "fullraw"))]
pub fn decode_full_raw(
    _path: &Path,
    _orient: u16,
    _max_edge: Option<u32>,
) -> Result<DecodedImage, DecodeError> {
    Err(DecodeError::Unsupported)
}

/// RAW 바이트에서 가장 큰 임베디드 JPEG(하위호환 API).
pub fn extract_embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
    extract_embedded_jpeg_sized(bytes, None)
}

/// RAW 바이트에서 임베디드 JPEG를 고른다.
///
/// `min_edge = Some(e)`이면 큰 변이 `e` 이상인 것 중 **가장 작은** 것(불필요하게 큰
/// 디코딩 회피). 없으면 가장 큰 것. `None`이면 가장 큰 것.
pub fn extract_embedded_jpeg_sized(bytes: &[u8], min_edge: Option<u32>) -> Option<&[u8]> {
    let cands = embedded_candidates(bytes); // max_edge 오름차순
    let pick = match min_edge {
        Some(me) => cands.iter().find(|c| c.2 >= me).or_else(|| cands.last()),
        None => cands.last(),
    };
    pick.map(|&(s, l, _)| &bytes[s..s + l])
}

/// 완전한(SOI..EOI + 유효 SOF) 임베디드 JPEG 후보를 모아 max_edge 오름차순으로 반환.
fn embedded_candidates(bytes: &[u8]) -> Vec<(usize, usize, u32)> {
    let mut cands: Vec<(usize, usize, u32)> = Vec::new();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            if let Some(eoi_rel) = find_eoi(&bytes[i..]) {
                let seg = &bytes[i..i + eoi_rel];
                if let Some((w, h)) = jpeg_dimensions(seg) {
                    cands.push((i, seg.len(), w.max(h) as u32));
                    i += eoi_rel;
                    continue;
                }
            }
        }
        i += 1;
    }
    cands.sort_by_key(|c| c.2);
    cands
}

/// JPEG 구조를 따라가 **진짜** EOI 위치(SOI 시작 기준, EOI 직후 오프셋)를 찾는다.
///
/// 단순히 첫 `FF D9`를 찾으면 APP1/EXIF 안에 박힌 (RW2 프리뷰의 EXIF 썸네일 등) 가짜
/// `FF D9`에 속아 JPEG를 짧게 잘라 → 디코더가 나머지를 회색으로 채운다(회색 셀 원인).
/// 그래서 마커를 길이대로 건너뛰고, SOS 이후 엔트로피 데이터에서는 `FF 00` 스터핑과
/// `FF D0..D7` 리스타트를 무시하고 진짜 `FF D9`만 EOI로 인식한다.
/// 가용 바이트 안에 진짜 EOI가 없으면(=잘림) `None`(→ 후보 제외, 전체읽기 폴백).
fn find_eoi(b: &[u8]) -> Option<usize> {
    let n = b.len();
    if n < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2; // SOI 다음
    while i + 1 < n {
        if b[i] != 0xFF {
            return None; // 마커 정렬 깨짐
        }
        // 정렬용 0xFF 패딩 스킵.
        let mut k = i + 1;
        while k < n && b[k] == 0xFF {
            k += 1;
        }
        if k >= n {
            return None;
        }
        let marker = b[k];
        i = k + 1; // 마커 바이트 다음
        match marker {
            0xD9 => return Some(i),         // EOI
            0x01 | 0xD0..=0xD7 => {}        // 길이 없는 단독 마커
            0xDA => {
                // SOS: 헤더(길이 있음) 건너뛴 뒤 엔트로피 데이터 스캔.
                if i + 1 >= n {
                    return None;
                }
                let len = ((b[i] as usize) << 8) | (b[i + 1] as usize);
                if len < 2 {
                    return None;
                }
                i += len;
                while i + 1 < n {
                    if b[i] == 0xFF {
                        let m = b[i + 1];
                        if m == 0x00 || (0xD0..=0xD7).contains(&m) {
                            i += 2; // 스터핑 / 리스타트 마커
                        } else if m == 0xD9 {
                            return Some(i + 2); // 진짜 EOI
                        } else {
                            break; // 다음 마커(프로그레시브 추가 스캔 등) → 마커 루프로
                        }
                    } else {
                        i += 1;
                    }
                }
                // 마커로 빠져나온 경우 바깥 루프가 이어서 처리(i는 0xFF 위치).
            }
            _ => {
                // 길이를 가진 마커(APPn, DQT, DHT, SOFn 등).
                if i + 1 >= n {
                    return None;
                }
                let len = ((b[i] as usize) << 8) | (b[i + 1] as usize);
                if len < 2 {
                    return None;
                }
                i += len;
            }
        }
    }
    None
}

fn jpeg_dimensions(b: &[u8]) -> Option<(u16, u16)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        if marker == 0xD8
            || marker == 0xD9
            || (0xD0..=0xD7).contains(&marker)
            || marker == 0x01
            || marker == 0xFF
        {
            i += 2;
            continue;
        }
        let len = ((b[i + 2] as usize) << 8) | (b[i + 3] as usize);
        // **디코딩 가능한** SOF만 인정: baseline(C0)·extended(C1)·progressive(C2).
        // CR2(캐논 5D/5D2 등) 등은 본 이미지가 무손실 JPEG(SOF3, C3)로 들어있는데
        // jpeg-decoder/image 둘 다 무손실·산술코딩 JPEG을 못 푼다. 이걸 프리뷰 후보로
        // 잡으면 엉뚱한 크기(예: 780×2048)로 깨져 단일뷰가 안 뜬다 → 후보에서 제외.
        let is_decodable_sof = matches!(marker, 0xC0 | 0xC1 | 0xC2);
        let is_other_sof = matches!(
            marker,
            0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
        );
        if is_decodable_sof {
            let h = ((b[i + 5] as u16) << 8) | b[i + 6] as u16;
            let w = ((b[i + 7] as u16) << 8) | b[i + 8] as u16;
            return Some((w, h));
        }
        if is_other_sof {
            // 디코딩 불가 JPEG → 이 SOI..EOI는 후보 아님(None 반환 유도). 길이만큼 건너뜀.
            let len = ((b[i + 2] as usize) << 8) | (b[i + 3] as usize);
            if len < 2 {
                break;
            }
            i += 2 + len;
            continue;
        }
        if marker == 0xDA {
            break;
        }
        if len < 2 {
            break;
        }
        i += 2 + len;
    }
    None
}
