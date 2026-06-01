//! [임시 검증] 합성 RW2(type-7 IFD0 JPEG blob)로 ORIG IFD-embedded 경로를 검증.
use image::{ImageBuffer, Rgb};
use rawblow_core::decode::{self, DecodeOptions};
use std::io::Write;

fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 64u8])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(buf)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

/// LE TIFF: header(8) + IFD0 at 8. One type-7 entry (tag 0x0127) -> jpeg blob appended after IFD.
/// If `truncate_blob` true, we write a byte length larger than the actual appended bytes
/// (simulating an offset/len pointing past EOF).
fn build_rw2(jpeg: &[u8], extra_entries: &[(u16, u16, u32, u32)], claim_len: Option<u32>) -> Vec<u8> {
    let mut entries: Vec<(u16, u16, u32, u32)> = Vec::new();
    // placeholder for the big jpeg entry; offset filled later.
    entries.extend_from_slice(extra_entries);
    let entry_count = entries.len() as u16 + 1; // +1 big jpeg
    // IFD layout: count(2) + entries*12 + next_ifd(4)
    let ifd_size = 2 + (entry_count as usize) * 12 + 4;
    let blob_offset = 8 + ifd_size;
    let mut file = Vec::new();
    file.extend_from_slice(b"II"); // LE
    file.extend_from_slice(&0x0055u16.to_le_bytes()); // RW2 magic 0x55
    file.extend_from_slice(&8u32.to_le_bytes()); // IFD0 @ 8
    // IFD0
    file.extend_from_slice(&entry_count.to_le_bytes());
    // big jpeg entry first (tag 0x0127, type 7, count=len, val=offset)
    let len = claim_len.unwrap_or(jpeg.len() as u32);
    file.extend_from_slice(&0x0127u16.to_le_bytes());
    file.extend_from_slice(&7u16.to_le_bytes());
    file.extend_from_slice(&len.to_le_bytes());
    file.extend_from_slice(&(blob_offset as u32).to_le_bytes());
    for &(tag, typ, cnt, val) in &entries {
        file.extend_from_slice(&tag.to_le_bytes());
        file.extend_from_slice(&typ.to_le_bytes());
        file.extend_from_slice(&cnt.to_le_bytes());
        file.extend_from_slice(&val.to_le_bytes());
    }
    file.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
    assert_eq!(file.len(), blob_offset);
    file.extend_from_slice(jpeg);
    file
}

fn write_tmp(name: &str, data: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("rb_ifd_probe");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::File::create(&p).unwrap().write_all(data).unwrap();
    p
}

fn main() {
    // 1) Happy path: 3200px embedded JPEG, ORIG (full_raw, max_edge 8192) -> should return 3200px.
    let jpeg = make_jpeg(3200, 2000);
    let rw2 = build_rw2(&jpeg, &[], None);
    let p = write_tmp("happy.rw2", &rw2);
    let img = decode::decode_file(&p, DecodeOptions { full_raw: true, max_edge: Some(8192) });
    println!("[1 happy 3200px] {:?}", img.as_ref().map(|i| (i.width, i.height)));

    // 2) Small embedded (1920px) < 3000 gate: ORIG should NOT accept it via IFD path -> falls through
    //    to decode_full_raw (which will fail on this fake) -> decode_largest_embedded (whole-file).
    let small = make_jpeg(1920, 1280);
    let rw2s = build_rw2(&small, &[], None);
    let ps = write_tmp("small.rw2", &rw2s);
    let imgs = decode::decode_file(&ps, DecodeOptions { full_raw: true, max_edge: Some(8192) });
    println!("[2 small 1920px, gate=3000] {:?}", imgs.as_ref().map(|i| (i.width, i.height)));

    // 3) TRUNCATION: claim len = real+5MB so read_range hits EOF; blob still starts FF D8.
    let jpeg2 = make_jpeg(3200, 2000);
    let rw2t = build_rw2(&jpeg2, &[], Some(jpeg2.len() as u32 + 5_000_000));
    let pt = write_tmp("trunc.rw2", &rw2t);
    let imgt = decode::decode_file(&pt, DecodeOptions { full_raw: true, max_edge: Some(8192) });
    println!("[3 trunc claim len huge] {:?}", imgt.as_ref().map(|i| (i.width, i.height)));

    // 4) Bogus offset (val points past EOF) for the type-7 blob.
    //    Build manually: same as happy but overwrite the offset field with a huge value.
    let mut rw2b = build_rw2(&make_jpeg(3200, 2000), &[], None);
    // offset field of first entry = bytes [8+2+8 .. +4] = header(8)+count(2)+tag(2)+type(2)+count(4)
    let off_pos = 8 + 2 + 2 + 2 + 4;
    rw2b[off_pos..off_pos + 4].copy_from_slice(&0xF000_0000u32.to_le_bytes());
    let pb = write_tmp("bogus.rw2", &rw2b);
    let imgb = decode::decode_file(&pb, DecodeOptions { full_raw: true, max_edge: Some(8192) });
    println!("[4 bogus offset past EOF] is_err={} {:?}", imgb.is_err(), imgb.as_ref().map(|i| (i.width, i.height)));

    // 5) Orientation: container IFD0 has Orientation=6 (rotate90). Embedded 3200x2000 -> rotated to 2000x3200.
    let mut extra = Vec::new();
    extra.push((0x0112u16, 3u16, 1u32, 6u32)); // Orientation SHORT inline = 6
    let rw2o = build_rw2(&make_jpeg(3200, 2000), &extra, None);
    let po = write_tmp("orient.rw2", &rw2o);
    let imgo = decode::decode_file(&po, DecodeOptions { full_raw: true, max_edge: Some(8192) });
    println!("[5 orient=6 rotate90] {:?} (expect 2000x3200)", imgo.as_ref().map(|i| (i.width, i.height)));
}
