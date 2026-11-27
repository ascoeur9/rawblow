//! #123 단축키 맵. 한 키는 언제나 동작 하나다 — 지정·리셋·프리셋·저장 파일 읽기 모두.

use rawblow_core::config::Config;
use rawblow_core::shortcut::{Chord, FixedKey, Resolve, ShortcutAction, ShortcutMap};

fn one(action: ShortcutAction) -> Resolve {
    Resolve::One(action)
}

/// 모든 동작의 키를 모아 한 키가 두 번 나오면 실패.
fn assert_one_action_per_key(map: &ShortcutMap) {
    let mut seen: Vec<(Chord, ShortcutAction)> = Vec::new();
    for action in ShortcutAction::ALL {
        for chord in map.chords(action) {
            if let Some((_, other)) = seen.iter().find(|(c, _)| *c == chord) {
                panic!("{chord:?}가 {other:?}와 {action:?}에 함께 묶였다");
            }
            assert_eq!(map.resolve(&chord), one(action));
            seen.push((chord, action));
        }
    }
}

fn load(json: &str) -> ShortcutMap {
    let cfg: Config = serde_json::from_str(json).unwrap();
    assert_one_action_per_key(&cfg.shortcuts);
    cfg.shortcuts
}

#[test]
fn default_map_matches_current_keys() {
    let map = ShortcutMap::default();
    assert_one_action_per_key(&map);
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("W")), one(ShortcutAction::Hold));
    assert_eq!(map.resolve(&Chord::plain("E")), one(ShortcutAction::Reject));
    assert_eq!(map.resolve(&Chord::plain("R")), one(ShortcutAction::Clear));
    assert_eq!(map.resolve(&Chord::new("E", true, false, false)), one(ShortcutAction::Transfer));
    assert_eq!(map.resolve(&Chord::plain("Space")), one(ShortcutAction::Fit));
    assert_eq!(map.resolve(&Chord::plain("Z")), Resolve::None, "창맞춤은 Space 하나");
    assert_eq!(map.resolve(&Chord::new("Z", true, false, false)), one(ShortcutAction::Undo));
    assert_eq!(map.resolve(&Chord::new("Z", true, true, false)), one(ShortcutAction::Redo));
    assert_eq!(map.resolve(&Chord::new("Y", true, false, false)), one(ShortcutAction::Redo));
    assert_eq!(map.resolve(&Chord::plain("Num1")), one(ShortcutAction::Star1));
    assert_eq!(map.resolve(&Chord::new("Num1", false, true, false)), one(ShortcutAction::Tag1));
    assert_eq!(map.resolve(&Chord::plain("Backtick")), one(ShortcutAction::StarClear));
    assert_eq!(map.resolve(&Chord::new("Num0", false, true, false)), one(ShortcutAction::TagClear));
    // 예전에 맵 밖에 있던 키(#123 재검토)도 맵에 있다.
    assert_eq!(map.resolve(&Chord::plain("CloseBracket")), one(ShortcutAction::Guides));
    assert_eq!(map.resolve(&Chord::plain("B")), one(ShortcutAction::Bulk));
    assert_eq!(map.resolve(&Chord::new("O", true, false, false)), one(ShortcutAction::OpenFolder));
    assert_eq!(map.resolve(&Chord::plain("Backslash")), one(ShortcutAction::LookCompare));
    assert_eq!(map.resolve(&Chord::plain("J")), Resolve::None);
    assert_eq!(map.chords(ShortcutAction::Fit), vec![Chord::plain("Space")]);
}

#[test]
fn binding_a_used_key_swaps_instead_of_sharing_it() {
    let mut map = ShortcutMap::default();
    assert_eq!(map.bind(ShortcutAction::Redo, Chord::plain("X")), Ok(None));
    assert_eq!(map.chords(ShortcutAction::Redo), vec![Chord::plain("X")]);
    assert_eq!(map.resolve(&Chord::new("Z", true, true, false)), Resolve::None);
    assert_eq!(map.resolve(&Chord::new("Y", true, false, false)), Resolve::None);

    // 채택에 보류의 W → 보류는 채택이 쓰던 Q를 받는다. W는 채택만 실행한다.
    assert_eq!(map.bind(ShortcutAction::Pick, Chord::plain("W")), Ok(Some(ShortcutAction::Hold)));
    assert_eq!(map.resolve(&Chord::plain("W")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Hold));
    assert_one_action_per_key(&map);
}

#[test]
fn close_bracket_guides_can_be_rebound_and_swapped() {
    let mut map = ShortcutMap::default();
    // 점프(G)를 ]로 → 구도선은 G를 받는다.
    assert_eq!(map.bind(ShortcutAction::Jump, Chord::plain("CloseBracket")), Ok(Some(ShortcutAction::Guides)));
    assert_eq!(map.resolve(&Chord::plain("CloseBracket")), one(ShortcutAction::Jump));
    assert_eq!(map.resolve(&Chord::plain("G")), one(ShortcutAction::Guides));
    // 구도선을 [로.
    assert_eq!(map.bind(ShortcutAction::Guides, Chord::plain("OpenBracket")), Ok(None));
    assert_eq!(map.chords(ShortcutAction::Guides), vec![Chord::plain("OpenBracket")]);
    assert_eq!(map.resolve(&Chord::plain("G")), Resolve::None);
    assert_one_action_per_key(&map);
}

#[test]
fn fixed_keys_cannot_be_bound() {
    let mut map = ShortcutMap::default();
    for chord in [
        Chord::plain("Escape"),
        Chord::plain("Enter"),
        Chord::plain("ArrowRight"),
        Chord::plain("F1"),
        Chord::new("Slash", false, true, false),
        Chord::plain("Tab"),
        Chord::new("Minus", true, false, false),
        Chord::new("Num0", true, false, false),
    ] {
        assert_eq!(map.bind(ShortcutAction::Guides, chord), Err(FixedKey));
    }
    assert_eq!(map, ShortcutMap::default());
}

#[test]
fn reset_and_presets_restore_the_stored_actions() {
    let mut map = ShortcutMap::default();
    map.bind(ShortcutAction::Pick, Chord::plain("W")).unwrap();
    map.bind(ShortcutAction::Fit, Chord::plain("X")).unwrap();
    map.reset();
    assert_eq!(map, ShortcutMap::default());

    assert!(map.preset("jkl"));
    assert_one_action_per_key(&map);
    assert_eq!(map.resolve(&Chord::plain("J")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("K")), one(ShortcutAction::Hold));
    assert_eq!(map.resolve(&Chord::plain("L")), one(ShortcutAction::Reject));
    assert_eq!(map.resolve(&Chord::plain("U")), one(ShortcutAction::Clear));
    assert_eq!(map.resolve(&Chord::plain("Q")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("E")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("Space")), one(ShortcutAction::Fit));
    assert_eq!(map.resolve(&Chord::new("E", true, false, false)), one(ShortcutAction::Transfer));
    assert_eq!(map.resolve(&Chord::plain("CloseBracket")), one(ShortcutAction::Guides));
    assert_eq!(map.resolve(&Chord::plain("B")), one(ShortcutAction::Bulk));

    assert!(map.preset("QWER"));
    assert_eq!(map, ShortcutMap::default());

    assert!(!map.preset("no-such-preset"));
    assert_eq!(map, ShortcutMap::default());
}

#[test]
fn missing_config_field_loads_the_default_map() {
    let cfg: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(cfg.shortcuts, ShortcutMap::default());
    assert_eq!(load(r#"{"shortcuts":{}}"#), ShortcutMap::default());

    let mut cfg = cfg;
    cfg.shortcuts.bind(ShortcutAction::Reject, Chord::plain("L")).unwrap();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(back.shortcuts, cfg.shortcuts);
    assert_eq!(back.shortcuts.resolve(&Chord::plain("L")), one(ShortcutAction::Reject));
    assert_eq!(back.shortcuts.resolve(&Chord::plain("E")), Resolve::None);
}

fn b(action: &str, key: &str, command: bool, shift: bool) -> String {
    format!(r#"{{"action":"{action}","chord":{{"key":"{key}","command":{command},"shift":{shift},"alt":false}}}}"#)
}

#[test]
fn a_saved_file_with_a_shared_key_keeps_the_later_binding() {
    // 예전 빌드가 남긴 파일: 채택에 W를 줬는데 보류도 W에 남아 있다(채택이 나중에 적힘).
    let json = format!(
        r#"{{"shortcuts":{{"bindings":[{},{},{},{}]}}}}"#,
        b("Hold", "W", false, false),
        b("Reject", "E", false, false),
        b("Pick", "W", false, false),
        b("Pick", "W", false, false),
    );
    let map = load(&json);
    assert_eq!(map.resolve(&Chord::plain("W")), one(ShortcutAction::Pick));
    assert_eq!(map.chords(ShortcutAction::Pick), vec![Chord::plain("W")], "같은 줄이 두 번이어도 한 번");
    assert!(map.chords(ShortcutAction::Hold).is_empty(), "보류의 기본 키 W는 채택이 써서 키 없음");
    // 파일에 없던 동작은 기본 키를 받는다(R·Space·]…).
    assert_eq!(map.resolve(&Chord::plain("R")), one(ShortcutAction::Clear));
    assert_eq!(map.resolve(&Chord::plain("CloseBracket")), one(ShortcutAction::Guides));
    // 다시 저장하고 읽어도 보류는 키 없음 그대로다(기본 키를 다시 채우지 않는다).
    let mut map = map;
    map.bind(ShortcutAction::Pick, Chord::plain("J")).unwrap();
    let cfg = Config { shortcuts: map, ..Config::default() };
    let back = load(&serde_json::to_string(&cfg).unwrap());
    assert!(back.chords(ShortcutAction::Hold).is_empty(), "W가 비었어도 키 없음은 저장된 대로");
    assert_eq!(back, cfg.shortcuts);
}

#[test]
fn a_saved_file_from_before_the_new_actions_gets_their_default_keys_when_free() {
    // ]·B·Ctrl+O·\가 맵 밖에 있던 빌드의 파일. 히스토그램을 ]로 바꿔 두었다.
    let json = format!(
        r#"{{"shortcuts":{{"bindings":[{},{},{}]}}}}"#,
        b("Pick", "Q", false, false),
        b("Histogram", "CloseBracket", false, false),
        b("Transfer", "E", true, false),
    );
    let map = load(&json);
    assert_eq!(map.resolve(&Chord::plain("CloseBracket")), one(ShortcutAction::Histogram), "사용자가 지정한 쪽이 우선");
    assert!(map.chords(ShortcutAction::Guides).is_empty(), "구도선 기본 키 ]는 이미 쓰여 키 없음");
    assert_eq!(map.resolve(&Chord::plain("B")), one(ShortcutAction::Bulk));
    assert_eq!(map.resolve(&Chord::new("O", true, false, false)), one(ShortcutAction::OpenFolder));
    assert_eq!(map.resolve(&Chord::plain("Backslash")), one(ShortcutAction::LookCompare));
    assert_eq!(map.resolve(&Chord::plain("H")), Resolve::None, "히스토그램의 원래 키 H는 비어 있다");
}

#[test]
fn a_saved_file_with_fixed_keys_drops_them() {
    let json = format!(
        r#"{{"shortcuts":{{"bindings":[{},{},{}]}}}}"#,
        b("Pick", "Escape", false, false),
        b("Hold", "Tab", false, false),
        b("Reject", "Minus", true, false),
    );
    let map = load(&json);
    // 고정 키를 버리면 키가 없으니 기본 키를 받는다.
    assert_eq!(map.chords(ShortcutAction::Pick), vec![Chord::plain("Q")]);
    assert_eq!(map.chords(ShortcutAction::Hold), vec![Chord::plain("W")]);
    assert_eq!(map.chords(ShortcutAction::Reject), vec![Chord::plain("E")]);
}

#[test]
fn binding_an_action_that_had_no_key_unbinds_the_owner_and_survives_reload() {
    let json = format!(
        r#"{{"shortcuts":{{"bindings":[{}]}}}}"#,
        b("Histogram", "CloseBracket", false, false),
    );
    let mut map = load(&json);
    assert!(map.chords(ShortcutAction::Guides).is_empty());
    // 키 없던 구도선에 A(AF 포인트)를 지정 → AF 포인트는 받을 키가 없어 키 없음.
    assert_eq!(map.bind(ShortcutAction::Guides, Chord::plain("A")), Ok(Some(ShortcutAction::Af)));
    assert_eq!(map.resolve(&Chord::plain("A")), one(ShortcutAction::Guides));
    assert!(map.chords(ShortcutAction::Af).is_empty());
    let cfg = Config { shortcuts: map, ..Config::default() };
    let back = load(&serde_json::to_string(&cfg).unwrap());
    assert!(back.chords(ShortcutAction::Af).is_empty());
    assert_eq!(back, cfg.shortcuts);
}
