//! 핵심 도메인 타입: 지원 확장자, 분류 라벨, 논리 항목(Entry), 보기/필터/정렬 enum.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 지원 RAW 확장자(소문자). RawPull과 동일 집합에서 시작.
pub const RAW_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "nef", "orf", "rw2", "raf", "dng", "pef", "srw", "raw",
];

/// 지원 이미지(비RAW) 확장자(소문자). PRD F3 확장 목록.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "tif", "tiff",
];

/// 표시 우선순위가 가장 높은 이미지 확장자(즉시 디코딩 가능·정확 색).
const PREFERRED_IMAGE: &[&str] = &["jpg", "jpeg", "heic", "heif", "png", "tif", "tiff", "webp"];

/// 확장자를 소문자 문자열로 얻는다(점 제외).
pub fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub fn is_raw_ext(ext: &str) -> bool {
    RAW_EXTENSIONS.contains(&ext)
}

pub fn is_image_ext(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext)
}

/// 지원 파일(스캔 대상)인지.
pub fn is_supported(path: &Path) -> bool {
    match ext_lower(path) {
        Some(e) => is_raw_ext(&e) || is_image_ext(&e),
        None => false,
    }
}

/// 파일 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Image,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let e = ext_lower(path)?;
    if is_raw_ext(&e) {
        Some(Kind::Raw)
    } else if is_image_ext(&e) {
        Some(Kind::Image)
    } else {
        None
    }
}

/// 4단계 분류 라벨. 직렬화 값은 사이드카 스펙(§8)의 pick/hold/reject/unrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    #[default]
    Unrated,
    Pick,
    Hold,
    Reject,
}

impl Label {
    /// 사람이 읽는 한국어 표기. (사이드카 TXT 등 언어 무관 데이터에 사용 — 안정적으로 한국어 유지.)
    pub fn ko(self) -> &'static str {
        match self {
            Label::Unrated => "미선택",
            Label::Pick => "선택",
            Label::Hold => "보류",
            Label::Reject => "제외",
        }
    }

    /// UI 표시용 다국어 라벨명(#30).
    pub fn name(self, lang: crate::config::Lang) -> &'static str {
        use crate::config::Lang;
        match lang {
            Lang::Ko => self.ko(),
            Lang::En => match self {
                Label::Unrated => "Unrated",
                Label::Pick => "Pick",
                Label::Hold => "Hold",
                Label::Reject => "Reject",
            },
            Lang::Ja => match self {
                Label::Unrated => "未選択",
                Label::Pick => "選択",
                Label::Hold => "保留",
                Label::Reject => "除外",
            },
        }
    }

    /// 디자인 핸드오프 토큰의 라벨 팔레트 RGB(pick/hold/rej/unr).
    pub fn color_rgb(self) -> [u8; 3] {
        match self {
            Label::Pick => [0x4a, 0xde, 0x80],
            Label::Hold => [0xfb, 0xbf, 0x24],
            Label::Reject => [0xf8, 0x71, 0x71],
            Label::Unrated => [0x6b, 0x72, 0x80],
        }
    }
}

/// 컬러 태그(#27): 라벨·별점과 **독립**된 3번째 분류축. 단일선택(한 항목에 한 색).
/// 보정 방식 등 사용자가 의미를 부여(설정에서 색별 이름 지정 가능 — `Config.tag_names`).
/// 색은 라벨 팔레트(pick=초록 / hold=황색 / reject=빨강 / unrated=회색)와 **겹치지 않게**
/// 고른 5색(주황·분홍·청록·파랑·보라). 라벨 점과 태그 점이 한눈에 구분된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorTag {
    #[default]
    None,
    Orange,
    Pink,
    Teal,
    Blue,
    Purple,
}

impl ColorTag {
    /// 부여 가능한 5색(레일·필터·전송 UI 순회용, ⇧1~5 순서). `None`(무태그)은 제외.
    pub const ALL: [ColorTag; 5] = [
        ColorTag::Orange,
        ColorTag::Pink,
        ColorTag::Teal,
        ColorTag::Blue,
        ColorTag::Purple,
    ];

    /// 점/칩 색(무태그는 None). 라벨 팔레트(초록·황색·빨강·회색)와 겹치지 않는 5색.
    pub fn color_rgb(self) -> Option<[u8; 3]> {
        Some(match self {
            ColorTag::None => return None,
            ColorTag::Orange => [0xfb, 0x92, 0x3c],
            ColorTag::Pink => [0xf4, 0x72, 0xb6],
            ColorTag::Teal => [0x2d, 0xd4, 0xbf],
            ColorTag::Blue => [0x60, 0xa5, 0xfa],
            ColorTag::Purple => [0xc0, 0x84, 0xfc],
        })
    }

    /// 0-기반 색 인덱스(Orange=0 … Purple=4). 무태그는 None. `Config.tag_names` 인덱싱에 사용.
    pub fn index(self) -> Option<usize> {
        Self::ALL.iter().position(|&t| t == self)
    }

    /// Shift+숫자(1~5) → 색 태그. 범위 밖이면 None.
    pub fn from_number(n: u8) -> Option<ColorTag> {
        Self::ALL.get(n.checked_sub(1)? as usize).copied()
    }

    /// 커스텀 이름이 없을 때 표시할 기본 색 이름(다국어).
    pub fn default_name(self, lang: crate::config::Lang) -> &'static str {
        use crate::config::Lang;
        match (lang, self) {
            (_, ColorTag::None) => match lang {
                Lang::Ko => "무태그",
                Lang::En => "Untagged",
                Lang::Ja => "タグなし",
            },
            (Lang::Ko, ColorTag::Orange) => "주황",
            (Lang::Ko, ColorTag::Pink) => "분홍",
            (Lang::Ko, ColorTag::Teal) => "청록",
            (Lang::Ko, ColorTag::Blue) => "파랑",
            (Lang::Ko, ColorTag::Purple) => "보라",
            (Lang::En, ColorTag::Orange) => "Orange",
            (Lang::En, ColorTag::Pink) => "Pink",
            (Lang::En, ColorTag::Teal) => "Teal",
            (Lang::En, ColorTag::Blue) => "Blue",
            (Lang::En, ColorTag::Purple) => "Purple",
            (Lang::Ja, ColorTag::Orange) => "オレンジ",
            (Lang::Ja, ColorTag::Pink) => "ピンク",
            (Lang::Ja, ColorTag::Teal) => "ティール",
            (Lang::Ja, ColorTag::Blue) => "青",
            (Lang::Ja, ColorTag::Purple) => "紫",
        }
    }

    /// 전송 분기 폴더명·파일명 토큰용 안정적 영문 슬러그(언어 무관).
    pub fn slug(self) -> &'static str {
        match self {
            ColorTag::None => "untagged",
            ColorTag::Orange => "orange",
            ColorTag::Pink => "pink",
            ColorTag::Teal => "teal",
            ColorTag::Blue => "blue",
            ColorTag::Purple => "purple",
        }
    }
}

/// 논리 항목: 동일 stem을 공유하는 파일들의 묶음(F3 페어링).
#[derive(Debug, Clone)]
pub struct Entry {
    /// 확장자를 제외한 파일명(페어링 키). 대소문자는 원본 유지하되 매칭은 소문자.
    pub stem: String,
    /// 이 항목에 속한 모든 파일(RAW·이미지 동반).
    pub members: Vec<PathBuf>,
    /// 화면에 표시할 파일(이미지 우선, 없으면 RAW).
    pub display: PathBuf,
    pub has_raw: bool,
    pub has_image: bool,
    /// 현재 분류 라벨.
    pub label: Label,
    /// 별점(0=무별점, 1~5). 라벨(pick/hold/reject)과 **독립**으로 동시에 매겨진다(#23).
    pub stars: u8,
    /// 컬러 태그(#27). 라벨·별점과 독립된 3번째 축(보정 방식 등). 기본 `None`(무태그).
    pub tag: ColorTag,
}

impl Entry {
    /// members로부터 표시 대상과 has_raw/has_image를 계산해 Entry를 만든다.
    pub fn from_members(stem: String, mut members: Vec<PathBuf>) -> Self {
        // 안정적 결과를 위해 멤버를 경로로 정렬.
        members.sort();
        let has_raw = members
            .iter()
            .any(|p| kind_of(p) == Some(Kind::Raw));
        let has_image = members
            .iter()
            .any(|p| kind_of(p) == Some(Kind::Image));
        let display = pick_display(&members);
        Entry {
            stem,
            members,
            display,
            has_raw,
            has_image,
            label: Label::Unrated,
            stars: 0,
            tag: ColorTag::None,
        }
    }

    /// RAW 동반 배지(`RAW+`) 표기 여부: 표시 대상이 이미지인데 RAW도 있을 때.
    pub fn shows_raw_badge(&self) -> bool {
        self.has_raw && self.has_image
    }

    /// 종류로 필터링한 멤버 목록(전송 시 동반 범위 결정에 사용).
    pub fn members_of_kind(&self, kind: Kind) -> Vec<&PathBuf> {
        self.members
            .iter()
            .filter(|p| kind_of(p) == Some(kind))
            .collect()
    }
}

/// 표시 우선순위에 따라 대표 파일을 고른다(이미지 > RAW, 이미지 내 PREFERRED_IMAGE 순).
fn pick_display(members: &[PathBuf]) -> PathBuf {
    // 1) 선호 이미지 확장자 순서대로 탐색
    for pref in PREFERRED_IMAGE {
        if let Some(p) = members
            .iter()
            .find(|p| ext_lower(p).as_deref() == Some(*pref))
        {
            return p.clone();
        }
    }
    // 2) 임의 이미지
    if let Some(p) = members.iter().find(|p| kind_of(p) == Some(Kind::Image)) {
        return p.clone();
    }
    // 3) RAW(또는 첫 멤버)
    members
        .iter()
        .find(|p| kind_of(p) == Some(Kind::Raw))
        .cloned()
        .unwrap_or_else(|| members[0].clone())
}

/// 보기 모드(F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Single,
    Grid,
}

/// 분류 필터(F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Pick,
    Hold,
    Reject,
    Unrated,
}

impl Filter {
    pub fn accepts(self, label: Label) -> bool {
        match self {
            Filter::All => true,
            Filter::Pick => label == Label::Pick,
            Filter::Hold => label == Label::Hold,
            Filter::Reject => label == Label::Reject,
            Filter::Unrated => label == Label::Unrated,
        }
    }

    /// `F` 키 순환 순서.
    pub fn next(self) -> Filter {
        match self {
            Filter::All => Filter::Pick,
            Filter::Pick => Filter::Hold,
            Filter::Hold => Filter::Reject,
            Filter::Reject => Filter::Unrated,
            Filter::Unrated => Filter::All,
        }
    }

    pub fn ko(self) -> &'static str {
        match self {
            Filter::All => "전체",
            Filter::Pick => "선택",
            Filter::Hold => "보류",
            Filter::Reject => "제외",
            Filter::Unrated => "미선택",
        }
    }

    /// UI 표시용 다국어 필터명(#30).
    pub fn name(self, lang: crate::config::Lang) -> &'static str {
        use crate::config::Lang;
        match lang {
            Lang::Ko => self.ko(),
            Lang::En => match self {
                Filter::All => "All",
                Filter::Pick => "Pick",
                Filter::Hold => "Hold",
                Filter::Reject => "Reject",
                Filter::Unrated => "Unrated",
            },
            Lang::Ja => match self {
                Filter::All => "すべて",
                Filter::Pick => "選択",
                Filter::Hold => "保留",
                Filter::Reject => "除外",
                Filter::Unrated => "未選択",
            },
        }
    }
}

/// 별점 필터. 라벨 필터(`Filter`)와 **독립**으로 AND 결합한다(예: 선택 AND 정확히 ★3).
/// `Any`=별점 무시, `Exact(n)`=정확히 n점(0=미부여).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarFilter {
    Any,
    Exact(u8),
}

impl StarFilter {
    pub fn accepts(self, stars: u8) -> bool {
        match self {
            StarFilter::Any => true,
            StarFilter::Exact(n) => stars == n,
        }
    }
}

/// 컬러 태그 필터(#27). 라벨·별점 필터와 **독립**으로 AND 결합한다.
/// `Any`=태그 무시, `Only(t)`=정확히 그 태그(`Only(None)`=무태그만).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagFilter {
    Any,
    Only(ColorTag),
}

impl TagFilter {
    pub fn accepts(self, tag: ColorTag) -> bool {
        match self {
            TagFilter::Any => true,
            TagFilter::Only(t) => tag == t,
        }
    }
}

/// 정렬 기준(F1). 설정에 저장되므로 serde 파생(#56). 기본은 촬영시간순(이슈 #56 코멘트) —
/// 여러 카메라 혼용 시 파일명이 촬영 순서와 어긋나므로 컬링에는 시간순이 자연스럽다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortOrder {
    Name,
    #[default]
    CaptureTime,
    Modified,
}

/// 파일번호 매칭 모드(F6, RawPull 계승).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Contains,
    Exact,
}
