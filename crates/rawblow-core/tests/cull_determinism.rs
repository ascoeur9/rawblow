//! 컬링 캐시의 전제 검증(#50): 같은 파일을 디코드+분석하면 **결정적으로 같은 QualityReport**가
//! 나온다 — 그래야 (경로, mtime, 설정)로 결과를 캐싱해 재컬링 시 재계산을 건너뛰는 게 안전하다.
//! 외부 샘플 없이 합성 JPEG를 생성해 자족적으로 검증한다.

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::quality::analyze_selective;

fn write_jpeg(path: &std::path::Path, seed: u32) {
    let (w, h) = (640u32, 480u32);
    let mut img = image::RgbImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let v = (((x ^ y).wrapping_add(seed)) % 256) as u8;
        *px = image::Rgb([v, v.wrapping_add(60), v.wrapping_add(120)]);
    }
    img.save(path).unwrap();
}

#[test]
fn decode_analyze_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.jpg");
    write_jpeg(&p, 7);

    let run = || {
        let img = decode_file(&p, DecodeOptions { full_raw: false, max_edge: Some(512) }).unwrap();
        analyze_selective(&img, true, true, true)
    };
    let q1 = run();
    let q2 = run();
    // 결정적이어야 캐시가 안전(같은 입력 → 같은 출력).
    assert_eq!(q1.exposure.score, q2.exposure.score, "노출 결정적");
    assert_eq!(q1.focus.sharpness, q2.focus.sharpness, "초점 결정적");
    assert_eq!(q1.tilt.degrees, q2.tilt.degrees, "기울기 결정적");
    // 실제 값이 계산되었는지(전부 0이 아님).
    assert!(q1.exposure.score > 0.0);
}

#[test]
fn different_content_gives_different_report() {
    // 파일이 바뀌면(내용 변경) 보고서도 달라져야 한다 → mtime 기반 캐시 무효화의 정당성.
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("flat.jpg");
    let p2 = dir.path().join("detail.jpg");
    // 평탄(저디테일) vs 체커(고디테일) → 초점 선명도가 달라야.
    {
        let mut flat = image::RgbImage::new(640, 480);
        for px in flat.pixels_mut() { *px = image::Rgb([128, 128, 128]); }
        flat.save(&p1).unwrap();
    }
    {
        let mut det = image::RgbImage::new(640, 480);
        for (x, y, px) in det.enumerate_pixels_mut() {
            let v = if (x + y) % 2 == 0 { 230 } else { 20 };
            *px = image::Rgb([v, v, v]);
        }
        det.save(&p2).unwrap();
    }
    let analyze = |p: &std::path::Path| {
        let img = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(512) }).unwrap();
        analyze_selective(&img, false, true, false)
    };
    let flat_focus = analyze(&p1).focus.sharpness;
    let detail_focus = analyze(&p2).focus.sharpness;
    assert!(detail_focus > flat_focus, "고디테일이 더 선명: {detail_focus} > {flat_focus}");
}
