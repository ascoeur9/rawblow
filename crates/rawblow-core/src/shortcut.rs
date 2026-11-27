//! 단축키 맵(#123). 앱이 키로 실행하는 동작은 이 맵에 저장된 바인딩이거나, 바꿀 수 없는 고정 키
//! ([`Chord::is_fixed`])뿐이다.
//!
//! 한 키는 언제나 동작 하나에만 묶인다. 같은 동작에 키가 둘인 것은 괜찮다
//! (창맞춤 Space·Z, 다시 실행 Ctrl+Shift+Z·Ctrl+Y).
//! - [`ShortcutMap::bind`]는 그 동작의 키를 새 키 하나로 바꾼다. 새 키를 다른 동작이 쓰고 있었으면
//!   그 동작은 그 키를 잃는다. 그래서 키가 하나도 안 남으면, 바꾼 동작이 직전까지 쓰던 첫 키를
//!   받는다(맞바꿈). 바꾼 동작에 키가 없었으면 그 동작은 키 없음이 된다.
//! - 저장된 파일을 읽을 때([`ShortcutMap`]의 `Deserialize`) 한 키에 동작이 둘이면 파일에서 나중에 적힌
//!   쪽(가장 최근에 지정한 쪽)이 키를 갖는다. 고정 키 바인딩은 버린다. 그 뒤 키가 없는 동작은
//!   「키 없음」으로 저장된 것이 아니면 기본 키 중 비어 있는 것을 받는다(새로 생긴 동작 포함).

use serde::{Deserialize, Serialize};

/// 사용자가 바꿀 수 있는 동작. 화살표·Esc·Enter·?·F1 등은 [`Chord::is_fixed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShortcutAction {
    Pick,
    Hold,
    Reject,
    Clear,
    Undo,
    Redo,
    Star1,
    Star2,
    Star3,
    Star4,
    Star5,
    StarClear,
    Tag1,
    Tag2,
    Tag3,
    Tag4,
    Tag5,
    TagClear,
    ToggleView,
    Fit,
    FullRaw,
    Exif,
    Histogram,
    /// 누르고 있는 동안 룩 끄기(#125). 눌림이 아니라 매 프레임 「눌려 있는지」로 읽는다.
    LookCompare,
    Map,
    Af,
    /// 구도 보조선 순환(#90).
    Guides,
    Filter,
    Fullscreen,
    Jump,
    /// 일괄 분류. 그리드 보기에서만 열린다.
    Bulk,
    Transfer,
    OpenFolder,
}

impl ShortcutAction {
    /// 설정 목록 순서.
    pub const ALL: [ShortcutAction; 33] = [
        ShortcutAction::Pick,
        ShortcutAction::Hold,
        ShortcutAction::Reject,
        ShortcutAction::Clear,
        ShortcutAction::Undo,
        ShortcutAction::Redo,
        ShortcutAction::Star1,
        ShortcutAction::Star2,
        ShortcutAction::Star3,
        ShortcutAction::Star4,
        ShortcutAction::Star5,
        ShortcutAction::StarClear,
        ShortcutAction::Tag1,
        ShortcutAction::Tag2,
        ShortcutAction::Tag3,
        ShortcutAction::Tag4,
        ShortcutAction::Tag5,
        ShortcutAction::TagClear,
        ShortcutAction::ToggleView,
        ShortcutAction::Fit,
        ShortcutAction::FullRaw,
        ShortcutAction::Exif,
        ShortcutAction::Histogram,
        ShortcutAction::LookCompare,
        ShortcutAction::Map,
        ShortcutAction::Af,
        ShortcutAction::Guides,
        ShortcutAction::Filter,
        ShortcutAction::Fullscreen,
        ShortcutAction::Jump,
        ShortcutAction::Bulk,
        ShortcutAction::Transfer,
        ShortcutAction::OpenFolder,
    ];
}

/// 키 하나. `key`는 egui `Key`의 Debug 이름(`Q`, `Num1`, `Space`, `F11`, `Backtick`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    pub key: String,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub fn new(key: impl Into<String>, command: bool, shift: bool, alt: bool) -> Self {
        Chord { key: key.into(), command, shift, alt }
    }

    pub fn plain(key: impl Into<String>) -> Self {
        Self::new(key, false, false, false)
    }

    /// 바꿀 수 없는 키. 맵에 넣지 않고, 설정에서 지정하려 하면 거절한다.
    /// - 화살표(넘김)·Esc(전체화면 해제·닫기)·Enter(전송)·F1·?·⇧/(치트시트): 수식키와 상관없이.
    /// - Tab: egui가 포커스 이동에 쓴다.
    /// - Ctrl/⌘ + `+`·`=`·`-`·`0`: egui가 화면 배율에 쓴다(⇧·Alt를 더 눌러도).
    /// - 복사·잘라내기·붙여넣기(Ctrl/⌘+C·X·V, 윈도우 Ctrl+Insert·⇧Insert·⇧Delete, 전용 키):
    ///   egui-winit이 키 대신 Copy·Cut·Paste 이벤트로 바꿔 보내 단축키로는 오지 않는다.
    pub fn is_fixed(&self) -> bool {
        match self.key.as_str() {
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Escape" | "Enter" | "F1"
            | "Questionmark" | "Tab" | "Copy" | "Cut" | "Paste" => true,
            "Slash" | "Delete" => self.shift,
            "Plus" | "Equals" | "Minus" | "Num0" | "C" | "X" | "V" => self.command,
            "Insert" => self.command || self.shift,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub action: ShortcutAction,
    pub chord: Chord,
}

/// `resolve` 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolve {
    None,
    One(ShortcutAction),
}

/// 고정 키([`Chord::is_fixed`])라 [`ShortcutMap::bind`]가 아무것도 바꾸지 않았다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedKey;

/// 저장된 바인딩. 한 키에 동작이 둘인 맵은 만들 수 없다(필드를 감추고 모든 변경이 아래 함수를 거친다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredMap")]
pub struct ShortcutMap {
    bindings: Vec<Binding>,
    /// 키 없음인 동작. 읽을 때 기본 키를 다시 채우지 않게 따로 적어 둔다.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unbound: Vec<ShortcutAction>,
}

/// 파일에 적힌 그대로. 비어 있는 JSON 객체는 기본(QWER)이다.
#[derive(Deserialize)]
#[serde(default)]
struct StoredMap {
    bindings: Vec<Binding>,
    unbound: Vec<ShortcutAction>,
}

impl Default for StoredMap {
    fn default() -> Self {
        StoredMap { bindings: default_bindings(), unbound: Vec::new() }
    }
}

impl From<StoredMap> for ShortcutMap {
    fn from(stored: StoredMap) -> Self {
        ShortcutMap::normalized(stored.bindings, stored.unbound)
    }
}

fn default_bindings() -> Vec<Binding> {
    let mut bindings = Vec::new();
    let mut put = |action: ShortcutAction, key: &str, command: bool, shift: bool| {
        bindings.push(Binding {
            action,
            chord: Chord::new(key, command, shift, false),
        });
    };
    put(ShortcutAction::Pick, "Q", false, false);
    put(ShortcutAction::Hold, "W", false, false);
    put(ShortcutAction::Reject, "E", false, false);
    put(ShortcutAction::Clear, "R", false, false);
    put(ShortcutAction::Undo, "Z", true, false);
    put(ShortcutAction::Redo, "Z", true, true);
    put(ShortcutAction::Redo, "Y", true, false);
    put(ShortcutAction::Star1, "Num1", false, false);
    put(ShortcutAction::Star2, "Num2", false, false);
    put(ShortcutAction::Star3, "Num3", false, false);
    put(ShortcutAction::Star4, "Num4", false, false);
    put(ShortcutAction::Star5, "Num5", false, false);
    put(ShortcutAction::StarClear, "Backtick", false, false);
    put(ShortcutAction::Tag1, "Num1", false, true);
    put(ShortcutAction::Tag2, "Num2", false, true);
    put(ShortcutAction::Tag3, "Num3", false, true);
    put(ShortcutAction::Tag4, "Num4", false, true);
    put(ShortcutAction::Tag5, "Num5", false, true);
    put(ShortcutAction::TagClear, "Num0", false, true);
    put(ShortcutAction::ToggleView, "T", false, false);
    put(ShortcutAction::Fit, "Space", false, false);
    put(ShortcutAction::Fit, "Z", false, false);
    put(ShortcutAction::FullRaw, "D", false, false);
    put(ShortcutAction::Exif, "I", false, false);
    put(ShortcutAction::Histogram, "H", false, false);
    put(ShortcutAction::LookCompare, "Backslash", false, false);
    put(ShortcutAction::Map, "M", false, false);
    put(ShortcutAction::Af, "A", false, false);
    put(ShortcutAction::Guides, "CloseBracket", false, false);
    put(ShortcutAction::Filter, "F", false, false);
    put(ShortcutAction::Fullscreen, "F11", false, false);
    put(ShortcutAction::Jump, "G", false, false);
    put(ShortcutAction::Bulk, "B", false, false);
    put(ShortcutAction::Transfer, "E", true, false);
    put(ShortcutAction::OpenFolder, "O", true, false);
    bindings
}

impl Default for ShortcutMap {
    fn default() -> Self {
        ShortcutMap { bindings: default_bindings(), unbound: Vec::new() }
    }
}

impl ShortcutMap {
    /// 파일에서 읽은 바인딩을 한 키에 동작 하나인 맵으로 정리한다.
    fn normalized(stored: Vec<Binding>, unbound: Vec<ShortcutAction>) -> Self {
        // 뒤에서부터 보며 키를 처음 만난 바인딩만 남긴다 — 나중에 지정한 쪽이 이긴다.
        let mut kept: Vec<Binding> = Vec::new();
        for b in stored.into_iter().rev() {
            if b.chord.is_fixed() || kept.iter().any(|k| k.chord == b.chord) {
                continue;
            }
            kept.push(b);
        }
        kept.reverse();
        let mut map = ShortcutMap { bindings: kept, unbound: Vec::new() };
        for action in ShortcutAction::ALL {
            if map.has_keys(action) {
                continue;
            }
            if !unbound.contains(&action) {
                for b in default_bindings().into_iter().filter(|b| b.action == action) {
                    if map.resolve(&b.chord) == Resolve::None {
                        map.bindings.push(b);
                    }
                }
            }
            if !map.has_keys(action) {
                map.unbound.push(action);
            }
        }
        map
    }

    pub fn resolve(&self, chord: &Chord) -> Resolve {
        match self.bindings.iter().find(|b| &b.chord == chord) {
            Some(b) => Resolve::One(b.action),
            None => Resolve::None,
        }
    }

    /// 이 동작에 묶인 키. 저장된 순서(첫 키가 툴바·안내에 찍히는 키).
    pub fn chords(&self, action: ShortcutAction) -> Vec<Chord> {
        self.bindings.iter().filter(|b| b.action == action).map(|b| b.chord.clone()).collect()
    }

    fn has_keys(&self, action: ShortcutAction) -> bool {
        self.bindings.iter().any(|b| b.action == action)
    }

    /// 이 동작의 키를 모두 지우고 `chord` 하나만 남긴다. 고정 키면 아무것도 바꾸지 않는다.
    ///
    /// `chord`를 다른 동작이 쓰고 있었으면 그 동작을 돌려준다(지금 키는 [`Self::chords`]).
    /// 그 동작은 `chord`를 잃고, 남은 키가 없으면 `action`이 직전까지 쓰던 첫 키를 받는다.
    /// `action`에 키가 없었으면 그 동작은 키 없음이 된다.
    pub fn bind(&mut self, action: ShortcutAction, chord: Chord) -> Result<Option<ShortcutAction>, FixedKey> {
        if chord.is_fixed() {
            return Err(FixedKey);
        }
        let previous = self.chords(action).into_iter().next();
        let owner = match self.resolve(&chord) {
            Resolve::One(other) if other != action => Some(other),
            _ => None,
        };
        self.bindings.retain(|b| b.action != action && b.chord != chord);
        self.bindings.push(Binding { action, chord });
        self.unbound.retain(|a| *a != action);
        if let Some(other) = owner {
            if !self.has_keys(other) {
                match previous {
                    Some(chord) => self.bindings.push(Binding { action: other, chord }),
                    None => {
                        // 읽을 때와 같은 순서(선언 순)로 둔다 — 저장·읽기 뒤에도 같은 맵.
                        self.unbound.push(other);
                        self.unbound.sort();
                    }
                }
            }
        }
        Ok(owner)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `qwer`는 기본. `jkl`은 채택 J, 보류 K, 제외 L, 해제 U이고 나머지는 기본과 같다.
    /// 모르는 이름이면 맵을 바꾸지 않고 false.
    pub fn preset(&mut self, name: &str) -> bool {
        match name.trim().to_ascii_lowercase().as_str() {
            "qwer" => {
                self.reset();
                true
            }
            "jkl" => {
                self.reset();
                // J·K·L·U는 기본 맵에서 비어 있어 아무것도 밀려나지 않는다(테스트가 본다).
                for (action, key) in [
                    (ShortcutAction::Pick, "J"),
                    (ShortcutAction::Hold, "K"),
                    (ShortcutAction::Reject, "L"),
                    (ShortcutAction::Clear, "U"),
                ] {
                    let _ = self.bind(action, Chord::plain(key));
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 한 키에 동작이 둘인 키. 비어 있어야 한다.
    fn duplicated(map: &ShortcutMap) -> Vec<Chord> {
        let mut seen: Vec<&Chord> = Vec::new();
        let mut dup = Vec::new();
        for b in &map.bindings {
            if seen.contains(&&b.chord) {
                dup.push(b.chord.clone());
            }
            seen.push(&b.chord);
        }
        dup
    }

    fn keys(map: &ShortcutMap, action: ShortcutAction) -> Vec<Chord> {
        map.chords(action)
    }

    #[test]
    fn swap_gives_the_previous_key_to_the_action_that_lost_its_key() {
        let mut map = ShortcutMap::default();
        assert_eq!(map.bind(ShortcutAction::Pick, Chord::plain("W")), Ok(Some(ShortcutAction::Hold)));
        assert_eq!(keys(&map, ShortcutAction::Pick), vec![Chord::plain("W")]);
        assert_eq!(keys(&map, ShortcutAction::Hold), vec![Chord::plain("Q")]);
        assert!(duplicated(&map).is_empty());
    }

    #[test]
    fn an_action_that_keeps_another_key_only_loses_the_taken_one() {
        let mut map = ShortcutMap::default();
        // 창맞춤은 Space·Z. Z를 가져가면 Space만 남고, 채택이 쓰던 Q는 비게 된다.
        assert_eq!(map.bind(ShortcutAction::Pick, Chord::plain("Z")), Ok(Some(ShortcutAction::Fit)));
        assert_eq!(keys(&map, ShortcutAction::Fit), vec![Chord::plain("Space")]);
        assert_eq!(map.resolve(&Chord::plain("Q")), Resolve::None);
        assert!(duplicated(&map).is_empty());
    }

    #[test]
    fn an_action_with_two_keys_hands_over_its_first_key() {
        let mut map = ShortcutMap::default();
        assert_eq!(map.bind(ShortcutAction::Fit, Chord::plain("Q")), Ok(Some(ShortcutAction::Pick)));
        assert_eq!(keys(&map, ShortcutAction::Fit), vec![Chord::plain("Q")]);
        assert_eq!(keys(&map, ShortcutAction::Pick), vec![Chord::plain("Space")]);
        assert_eq!(map.resolve(&Chord::plain("Z")), Resolve::None, "창맞춤의 둘째 키는 풀린다");
    }

    #[test]
    fn binding_an_unbound_action_to_a_used_key_unbinds_the_owner() {
        let mut map = ShortcutMap::normalized(
            default_bindings().into_iter().filter(|b| b.action != ShortcutAction::Guides).collect(),
            vec![ShortcutAction::Guides],
        );
        assert!(keys(&map, ShortcutAction::Guides).is_empty());
        assert_eq!(map.bind(ShortcutAction::Guides, Chord::plain("H")), Ok(Some(ShortcutAction::Histogram)));
        assert!(keys(&map, ShortcutAction::Histogram).is_empty());
        assert_eq!(map.unbound, vec![ShortcutAction::Histogram]);
        // 빈 키로 지정하면 아무것도 밀려나지 않는다.
        assert_eq!(map.bind(ShortcutAction::Histogram, Chord::plain("CloseBracket")), Ok(None));
        assert!(map.unbound.is_empty());
        assert!(duplicated(&map).is_empty());
    }

    #[test]
    fn rebinding_to_its_own_key_changes_nobody_else() {
        let mut map = ShortcutMap::default();
        assert_eq!(map.bind(ShortcutAction::Fit, Chord::plain("Z")), Ok(None));
        assert_eq!(keys(&map, ShortcutAction::Fit), vec![Chord::plain("Z")]);
        assert_eq!(map.resolve(&Chord::plain("Space")), Resolve::None);
    }

    #[test]
    fn fixed_keys_are_refused_and_change_nothing() {
        let mut map = ShortcutMap::default();
        for chord in [
            Chord::plain("Escape"),
            Chord::plain("Enter"),
            Chord::plain("ArrowLeft"),
            Chord::new("ArrowUp", false, true, false),
            Chord::plain("F1"),
            Chord::plain("Questionmark"),
            Chord::new("Slash", false, true, false),
            Chord::plain("Tab"),
            Chord::new("Tab", false, true, false),
            Chord::new("Minus", true, false, false),
            Chord::new("Equals", true, false, false),
            Chord::new("Plus", true, true, false),
            Chord::new("Num0", true, false, false),
            Chord::new("C", true, false, false),
            Chord::new("X", true, true, false),
            Chord::new("V", true, false, false),
            Chord::new("Insert", true, false, false),
            Chord::new("Delete", false, true, false),
        ] {
            assert_eq!(map.bind(ShortcutAction::Pick, chord.clone()), Err(FixedKey), "{chord:?}");
        }
        assert_eq!(map, ShortcutMap::default());
        // 수식키 없는 -, =, 0, /, C, V, Delete는 바꿀 수 있다.
        for key in ["Minus", "Equals", "Num0", "Slash", "C", "V", "Delete"] {
            assert!(!Chord::plain(key).is_fixed(), "{key}");
        }
    }

    #[test]
    fn presets_have_no_duplicate_and_keep_the_new_actions() {
        let mut map = ShortcutMap::default();
        for name in ["qwer", "jkl"] {
            assert!(map.preset(name));
            assert!(duplicated(&map).is_empty(), "{name}");
            assert!(map.unbound.is_empty(), "{name}");
            for action in ShortcutAction::ALL {
                assert!(!keys(&map, action).is_empty(), "{name}: {action:?}에 키가 없다");
                for c in keys(&map, action) {
                    assert!(!c.is_fixed(), "{name}: {action:?} {c:?}");
                }
            }
            assert_eq!(keys(&map, ShortcutAction::Guides), vec![Chord::plain("CloseBracket")]);
            assert_eq!(keys(&map, ShortcutAction::Bulk), vec![Chord::plain("B")]);
            assert_eq!(keys(&map, ShortcutAction::OpenFolder), vec![Chord::new("O", true, false, false)]);
            assert_eq!(keys(&map, ShortcutAction::LookCompare), vec![Chord::plain("Backslash")]);
        }
    }

    #[test]
    fn any_sequence_of_binds_keeps_one_action_per_key() {
        let chords = [
            Chord::plain("Q"),
            Chord::plain("W"),
            Chord::plain("Space"),
            Chord::plain("Z"),
            Chord::new("Z", true, false, false),
            Chord::plain("CloseBracket"),
            Chord::plain("B"),
            Chord::plain("X"),
        ];
        let mut map = ShortcutMap::default();
        let mut step = 7usize;
        for _ in 0..400 {
            step = step.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 1_000_003;
            let action = ShortcutAction::ALL[step % ShortcutAction::ALL.len()];
            let chord = chords[(step / 7) % chords.len()].clone();
            map.bind(action, chord).unwrap();
            assert!(duplicated(&map).is_empty());
            for a in ShortcutAction::ALL {
                assert_eq!(keys(&map, a).is_empty(), map.unbound.contains(&a), "{a:?}");
            }
        }
    }
}
