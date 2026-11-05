//! 광색역 표시(#122). 입력 선택, 출력 폴백, ICC→sRGB와 Display P3의 픽셀 차이.

use rawblow_core::color::{
    choose_input, convert_display, convert_to_srgb, effective_output, source_histogram, InputColor,
    OsColor, OutputColor,
};

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn put_u16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn s15(v: f64) -> u32 {
    (v * 65536.0).round() as i32 as u32
}

/// 감마 2.2 매트릭스 ICC. r/g/b XYZ는 D50에 가깝게 합이 맞아야 qcms가 변환을 만든다.
fn matrix_icc(rgb_xyz: [[f64; 3]; 3]) -> Vec<u8> {
    let mut buf = vec![0u8; 280];
    let len = buf.len() as u32;
    put_u32(&mut buf, 0, len);
    buf[8] = 2;
    buf[9] = 0x40;
    put_u32(&mut buf, 12, u32::from_be_bytes(*b"mntr"));
    put_u32(&mut buf, 16, u32::from_be_bytes(*b"RGB "));
    put_u32(&mut buf, 20, u32::from_be_bytes(*b"XYZ "));
    put_u32(&mut buf, 36, u32::from_be_bytes(*b"acsp"));
    put_u32(&mut buf, 68, s15(0.9642));
    put_u32(&mut buf, 72, s15(1.0));
    put_u32(&mut buf, 76, s15(0.8249));
    put_u32(&mut buf, 128, 6);
    let tags = [
        (*b"rXYZ", 204u32, 20u32),
        (*b"gXYZ", 224, 20),
        (*b"bXYZ", 244, 20),
        (*b"rTRC", 264, 16),
        (*b"gTRC", 264, 16),
        (*b"bTRC", 264, 16),
    ];
    for (i, (sig, off, size)) in tags.iter().enumerate() {
        let at = 132 + i * 12;
        put_u32(&mut buf, at, u32::from_be_bytes(*sig));
        put_u32(&mut buf, at + 4, *off);
        put_u32(&mut buf, at + 8, *size);
    }
    for (i, xyz) in rgb_xyz.iter().enumerate() {
        let at = 204 + i * 20;
        put_u32(&mut buf, at, u32::from_be_bytes(*b"XYZ "));
        put_u32(&mut buf, at + 8, s15(xyz[0]));
        put_u32(&mut buf, at + 12, s15(xyz[1]));
        put_u32(&mut buf, at + 16, s15(xyz[2]));
    }
    put_u32(&mut buf, 264, u32::from_be_bytes(*b"curv"));
    put_u32(&mut buf, 272, 1);
    put_u16(&mut buf, 276, (2.2_f64 * 256.0).round() as u16);
    buf
}

/// sRGB에 가깝지 않은 녹색이 넓은 매트릭스. 합은 D50 근처.
fn wide_green_icc() -> Vec<u8> {
    matrix_icc([
        [0.40, 0.20, 0.02],
        [0.30, 0.70, 0.10],
        [0.2642, 0.10, 0.7049],
    ])
}

fn px() -> Vec<u8> {
    vec![220, 40, 30, 255, 20, 180, 40, 255]
}

#[test]
fn r03_without_icc_is_adobe_rgb_and_icc_wins() {
    assert_eq!(choose_input(None, Some(0xFFFF), Some("R03")), InputColor::AdobeRgb);
    assert_eq!(choose_input(None, Some(0xFFFF), Some("R03\0")), InputColor::AdobeRgb);
    assert_eq!(choose_input(None, Some(0xFFFF), Some(" r03 ")), InputColor::AdobeRgb);
    assert_eq!(choose_input(None, Some(1), Some("R03")), InputColor::Srgb);
    assert_eq!(choose_input(None, Some(0xFFFF), Some("R98")), InputColor::Srgb);
    assert_eq!(choose_input(None, None, None), InputColor::Srgb);
    let icc = b"prof";
    assert_eq!(choose_input(Some(icc), Some(0xFFFF), Some("R03")), InputColor::Icc(icc));
    assert_eq!(choose_input(Some(&[]), Some(0xFFFF), Some("R03")), InputColor::AdobeRgb);
}

#[test]
fn output_falls_back_to_srgb() {
    assert_eq!(effective_output(false, &OsColor::MacOs { layer_ok: true }), OutputColor::Srgb);
    assert_eq!(effective_output(true, &OsColor::Linux), OutputColor::Srgb);
    assert_eq!(effective_output(true, &OsColor::MacOs { layer_ok: false }), OutputColor::Srgb);
    assert_eq!(
        effective_output(true, &OsColor::Windows { icc: None, acm_or_hdr: false }),
        OutputColor::Srgb
    );
    assert_eq!(
        effective_output(true, &OsColor::Windows { icc: Some(b"x".to_vec()), acm_or_hdr: true }),
        OutputColor::Srgb
    );
    assert_eq!(effective_output(true, &OsColor::MacOs { layer_ok: true }), OutputColor::DisplayP3);
    let icc = b"monitor".to_vec();
    assert_eq!(
        effective_output(true, &OsColor::Windows { icc: Some(icc.clone()), acm_or_hdr: false }),
        OutputColor::Icc(icc)
    );
    assert_eq!(OutputColor::Srgb.cache_tag(), 0);
    assert_ne!(OutputColor::DisplayP3.cache_tag(), 0);
}

#[test]
fn histogram_is_taken_from_the_buffer_before_conversion() {
    let src = px();
    let before = source_histogram(&src);
    let icc = wide_green_icc();
    let mut a = src.clone();
    let mut b = src.clone();
    let _ = convert_display(&mut a, InputColor::Icc(&icc), &OutputColor::Srgb);
    let _ = convert_display(&mut b, InputColor::Icc(&icc), &OutputColor::DisplayP3);
    // 목적지가 달라도 변환 전에 잰 값은 같다. 변환된 버퍼로 다시 재면 목적지에 끌려간다.
    assert_eq!(before, source_histogram(&src));
    assert_eq!(source_histogram(&a) == source_histogram(&b), a == b);
}

#[cfg(feature = "color")]
#[test]
fn display_p3_differs_and_srgb_matches_current_path() {
    let icc = wide_green_icc();
    let mut via_new = px();
    let mut via_old = px();
    assert!(convert_display(&mut via_new, InputColor::Icc(&icc), &OutputColor::Srgb));
    assert!(convert_to_srgb(&mut via_old, &icc));
    assert_eq!(via_new, via_old, "sRGB 목적지는 기존 convert_to_srgb와 같은 픽셀");

    let mut p3 = px();
    assert!(convert_display(&mut p3, InputColor::Icc(&icc), &OutputColor::DisplayP3));
    assert_ne!(p3, via_old, "Display P3는 sRGB 변환과 다른 픽셀");

    let mut adobe = px();
    let before = adobe.clone();
    assert!(convert_display(&mut adobe, InputColor::AdobeRgb, &OutputColor::Srgb));
    assert_ne!(adobe, before, "R03 Adobe RGB는 sRGB로 바뀐다");

    let mut tagged_off = px();
    assert!(!convert_display(&mut tagged_off, InputColor::Srgb, &OutputColor::Srgb));
    assert_eq!(tagged_off, px(), "태그 없고 목적지가 sRGB면 픽셀이 그대로");

    let mut bad = px();
    assert!(!convert_display(&mut bad, InputColor::Icc(b"not-an-icc"), &OutputColor::DisplayP3));
    assert_eq!(bad, px(), "입력 프로파일을 못 읽으면 픽셀이 그대로");

    let mut bad_out = px();
    assert!(convert_display(&mut bad_out, InputColor::Icc(&icc), &OutputColor::Icc(b"not-an-icc".to_vec())));
    assert_eq!(bad_out, via_old, "출력 프로파일을 못 읽으면 sRGB 변환과 같다");
}

#[test]
fn untagged_jpeg_keeps_histogram_and_follows_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.jpg");
    let buf = image::RgbImage::from_fn(8, 8, |x, y| image::Rgb([(x * 30) as u8, 40, (y * 20) as u8]));
    buf.save(&path).unwrap();
    let opts = rawblow_core::decode::DecodeOptions::default();
    let srgb = rawblow_core::decode::decode_file(&path, opts).unwrap();
    let p3 = rawblow_core::decode::decode_file_to(&path, opts, &OutputColor::DisplayP3).unwrap();
    assert_eq!(srgb.source_histo, p3.source_histo);
    assert!(srgb.source_histo.is_some());
    #[cfg(feature = "color")]
    {
        assert_ne!(srgb.rgba, p3.rgba, "태그 없는 JPEG도 Display P3면 픽셀이 바뀐다");
        assert!(!srgb.color_managed);
        assert!(p3.color_managed);
    }
    #[cfg(not(feature = "color"))]
    assert_eq!(srgb.rgba, p3.rgba);
}

#[cfg(not(feature = "color"))]
#[test]
fn feature_off_leaves_pixels() {
    let icc = wide_green_icc();
    let mut buf = px();
    assert!(!convert_display(&mut buf, InputColor::Icc(&icc), &OutputColor::DisplayP3));
    assert!(!convert_to_srgb(&mut buf, &icc));
    assert_eq!(buf, px());
}
