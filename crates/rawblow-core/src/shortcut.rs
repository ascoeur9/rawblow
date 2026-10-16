//! 단축키 맵(#123). 앱이 실행하는 키는 이 맵에 저장된 바인딩뿐이다.
//!
//! 같은 동작에 키가 둘이면(창맞춤 Space·Z, 다시 실행 Ctrl+Shift+Z·Ctrl+Y) 둘 다 그 동작이다.
//! 서로 다른 동작이 한 키를 쓰면 충돌이고, 그 키는 어느 쪽도 실행하지 않는다.
//! `bind`는 그 동작의 기존 키를 모두 새 키 하나로 바꾼다. 다른 동작의 키는 지우지 않는다.

use serde::{Deserialize, Serialize};

/// 사용자가 바꿀 수 있는 동작. 화살표·Esc·Enter·?·F1은 여기 없다.
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
    Map,
    Af,
    Filter,
    Fullscreen,
    Jump,
    Transfer,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 29] = [
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
        ShortcutAction::Map,
        ShortcutAction::Af,
        ShortcutAction::Filter,
        ShortcutAction::Fullscreen,
        ShortcutAction::Jump,
        ShortcutAction::Transfer,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub action: ShortcutAction,
    pub chord: Chord,
}

/// `resolve` 결과. 충돌이면 동작 목록은 선언 순서다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolve {
    None,
    One(ShortcutAction),
    Conflict(Vec<ShortcutAction>),
}

/// 저장된 바인딩. 비어 있는 JSON 객체는 기본(QWER)으로 채운다.
/// `bindings`가 빈 배열이면 그건 그대로 빈 맵이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutMap {
    pub bindings: Vec<Binding>,
}

impl Default for ShortcutMap {
    fn default() -> Self {
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
        put(ShortcutAction::Map, "M", false, false);
        put(ShortcutAction::Af, "A", false, false);
        put(ShortcutAction::Filter, "F", false, false);
        put(ShortcutAction::Fullscreen, "F11", false, false);
        put(ShortcutAction::Jump, "G", false, false);
        put(ShortcutAction::Transfer, "E", true, false);
        ShortcutMap { bindings }
    }
}

impl ShortcutMap {
    pub fn resolve(&self, chord: &Chord) -> Resolve {
        let mut hits: Vec<ShortcutAction> = self
            .bindings
            .iter()
            .filter(|b| &b.chord == chord)
            .map(|b| b.action)
            .collect();
        hits.sort();
        hits.dedup();
        match hits.len() {
            0 => Resolve::None,
            1 => Resolve::One(hits[0]),
            _ => Resolve::Conflict(hits),
        }
    }

    /// 이 동작에 묶인 키. 저장된 순서.
    pub fn chords(&self, action: ShortcutAction) -> Vec<Chord> {
        self.bindings.iter().filter(|b| b.action == action).map(|b| b.chord.clone()).collect()
    }

    /// 한 키에 동작이 둘 이상인 것만. 키를 처음 만난 순서.
    pub fn conflicts(&self) -> Vec<(Chord, Vec<ShortcutAction>)> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for b in &self.bindings {
            if seen.iter().any(|c| c == &b.chord) {
                continue;
            }
            seen.push(b.chord.clone());
            if let Resolve::Conflict(actions) = self.resolve(&b.chord) {
                out.push((b.chord.clone(), actions));
            }
        }
        out
    }

    /// 이 동작의 키를 모두 지우고 `chord` 하나만 남긴다. 다른 동작은 그대로라 겹치면 충돌이 된다.
    pub fn bind(&mut self, action: ShortcutAction, chord: Chord) {
        self.bindings.retain(|b| b.action != action);
        self.bindings.push(Binding { action, chord });
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
                self.bind(ShortcutAction::Pick, Chord::plain("J"));
                self.bind(ShortcutAction::Hold, Chord::plain("K"));
                self.bind(ShortcutAction::Reject, Chord::plain("L"));
                self.bind(ShortcutAction::Clear, Chord::plain("U"));
                true
            }
            _ => false,
        }
    }
}
