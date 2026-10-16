//! 구도 보조선(#90). 화면 좌표만 돌려준다. 픽셀을 바꾸지 않는다.

use serde::{Deserialize, Serialize};

/// 세로·가로 각각 이 개수까지만 쓴다.
pub const GUIDE_LINE_CAP: usize = 8;

/// 툴바가 순환하는 프리셋. 기본은 끔.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GuidePreset {
    #[default]
    Off,
    Thirds,
    CenterCross,
    Square,
    Custom,
}

impl GuidePreset {
    /// 끔 → 삼분할 → 중앙 십자 → 정방형 → 사용자 지정 → 끔.
    pub fn cycle(self) -> Self {
        match self {
            GuidePreset::Off => GuidePreset::Thirds,
            GuidePreset::Thirds => GuidePreset::CenterCross,
            GuidePreset::CenterCross => GuidePreset::Square,
            GuidePreset::Square => GuidePreset::Custom,
            GuidePreset::Custom => GuidePreset::Off,
        }
    }

    /// 툴바·i18n 키. 끔일 때는 기능 이름.
    pub fn label_ko(self) -> &'static str {
        match self {
            GuidePreset::Off => "구도",
            GuidePreset::Thirds => "삼분할",
            GuidePreset::CenterCross => "중앙 십자",
            GuidePreset::Square => "정방형",
            GuidePreset::Custom => "사용자 지정",
        }
    }
}

/// 마지막 사용자 지정 하나. 이름을 붙인 프리셋 목록은 두지 않는다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuideCustom {
    /// 세로 선. 표시된 사진의 왼쪽에서 오른쪽, 0~1.
    pub vertical: Vec<f32>,
    /// 가로 선. 표시된 사진의 위에서 아래, 0~1.
    pub horizontal: Vec<f32>,
    pub rgb: [u8; 3],
    /// 1~4px.
    pub weight: f32,
    /// 0.2~1.
    pub opacity: f32,
}

impl Default for GuideCustom {
    fn default() -> Self {
        GuideCustom {
            vertical: vec![1.0 / 3.0, 2.0 / 3.0],
            horizontal: vec![1.0 / 3.0, 2.0 / 3.0],
            rgb: [255, 255, 255],
            weight: 1.0,
            opacity: 0.8,
        }
    }
}

/// 표시된 사진(0~1) 위의 선. 앱이 화면 사각형에 곱해 그린다.
#[derive(Clone, Debug, PartialEq)]
pub struct GuideLines {
    pub vertical: Vec<f32>,
    pub horizontal: Vec<f32>,
}

impl GuideLines {
    pub fn is_empty(&self) -> bool {
        self.vertical.is_empty() && self.horizontal.is_empty()
    }
}

/// `displayed_w`/`displayed_h`는 회전이 적용된 뒤 화면에 놓인 사진의 가로·세로.
/// 삼분할·십자는 비율만 쓰고, 정방형은 그 가로·세로로 가운데 정사각형의 변을 잡는다.
pub fn guide_lines(
    preset: GuidePreset,
    displayed_w: f32,
    displayed_h: f32,
    custom_vertical: &[f32],
    custom_horizontal: &[f32],
) -> GuideLines {
    match preset {
        GuidePreset::Off => GuideLines {
            vertical: Vec::new(),
            horizontal: Vec::new(),
        },
        GuidePreset::Thirds => GuideLines {
            vertical: vec![1.0 / 3.0, 2.0 / 3.0],
            horizontal: vec![1.0 / 3.0, 2.0 / 3.0],
        },
        GuidePreset::CenterCross => GuideLines {
            vertical: vec![0.5],
            horizontal: vec![0.5],
        },
        GuidePreset::Square => square_lines(displayed_w, displayed_h),
        GuidePreset::Custom => GuideLines {
            vertical: clamp_positions(custom_vertical),
            horizontal: clamp_positions(custom_horizontal),
        },
    }
}

/// 굵기 1~4, 불투명도 0.2~1. 유한하지 않으면 기본(1, 0.8).
pub fn clamp_guide_style(weight: f32, opacity: f32) -> (f32, f32) {
    let weight = if weight.is_finite() { weight } else { 1.0 };
    let opacity = if opacity.is_finite() { opacity } else { 0.8 };
    (weight.clamp(1.0, 4.0), opacity.clamp(0.2, 1.0))
}

fn clamp_positions(src: &[f32]) -> Vec<f32> {
    src.iter()
        .copied()
        .filter(|t| t.is_finite())
        .take(GUIDE_LINE_CAP)
        .map(|t| t.clamp(0.0, 1.0))
        .collect()
}

fn square_lines(w: f32, h: f32) -> GuideLines {
    let empty = GuideLines {
        vertical: Vec::new(),
        horizontal: Vec::new(),
    };
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return empty;
    }
    let long = w.max(h);
    // 이미 정방이면 그을 변이 없다.
    if (w - h).abs() <= long * 1.0e-4 {
        return empty;
    }
    if w > h {
        let inset = (1.0 - h / w) / 2.0;
        GuideLines {
            vertical: vec![inset, 1.0 - inset],
            horizontal: Vec::new(),
        }
    } else {
        let inset = (1.0 - w / h) / 2.0;
        GuideLines {
            vertical: Vec::new(),
            horizontal: vec![inset, 1.0 - inset],
        }
    }
}
