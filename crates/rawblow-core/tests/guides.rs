//! 구도 보조선(#90). 출하된 `guide_lines`의 표시 좌표를 확인한다.

use rawblow_core::config::Config;
use rawblow_core::guides::{clamp_guide_style, guide_lines, GuidePreset};

fn approx(got: &[f32], expect: &[f32]) {
    assert_eq!(got.len(), expect.len(), "len {got:?} vs {expect:?}");
    for (a, b) in got.iter().zip(expect) {
        assert!((a - b).abs() < 1.0e-5, "{a} != {b}");
    }
}

#[test]
fn preset_cycle_order() {
    assert_eq!(GuidePreset::Off.cycle(), GuidePreset::Thirds);
    assert_eq!(GuidePreset::Thirds.cycle(), GuidePreset::CenterCross);
    assert_eq!(GuidePreset::CenterCross.cycle(), GuidePreset::Square);
    assert_eq!(GuidePreset::Square.cycle(), GuidePreset::Custom);
    assert_eq!(GuidePreset::Custom.cycle(), GuidePreset::Off);
}

#[test]
fn thirds_and_center_are_relative() {
    let land = guide_lines(GuidePreset::Thirds, 300.0, 200.0, &[], &[]);
    let port = guide_lines(GuidePreset::Thirds, 200.0, 300.0, &[], &[]);
    approx(&land.vertical, &[1.0 / 3.0, 2.0 / 3.0]);
    approx(&land.horizontal, &[1.0 / 3.0, 2.0 / 3.0]);
    assert_eq!(land, port);

    let cross = guide_lines(GuidePreset::CenterCross, 300.0, 200.0, &[], &[]);
    approx(&cross.vertical, &[0.5]);
    approx(&cross.horizontal, &[0.5]);
}

#[test]
fn square_follows_displayed_size_after_rotation() {
    // 300×200으로 보이는 가로 사진. 정방의 변은 세로선 1/6, 5/6.
    let land = guide_lines(GuidePreset::Square, 300.0, 200.0, &[], &[]);
    approx(&land.vertical, &[1.0 / 6.0, 5.0 / 6.0]);
    assert!(land.horizontal.is_empty());

    // 90° 회전 뒤 앱이 넘기는 표시 크기는 200×300. 같은 정방이 가로선으로 옮겨 간다.
    let turned = guide_lines(GuidePreset::Square, 200.0, 300.0, &[], &[]);
    assert!(turned.vertical.is_empty());
    approx(&turned.horizontal, &[1.0 / 6.0, 5.0 / 6.0]);

    let already = guide_lines(GuidePreset::Square, 400.0, 400.0, &[], &[]);
    assert!(already.is_empty());
    assert!(guide_lines(GuidePreset::Square, 0.0, 100.0, &[], &[]).is_empty());
    assert!(guide_lines(GuidePreset::Square, f32::NAN, 100.0, &[], &[]).is_empty());
}

#[test]
fn custom_clamps_drops_non_finite_and_caps_at_eight() {
    let vertical = [
        0.1,
        f32::NAN,
        -0.2,
        1.4,
        0.2,
        0.3,
        0.4,
        0.5,
        0.6,
        0.7,
    ];
    let lines = guide_lines(
        GuidePreset::Custom,
        10.0,
        10.0,
        &vertical,
        &[2.0, f32::INFINITY],
    );
    approx(
        &lines.vertical,
        &[0.1, 0.0, 1.0, 0.2, 0.3, 0.4, 0.5, 0.6],
    );
    approx(&lines.horizontal, &[1.0]);

    let off = guide_lines(GuidePreset::Off, 10.0, 10.0, &[0.25], &[0.75]);
    assert!(off.is_empty());

    let empty = guide_lines(GuidePreset::Custom, 10.0, 10.0, &[], &[]);
    assert!(empty.is_empty());
}

#[test]
fn style_stays_inside_weight_and_opacity() {
    assert_eq!(clamp_guide_style(2.5, 0.5), (2.5, 0.5));
    assert_eq!(clamp_guide_style(0.0, 0.0), (1.0, 0.2));
    assert_eq!(clamp_guide_style(9.0, 3.0), (4.0, 1.0));
    assert_eq!(clamp_guide_style(f32::NAN, f32::INFINITY), (1.0, 0.8));
}

#[test]
fn missing_guide_fields_stay_off() {
    let cfg: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(cfg.guides, GuidePreset::Off);
    let lines = guide_lines(
        GuidePreset::Custom,
        300.0,
        200.0,
        &cfg.guide_custom.vertical,
        &cfg.guide_custom.horizontal,
    );
    approx(&lines.vertical, &[1.0 / 3.0, 2.0 / 3.0]);
    approx(&lines.horizontal, &[1.0 / 3.0, 2.0 / 3.0]);
    assert_eq!(cfg.guide_custom.rgb, [255, 255, 255]);
    assert_eq!(clamp_guide_style(cfg.guide_custom.weight, cfg.guide_custom.opacity), (1.0, 0.8));
}
