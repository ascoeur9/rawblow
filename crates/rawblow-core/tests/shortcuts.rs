//! #123 단축키 맵. 저장된 키가 resolve 결과이고, 리셋·프리셋·충돌을 그대로 돌려준다.

use rawblow_core::config::Config;
use rawblow_core::shortcut::{Chord, Resolve, ShortcutAction, ShortcutMap};

fn one(action: ShortcutAction) -> Resolve {
    Resolve::One(action)
}

#[test]
fn default_map_matches_current_keys() {
    let map = ShortcutMap::default();
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("W")), one(ShortcutAction::Hold));
    assert_eq!(map.resolve(&Chord::plain("E")), one(ShortcutAction::Reject));
    assert_eq!(map.resolve(&Chord::plain("R")), one(ShortcutAction::Clear));
    assert_eq!(map.resolve(&Chord::new("E", true, false, false)), one(ShortcutAction::Transfer));
    assert_eq!(map.resolve(&Chord::plain("Space")), one(ShortcutAction::Fit));
    assert_eq!(map.resolve(&Chord::plain("Z")), one(ShortcutAction::Fit));
    assert_eq!(map.resolve(&Chord::new("Z", true, false, false)), one(ShortcutAction::Undo));
    assert_eq!(map.resolve(&Chord::new("Z", true, true, false)), one(ShortcutAction::Redo));
    assert_eq!(map.resolve(&Chord::new("Y", true, false, false)), one(ShortcutAction::Redo));
    assert_eq!(map.resolve(&Chord::plain("Num1")), one(ShortcutAction::Star1));
    assert_eq!(map.resolve(&Chord::new("Num1", false, true, false)), one(ShortcutAction::Tag1));
    assert_eq!(map.resolve(&Chord::plain("Backtick")), one(ShortcutAction::StarClear));
    assert_eq!(map.resolve(&Chord::new("Num0", false, true, false)), one(ShortcutAction::TagClear));
    assert_eq!(map.resolve(&Chord::plain("J")), Resolve::None);
    assert_eq!(map.chords(ShortcutAction::Fit).len(), 2);
}

#[test]
fn bind_replaces_every_chord_of_that_action_and_reports_conflict() {
    let mut map = ShortcutMap::default();
    map.bind(ShortcutAction::Fit, Chord::plain("X"));
    assert_eq!(map.chords(ShortcutAction::Fit), vec![Chord::plain("X")]);
    assert_eq!(map.resolve(&Chord::plain("Space")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("Z")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("X")), one(ShortcutAction::Fit));

    map.bind(ShortcutAction::Pick, Chord::plain("W"));
    assert_eq!(map.resolve(&Chord::plain("Q")), Resolve::None);
    assert_eq!(
        map.resolve(&Chord::plain("W")),
        Resolve::Conflict(vec![ShortcutAction::Pick, ShortcutAction::Hold])
    );
    assert_eq!(map.conflicts().len(), 1);
    assert_eq!(map.chords(ShortcutAction::Hold), vec![Chord::plain("W")]);
}

#[test]
fn reset_and_presets_restore_the_stored_actions() {
    let mut map = ShortcutMap::default();
    map.bind(ShortcutAction::Pick, Chord::plain("W"));
    map.bind(ShortcutAction::Fit, Chord::plain("X"));
    map.reset();
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("W")), one(ShortcutAction::Hold));
    assert_eq!(map.chords(ShortcutAction::Fit).len(), 2);

    assert!(map.preset("jkl"));
    assert_eq!(map.resolve(&Chord::plain("J")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("K")), one(ShortcutAction::Hold));
    assert_eq!(map.resolve(&Chord::plain("L")), one(ShortcutAction::Reject));
    assert_eq!(map.resolve(&Chord::plain("U")), one(ShortcutAction::Clear));
    assert_eq!(map.resolve(&Chord::plain("Q")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("E")), Resolve::None);
    assert_eq!(map.resolve(&Chord::plain("Space")), one(ShortcutAction::Fit));
    assert_eq!(map.resolve(&Chord::new("E", true, false, false)), one(ShortcutAction::Transfer));

    assert!(map.preset("QWER"));
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
    assert_eq!(map.resolve(&Chord::plain("J")), Resolve::None);

    assert!(!map.preset("no-such-preset"));
    assert_eq!(map.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
}

#[test]
fn missing_config_field_loads_the_default_map() {
    let cfg: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(cfg.shortcuts.resolve(&Chord::plain("Q")), one(ShortcutAction::Pick));
    assert_eq!(cfg.shortcuts.resolve(&Chord::new("Num3", false, true, false)), one(ShortcutAction::Tag3));

    let mut cfg = cfg;
    cfg.shortcuts.bind(ShortcutAction::Reject, Chord::plain("L"));
    let json = serde_json::to_string(&cfg).unwrap();
    let back: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(back.shortcuts.resolve(&Chord::plain("L")), one(ShortcutAction::Reject));
    assert_eq!(back.shortcuts.resolve(&Chord::plain("E")), Resolve::None);
}
