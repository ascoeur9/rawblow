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

/// 튜닝 파라미터: env override(있으면) 또는 기본값. OnceLock으로 1회만 읽어 프로덕션 비용 0.
/// 성능 스윕(개선 사이클)에서 재빌드 없이 값을 바꿔 측정하기 위함이며, 기본값은 운영 최적값.
fn tunable(var: &str, default: usize, cell: &'static std::sync::OnceLock<usize>) -> usize {
    *cell.get_or_init(|| {
        std::env::var(var)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(default)
    })
}
/// 썸네일 prefix 바이트(기본 512KB). env `RB_THUMB_PREFIX_KB`.
fn thumb_prefix_size() -> usize {
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    tunable("RB_THUMB_PREFIX_KB", 512, &C) * 1024
}
/// 프리뷰 prefix 폴백 바이트(기본 1MB). env `RB_PREVIEW_PREFIX_KB`.
fn preview_prefix_size() -> usize {
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    tunable("RB_PREVIEW_PREFIX_KB", 1024, &C) * 1024
}
/// ORIG IFD 임베디드 최소 긴변(기본 3000). env `RB_ORIG_GATE`.
fn orig_gate() -> u32 {
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    tunable("RB_ORIG_GATE", 3000, &C) as u32
}
/// 프리뷰 IFD 임베디드 최소 긴변(기본 1600). env `RB_PREVIEW_MIN`.
fn preview_min_edge() -> u32 {
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    tunable("RB_PREVIEW_MIN", 1600, &C) as u32
}
/// 점진적 썸네일 prefix의 1단계 크기 바이트(기본 128KB). env `RB_THUMB_STEP1_KB`.
fn thumb_step1_size() -> usize {
    static C: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    tunable("RB_THUMB_STEP1_KB", 128, &C) * 1024
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
                // 썸네일 크기 요청이면 전체(수 MB)를 읽지 않고, 앞부분의 임베디드 EXIF 썸네일을
                // 먼저 시도해 I/O를 크게 줄인다(예: 8MB JPG → 512KB). find_eoi 마커워킹이 본
                // 이미지의 가짜 EOI에 안 속으므로 완전한 임베디드 썸네일만 잡힌다. 없으면 전체 폴백.
                // (프리뷰는 ≥1600 임베디드가 없어 — Panasonic JPG는 MPF 스크리닐이 작음 —
                //  prefix 시도가 헛읽기만 더해 손해라, 본 이미지 전체 디코딩이 정답. Cycle93 측정.)
                let thumb = matches!(opts.max_edge, Some(e) if e <= 384);
                if thumb {
                    // 점진적 prefix(128KB→512KB): EXIF 썸네일이 보통 맨 앞이라 느린 드라이브서도
                    // 적게 읽어 빠르다. 못 찾으면 늘리고, 그래도 없으면 본 이미지 전체 디코딩.
                    let mut tried = 0usize;
                    for &sz in &[thumb_step1_size(), thumb_prefix_size()] {
                        if sz <= tried {
                            continue;
                        }
                        tried = sz;
                        if let Ok(prefix) = read_prefix(path, sz) {
                            if let Some(img) = decode_best_embedded(&prefix, true, orient, opts.max_edge) {
                                return Ok(img);
                            }
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
                if let Some(img) = decode_ifd_embedded(path, orient, opts.max_edge, orig_gate(), false) {
                    return Ok(img);
                }
                // 큰 임베디드가 없을 때만 풀 RAW 현상 시도(미지원 카메라는 패닉 → 폴백).
                let full = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decode_full_raw(path, orient, opts.max_edge)
                }));
                if let Ok(Ok(img)) = full {
                    return Ok(img);
                }
                // rawloader 실패(이 RW2서 패닉/에러) + 풀해상도 임베디드 없는 카메라: IFD가 가리키는
                // **가장 큰** 임베디드(크기 무관, 보통 1920)를 그 구간만 읽어 쓴다 — 전체파일(수십 MB)
                // 읽기를 피한다. 출력은 decode_largest_embedded와 동일(가용 최대)하면서 I/O만
                // 36MB→0.5MB로 줄인다(느린 드라이브서 결정적). IFD가 비면 전체파일 폴백(최후).
                if let Some(img) = decode_ifd_embedded(path, orient, opts.max_edge, 0, false) {
                    return Ok(img);
                }
                decode_largest_embedded(path, orient, opts.max_edge)
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

    // 프리뷰: IFD가 가리키는 **가장 작은 ≥1600 임베디드**(RW2 0x002e ~1920)를 그 구간만 읽어
    // 디코딩한다(blind 1MB prefix·전체파일 폴백 회피, 1920이 1MB 밖이어도 안전). IFD가 없거나
    // ≥1600 임베디드가 없으면(비RW2 등) 아래 prefix 경로로 폴백.
    if !thumb {
        if let Some(img) = decode_ifd_embedded(path, orient, decode_edge, preview_min_edge(), true) {
            return Ok(img);
        }
    }

    // **점진적 prefix**: 썸네일은 작은 것(128KB)부터 시도한다. EXIF 썸네일은 보통 맨 앞
    // ~50KB에 있어, 느린 드라이브(외장 HDD/USB/네트워크)에서도 적게 읽어 빠르게 뜬다(X: 드라이브
    // 실측: 512KB 37ms → 128KB 18ms). 안 들어오면 512KB로 늘려 안전 마진을 지키고, 그래도
    // 없으면 전체를 읽는다(최후). find_eoi 마커워킹이 잘린 임베디드를 후보에서 제외하므로
    // 작은 prefix에서도 회색 위험이 없다. 프리뷰는 IFD가 처리하므로 1MB 단일(폴백용).
    let sizes: &[usize] = if thumb {
        &[thumb_step1_size(), thumb_prefix_size()]
    } else {
        &[preview_prefix_size()]
    };
    let mut tried = 0usize;
    for &sz in sizes {
        if sz <= tried {
            continue; // 같은/작은 크기 중복 회피(예: thumb_prefix_size()==128KB로 튜닝된 경우)
        }
        tried = sz;
        let prefix = read_prefix(path, sz)?;
        if let Some(img) = decode_best_embedded(&prefix, thumb, orient, decode_edge) {
            // 프리뷰가 충분히 크면 채택. 너무 작으면(1920을 못 찾음) 다음 단계/전체로 재시도.
            if thumb || img.width.max(img.height) >= 1200 {
                return Ok(img);
            }
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

/// 파일 앞 `max` 바이트만 헤더로 읽는다(IFD 파싱용). 못 읽으면 빈 벡터.
fn read_header(path: &Path, max: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; max];
    let n = match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    buf.truncate(n);
    count_read(n);
    buf
}

/// IFD0 + IFD 체인(next-IFD) + SubIFD(0x014A)를 재귀적으로 걸어 임베디드 JPEG 블롭 (offset, len)을
/// 모은다. 수집 대상:
/// - type=7 UNDEFINED 블롭(Panasonic RW2 0x002e ~1920 프리뷰 / 0x0127 8144 풀해상도),
/// - StripOffsets(0x0111)+StripByteCounts(0x0117)(Canon CR2·DNG SubIFD 프리뷰),
/// - JPEGInterchangeFormat(0x0201)+Length(0x0202)(Nikon/Pentax/Sony 등 IFD-체인/SubIFD 프리뷰).
///
/// 반환 `(blobs, need)`: `need`는 파싱에 필요한 최대 헤더 오프셋 — 헤더가 짧으면 caller가 키워 1회
/// 재시도(RW2/CR2는 IFD가 앞 64KB 안이라 재시도 없음 → 추가 I/O 0). FF D8 head 검사로 비-JPEG
/// 스트립·손상 오프셋은 후보에서 제외. 블롭은 길이 내림차순·중복 제거.
fn scan_ifds(header: &[u8], file_len: u64, path: &Path) -> (Vec<(u64, usize)>, usize) {
    let le = match header.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return (Vec::new(), 0),
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
    // 오프셋이 가리키는 곳이 실제 JPEG(FF D8)인지 — 헤더 안이면 직접, 밖이면 2바이트만 seek 읽기.
    let is_jpeg_at = |off: u64| -> bool {
        if (off as usize) + 2 <= header.len() {
            header.get(off as usize..off as usize + 2) == Some(&[0xFF, 0xD8][..])
        } else {
            read_range(path, off, 2).map(|b| b.len() >= 2 && b[0] == 0xFF && b[1] == 0xD8).unwrap_or(false)
        }
    };
    let mut blobs: Vec<(u64, usize)> = Vec::new();
    let mut need = 0usize;
    // 스캔할 IFD 오프셋 큐. 체인·SubIFD를 따라가며 추가하되 무한루프·과다 방어로 상한(16 IFD).
    let mut queue: Vec<usize> = match r32(4) {
        Some(v) => vec![v as usize],
        None => return (blobs, need),
    };
    let mut qi = 0;
    let mut visited = 0;
    while qi < queue.len() && visited < 16 {
        let ifd = queue[qi];
        qi += 1;
        let cnt = match r16(ifd) {
            Some(v) => v as usize,
            // IFD 테이블이 헤더 밖 → cnt를 모르니 **전체 엔트리 테이블**(최대 512개)을 보수적으로
            // 추정해 need에 반영한다(caller가 그만큼 헤더를 키워 재스캔). cnt+엔트리+next-IFD까지
            // 한 번에 덮어 DNG SubIFD·PEF 체인(둘 다 ~100KB)이 1회 확장으로 수렴한다.
            None => {
                need = need.max(ifd + 2 + 512 * 12 + 4);
                continue;
            }
        };
        if queue[..qi - 1].contains(&ifd) {
            continue; // 이미 방문한 IFD(체인 순환 방어)
        }
        visited += 1;
        let n = cnt.min(512);
        need = need.max(ifd + 2 + n * 12 + 4); // +4: next-IFD 포인터
        let (mut s_off, mut s_len): (Option<u64>, Option<usize>) = (None, None);
        let (mut j_off, mut j_len): (Option<u64>, Option<usize>) = (None, None);
        for i in 0..n {
            let e = ifd + 2 + i * 12;
            let (tag, typ, c, val) = match (r16(e), r16(e + 2), r32(e + 4), r32(e + 8)) {
                (Some(tg), Some(t), Some(cc), Some(v)) => (tg, t, cc as usize, v as u64),
                _ => break,
            };
            // type=7(UNDEFINED) && >1KB → val은 오프셋, c는 바이트 길이(RW2 통짜 JPEG).
            if typ == 7 && c > 1024 && val < file_len && is_jpeg_at(val) {
                blobs.push((val, c));
            }
            match tag {
                0x0111 => s_off = Some(val),          // StripOffsets
                0x0117 => s_len = Some(val as usize), // StripByteCounts
                0x0201 => j_off = Some(val),          // JPEGInterchangeFormat
                0x0202 => j_len = Some(val as usize), // JPEGInterchangeFormatLength
                0x014a => {
                    // SubIFDs: count=1이면 val이 SubIFD 오프셋, 아니면 val은 오프셋 배열 위치.
                    if c == 1 {
                        queue.push(val as usize);
                    } else {
                        need = need.max(val as usize + c.min(16) * 4);
                        for k in 0..c.min(16) {
                            if let Some(so) = r32(val as usize + k * 4) {
                                queue.push(so as usize);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // StripOffsets/JPEGInterchangeFormat가 가리키는 곳이 실제 JPEG면 후보로(거대한 비-JPEG
        // 스트립=무압축 본데이터를 헛읽지 않게 head만 확인). 길이 과대선언은 read_range가 EOF까지만
        // 읽어 안전.
        for (off, len) in [(s_off, s_len), (j_off, j_len)] {
            if let (Some(off), Some(len)) = (off, len) {
                if len > 1024 && off.saturating_add(len as u64) <= file_len && is_jpeg_at(off) {
                    blobs.push((off, len));
                }
            }
        }
        // IFD 체인: 엔트리 테이블 직후의 next-IFD 포인터.
        if let Some(nxt) = r32(ifd + 2 + n * 12) {
            if nxt != 0 && (nxt as u64) < file_len {
                queue.push(nxt as usize);
            }
        }
    }
    blobs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); // 길이 내림차순
    blobs.dedup();
    (blobs, need)
}

/// TIFF/RW2 IFD가 가리키는 임베디드 JPEG 블롭들의 (offset, len)을 길이 내림차순으로 반환한다.
/// 먼저 앞 64KB 헤더로 IFD0+체인+SubIFD를 걸고(RW2/CR2는 여기서 끝 → 추가 I/O 0), IFD가 64KB
/// 밖에 있으면(예: DNG/PEF의 SubIFD·체인이 ~100KB) 필요한 만큼(≤1MB) 헤더를 키워 1회 재스캔한다.
/// 표준 TIFF(0x2a)·RW2(0x55) 모두 처리(매직 검사 생략). 못 읽으면 빈 벡터.
fn tiff_ifd0_jpeg_blobs(path: &Path) -> Vec<(u64, usize)> {
    let file_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    let mut header = read_header(path, 64 * 1024);
    if header.len() < 16 {
        return Vec::new();
    }
    let (mut blobs, mut need) = scan_ifds(&header, file_len, path);
    // IFD가 헤더 밖이면(DNG SubIFD·PEF 체인이 ~100KB) 필요한 만큼 키워 재스캔. 보수적 추정이라
    // 보통 1회로 수렴하지만 체인이 깊을 수 있어 몇 번 반복(1MB 상한). RW2/CR2는 need≤64KB라 0회.
    let mut guard = 0;
    while need > header.len() && header.len() < 1024 * 1024 && guard < 4 {
        guard += 1;
        let grown = read_header(path, need.min(1024 * 1024));
        if grown.len() <= header.len() {
            break; // 파일이 더 짧아 더 못 키움 → 현재까지로 확정
        }
        header = grown;
        let (b, nd) = scan_ifds(&header, file_len, path);
        blobs = b;
        need = nd;
    }
    blobs
}

/// ORIG용: RW2 IFD가 가리키는 **가장 큰** 임베디드 JPEG를 그 구간만 읽어 디코딩한다.
/// 전체파일(수십 MB) 읽기와 rawloader 패닉을 모두 피한다. JPEG가 충분히 크면(`min_long_edge`
/// 이상) 원본 디테일로 채택, 작으면 None(풀 RAW 현상으로 폴백 유도).
/// IFD가 가리키는 임베디드 JPEG 중 `min_long_edge` 이상인 것을 그 구간만 읽어 디코딩한다.
/// `prefer_smallest=false`(ORIG): 큰 것부터 → 가장 큰 풀해상도. `true`(프리뷰): 작은 것부터
/// → 화면에 충분한 **가장 작은** 임베디드(거대 임베디드 디코딩 회피, 전체파일·blind prefix 회피).
fn decode_ifd_embedded(
    path: &Path,
    orient: u16,
    max_edge: Option<u32>,
    min_long_edge: u32,
    prefer_smallest: bool,
) -> Option<DecodedImage> {
    let blobs = tiff_ifd0_jpeg_blobs(path);
    if blobs.is_empty() {
        return None;
    }
    // 후보를 **바이트 길이가 아니라 디코딩 해상도**로 고른다. 각 블롭의 SOF를 앞 64KB head probe로
    // 싸게 읽어 (긴 변) 크기를 구한 뒤, ORIG은 가장 큰 해상도, 프리뷰는 min 이상 중 가장 작은
    // 해상도를 선택한다. 이렇게 해야 CR2의 IFD 체인에 섞인 저해상·대용량 JPEG(예: 1448×3804, 20MB)가
    // 길이상 1위라도 ORIG의 본 프리뷰(5616)를 밀어내지 못한다(회귀 가드). 무손실(SOF3) 등 디코딩
    // 불가 JPEG은 jpeg_dimensions가 None → 자동 제외. head만 읽어 거대 블롭 전체를 헛읽지도 않는다.
    let mut scored: Vec<(u32, u64, usize)> = Vec::new(); // (long_edge, off, len)
    for (off, len) in &blobs {
        let probe = match read_range(path, *off, (*len).min(64 * 1024)) {
            Ok(b) if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 => b,
            _ => continue,
        };
        if let Some((w, h)) = jpeg_dimensions(&probe) {
            let le = w.max(h) as u32;
            if le >= min_long_edge {
                scored.push((le, *off, *len));
            }
        }
    }
    if scored.is_empty() {
        return None;
    }
    scored.sort_by_key(|s| s.0); // 해상도 오름차순
    // ORIG(prefer_smallest=false): 큰 해상도부터. 프리뷰(true): min 이상 중 작은 해상도부터.
    let order: Vec<usize> =
        if prefer_smallest { (0..scored.len()).collect() } else { (0..scored.len()).rev().collect() };
    for idx in order {
        let (_, off, len) = scored[idx];
        if let Ok(bytes) = read_range(path, off, len) {
            if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
                if let Ok(img) = decode_jpeg_scaled(&bytes, orient, max_edge) {
                    return Some(img);
                }
            }
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
