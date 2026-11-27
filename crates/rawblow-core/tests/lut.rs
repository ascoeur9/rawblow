//! 표시 LUT(#125). 앱이 텍스처를 만들 때 부르는 `display_rgba`와 `parse_cube`를 검사한다.

use rawblow_core::config::Config;
use rawblow_core::lut::{builtin, display_rgba, parse_cube, scan_cube_dir, CubeDir, CubeError, BUILTINS, CUBE_DIR_CAP};
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
        let sample = if name == "Rawblow-Pan" || name == "Rawblow-Vivid" || name == "Rawblow-Snap" { &red } else { &gray };
        assert_ne!(display_rgba(sample, Some(&lut), false), *sample, "{name}");
    }
    let warm = display_rgba(&gray, Some(&builtin("Rawblow-Warm").unwrap()), false);
    assert!(warm[0] > warm[2], "Warm은 회색을 따뜻하게");
    let pan = display_rgba(&red, Some(&builtin("Rawblow-Pan").unwrap()), false);
    assert!((pan[0] as i16 - pan[1] as i16).abs() <= 2);
    assert!((pan[1] as i16 - pan[2] as i16).abs() <= 2);
    let vivid = display_rgba(&red, Some(&builtin("Rawblow-Vivid").unwrap()), false);
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
    cfg.look.name = "Rawblow-Warm".into();
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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rb_cube_dir_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cube_dir_lists_top_level_cube_files_by_lowercase_name() {
    let dir = temp_dir("list");
    for name in ["b2.CUBE", "a.cube", "B.Cube", "c.txt", ".hidden.cube", "._a.cube", "cube"] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    // 하위 폴더 안의 .cube와 이름이 .cube인 폴더는 넣지 않는다.
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("d.cube"), "x").unwrap();
    std::fs::create_dir_all(dir.join("folder.cube")).unwrap();
    let (files, status) = scan_cube_dir(Some(&dir));
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.cube", "B.Cube", "b2.CUBE"]);
    assert_eq!(status, CubeDir::Found { shown: 3, total: 3 });
    assert!(files.iter().all(|p| p.parent() == Some(dir.as_path())), "절대 경로(폴더 + 이름)");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cube_dir_states_unset_missing_empty_and_cap() {
    assert_eq!(scan_cube_dir(None), (Vec::new(), CubeDir::Unset));
    let gone = std::env::temp_dir().join(format!("rb_cube_dir_gone_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&gone);
    assert_eq!(scan_cube_dir(Some(&gone)), (Vec::new(), CubeDir::Missing));
    // 폴더 자리에 파일이 있어도 읽을 수 없는 폴더다.
    let dir = temp_dir("states");
    let file = dir.join("not_a_dir.cube");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(scan_cube_dir(Some(&file)).1, CubeDir::Missing);
    std::fs::remove_file(&file).unwrap();
    std::fs::write(dir.join("readme.txt"), "x").unwrap();
    assert_eq!(scan_cube_dir(Some(&dir)), (Vec::new(), CubeDir::Empty));
    for i in 0..(CUBE_DIR_CAP + 3) {
        std::fs::write(dir.join(format!("l{i:04}.cube")), "x").unwrap();
    }
    let (files, status) = scan_cube_dir(Some(&dir));
    assert_eq!(files.len(), CUBE_DIR_CAP);
    assert_eq!(status, CubeDir::Found { shown: CUBE_DIR_CAP, total: CUBE_DIR_CAP + 3 }, "잘려도 전체 개수를 안다");
    assert_eq!(files[0].file_name().unwrap(), "l0000.cube", "이름순 앞에서 자른다");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 맥에서 이름을 바꾼 NFD 한글 파일과 NFC 파일이 섞여도 화면(NFC)에서 보이는 순서대로 놓는다.
#[test]
fn cube_dir_sorts_nfd_and_nfc_hangul_names_together() {
    let dir = temp_dir("nfd_sort");
    // 나(NFD: ㄴ + ㅏ)만 자모로 쓴다.
    for name in ["가.cube", "\u{1102}\u{1161}.cube", "다.cube", "Z.cube"] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    let (files, _) = scan_cube_dir(Some(&dir));
    let shown: Vec<String> = files
        .iter()
        .map(|p| rawblow_core::text::nfc_hangul(&p.file_stem().unwrap().to_string_lossy()))
        .collect();
    assert_eq!(shown, ["Z", "가", "나", "다"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 폴더는 있는데 읽을 수 없으면(권한) 「없음」이 아니라 「읽을 수 없음」.
#[cfg(unix)]
#[test]
fn cube_dir_without_permission_is_unreadable_not_missing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("noperm");
    std::fs::write(dir.join("a.cube"), "x").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    // root는 권한과 상관없이 읽으므로 그때는 이 경우를 만들 수 없다.
    let blocked = std::fs::read_dir(&dir).is_err();
    let got = scan_cube_dir(Some(&dir));
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    if blocked {
        assert_eq!(got, (Vec::new(), CubeDir::Unreadable));
    }
}
