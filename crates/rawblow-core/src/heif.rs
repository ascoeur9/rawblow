//! HEIC/HEIF 디코드(#97). `heif-oxide`(순수 Rust HEVC)로 본 이미지를 풀고,
//! 썸네일은 컨테이너 안 JPEG이 있으면 그걸 먼저 쓴다(#114).

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

pub fn decode(path: &Path, max_edge: Option<u32>) -> Result<DecodedImage, DecodeError> {
    let bytes = std::fs::read(path).map_err(|e| DecodeError::Io(e.to_string()))?;
    crate::decode::count_read(bytes.len());

    let thumb = matches!(max_edge, Some(e) if e <= 384);
    if thumb {
        if let Some(jpeg) = crate::decode::extract_embedded_jpeg_sized(&bytes, Some(160)) {
            if let Ok(img) = crate::decode::decode_jpeg_scaled(jpeg, 1, max_edge) {
                return Ok(img);
            }
        }
    }

    let _guard = HEIC_DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let decoded = heif_oxide::decode_bytes(&bytes).map_err(|e| DecodeError::Decode(e.to_string()))?;
    drop(_guard);

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
