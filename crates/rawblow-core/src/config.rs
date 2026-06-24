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

/// CLIP-IQA 백본 종류(#50). 정확도/속도/크기 절충 — 사용자가 선택.
/// 속도는 CPU int8 추론 기준(M1 Max 워커풀 실측): ViT-B/32 ≫ ViT-L/14 > RN50.
/// (RN50은 다운로드는 작지만 CPU에서 int8 conv가 느려 처리량이 가장 낮다.)
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClipIqaBackbone {
    /// ResNet-50 (~39 MB int8). 다운로드는 가장 작으나 CPU 추론은 가장 느리다.
    RN50,
    /// ViT-B/32 (~89 MB int8). **권장 기본값 — 가장 빠르고 정확도도 충분**.
    #[default]
    ViTB32,
    /// ViT-L/14 (~307 MB int8). 최고 화질이지만 ViT-B/32보다 ~15배 느리다.
    ViTL14,
}

impl ClipIqaBackbone {
    /// 모델 파일명(`clip-iqa-<backbone>.int8.onnx`).
    pub fn filename(self) -> &'static str {
        match self {
            ClipIqaBackbone::RN50   => "clip-iqa-RN50.int8.onnx",
            ClipIqaBackbone::ViTB32 => "clip-iqa-ViT-B-32.int8.onnx",
            ClipIqaBackbone::ViTL14 => "clip-iqa-ViT-L-14.int8.onnx",
        }
    }

    /// GitHub Releases 다운로드 URL.
    pub fn download_url(self) -> &'static str {
        match self {
            ClipIqaBackbone::RN50   => "https://github.com/ascoeur9/rawblow/releases/download/models-v1/clip-iqa-RN50.int8.onnx",
            ClipIqaBackbone::ViTB32 => "https://github.com/ascoeur9/rawblow/releases/download/models-v1/clip-iqa-ViT-B-32.int8.onnx",
            ClipIqaBackbone::ViTL14 => "https://github.com/ascoeur9/rawblow/releases/download/models-v1/clip-iqa-ViT-L-14.int8.onnx",
        }
    }

    /// 예상 파일 크기(바이트, 표시용).
    pub fn expected_bytes(self) -> u64 {
        match self {
            ClipIqaBackbone::RN50   => 38_862_297,
            ClipIqaBackbone::ViTB32 => 88_935_710,
            ClipIqaBackbone::ViTL14 => 306_927_473,
        }
    }

    /// sha256(hex) — 다운로드 후 무결성 검증.
    pub fn sha256(self) -> &'static str {
        match self {
            ClipIqaBackbone::RN50   => "9ed26151a00618d10c912d0198bedff1b831a65b9dde164df72b47f6330c4886",
            ClipIqaBackbone::ViTB32 => "e23fcc1532944501ec7516f7421127e65fc43c63cb37f934e6706c0f38e4fd96",
            ClipIqaBackbone::ViTL14 => "020c8f33d52aa38e55c0a6bb9cf07af1f9444118ac403599b139c7bc11820dd2",
        }
    }

    /// 표시 이름(UI 칩용). 속도/화질 힌트 포함.
    pub fn label(self) -> &'static str {
        match self {
            ClipIqaBackbone::ViTB32 => "ViT-B/32 ⚡권장",
            ClipIqaBackbone::ViTL14 => "ViT-L/14 정밀·느림",
            ClipIqaBackbone::RN50   => "RN50 작은용량·느림",
        }
    }

    /// 표시 순서: 권장(ViT-B/32) 먼저.
    pub const ALL: [ClipIqaBackbone; 3] = [ClipIqaBackbone::ViTB32, ClipIqaBackbone::ViTL14, ClipIqaBackbone::RN50];

    /// CPU(int8) 모델 명세.
    pub fn spec(self) -> ModelSpec {
        ModelSpec { file: self.filename(), url: self.download_url(), sha256: self.sha256(), bytes: self.expected_bytes() }
    }
}

/// 다운로드·로드할 ONNX 모델 명세(파일명·URL·sha256·바이트).
#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// GPU 고속 모드 전용 모델: **RN50 fp32**. CNN이라 CoreML(Apple)/WebGPU(Windows)에서 그래프 분할
/// 없이 통째로 가속돼 압도적으로 빠르다(M1 Max 실측: CoreML 멀티세션+배치 400+ img/s vs RN50 CPU 9).
/// int8은 GPU에서 fallback하므로 GPU 경로는 fp32를 받는다(트랜스포머 ViT는 GPU에서 분할되어 느림).
pub const GPU_MODEL: ModelSpec = ModelSpec {
    file: "clip-iqa-RN50.onnx",
    url: "https://github.com/ascoeur9/rawblow/releases/download/models-v1/clip-iqa-RN50.onnx",
    sha256: "29cce2e00f7c5c6afc5eef953c2b27d0565f23ee4a7c165f3d75a30b31cb6eaa",
    bytes: 153_218_127,
};

/// AI 컬링 결과를 어느 분류축에 배정할지(#50). 라벨·별점·태그는 서로 독립이므로
/// 사용자가 수동으로 쓰는 축을 덮어쓰지 않도록 하나만 고른다.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiCullTarget {
    /// 좋음 → Pick, 탈락 → Reject.
    Label,
    /// 좋음 → `good_stars`★, 탈락 → `bad_stars`★.
    Stars,
    /// 좋음 → `good_tag`, 탈락 → `bad_tag`.
    Tag,
}

/// AI 컬링(#50) 설정. 어떤 신호로 거를지 + 임계값 + 결과 배정 방식. 영속화된다.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AiCullConfig {
    pub use_focus: bool,
    pub use_exposure: bool,
    pub use_tilt: bool,
    /// CLIP-IQA 미적 점수 사용 여부. 모델 미다운로드 시 채점에서 자동 건너뜀.
    pub use_aesthetic: bool,
    /// 초점을 AF 측거점 영역에서만 측정(합초점 기준). off면 전체 프레임.
    pub use_af_focus: bool,
    pub focus_thresh: f32,
    pub exposure_min: f32,
    pub tilt_max_deg: f32,
    /// P(good) 하한(0..1). 미만이면 탈락. 기본 0.4.
    pub aesthetic_min: f32,
    /// 사용할 CLIP-IQA 백본(CPU 모드). 기본 ViT-B/32.
    pub backbone: ClipIqaBackbone,
    /// GPU 고속 모드. on이면 RN50 fp32를 CoreML(mac)/WebGPU(win)로 배치·병렬 추론(400+ img/s).
    /// CNN이라 GPU에서 분할 없이 가속됨. off면 CPU(int8 backbone). 미적 점수 사용 시에만 의미.
    pub use_gpu: bool,
    pub target: AiCullTarget,
    pub good_stars: u8,
    pub bad_stars: u8,
    pub good_tag: crate::model::ColorTag,
    pub bad_tag: crate::model::ColorTag,
    /// 미적 점수 상위 N장만 Good으로 선택(0=비활성, threshold 모드 사용).
    /// use_aesthetic=true일 때만 동작.
    pub top_n: usize,
    /// true=전체 항목, false=현재 필터를 통과한 항목만 채점.
    pub scope_all: bool,
}

impl Default for AiCullConfig {
    fn default() -> Self {
        let d = crate::quality::CullCriteria::default();
        AiCullConfig {
            use_focus: d.use_focus,
            use_exposure: d.use_exposure,
            use_tilt: d.use_tilt,
            use_aesthetic: false,
            use_af_focus: true,
            focus_thresh: d.focus_thresh,
            exposure_min: d.exposure_min,
            tilt_max_deg: d.tilt_max_deg,
            aesthetic_min: 0.55,
            backbone: ClipIqaBackbone::default(),
            use_gpu: false,
            target: AiCullTarget::Label,
            good_stars: 5,
            bad_stars: 1,
            good_tag: crate::model::ColorTag::Teal,
            bad_tag: crate::model::ColorTag::Orange,
            top_n: 0,
            scope_all: true,
        }
    }
}

impl AiCullConfig {
    /// 코어 채점 기준으로 변환.
    pub fn criteria(&self) -> crate::quality::CullCriteria {
        crate::quality::CullCriteria {
            use_focus: self.use_focus,
            use_exposure: self.use_exposure,
            use_tilt: self.use_tilt,
            use_aesthetic: self.use_aesthetic,
            focus_thresh: self.focus_thresh,
            exposure_min: self.exposure_min,
            tilt_max_deg: self.tilt_max_deg,
            aesthetic_min: self.aesthetic_min,
        }
    }

    /// 켜진 검사가 하나도 없으면 채점 의미 없음.
    pub fn any_enabled(&self) -> bool {
        self.use_focus || self.use_exposure || self.use_tilt || self.use_aesthetic
    }

    /// 미적 점수에 쓸 모델 명세: GPU 모드면 RN50 fp32, 아니면 선택한 백본 int8.
    pub fn model_spec(&self) -> ModelSpec {
        if self.use_gpu {
            GPU_MODEL
        } else {
            self.backbone.spec()
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
    /// 사진 표시 화면(매팅) 배경색 [r,g,b](#36). `None`이면 앱 기본(near-black void).
    /// 설정에서 프리셋 또는 HEX/RGB로 변경.
    #[serde(default)]
    pub photo_bg: Option<[u8; 3]>,
    /// GPS 촬영 위치 미니 지도 표시(#38). `M`으로 토글, 저장. 기본 off.
    #[serde(default)]
    pub show_map: bool,
    /// AF 포인트 오버레이 표시(#37). `A`로 토글, 저장. 기본 off.
    #[serde(default)]
    pub show_af: bool,
    /// 스트립·그리드 셀의 표기(선택 표시·별점·색상 태그)를 크게 표시(#44). true=크게(기본), false=작게(이전 크기).
    /// 필드 레벨 `#[serde(default)]`를 **달지 않는다**: 컨테이너 `#[serde(default)]`가 누락 필드를
    /// `Config::default()`(=true)로 채우게 해야 한다. 필드 레벨을 달면 bool 기본값 false가 되어
    /// 기존 사용자가 '작게'로 켜진다.
    pub large_badges: bool,
    /// AI 컬링(#50) 설정. 누락 시 기본값.
    #[serde(default)]
    pub ai_cull: AiCullConfig,
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
            photo_bg: None,       // 앱 기본 배경(near-black void)(#36).
            show_map: false,      // GPS 미니 지도(#38). 기본 off.
            show_af: false,       // AF 포인트 오버레이(#37). 기본 off.
            large_badges: true,   // 스트립·그리드 표기 크게가 기본(#44).
            ai_cull: AiCullConfig::default(), // AI 컬링 기본 설정(#50).
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
        .map_err(std::io::Error::other)?;
    std::fs::write(config_path(), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_spec_gpu_uses_rn50_fp32() {
        let mut c = AiCullConfig { use_gpu: true, ..Default::default() };
        // GPU 모드는 백본 선택과 무관하게 RN50 fp32(GPU_MODEL)를 쓴다(int8은 GPU fallback).
        c.backbone = ClipIqaBackbone::ViTL14;
        let s = c.model_spec();
        assert_eq!(s.file, "clip-iqa-RN50.onnx");
        assert!(!s.file.contains("int8"), "GPU는 fp32여야 함");
        assert_eq!(s.sha256, GPU_MODEL.sha256);
    }

    #[test]
    fn model_spec_cpu_uses_backbone_int8() {
        let c = AiCullConfig { use_gpu: false, backbone: ClipIqaBackbone::ViTB32, ..Default::default() };
        let s = c.model_spec();
        assert_eq!(s.file, "clip-iqa-ViT-B-32.int8.onnx");
        assert_eq!(s.file, ClipIqaBackbone::ViTB32.spec().file);
    }

    #[test]
    fn backbone_specs_are_consistent() {
        // filename/url/sha/bytes가 spec()로 일관되게 묶이는지.
        for b in ClipIqaBackbone::ALL {
            let s = b.spec();
            assert_eq!(s.file, b.filename());
            assert_eq!(s.url, b.download_url());
            assert!(s.url.ends_with(s.file), "URL이 파일명으로 끝나야 함: {}", s.url);
            assert_eq!(s.sha256.len(), 64, "sha256 hex 64자");
            assert!(s.bytes > 0);
        }
    }

    #[test]
    fn default_backbone_is_recommended_vitb32() {
        assert_eq!(ClipIqaBackbone::default(), ClipIqaBackbone::ViTB32);
        assert_eq!(ClipIqaBackbone::ALL[0], ClipIqaBackbone::ViTB32); // 표시 순서 권장 먼저
        assert!(!AiCullConfig::default().use_gpu); // 기본 CPU
    }
}
