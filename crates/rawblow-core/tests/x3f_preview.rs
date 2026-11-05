//! X3F(FOVb) 스캔과 임베디드 JPEG 프리뷰 디코드(#126).

use std::io::Cursor;
use std::path::Path;

use rawblow_core::decode::{self, DecodeOptions};
use rawblow_core::model::{is_supported, Kind};
use rawblow_core::scan::scan_folder;
use rawblow_core::{x3f, SortOrder};

fn tiny_jpeg(rgb: [u8; 3], w: u32, h: u32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 3) as usize];
    for chunk in px.chunks_exact_mut(3) {
        chunk.copy_from_slice(&rgb);
    }
    let mut buf = Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
    enc.encode(&px, w, h, image::ExtendedColorType::Rgb8).expect("jpeg");
    buf.into_inner()
}

/// 최소 X3F: FOVb 헤더 + SECi JPEG 섹션 + SECd 디렉터리 + 끝의 디렉터리 오프셋.
fn minimal_x3f(jpeg: &[u8], rotation: u32) -> Vec<u8> {
    let mut v = vec![0u8; 40];
    v[0..4].copy_from_slice(b"FOVb");
    v[4..8].copy_from_slice(&0x0002_0000u32.to_le_bytes());
    v[36..40].copy_from_slice(&rotation.to_le_bytes());
    let section_off = v.len() as u32;
    let mut sec = Vec::new();
    sec.extend_from_slice(b"SECi");
    sec.extend_from_slice(&0x0002_0001u32.to_le_bytes());
    sec.extend_from_slice(&2u32.to_le_bytes());
    sec.extend_from_slice(&18u32.to_le_bytes());
    sec.extend_from_slice(&16u32.to_le_bytes());
    sec.extend_from_slice(&8u32.to_le_bytes());
    sec.extend_from_slice(&0u32.to_le_bytes());
    sec.extend_from_slice(jpeg);
    let section_size = sec.len() as u32;
    v.extend_from_slice(&sec);
    let dir_off = v.len() as u32;
    v.extend_from_slice(b"SECd");
    v.extend_from_slice(&0x0002_0001u32.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&section_off.to_le_bytes());
    v.extend_from_slice(&section_size.to_le_bytes());
    v.extend_from_slice(&JPEG_TYPE.to_le_bytes());
    v.extend_from_slice(&dir_off.to_le_bytes());
    v
}

const JPEG_TYPE: u32 = 0x0002_0012;

#[test]
fn fovb_x3f_is_scanned_and_decodes_preview() {
    let dir = tempfile::tempdir().unwrap();
    let jpeg = tiny_jpeg([220, 10, 10], 16, 8);
    let path = dir.path().join("sigma.X3F");
    std::fs::write(&path, minimal_x3f(&jpeg, 0)).unwrap();
    assert!(is_supported(&path));
    assert_eq!(rawblow_core::model::kind_of(&path), Some(Kind::Raw));

    let entries = scan_folder(dir.path(), false, SortOrder::Name);
    assert_eq!(entries.len(), 1, "FOVb .x3f 가 스캔에 포함됨");
    assert!(entries[0].display.extension().unwrap().eq_ignore_ascii_case("x3f"));

    let img = decode::decode_file(&path, DecodeOptions { full_raw: false, max_edge: Some(64) })
        .expect("임베디드 JPEG 프리뷰");
    assert!(img.width > 0 && img.height > 0);
    assert!(!img.rgba.is_empty());
    assert!(img.rgba.len() >= 4);
    assert!(img.rgba[0] > 180 && img.rgba[1] < 40 && img.rgba[2] < 40, "빨간 프리뷰 {:?}", &img.rgba[..4]);
    assert_eq!((img.width, img.height), (16, 8));
}

#[test]
fn x3f_header_rotation_turns_the_preview() {
    assert_eq!(x3f::rotation_to_orientation(0), 1);
    assert_eq!(x3f::rotation_to_orientation(90), 6);
    assert_eq!(x3f::rotation_to_orientation(180), 3);
    assert_eq!(x3f::rotation_to_orientation(270), 8);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("turn.x3f");
    std::fs::write(&path, minimal_x3f(&tiny_jpeg([10, 10, 200], 16, 8), 90)).unwrap();
    let img = decode::decode_file(Path::new(&path), DecodeOptions::default()).unwrap();
    assert_eq!((img.width, img.height), (8, 16), "90도 회전 후 가로세로가 바뀜");
    assert!(img.rgba[2] > 180, "파란 채널이 남음");
}

#[test]
fn x3f_without_jpeg_section_fails_decode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.x3f");
    std::fs::write(&path, b"FOVb").unwrap();
    assert!(is_supported(&path));
    assert!(decode::decode_file(&path, DecodeOptions::default()).is_err());
}
