//! HEIC/HEIF 디코드(#97). `heif-oxide`(순수 Rust HEVC)로 본 이미지를 풀고,
//! 그리드·컬링은 컨테이너의 `jpeg`/`thmb` 항목을 먼저 쓴다(#114, 색인은 `heif_index`).

use crate::decode::{DecodedImage, DecodeError};
use crate::model::ext_lower;
use image::DynamicImage;
use std::path::Path;
use std::sync::Mutex;

/// HEVC 타일 병렬이 워커 풀과 CPU를 겹치지 않게 한 번에 하나만 디코드(#118).
static HEIC_DECODE: Mutex<()> = Mutex::new(());

pub fn is_heic_path(path: &Path) -> bool {
    matches!(ext_lower(path).as_deref(), Some("heic") | Some("heif"))
}

/// 컨테이너 `ispe`에서 원본 긴 변(px). HEVC를 풀지 않는다(#97 배율 기준).
pub fn orig_long_edge(path: &Path) -> Option<u32> {
    let bytes = std::fs::read(path).ok()?;
    let mut best = 0u32;
    walk_boxes(&bytes, &mut |typ, body| {
        if typ == b"ispe" && body.len() >= 12 {
            let w = u32::from_be_bytes(body[4..8].try_into().unwrap_or([0; 4]));
            let h = u32::from_be_bytes(body[8..12].try_into().unwrap_or([0; 4]));
            best = best.max(w.max(h));
        }
    });
    (best > 0).then_some(best)
}

/// HEIC/HEIF 디코드. `want_orig`는 ORIG(원본 보기) 요청 여부 — 본 이미지를 푼 경로만
/// `full_raw=true`로 표시해 UI가 원본 성공과 썸네일 폴백을 구분하게 한다(#109).
///
/// 순서(#114): JPEG-in-HEIF primary → (그리드·컬링 ≤1024만) `jpeg`/`thmb` 항목 →
/// 컨테이너 안 JPEG 스캔(≤384) → primary HEVC. 본 화면에서는 작은 썸네일 항목을 쓰지 않는다
/// (12MP 사진 자리에 수백 px 썸네일이 뜨는 것 방지).
pub fn decode(path: &Path, max_edge: Option<u32>, want_orig: bool) -> Result<DecodedImage, DecodeError> {
    let bytes = std::fs::read(path).map_err(|e| DecodeError::Io(e.to_string()))?;
    crate::decode::count_read(bytes.len());
    decode_bytes(&bytes, max_edge, want_orig)
}

pub(crate) fn decode_bytes(
    bytes: &[u8],
    max_edge: Option<u32>,
    want_orig: bool,
) -> Result<DecodedImage, DecodeError> {
    let preview = matches!(max_edge, Some(e) if e <= 1024);
    if let Some(idx) = crate::heif_index::index(bytes) {
        // heif-oxide는 JPEG primary를 거절한다 — 그 JPEG가 곧 본 이미지.
        if idx.is_jpeg(idx.primary) {
            if let Some(mut img) = jpeg_item(bytes, &idx, idx.primary, max_edge) {
                img.full_raw = want_orig;
                return Ok(img);
            }
        }
        if preview {
            if let Some(img) = jpeg_thumb_items(bytes, &idx, max_edge) {
                return Ok(img);
            }
            if let Some(img) = hevc_thumb_item(bytes, &idx, max_edge) {
                return Ok(img);
            }
        }
    }

    let thumb = matches!(max_edge, Some(e) if e <= 384);
    if thumb {
        if let Some(jpeg) = crate::decode::extract_embedded_jpeg_sized(bytes, Some(160)) {
            if let Ok(img) = crate::decode::decode_jpeg_scaled(jpeg, 1, max_edge) {
                return Ok(img);
            }
        }
    }

    let mut img = decode_hevc(bytes, max_edge)?;
    img.full_raw = want_orig;
    Ok(img)
}

/// 항목 하나를 JPEG로 디코딩(SOI 확인). 이 경로는 컨테이너 `irot`를 적용하지 않는다.
fn jpeg_item(
    bytes: &[u8],
    idx: &crate::heif_index::HeifIndex,
    id: u32,
    max_edge: Option<u32>,
) -> Option<DecodedImage> {
    let data = idx.item_bytes(bytes, id)?;
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    crate::decode::decode_jpeg_scaled(&data, 1, max_edge).ok()
}

fn jpeg_thumb_items(
    bytes: &[u8],
    idx: &crate::heif_index::HeifIndex,
    max_edge: Option<u32>,
) -> Option<DecodedImage> {
    let mut ids: Vec<u32> = idx.thumb_ids.iter().copied().filter(|&id| idx.is_jpeg(id)).collect();
    let mut rest: Vec<u32> = idx.jpeg_ids.iter().copied().filter(|id| !ids.contains(id)).collect();
    rest.sort_unstable(); // HashMap 순회 순서에 결과가 흔들리지 않게
    ids.extend(rest);
    ids.into_iter()
        .filter(|&id| id != idx.primary)
        .find_map(|id| jpeg_item(bytes, idx, id, max_edge))
}

/// `thmb`가 가리키는 작은 HEVC 항목을 `pitm` 패치로 primary 삼아 푼다.
fn hevc_thumb_item(
    bytes: &[u8],
    idx: &crate::heif_index::HeifIndex,
    max_edge: Option<u32>,
) -> Option<DecodedImage> {
    let thumb = idx
        .thumb_ids
        .iter()
        .copied()
        .find(|&id| id != idx.primary && idx.is_hevc_image(id))?;
    let mut patched = bytes.to_vec();
    if !idx.patch_pitm(&mut patched, thumb) {
        return None;
    }
    decode_hevc(&patched, max_edge).ok()
}

fn decode_hevc(bytes: &[u8], max_edge: Option<u32>) -> Result<DecodedImage, DecodeError> {
    let guard = HEIC_DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| heif_oxide::decode_bytes(bytes)))
        .map_err(|_| DecodeError::Decode("heic decoder panic".into()))?
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    drop(guard);

    let rgba = decoded.to_rgba8();
    let dynimg = image::RgbaImage::from_raw(decoded.width, decoded.height, rgba)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| DecodeError::Decode("heic rgba size mismatch".into()))?;
    // heif-oxide가 irot/Display P3→sRGB를 이미 적용. 이중 회전 금지(orient=1).
    Ok(crate::decode::finish(dynimg, None, false, 1, max_edge))
}

fn walk_boxes(data: &[u8], f: &mut dyn FnMut(&[u8; 4], &[u8])) {
    let mut i = 0usize;
    let mut n = 0usize;
    while i + 8 <= data.len() && n < 4096 {
        n += 1;
        let size32 = u32::from_be_bytes(data[i..i + 4].try_into().unwrap_or([0; 4]));
        let typ: [u8; 4] = data[i + 4..i + 8].try_into().unwrap_or([0; 4]);
        let (hdr, size) = if size32 == 1 {
            if i + 16 > data.len() {
                break;
            }
            let sz = u64::from_be_bytes(data[i + 8..i + 16].try_into().unwrap_or([0; 8])) as usize;
            (16usize, sz)
        } else if size32 == 0 {
            (8usize, data.len() - i)
        } else {
            (8usize, size32 as usize)
        };
        if size < hdr || i.checked_add(size).map(|end| end > data.len()).unwrap_or(true) {
            break;
        }
        let body = &data[i + hdr..i + size];
        f(&typ, body);
        if matches!(&typ, b"moov" | b"iprp" | b"ipco" | b"dinf") {
            walk_boxes(body, f);
        } else if &typ == b"meta" && body.len() >= 4 {
            walk_boxes(&body[4..], f);
        }
        i += size;
    }
}
