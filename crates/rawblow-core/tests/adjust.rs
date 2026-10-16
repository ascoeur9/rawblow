//! 표시 조정(#130). 앱이 텍스처를 만들 때 부르는 `display_rgba`를 그대로 검사한다.

use rawblow_core::adjust::{display_rgba, Adjust};
use rawblow_core::config::Config;
use rawblow_core::model::{ColorTag, Label};
use rawblow_core::sidecar::ItemRec;

fn px(rgb: [u8; 3], a: u8) -> Vec<u8> {
    vec![rgb[0], rgb[1], rgb[2], a]
}

#[test]
fn zero_adjustment_keeps_every_byte() {
    let src = vec![0, 1, 2, 3, 128, 64, 200, 7, 255, 255, 255, 255, 9];
    let out = display_rgba(&src, Adjust::default(), false);
    assert_eq!(out, src);
}

#[test]
fn compare_hold_ignores_a_nonzero_adjustment() {
    let src = px([128, 128, 128], 200);
    let mut adj = Adjust::default();
    adj.exposure = 1.0;
    assert_eq!(display_rgba(&src, adj, true), src);
}

#[test]
fn exposure_plus_one_stops_lifts_mid_gray_to_white() {
    let src = px([128, 128, 128], 200);
    let mut adj = Adjust::default();
    adj.exposure = 1.0;
    let out = display_rgba(&src, adj, false);
    assert_eq!(&out[..3], &[255, 255, 255]);
    assert_eq!(out[3], 200, "알파는 그대로");
}

#[test]
fn exposure_above_the_slider_matches_plus_two() {
    let src = px([128, 40, 20], 255);
    let mut hi = Adjust::default();
    hi.exposure = 9.0;
    let mut cap = Adjust::default();
    cap.exposure = 2.0;
    assert_eq!(display_rgba(&src, hi, false), display_rgba(&src, cap, false));
}

#[test]
fn temperature_warms_by_raising_red_and_lowering_blue() {
    let src = px([128, 128, 128], 255);
    let mut adj = Adjust::default();
    adj.temperature = 1.0;
    let out = display_rgba(&src, adj, false);
    assert_eq!(out[0], 160, "R");
    assert_eq!(out[1], 128, "G는 그대로");
    assert_eq!(out[2], 96, "B");
    let mut cold = Adjust::default();
    cold.temperature = -1.0;
    let c = display_rgba(&src, cold, false);
    assert!(c[0] < 128 && c[2] > 128, "음수는 차갑게");
}

#[test]
fn contrast_pivots_around_mid_gray() {
    let mut adj = Adjust::default();
    adj.contrast = 1.0;
    let dark = display_rgba(&px([64, 64, 64], 1), adj, false);
    assert_eq!(dark[0], 1, "어두운 픽셀은 더 어두워진다");
    assert_eq!(dark[3], 1);
    let mut flat = Adjust::default();
    flat.contrast = -1.0;
    let mid = display_rgba(&px([10, 200, 40], 9), flat, false);
    assert_eq!(&mid[..3], &[128, 128, 128]);
    assert_eq!(mid[3], 9);
}

#[test]
fn grain_is_deterministic_and_leaves_alpha() {
    let src = vec![128u8; 64];
    let mut adj = Adjust::default();
    adj.grain = 1.0;
    let a = display_rgba(&src, adj, false);
    let b = display_rgba(&src, adj, false);
    assert_eq!(a, b);
    assert_ne!(a, src, "그레인 1은 중간의 회색을 움직인다");
    for px in a.chunks_exact(4) {
        assert_eq!(px[3], 128);
    }
    adj.grain = 0.0;
    assert_eq!(display_rgba(&src, adj, false), src);
}

#[test]
fn non_finite_adjustment_matches_the_original() {
    let src = px([40, 50, 60], 255);
    let adj = Adjust {
        exposure: f32::NAN,
        temperature: f32::INFINITY,
        contrast: f32::NEG_INFINITY,
        grain: f32::NAN,
    };
    assert_eq!(display_rgba(&src, adj, false), src);
}

#[test]
fn missing_config_field_is_neutral_and_roundtrips() {
    let cfg: Config = serde_json::from_str("{}").expect("빈 설정");
    assert!(cfg.adjust.is_neutral());
    let mut cfg = Config::default();
    cfg.adjust.exposure = -1.5;
    cfg.adjust.temperature = 0.25;
    cfg.adjust.contrast = 0.5;
    cfg.adjust.grain = 0.3;
    let back: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
    assert_eq!(back.adjust, cfg.adjust);
}

#[test]
fn session_record_has_no_per_photo_adjustment() {
    let rec = ItemRec {
        label: Label::Pick,
        stars: 2,
        tag: ColorTag::None,
        members: vec!["IMG_0001.NEF".into()],
    };
    let v = serde_json::to_value(&rec).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["label", "members", "stars", "tag"]);
}
