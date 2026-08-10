//! 설정·단축키 영속화 (M5). OS 표준 설정 경로에 JSON으로 저장.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// 얼굴 검출 모델: **YuNet 2023mar**(OpenCV Zoo, 232KB). "얼굴 있는 컷"·"장르 인물/풍경"용.
/// 고정 입력 640×640 BGR, per-cell cls·obj로 얼굴 존재만 판정(정확한 박스 불필요, #51 후속).
pub const FACE_MODEL: ModelSpec = ModelSpec {
    file: "yunet.onnx",
    url: "https://github.com/ascoeur9/rawblow/releases/download/models-v1/yunet.onnx",
    sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    bytes: 232_589,
};

/// CLIP 다축 모델(ViT-B/32 int8): 프롬프트쌍 4개(quality·genre_portrait·sharp·compose)를 구워넣어
/// (N,4) P(positive) 출력. "AI 선명도"(sharp 축)에 사용(#51 후속). 입력 224² CLIP 정규화.
pub const CLIP_AXES_MODEL: ModelSpec = ModelSpec {
    file: "clip-axes-ViT-B-32.int8.onnx",
    url: "https://github.com/ascoeur9/rawblow/releases/download/models-v1/clip-axes-ViT-B-32.int8.onnx",
    sha256: "f0cf17921d2b0aaa2c489d209b61e02cb7cf235c066f38e49451c0d7f8b74606",
    bytes: 88_939_201,
};

/// 객체 검출 모델: **YOLOv10n**(onnx-community, 9.4MB, NMS-free). "객체 포함"(COCO 80클래스)용.
/// 출력 (1,300,6)=[x1,y1,x2,y2,conf,cls]. 입력 640² RGB(/255), #51 후속.
pub const OBJECT_MODEL: ModelSpec = ModelSpec {
    file: "yolov10n.onnx",
    url: "https://github.com/ascoeur9/rawblow/releases/download/models-v1/yolov10n.onnx",
    sha256: "a77dd863933f184a19e84361c64b788228a7c7dacc2c78939239a96ad3efca3b",
    bytes: 9_386_116,
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

    // ───── 메타데이터 필터(Tier1, 모델 불필요) ─────
    /// 방향 한정(세로/가로/전체).
    pub filter_orientation: OrientationFilter,
    pub use_iso_max: bool,
    /// ISO 상한(초과 컷 제외). 고감도 노이즈 거르기.
    pub iso_max: u32,
    pub use_focal_range: bool,
    pub focal_min_mm: f32,
    pub focal_max_mm: f32,
    pub use_aperture_max: bool,
    /// 조리개 f값 상한(이하만 통과 — 밝은 렌즈/얕은 심도 컷만).
    pub aperture_max: f32,
    pub use_shutter_min: bool,
    /// 셔터속도 하한(초). 미만(더 느림)이면 손떨림 후보로 제외.
    pub shutter_min_secs: f32,
    /// 카메라/렌즈 모델 부분일치(빈 문자열=무제한).
    pub camera_contains: String,
    pub lens_contains: String,

    // ───── 연사 베스트-N(Tier2, 모델 불필요) ─────
    pub use_burst: bool,
    /// 인접 컷 간격(초) 이하면 같은 연사로 묶음.
    pub burst_gap_secs: u32,
    /// 연사 그룹마다 점수 상위 N장만 Good.
    pub burst_keep: u32,

    // ───── 시각적 근접중복(Tier3a, 모델 불필요) ─────
    pub use_dedup: bool,
    /// dHash 해밍거리 이하면 유사컷으로 클러스터.
    pub dedup_hamming: u32,
    /// 유사 클러스터마다 상위 N장만 Good.
    pub dedup_keep: u32,

    // ───── CLIP 의미 축(Tier3b, 임베딩 모델 필요) ─────
    /// 장르 픽: 인물(genre_portrait=true) 또는 풍경 위주.
    pub use_genre: bool,
    pub genre_portrait: bool,
    /// AI 선명도(CLIP) 하한.
    pub use_sharp_ai: bool,
    pub sharp_min: f32,
    /// 커스텀 프롬프트(예: "dramatic lighting") 하한.
    pub use_custom_prompt: bool,
    pub custom_prompt: String,
    pub custom_min: f32,

    // ───── 검출(Tier4, 검출 모델 필요) ─────
    /// 얼굴 있는 컷만 Good(인물 컬링).
    pub use_face: bool,
    /// 눈 뜬 컷만(얼굴 검출 기반).
    pub use_eyes_open: bool,
    /// 특정 객체(COCO 클래스명) 포함 컷만.
    pub use_object: bool,
    pub object_class: String,

    // ───── 다이얼로그 표시 상태(#70) ─────
    /// '고급 옵션' 섹션 펼침 여부. 기본 접힘(옛 설정 파일도 컨테이너 serde(default)로 false).
    /// 접혀 있어도 켜 둔 고급 옵션 값은 계속 적용된다(다이얼로그가 경고 줄로 알림).
    pub advanced_open: bool,
}

/// 방향 필터(Tier1).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OrientationFilter {
    #[default]
    Any,
    Portrait,
    Landscape,
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
            filter_orientation: OrientationFilter::Any,
            use_iso_max: false,
            iso_max: 6400,
            use_focal_range: false,
            focal_min_mm: 0.0,
            focal_max_mm: 1000.0,
            use_aperture_max: false,
            aperture_max: 2.8,
            use_shutter_min: false,
            shutter_min_secs: 1.0 / 60.0,
            camera_contains: String::new(),
            lens_contains: String::new(),
            use_burst: false,
            burst_gap_secs: 2,
            burst_keep: 1,
            use_dedup: false,
            dedup_hamming: 6,
            dedup_keep: 1,
            use_genre: false,
            genre_portrait: true,
            use_sharp_ai: false,
            sharp_min: 0.5,
            use_custom_prompt: false,
            custom_prompt: String::new(),
            custom_min: 0.5,
            use_face: false,
            use_eyes_open: false,
            use_object: false,
            object_class: String::new(),
            advanced_open: false,
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
        self.use_focus
            || self.use_exposure
            || self.use_tilt
            || self.use_aesthetic
            || self.filter_orientation != OrientationFilter::Any
            || self.use_iso_max
            || self.use_focal_range
            || self.use_aperture_max
            || self.use_shutter_min
            || !self.camera_contains.trim().is_empty()
            || !self.lens_contains.trim().is_empty()
            || self.use_burst
            || self.use_dedup
            || self.use_genre
            || self.use_sharp_ai
            || self.use_custom_prompt
            || self.use_face
            || self.use_eyes_open
            || self.use_object
    }

    /// Tier1 메타 필터로 변환(UI config → 코어 필터).
    pub fn meta_filter(&self) -> crate::cull_ext::MetaFilter {
        use crate::cull_ext::{MetaFilter, Orientation};
        MetaFilter {
            orientation: match self.filter_orientation {
                OrientationFilter::Any => None,
                OrientationFilter::Portrait => Some(Orientation::Portrait),
                OrientationFilter::Landscape => Some(Orientation::Landscape),
            },
            iso_max: self.use_iso_max.then_some(self.iso_max),
            iso_min: None,
            aperture_max: self.use_aperture_max.then_some(self.aperture_max),
            aperture_min: None,
            shutter_min_secs: self.use_shutter_min.then_some(self.shutter_min_secs),
            shutter_max_secs: None,
            focal_min_mm: self.use_focal_range.then_some(self.focal_min_mm),
            focal_max_mm: self.use_focal_range.then_some(self.focal_max_mm),
            camera_contains: {
                let t = self.camera_contains.trim();
                (!t.is_empty()).then(|| t.to_string())
            },
            lens_contains: {
                let t = self.lens_contains.trim();
                (!t.is_empty()).then(|| t.to_string())
            },
        }
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

/// 폴더별로 마지막에 보고 있던 사진(#86). 같은 폴더를 다시 열면 그 사진에서 재개한다.
///
/// 배열 인덱스가 아니라 **대표 파일의 전체 경로**를 저장한다 — 파일이 추가·삭제되거나
/// 정렬 기준이 바뀌어도 같은 사진을 다시 집을 수 있고, 하위 폴더 포함 스캔에서 이름이
/// 겹치는 파일도 구분된다. 저장된 파일이 사라지면 호출부가 첫 사진으로 폴백한다.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FolderResume {
    /// 폴더 경로(문자열).
    pub folder: String,
    /// 그 폴더에서 마지막으로 보던 항목의 대표 파일 전체 경로.
    pub file: String,
}

/// 사진을 넘길 때 원본 보기(ORIG) 상태를 이어가는 방식(#87). 사용자마다 빠른 탐색과
/// 원본 연속 확인 중 선호가 갈려 설정에서 고른다.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViewCarry {
    /// 기존 동작(#85 규칙, 기본값): 확대 중이면 ORIG를 유지하고, 창맞춤 상태면 프리뷰로 복귀한다.
    #[default]
    ZoomOnly,
    /// 현재 보기 상태를 그대로 유지: ORIG면 다음 사진도 ORIG, 프리뷰면 계속 프리뷰.
    Keep,
}

/// 전송(내보내기) 리네임 프리셋(#26 UI, #57 저장). Off=원본 이름 유지.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RenameMode {
    #[default]
    Off,
    Seq,
    Grade,
    Custom,
}

/// 전송(내보내기) 다이얼로그의 마지막 사용 옵션(#57). 시작할 때 저장하고 다음 열기 때
/// 기본값으로 로드한다. dest(대상 폴더)는 저장하지 않는다 — 열 때마다 현재 폴더 기준 제안.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TransferDefaults {
    pub labels: Vec<crate::model::Label>,
    pub stars: Vec<u8>,
    pub tags: Vec<crate::model::ColorTag>,
    pub action: crate::transfer::Action,
    pub companions: crate::transfer::Companions,
    pub split_by_label: bool,
    pub split_by_tag: bool,
    pub conflict: crate::transfer::ConflictPolicy,
    pub rename_mode: RenameMode,
    pub rename_template: String,
    pub rename_numbering: crate::transfer::Numbering,
    /// 전송 대상 범위(#68): true=전체 items, false=현재 필터(라벨·별점·태그 AND) 통과분만. 기본 전체.
    /// 컨테이너 `#[serde(default)]`가 옛 설정의 누락 필드를 `Default`(=true)로 채운다.
    pub scope_all: bool,
}

impl Default for TransferDefaults {
    fn default() -> Self {
        TransferDefaults {
            labels: vec![crate::model::Label::Pick],
            stars: Vec::new(),
            tags: Vec::new(),
            action: crate::transfer::Action::Copy,
            companions: crate::transfer::Companions::Both,
            split_by_label: false,
            split_by_tag: false,
            conflict: crate::transfer::ConflictPolicy::AutoIncrement,
            rename_mode: RenameMode::Off,
            rename_template: "{gradeseq}_{orig}".into(),
            rename_numbering: crate::transfer::Numbering::GradeGrouped,
            scope_all: true,
        }
    }
}

/// 폴더 자동 분류 다이얼로그의 마지막 사용 옵션(#57). dest는 저장하지 않는다(현재 폴더 기준).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OrganizeDefaults {
    pub key: crate::organize::OrganizeKey,
    pub action: crate::transfer::Action,
    pub conflict: crate::transfer::ConflictPolicy,
}

impl Default for OrganizeDefaults {
    fn default() -> Self {
        OrganizeDefaults {
            key: crate::organize::OrganizeKey::Date,
            action: crate::transfer::Action::Move, // 이슈 #34: 폴더분류는 "이동"이 기본 의도.
            conflict: crate::transfer::ConflictPolicy::AutoIncrement,
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
    /// 새 버전 자동 확인(#69). 기본 켬. 끄면 `maybe_check_update`가 아무 것도 시작하지 않는다.
    /// `large_badges`와 같은 이유로 **필드 레벨 `#[serde(default)]`를 달지 않는다**(달면 누락 시
    /// bool 기본값 false가 되어 기존 사용자에게서 자동 확인이 꺼진다). 컨테이너 `#[serde(default)]`가
    /// 누락 필드를 `Config::default()`(=true)로 채우게 한다.
    pub check_updates: bool,
    /// AI 컬링(#50) 설정. 누락 시 기본값.
    #[serde(default)]
    pub ai_cull: AiCullConfig,
    /// 정렬 기준(#56): 촬영시간순(기본)/파일명순. 설정에서 변경, 저장.
    #[serde(default)]
    pub sort: crate::model::SortOrder,
    /// 전송(내보내기) 다이얼로그 마지막 사용 옵션(#57).
    #[serde(default)]
    pub transfer_defaults: TransferDefaults,
    /// 폴더 자동 분류 다이얼로그 마지막 사용 옵션(#57).
    #[serde(default)]
    pub organize_defaults: OrganizeDefaults,
    /// 사진 이동 시 원본 보기(ORIG) 유지 방식(#87). 기본은 기존 동작(`ZoomOnly`)이라
    /// 설정을 건드리지 않은 사용자의 체감은 v0.5.10과 같다.
    #[serde(default)]
    pub view_carry: ViewCarry,
    /// 폴더별 마지막으로 보던 사진(#86). 최근 사용 순(맨 앞이 가장 최근), 최대 64개.
    #[serde(default)]
    pub folder_resume: Vec<FolderResume>,
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
            check_updates: true,  // 새 버전 자동 확인 기본 켬(#69).
            ai_cull: AiCullConfig::default(), // AI 컬링 기본 설정(#50).
            sort: crate::model::SortOrder::default(), // 촬영시간순이 기본(#56 코멘트).
            transfer_defaults: TransferDefaults::default(), // 전송 마지막 사용 옵션(#57).
            organize_defaults: OrganizeDefaults::default(), // 정리 마지막 사용 옵션(#57).
            view_carry: ViewCarry::default(), // 이동 시 ORIG 유지 방식(#87). 기본=기존 동작.
            folder_resume: Vec::new(),        // 폴더별 재개 위치(#86). 처음엔 비어 있음.
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

    /// 폴더의 재개 위치를 기록한다(#86). 최근 것이 맨 앞으로 오고 64개를 넘으면 오래된
    /// 폴더부터 버린다(`recent_folders`와 같은 LRU 방식 — 오래 쓴 앱에서 무한히 자라지 않게).
    pub fn set_folder_resume(&mut self, folder: &str, file: &str) {
        self.folder_resume.retain(|r| r.folder != folder);
        self.folder_resume.insert(
            0,
            FolderResume {
                folder: folder.to_string(),
                file: file.to_string(),
            },
        );
        self.folder_resume.truncate(64);
    }

    /// 폴더에 기록된 재개 위치(대표 파일 전체 경로). 없으면 `None` → 호출부는 첫 사진에서 시작.
    pub fn folder_resume_file(&self, folder: &str) -> Option<&str> {
        self.folder_resume
            .iter()
            .find(|r| r.folder == folder)
            .map(|r| r.file.as_str())
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

/// 설정을 로드(없으면 기본값). 파손 시 손상본을 `config.json.corrupt`로 치워 두고
/// (다음 저장이 덮어써 증거가 사라지는 것 방지, 수동 복구 여지 유지) 기본값을 반환한다.
pub fn load() -> Config {
    load_from(&config_path())
}

fn load_from(path: &Path) -> Config {
    let Ok(data) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    match serde_json::from_str(&data) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::rename(path, path.with_extension("json.corrupt"));
            Config::default()
        }
    }
}

/// 설정을 저장. 원자적 교체(임시 파일 → rename)라 저장 도중 크래시에도 기존 설정이
/// 잘린 채 남지 않는다. 슬라이더 조작 등 UI 스레드에서 빈번히 불리므로 fsync는 생략
/// (유실 시 재생성 가능한 데이터 — [`crate::fsio::write_atomic_nosync`] 참조).
pub fn save(config: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    save_to(&config_path(), config)
}

fn save_to(path: &Path, config: &Config) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(config)
        .map_err(std::io::Error::other)?;
    crate::fsio::write_atomic_nosync(path, json.as_bytes())
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
    fn gpu_model_spec_is_rn50_fp32_consistent() {
        // GPU 전용 모델(RN50 fp32) 명세 무결성: fp32(.onnx, int8 아님), URL이 파일명으로 끝남, sha 64자.
        assert_eq!(GPU_MODEL.file, "clip-iqa-RN50.onnx");
        assert!(!GPU_MODEL.file.contains("int8"));
        assert!(GPU_MODEL.url.ends_with(GPU_MODEL.file));
        assert_eq!(GPU_MODEL.sha256.len(), 64);
        // fp32 RN50 ≈ 153MB — 상수 비교라 컴파일타임 검증으로 둔다(clippy: 상수 assert 지양).
        const _: () = assert!(GPU_MODEL.bytes > 100_000_000);
    }

    #[test]
    fn default_backbone_is_recommended_vitb32() {
        assert_eq!(ClipIqaBackbone::default(), ClipIqaBackbone::ViTB32);
        assert_eq!(ClipIqaBackbone::ALL[0], ClipIqaBackbone::ViTB32); // 표시 순서 권장 먼저
        assert!(!AiCullConfig::default().use_gpu); // 기본 CPU
    }

    #[test]
    fn config_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let c = Config {
            recent_folders: vec!["/tmp/a".into(), "/tmp/b".into()],
            ..Default::default()
        };
        save_to(&path, &c).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.recent_folders, c.recent_folders);
    }

    #[test]
    fn dialog_defaults_round_trip_and_legacy_config_gets_defaults() {
        // #57: 전송/정리 마지막 사용 옵션이 저장·복원되고, 필드가 없는 옛 설정 파일은 기본값으로 채워진다.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let c = Config {
            transfer_defaults: TransferDefaults {
                labels: vec![crate::model::Label::Pick, crate::model::Label::Hold],
                stars: vec![4, 5],
                action: crate::transfer::Action::Move,
                split_by_label: true,
                rename_mode: RenameMode::Custom,
                rename_template: "{orig}_final".into(),
                rename_numbering: crate::transfer::Numbering::Order,
                scope_all: false, // 기본(true)과 다른 값으로 저장·복원 확인(#68)
                ..Default::default()
            },
            organize_defaults: OrganizeDefaults {
                key: crate::organize::OrganizeKey::Camera,
                action: crate::transfer::Action::Copy,
                ..Default::default()
            },
            ..Default::default()
        };
        save_to(&path, &c).unwrap();
        let l = load_from(&path);
        assert_eq!(l.transfer_defaults.labels, c.transfer_defaults.labels);
        assert_eq!(l.transfer_defaults.stars, vec![4, 5]);
        assert_eq!(l.transfer_defaults.action, crate::transfer::Action::Move);
        assert!(l.transfer_defaults.split_by_label);
        assert_eq!(l.transfer_defaults.rename_mode, RenameMode::Custom);
        assert_eq!(l.transfer_defaults.rename_template, "{orig}_final");
        assert_eq!(l.transfer_defaults.rename_numbering, crate::transfer::Numbering::Order);
        assert!(!l.transfer_defaults.scope_all); // 전송 범위(#68)도 저장·복원
        assert_eq!(l.organize_defaults.key, crate::organize::OrganizeKey::Camera);
        assert_eq!(l.organize_defaults.action, crate::transfer::Action::Copy);

        // 옛 설정 파일(새 필드 없음) → 컨테이너 serde(default)로 기본값 채움.
        std::fs::write(&path, "{}").unwrap();
        let legacy = load_from(&path);
        assert_eq!(legacy.transfer_defaults.labels, vec![crate::model::Label::Pick]);
        assert_eq!(legacy.transfer_defaults.action, crate::transfer::Action::Copy);
        assert!(legacy.transfer_defaults.scope_all); // 누락 필드는 기본 전체(#68)
        assert_eq!(legacy.organize_defaults.action, crate::transfer::Action::Move); // 정리는 이동 기본(#34)
    }

    #[test]
    fn folder_resume_keeps_each_folder_separately() {
        // 폴더 A와 B가 서로 다른 마지막 위치를 각각 기억한다(#86 완료 기준).
        let mut c = Config::default();
        assert_eq!(c.folder_resume_file("/photos/A"), None, "새 폴더는 기록 없음 → 첫 사진");
        c.set_folder_resume("/photos/A", "/photos/A/DSC_0100.NEF");
        c.set_folder_resume("/photos/B", "/photos/B/DSC_0200.NEF");
        assert_eq!(c.folder_resume_file("/photos/A"), Some("/photos/A/DSC_0100.NEF"));
        assert_eq!(c.folder_resume_file("/photos/B"), Some("/photos/B/DSC_0200.NEF"));
        // 같은 폴더를 다시 기록하면 덮어쓰고 맨 앞으로(중복 누적 금지).
        c.set_folder_resume("/photos/A", "/photos/A/DSC_0999.NEF");
        assert_eq!(c.folder_resume_file("/photos/A"), Some("/photos/A/DSC_0999.NEF"));
        assert_eq!(c.folder_resume.len(), 2);
        assert_eq!(c.folder_resume[0].folder, "/photos/A");
    }

    #[test]
    fn folder_resume_evicts_oldest_beyond_cap() {
        // 오래 쓴 앱에서 설정 파일이 무한히 자라지 않도록 64개에서 잘린다(#86).
        let mut c = Config::default();
        for i in 0..70 {
            c.set_folder_resume(&format!("/photos/{i}"), &format!("/photos/{i}/x.NEF"));
        }
        assert_eq!(c.folder_resume.len(), 64);
        assert_eq!(c.folder_resume_file("/photos/69"), Some("/photos/69/x.NEF"), "가장 최근은 남음");
        assert_eq!(c.folder_resume_file("/photos/0"), None, "가장 오래된 것부터 버려짐");
    }

    #[test]
    fn view_carry_defaults_to_existing_behavior() {
        // #87: 설정을 건드리지 않은 기존 사용자는 v0.5.10과 같은 동작(ZoomOnly)이어야 한다.
        assert_eq!(Config::default().view_carry, ViewCarry::ZoomOnly);
    }

    #[test]
    fn config_corrupt_is_preserved_and_defaults_returned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json !!").unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.recent_folders, Config::default().recent_folders);
        // 손상본은 .corrupt로 보존되고 원 위치에서는 치워진다(다음 저장이 덮어쓰지 않게).
        assert!(!path.exists());
        let corrupt = dir.path().join("config.json.corrupt");
        assert_eq!(std::fs::read_to_string(corrupt).unwrap(), "{ not json !!");
    }
}
