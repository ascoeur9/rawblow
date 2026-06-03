//! 설정·단축키 영속화 (M5). OS 표준 설정 경로에 JSON으로 저장.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// UI 표시 언어(#30). `Config.lang`이 `None`이면 OS 언어를 따른다(앱에서 감지).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ko,
    En,
    Ja,
}

impl Lang {
    /// 로케일 코드("ko", "ko-KR", "ja_JP", "en-US"…)의 접두로 매핑. 모르면 None.
    pub fn from_locale(code: &str) -> Option<Lang> {
        let c = code.trim().to_ascii_lowercase();
        if c.starts_with("ko") {
            Some(Lang::Ko)
        } else if c.starts_with("ja") {
            Some(Lang::Ja)
        } else if c.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }
    /// 2글자 언어 코드.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }
    /// 설정 메뉴에 표시할 자기 언어 이름(각 언어 고유 표기).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ko => "한국어",
            Lang::En => "English",
            Lang::Ja => "日本語",
        }
    }
}

/// 단축키 맵(핸드오프 기본값). 값은 표시용 키 문자열.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeyMap {
    pub pick: String,
    pub hold: String,
    pub reject: String,
    pub clear: String,
    pub single_grid: String,
    pub fit_oneone: String,
    pub full_raw: String,
    pub exif: String,
    pub histogram: String,
    pub filter: String,
    pub transfer: String,
    pub jump: String,
    pub fullscreen: String,
}

impl Default for KeyMap {
    fn default() -> Self {
        KeyMap {
            pick: "Q".into(),
            hold: "W".into(),
            reject: "E".into(),
            clear: "R".into(),
            single_grid: "T".into(),
            fit_oneone: "Space".into(),
            full_raw: "D".into(),
            exif: "I".into(),
            histogram: "H".into(),
            filter: "F".into(),
            transfer: "Ctrl+E".into(),
            jump: "G".into(),
            fullscreen: "F11".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub auto_advance: bool,
    pub preload: i32,
    pub grid_cols: usize,
    pub recursive: bool,
    pub show_exif: bool,
    pub show_histogram: bool,
    pub last_folder: Option<String>,
    pub recent_folders: Vec<String>,
    pub keymap: KeyMap,
    /// 썸네일 디스크 캐시 상한(MB). 초과 시 오래된 것부터 자동 삭제. 0 = 무제한(#22).
    pub cache_limit_mb: u64,
    /// UI 언어(#30). `None` = OS 언어 자동. 설정에서 변경하면 저장된다.
    pub lang: Option<Lang>,
    /// 컬러 태그(#27) 색별 커스텀 이름 [Red, Yellow, Green, Blue, Purple]. 빈 문자열이면
    /// 기본 색 이름을 표시한다(예: Green="템플릿"). 설정에서 편집.
    #[serde(default)]
    pub tag_names: [String; 5],
}

impl Default for Config {
    fn default() -> Self {
        Config {
            auto_advance: true,
            preload: 3,
            grid_cols: 8,
            recursive: false,
            show_exif: true,
            show_histogram: true,
            last_folder: None,
            recent_folders: Vec::new(),
            keymap: KeyMap::default(),
            cache_limit_mb: 1024, // 기본 1GB. 0이면 무제한.
            lang: None,           // OS 언어 자동(#30).
            tag_names: Default::default(), // 5색 모두 빈 이름(기본 색 이름 표시)(#27).
        }
    }
}

impl Config {
    /// 컬러 태그의 표시 이름: 커스텀 이름이 있으면 그것을, 없으면 기본 색 이름(다국어).
    pub fn tag_label(&self, tag: crate::model::ColorTag, lang: Lang) -> String {
        match tag.index() {
            Some(i) if !self.tag_names[i].trim().is_empty() => self.tag_names[i].clone(),
            _ => tag.default_name(lang).to_string(),
        }
    }
}

impl Config {
    /// 최근 폴더 목록 맨 앞에 추가(중복 제거, 최대 12개).
    pub fn push_recent(&mut self, folder: &str) {
        self.recent_folders.retain(|f| f != folder);
        self.recent_folders.insert(0, folder.to_string());
        self.recent_folders.truncate(12);
        self.last_folder = Some(folder.to_string());
    }
}

/// OS 표준 설정 디렉토리(`…/rawblow`).
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("RawBlow");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library/Application Support/RawBlow");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("rawblow");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config/rawblow");
        }
    }
    PathBuf::from(".rawblow-config")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// OS 표준 **캐시** 디렉토리(`…/RawBlow/thumb-cache` 등). 썸네일 디스크 캐시(#22)에 쓴다.
/// 설정(config_dir)과 달리 OS가 정리해도 무방한 캐시 위치를 쓴다(Windows는 LOCALAPPDATA).
pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("RawBlow").join("thumb-cache");
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("RawBlow").join("thumb-cache");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Caches/RawBlow/thumb-cache");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Windows/macOS와 동일하게 thumb-cache 하위폴더로 통일(향후 캐시 종류 분리·정리 안전).
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("rawblow").join("thumb-cache");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".cache/rawblow/thumb-cache");
        }
    }
    std::env::temp_dir().join("rawblow").join("thumb-cache")
}

/// 설정을 로드(없으면 기본값).
pub fn load() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 설정을 저장.
pub fn save(config: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(config_path(), json)
}
