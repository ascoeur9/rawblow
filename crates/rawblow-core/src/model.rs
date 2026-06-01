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
    /// 사람이 읽는 한국어 표기.
    pub fn ko(self) -> &'static str {
        match self {
            Label::Unrated => "미선택",
            Label::Pick => "선택",
            Label::Hold => "보류",
            Label::Reject => "제외",
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

/// 정렬 기준(F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Name,
    CaptureTime,
    Modified,
}

/// 파일번호 매칭 모드(F6, RawPull 계승).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Contains,
    Exact,
}
