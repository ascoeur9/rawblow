//! Sigma X3F(FOVb) — 임베디드 JPEG 프리뷰만 연다.
//!
//! 디렉터리는 파일 끝 4바이트가 가리키는 `SECd`. JPEG 프리뷰 섹션은
//! `SECi` 헤더의 type 2 · format 18이고, JPEG 바이트는 섹션 시작 + 28바이트.
//! Foveon RAW 자체는 현상하지 않는다.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::decode::{decode_jpeg_scaled, DecodeError, DecodedImage};
use crate::model::ext_lower;

const JPEG_PREVIEW: u32 = 0x0002_0012;
const IMAGE_HEADER: u64 = 28;
const MAX_DIR_ENTRIES: u32 = 64;

pub fn is_x3f_path(path: &Path) -> bool {
    matches!(ext_lower(path).as_deref(), Some("x3f"))
}

/// 헤더 rotation(0/90/180/270)을 EXIF orientation으로.
pub fn rotation_to_orientation(rotation: u32) -> u16 {
    match rotation {
        90 => 6,
        180 => 3,
        270 => 8,
        _ => 1,
    }
}

/// `.x3f`를 임베디드 JPEG 프리뷰로 디코드한다.
pub fn decode_x3f(path: &Path, max_edge: Option<u32>) -> Result<DecodedImage, DecodeError> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"FOVb" {
        return Err(DecodeError::Decode("x3f: FOVb 시그니처가 아님".into()));
    }
    let rotation = read_u32_at(&mut file, 36).unwrap_or(0);
    let orient = rotation_to_orientation(rotation);
    let jpeg = extract_preview_jpeg(&mut file)?;
    decode_jpeg_scaled(&jpeg, orient, max_edge)
}

fn extract_preview_jpeg(file: &mut File) -> Result<Vec<u8>, DecodeError> {
    let len = file.seek(SeekFrom::End(0))?;
    if len < 8 {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    let dir_off = read_u32_at(file, len - 4).ok_or(DecodeError::NoEmbeddedPreview)? as u64;
    if dir_off + 12 > len {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    let mut hdr = [0u8; 12];
    file.seek(SeekFrom::Start(dir_off))?;
    file.read_exact(&mut hdr)?;
    if &hdr[0..4] != b"SECd" {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    let count = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
    if count == 0 || count > MAX_DIR_ENTRIES {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    let mut best: Option<(u64, u32)> = None;
    for _ in 0..count {
        let mut ent = [0u8; 12];
        file.read_exact(&mut ent)?;
        let offset = u32::from_le_bytes(ent[0..4].try_into().unwrap()) as u64;
        let size = u32::from_le_bytes(ent[4..8].try_into().unwrap());
        if offset.saturating_add(IMAGE_HEADER) > len || size < IMAGE_HEADER as u32 {
            continue;
        }
        let mut sec = [0u8; 16];
        let pos = file.stream_position()?;
        file.seek(SeekFrom::Start(offset))?;
        if file.read_exact(&mut sec).is_err() {
            file.seek(SeekFrom::Start(pos))?;
            continue;
        }
        file.seek(SeekFrom::Start(pos))?;
        if &sec[0..4] != b"SECi" {
            continue;
        }
        let type_format = (u32::from_le_bytes(sec[8..12].try_into().unwrap()) << 16)
            + u32::from_le_bytes(sec[12..16].try_into().unwrap());
        if type_format == JPEG_PREVIEW && best.is_none_or(|(_, s)| size > s) {
            best = Some((offset, size));
        }
    }
    let Some((offset, size)) = best else {
        return Err(DecodeError::NoEmbeddedPreview);
    };
    let jpeg_len = size as u64 - IMAGE_HEADER;
    if offset + IMAGE_HEADER + jpeg_len > len {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    let mut jpeg = vec![0u8; jpeg_len as usize];
    file.seek(SeekFrom::Start(offset + IMAGE_HEADER))?;
    file.read_exact(&mut jpeg)?;
    if jpeg.len() < 3 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Err(DecodeError::NoEmbeddedPreview);
    }
    Ok(jpeg)
}

fn read_u32_at(file: &mut File, pos: u64) -> Option<u32> {
    file.seek(SeekFrom::Start(pos)).ok()?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    Some(u32::from_le_bytes(buf))
}
