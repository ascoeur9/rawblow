//! 표시 LUT(#125). 앱이 텍스처를 만들 때 부르는 `display_rgba`와 `parse_cube`를 검사한다.

use rawblow_core::config::Config;
use rawblow_core::lut::{builtin, display_rgba, parse_cube, CubeError, BUILTINS};
use rawblow_core::model::{ColorTag, Label};
use rawblow_core::sidecar::ItemRec;

fn px(rgb: [u8; 3]) -> Vec<u8> {
    vec![rgb[0], rgb[1], rgb[2], 200]
}

fn swap_cube() -> &'static str {
    "\
TITLE \"swap\"
# comment
LUT_3D_SIZE 2
DOMAIN_MIN 0 0 0
DOMAIN_MAX 1 1 1
0 0 0
0 0 1
0 1 0
0 1 1
1 0 0
1 0 1
1 1 0
1 1 1
"
}

#[test]
fn no_lut_keeps_every_byte() {
    let src = vec![1, 2, 3, 4, 9];
    assert_eq!(display_rgba(&src, None, false), src);
}

#[test]
fn cube_swaps_red_and_blue_and_hold_restores_the_source() {
    let lut = parse_cube(swap_cube()).unwrap();
    let src = px([255, 0, 0]);
    let out = display_rgba(&src, Some(&lut), false);
    assert_eq!(&out[..3], &[0, 0, 255]);
    assert_eq!(out[3], 200);
    assert_eq!(display_rgba(&src, Some(&lut), true), src);
}

#[test]
fn red_cube_paints_every_pixel() {
    let mut text = String::from("LUT_3D_SIZE 2\n");
    for _ in 0..8 {
        text.push_str("1 0 0\n");
    }
    let lut = parse_cube(&text).unwrap();
    let out = display_rgba(&px([10, 20, 30]), Some(&lut), false);
    assert_eq!(&out[..3], &[255, 0, 0]);
}

#[test]
fn rejects_1d_bad_size_and_short_data() {
    assert_eq!(parse_cube("LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap_err(), CubeError::Not3d);
    assert_eq!(parse_cube("LUT_3D_SIZE 1\n0 0 0\n").unwrap_err(), CubeError::Size);
    assert_eq!(parse_cube("LUT_3D_SIZE 65\n0 0 0\n").unwrap_err(), CubeError::Size);
    assert_eq!(parse_cube("LUT_3D_SIZE 2\n0 0 0\n").unwrap_err(), CubeError::Short);
    assert_eq!(parse_cube("0 0 0\n").unwrap_err(), CubeError::Not3d);
}

#[test]
fn size_64_parses() {
    let mut text = String::from("LUT_3D_SIZE 64\n");
    let n = 64usize * 64 * 64;
    text.reserve(n * 6);
    for _ in 0..n {
        text.push_str("0 0 0\n");
    }
    let lut = parse_cube(&text).unwrap();
    let out = display_rgba(&px([128, 64, 32]), Some(&lut), false);
    assert_eq!(&out[..3], &[0, 0, 0]);
}

#[test]
fn builtins_change_pixels_and_brand_names_do_not_exist() {
    let gray = px([128, 128, 128]);
    let red = px([180, 40, 40]);
    for name in BUILTINS {
        let lut = builtin(name).unwrap_or_else(|| panic!("{name}"));
        let sample = if name == "Pan" || name == "Vivid" || name == "Snap" { &red } else { &gray };
        assert_ne!(display_rgba(sample, Some(&lut), false), *sample, "{name}");
    }
    let warm = display_rgba(&gray, Some(&builtin("Warm").unwrap()), false);
    assert!(warm[0] > warm[2], "Warm은 회색을 따뜻하게");
    let pan = display_rgba(&red, Some(&builtin("Pan").unwrap()), false);
    assert!((pan[0] as i16 - pan[1] as i16).abs() <= 2);
    assert!((pan[1] as i16 - pan[2] as i16).abs() <= 2);
    let vivid = display_rgba(&red, Some(&builtin("Vivid").unwrap()), false);
    let src_span = 180 - 40;
    let out_span = vivid[0].max(vivid[1]).max(vivid[2]) - vivid[0].min(vivid[1]).min(vivid[2]);
    assert!(out_span > src_span, "Vivid는 채도를 올린다");
    for absent in ["Kodak", "Fuji", "Ilford", "Portra", "Velvia", "HP5", "warm", ""] {
        assert!(builtin(absent).is_none(), "{absent}");
    }
}

#[test]
fn missing_config_field_is_off_and_session_has_no_look() {
    let cfg: Config = serde_json::from_str("{}").unwrap();
    assert!(cfg.look.name.is_empty());
    assert!(cfg.look.cube_path.is_empty());
    let mut cfg = Config::default();
    cfg.look.name = "Warm".into();
    cfg.look.cube_path = "/tmp/a.cube".into();
    let back: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
    assert_eq!(back.look, cfg.look);

    let rec = ItemRec {
        label: Label::Hold,
        stars: 0,
        tag: ColorTag::None,
        members: vec!["a.NEF".into()],
    };
    let v = serde_json::to_value(&rec).unwrap();
    assert!(v.get("name").is_none());
    assert!(v.get("cube_path").is_none());
    assert!(v.get("look").is_none());
}
