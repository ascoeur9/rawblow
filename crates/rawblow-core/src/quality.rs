//! 품질 채점(컬링 보조, #50). 사진을 읽어 **노출이 적정한가**·**구도가 안정적인가(수평
//! 기울기)**를 채점한다. 노출·기울기는 순수 CV로 모델 없이 즉시 채점하고(전 플랫폼 동일,
//! 의존성 0), 미적 점수는 선택 기능 `ai`로 CLIP-IQA(제로샷, MIT 라이선스) ONNX 모델을 낸다.
//!
//! 입력은 이미 디코딩된 `DecodedImage`(RGBA8) — 워커가 프리뷰용으로 만든 버퍼를 그대로
//! 재활용하므로 추가 디스크 I/O가 없다. 결과는 **점수일 뿐 분류가 아니다**: 호출부(UI)가
//! "Reject 후보"·"Pick 후보" 같은 *제안*으로만 노출하고 최종 라벨은 사용자가 정한다.

use crate::decode::DecodedImage;
use serde::{Deserialize, Serialize};

/// 노출 분석 결과. 모든 비율·점수는 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExposureReport {
    /// 평균 휘도(0=흑, 1=백).
    pub mean: f32,
    /// 하이라이트 클리핑 비율(Y≥250인 픽셀). 날아간 하늘·창문 등.
    pub highlight_clip: f32,
    /// 섀도우 클리핑 비율(Y≤5인 픽셀). 뭉개진 검은 영역.
    pub shadow_clip: f32,
    /// 사용 다이내믹 레인지(0.5%~99.5% 분위 폭 / 255). 낮으면 저대비(안개·플랫).
    pub dynamic_range: f32,
    /// 노출 양호도 종합(1=양호). 클리핑·극단 평균휘도에 비례해 감점.
    pub score: f32,
}

/// 초점/선명도 분석 결과(흐림·핀나감 거르기).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FocusReport {
    /// 정규화 선명도(0..1, 높을수록 선명). 흐림 후보 거르기에 사용.
    pub sharpness: f32,
    /// 원시 지표(정규화 전 라플라시안 RMS). 임계 튜닝·연사 내 상대 비교용.
    pub acutance: f32,
    /// 선명도 임계(`IN_FOCUS_THRESH`) 초과 여부. false면 흐림/핀나감 후보.
    /// 주의: 하늘·단색면처럼 디테일 자체가 없는 장면은 선명해도 낮게 나올 수 있음(설명 가능 한계).
    pub in_focus: bool,
}

/// 수평 기울기(구도 안정감) 분석 결과.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TiltReport {
    /// 지배적 수평/수직 구조의 기울기(도). 0이면 반듯함. 보통 -12..12 범위로 보고.
    pub degrees: f32,
    /// 추정 신뢰도(0..1). 정렬된 강한 엣지 비율 — 낮으면 기울기 단서가 약한 장면(추정 무시 권장).
    pub confidence: f32,
}

/// 한 이미지의 품질 종합. `aesthetic`은 `ai` 기능으로 모델을 돌렸을 때만 채워진다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QualityReport {
    pub exposure: ExposureReport,
    pub focus: FocusReport,
    pub tilt: TiltReport,
    /// CLIP-IQA P(good) 0..1(높을수록 미적으로 좋음). 모델 미사용이면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aesthetic: Option<f32>,
}

impl QualityReport {
    /// 빈/손상 이미지에 대한 무의미 기본값(전부 0, 신뢰도 0).
    fn empty() -> Self {
        QualityReport {
            exposure: ExposureReport {
                mean: 0.0,
                highlight_clip: 0.0,
                shadow_clip: 0.0,
                dynamic_range: 0.0,
                score: 0.0,
            },
            focus: FocusReport { sharpness: 0.0, acutance: 0.0, in_focus: false },
            tilt: TiltReport { degrees: 0.0, confidence: 0.0 },
            aesthetic: None,
        }
    }
}

/// 모델 없이(노출 + 초점 + 수평 기울기) 채점한다. 항상 성공(빈 이미지는 0 리포트).
/// 초점은 **전체 프레임** 선명도. AF 측거점에서만 재고 싶으면 [`focus_report_regions`]를
/// 써서 `focus`를 교체하라(앱은 표시좌표 기준 AF 사각형을 이미 안다).
pub fn analyze(img: &DecodedImage) -> QualityReport {
    analyze_selective(img, true, true, true)
}

/// 켜진 신호만 계산한다(컬링 워커용 — 꺼진 검사는 건너뛰어 시간을 아낀다). 꺼진 항목은
/// `empty()` 기본값으로 남고, `CullCriteria::verdict`은 `use_*`가 false면 그 값을 무시한다.
/// 초점을 AF 영역에서만 잴 땐 `focus=false`로 두고 호출부가 [`focus_report_regions`]로 채운다
/// (전체 프레임 초점을 중복 계산하지 않음).
pub fn analyze_selective(img: &DecodedImage, exposure: bool, focus: bool, tilt: bool) -> QualityReport {
    let mut q = QualityReport::empty();
    if !valid(img) {
        return q;
    }
    if exposure {
        q.exposure = exposure_report(img);
    }
    if focus {
        q.focus = focus_report(img);
    }
    if tilt {
        q.tilt = tilt_report(img);
    }
    q
}

/// rgba 버퍼가 width*height*4 이상인지(디코더가 부분 버퍼를 줄 가능성 방어).
fn valid(img: &DecodedImage) -> bool {
    let px = (img.width as usize).saturating_mul(img.height as usize);
    px > 0 && img.rgba.len() >= px * 4
}

/// Rec.709 가중 휘도(정수 근사, 합 256). 0..255.
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u32 {
    (54 * r as u32 + 183 * g as u32 + 19 * b as u32) >> 8
}

fn exposure_report(img: &DecodedImage) -> ExposureReport {
    let px = (img.width as usize) * (img.height as usize);
    let mut hist = [0u32; 256];
    for c in img.rgba.chunks_exact(4) {
        hist[luma(c[0], c[1], c[2]) as usize] += 1;
    }
    let total = px as f64;

    let sum: u64 = hist.iter().enumerate().map(|(i, &n)| i as u64 * n as u64).sum();
    let mean = (sum as f64 / total / 255.0) as f32;

    let highlight_clip = (hist[250..].iter().map(|&n| n as u64).sum::<u64>() as f64 / total) as f32;
    let shadow_clip = (hist[..=5].iter().map(|&n| n as u64).sum::<u64>() as f64 / total) as f32;

    // 0.5%·99.5% 분위로 다이내믹 레인지(극단 소수 픽셀에 휘둘리지 않게).
    let lo_t = (total * 0.005) as u64;
    let hi_t = (total * 0.995) as u64;
    let mut cum = 0u64;
    let mut p_lo = 0usize;
    for (i, &n) in hist.iter().enumerate() {
        cum += n as u64;
        if cum >= lo_t {
            p_lo = i;
            break;
        }
    }
    cum = 0;
    let mut p_hi = 255usize;
    for (i, &n) in hist.iter().enumerate() {
        cum += n as u64;
        if cum >= hi_t {
            p_hi = i;
            break;
        }
    }
    let dynamic_range = p_hi.saturating_sub(p_lo) as f32 / 255.0;

    // 종합 점수: 클리핑이 주 감점(하이라이트가 더 치명적), 극단 평균휘도는 약한 감점.
    let clip_penalty = (highlight_clip * 6.0 + shadow_clip * 4.0).min(1.0);
    let mean_penalty = if mean > 0.85 {
        ((mean - 0.85) / 0.15 * 0.5).min(0.5)
    } else if mean < 0.10 {
        ((0.10 - mean) / 0.10 * 0.5).min(0.5)
    } else {
        0.0
    };
    let score = ((1.0 - clip_penalty) * (1.0 - mean_penalty)).clamp(0.0, 1.0);

    ExposureReport {
        mean,
        highlight_clip,
        shadow_clip,
        dynamic_range,
        score,
    }
}

// 초점 검출 파라미터.
const FOCUS_MAX_EDGE: u32 = 1024; // 디테일 보존(블러 판별력)과 속도의 절충.
const FOCUS_K: f32 = 8.0; // 선명도 정규화 곡선 상수: sharpness = a/(a+K).
/// 흐림 후보로 거르는 선명도 임계(보수적 기본값). 0.45 ≈ 라플라시안 RMS 6.5.
pub const IN_FOCUS_THRESH: f32 = 0.45;
/// AF가 점만 줄 때(크기 0) 측정에 쓸 최소 박스 변(프레임 비율).
const AF_MIN_BOX: f32 = 0.06;

/// 전체 프레임 선명도. 라플라시안 RMS 기반.
pub fn focus_report(img: &DecodedImage) -> FocusReport {
    focus_report_regions(img, &[])
}

/// 주어진 영역(들)에서만 선명도를 잰다. `regions`는 **표시 이미지 기준** 정규화 사각형
/// `(cx, cy, w, h)`(0..1, 원점 좌상단) — AF 측거점 등. 비어 있으면 전체 프레임.
/// 여러 영역은 픽셀 수 가중 RMS로 합친다. 앱은 EXIF 회전을 적용한 표시좌표로 넘겨야 한다
/// (AF 원시 좌표는 센서 미회전 기준이므로 그대로 쓰면 회전 사진에서 어긋남).
pub fn focus_report_regions(img: &DecodedImage, regions: &[(f32, f32, f32, f32)]) -> FocusReport {
    let none = FocusReport { sharpness: 0.0, acutance: 0.0, in_focus: false };
    let Some(gray) = to_gray_resized(img, FOCUS_MAX_EDGE) else {
        return none;
    };
    let (gw, gh) = (gray.width() as f32, gray.height() as f32);

    let acutance = if regions.is_empty() {
        laplacian_rms(&gray, None).0
    } else {
        let mut sq_sum = 0f64;
        let mut wsum = 0u64;
        for &(cx, cy, bw, bh) in regions {
            let bw = bw.max(AF_MIN_BOX);
            let bh = bh.max(AF_MIN_BOX);
            let x0 = ((cx - bw / 2.0) * gw).clamp(0.0, gw) as u32;
            let y0 = ((cy - bh / 2.0) * gh).clamp(0.0, gh) as u32;
            let x1 = ((cx + bw / 2.0) * gw).clamp(0.0, gw) as u32;
            let y1 = ((cy + bh / 2.0) * gh).clamp(0.0, gh) as u32;
            let (rms, n) = laplacian_rms(&gray, Some((x0, y0, x1, y1)));
            sq_sum += (rms * rms) as f64 * n as f64;
            wsum += n;
        }
        if wsum > 0 {
            (sq_sum / wsum as f64).sqrt() as f32
        } else {
            laplacian_rms(&gray, None).0
        }
    };

    let sharpness = acutance / (acutance + FOCUS_K);
    FocusReport {
        sharpness,
        acutance,
        in_focus: sharpness >= IN_FOCUS_THRESH,
    }
}

/// 4-이웃 라플라시안의 RMS와 표본 수. `rect`=픽셀 경계 `(x0,y0,x1,y1)`(없으면 전체).
fn laplacian_rms(gray: &image::GrayImage, rect: Option<(u32, u32, u32, u32)>) -> (f32, u64) {
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    let g = gray.as_raw();
    let at = |x: i32, y: i32| g[(y * w + x) as usize] as f32;
    let (x0, y0, x1, y1) = rect
        .map(|(a, b, c, d)| (a as i32, b as i32, c as i32, d as i32))
        .unwrap_or((0, 0, w, h));
    // 1px 테두리는 라플라시안 정의 불가 → 안쪽으로.
    let (x0, y0) = (x0.max(1), y0.max(1));
    let (x1, y1) = (x1.min(w - 1), y1.min(h - 1));
    let mut sq_sum = 0f64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let lap = 4.0 * at(x, y) - at(x - 1, y) - at(x + 1, y) - at(x, y - 1) - at(x, y + 1);
            sq_sum += (lap * lap) as f64;
            n += 1;
        }
    }
    if n == 0 {
        (0.0, 0)
    } else {
        ((sq_sum / n as f64).sqrt() as f32, n)
    }
}

// 기울기 검출 파라미터.
const TILT_MAX_EDGE: u32 = 512; // 채점용 다운스케일(노이즈 억제 + 속도).
const TILT_WINDOW_DEG: f32 = 12.0; // 기울기 단서로 인정할 ±각도(이보다 큰 엣지는 대각 구도로 간주).
const TILT_BINS: usize = 121; // 0.2° 해상도.
const TILT_MAG_THRESH: f32 = 60.0; // 약한 엣지(노이즈) 무시 임계.

fn tilt_report(img: &DecodedImage) -> TiltReport {
    let none = TiltReport { degrees: 0.0, confidence: 0.0 };
    let Some(gray) = to_gray_resized(img, TILT_MAX_EDGE) else {
        return none;
    };
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    if w < 8 || h < 8 {
        return none;
    }
    let g = gray.as_raw();
    let at = |x: i32, y: i32| g[(y * w + x) as usize] as f32;

    let mut hist = [0f32; TILT_BINS];
    let mut total_mag = 0f32;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            // Sobel.
            let gx = -at(x - 1, y - 1) - 2.0 * at(x - 1, y) - at(x - 1, y + 1)
                + at(x + 1, y - 1)
                + 2.0 * at(x + 1, y)
                + at(x + 1, y + 1);
            let gy = -at(x - 1, y - 1) - 2.0 * at(x, y - 1) - at(x + 1, y - 1)
                + at(x - 1, y + 1)
                + 2.0 * at(x, y + 1)
                + at(x + 1, y + 1);
            let mag = (gx * gx + gy * gy).sqrt();
            if mag < TILT_MAG_THRESH {
                continue;
            }
            total_mag += mag;
            // 엣지 방향 = 그래디언트 방향 - 90°. mod 90으로 수평·수직 구조를 한 축으로 합치고,
            // (-45,45] 부호 편차로 환원 → 부호는 mod 처리로 그래디언트 방향과 무관하게 일관.
            let edge = gy.atan2(gx).to_degrees() - 90.0;
            let r = edge.rem_euclid(90.0);
            let dev = if r > 45.0 { r - 90.0 } else { r };
            if dev.abs() <= TILT_WINDOW_DEG {
                let idx = ((dev + TILT_WINDOW_DEG) / (2.0 * TILT_WINDOW_DEG)
                    * (TILT_BINS as f32 - 1.0))
                    .round() as usize;
                hist[idx.min(TILT_BINS - 1)] += mag;
            }
        }
    }
    if total_mag <= 0.0 {
        return none;
    }

    // 약한 스무딩 후 피크 빈.
    let mut best = 0usize;
    let mut best_v = -1.0f32;
    for i in 0..TILT_BINS {
        let l = if i > 0 { hist[i - 1] } else { 0.0 };
        let rr = if i + 1 < TILT_BINS { hist[i + 1] } else { 0.0 };
        let v = hist[i] + 0.5 * l + 0.5 * rr;
        if v > best_v {
            best_v = v;
            best = i;
        }
    }
    // 피크 주변 가중 중심으로 소수점 정밀화.
    let lo = best.saturating_sub(3);
    let hi = (best + 3).min(TILT_BINS - 1);
    let mut wsum = 0f32;
    let mut isum = 0f32;
    for i in lo..=hi {
        wsum += hist[i];
        isum += hist[i] * i as f32;
    }
    let peak = if wsum > 0.0 { isum / wsum } else { best as f32 };
    let degrees = peak / (TILT_BINS as f32 - 1.0) * (2.0 * TILT_WINDOW_DEG) - TILT_WINDOW_DEG;

    let window_mass: f32 = hist.iter().sum();
    let confidence = (window_mass / total_mag).clamp(0.0, 1.0);

    TiltReport { degrees, confidence }
}

/// RGBA8 → 긴 변 max_edge 이하의 그레이스케일. 부적합/실패 시 None.
fn to_gray_resized(img: &DecodedImage, max_edge: u32) -> Option<image::GrayImage> {
    if !valid(img) {
        return None;
    }
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())?;
    let dynimg = image::DynamicImage::ImageRgba8(buf);
    let scaled = if img.width.max(img.height) > max_edge {
        let (w, h) = fit(img.width, img.height, max_edge);
        dynimg.resize_exact(w, h, image::imageops::FilterType::Triangle)
    } else {
        dynimg
    };
    Some(scaled.to_luma8())
}

/// 긴 변을 max_edge로 맞춘 (w,h)(비율 유지, 최소 1).
fn fit(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    if w >= h {
        let nh = ((h as u64 * max_edge as u64) / w as u64).max(1) as u32;
        (max_edge, nh)
    } else {
        let nw = ((w as u64 * max_edge as u64) / h as u64).max(1) as u32;
        (nw, max_edge)
    }
}

// ───────────────────────── 컬링 판정(기준 → 좋음/탈락) ─────────────────────────

/// AI 컬링 판정 결과. "양쪽 다 표시" 모드라 검사한 항목은 둘 중 하나로 분류된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 켜진 모든 검사를 통과 — 추천(좋은 사진).
    Good,
    /// 켜진 검사 중 하나라도 실패 — 탈락 후보(흐림·노출불량·기울어짐).
    Bad,
}

/// 어떤 신호로 거를지 + 임계값. UI가 사용자 설정에서 채워 [`QualityReport`]에 적용한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CullCriteria {
    pub use_focus: bool,
    pub use_exposure: bool,
    pub use_tilt: bool,
    /// CLIP-IQA 미적 점수 사용 여부. 모델이 없으면 이 검사는 건너뜀.
    pub use_aesthetic: bool,
    /// 선명도 하한(미만이면 흐림 후보). 보통 [`IN_FOCUS_THRESH`].
    pub focus_thresh: f32,
    /// 노출 양호도 하한(미만이면 탈락). 0..1.
    pub exposure_min: f32,
    /// 허용 최대 기울기(도). 초과면 탈락. 단 기울기 신뢰도가 낮으면 무시(오판 방지).
    pub tilt_max_deg: f32,
    /// P(good) 하한. 미만이면 탈락. 0..1.
    pub aesthetic_min: f32,
}

impl Default for CullCriteria {
    fn default() -> Self {
        CullCriteria {
            use_focus: true,
            use_exposure: true,
            use_tilt: true,
            use_aesthetic: false,
            focus_thresh: IN_FOCUS_THRESH,
            exposure_min: 0.5,
            tilt_max_deg: 3.0,
            aesthetic_min: 0.55,
        }
    }
}

/// 기울기 판정에 필요한 최소 신뢰도. 이보다 낮으면(평탄·텍스처 부족 장면) 기울기로 거르지 않는다.
const TILT_MIN_CONFIDENCE: f32 = 0.2;

impl CullCriteria {
    /// 켜진 검사 중 하나라도 실패하면 `Bad`, 전부 통과면 `Good`.
    pub fn verdict(&self, q: &QualityReport) -> Verdict {
        let focus_fail = self.use_focus && q.focus.sharpness < self.focus_thresh;
        let expo_fail = self.use_exposure && q.exposure.score < self.exposure_min;
        let tilt_fail = self.use_tilt
            && q.tilt.confidence >= TILT_MIN_CONFIDENCE
            && q.tilt.degrees.abs() > self.tilt_max_deg;
        let aesthetic_fail = self.use_aesthetic
            && q.aesthetic.is_some_and(|a| a < self.aesthetic_min);
        if focus_fail || expo_fail || tilt_fail || aesthetic_fail {
            Verdict::Bad
        } else {
            Verdict::Good
        }
    }
}

// ───────────────────────── 선택 기능: CLIP-IQA 미적 점수 ─────────────────────────
#[cfg(feature = "ai")]
pub use ai::{Accel, AestheticModel};

#[cfg(feature = "ai")]
mod ai {
    use super::*;
    use ort::session::Session;
    use ort::value::Tensor;
    use std::path::Path;
    use std::sync::Mutex;

    const SIDE: usize = 224;
    // CLIP 정규화 = ImageNet 통계(CLIP-IQA 논문·open_clip 표준 전처리와 동일).
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];

    /// 추론 가속기 선택. **벤치 결과 CLIP 계열 모델은 CPU(int8) 병렬이 CoreML/WebGPU보다
    /// 빠르고 안정적**(GPU EP가 트랜스포머 그래프를 잘게 분할해 데이터 왕복 오버헤드가 큼)이라
    /// `Auto`=CPU가 기본이다. `Gpu`는 실험·향후 모델용(macOS=CoreML, Windows=WebGPU).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Accel {
        Auto,
        Cpu,
        Gpu,
    }

    /// CLIP-IQA(ONNX) 미적 점수 모델. 내부 Mutex로 스레드 안전(워커 여러 개가 공유 가능).
    /// 입력: (N,3,224,224) CHW f32 CLIP 정규화. 출력: (N,2) softmax [P(good), P(bad)].
    pub struct AestheticModel {
        session: Mutex<Session>,
    }

    impl AestheticModel {
        /// `.onnx` 파일에서 모델을 로드한다(CPU 기본 — 벤치상 가장 빠름).
        pub fn load(path: &Path) -> Result<Self, String> {
            Self::load_tuned(path, Accel::Auto, None)
        }

        /// 가속기를 명시해 로드(intra-op 스레드는 ort 기본).
        pub fn load_with(path: &Path, accel: Accel) -> Result<Self, String> {
            Self::load_tuned(path, accel, None)
        }

        /// 가속기 + 세션당 intra-op 스레드 수를 명시해 로드. 병렬 워커 풀에서는 `intra=Some(1)`로
        /// 두고 워커 수로 코어를 채우는 편이 (세션 내부 스레드와의 경합이 없어) 처리량이 가장 높다.
        /// GPU EP 등록이 실패해도(미지원 빌드) CPU로 폴백한다.
        #[allow(unused_mut)]
        pub fn load_tuned(path: &Path, accel: Accel, intra: Option<usize>) -> Result<Self, String> {
            let mut builder = Session::builder().map_err(|e| format!("ort builder: {e}"))?;
            // 인자 > 환경변수(RB_INTRA, 벤치용) 순으로 intra-op 스레드 수를 정한다.
            let intra = intra.or_else(|| std::env::var("RB_INTRA").ok().and_then(|s| s.parse().ok()));
            if let Some(n) = intra {
                builder = builder.with_intra_threads(n).map_err(|e| format!("ort intra: {e}"))?;
            }
            if matches!(accel, Accel::Gpu) {
                #[cfg(target_os = "macos")]
                {
                    // 벤치용: RB_GPU_EP=webgpu로 WebGPU, 기본은 CoreML.
                    if std::env::var("RB_GPU_EP").as_deref() == Ok("webgpu") {
                        builder = builder
                            .with_execution_providers([ort::ep::WebGPU::default().build()])
                            .map_err(|e| format!("ort webgpu ep: {e}"))?;
                    } else {
                        builder = builder
                            .with_execution_providers([ort::ep::CoreML::default().build()])
                            .map_err(|e| format!("ort coreml ep: {e}"))?;
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    builder = builder
                        .with_execution_providers([ort::ep::WebGPU::default().build()])
                        .map_err(|e| format!("ort webgpu ep: {e}"))?;
                }
            }
            let session = builder
                .commit_from_file(path)
                .map_err(|e| format!("ort load {}: {e}", path.display()))?;
            Ok(Self { session: Mutex::new(session) })
        }

        /// P(good) 0..1(높을수록 미적으로 좋음). 디코딩된 이미지를 224²로 전처리해 추론.
        pub fn score(&self, img: &DecodedImage) -> Result<f32, String> {
            let chw = preprocess(img).ok_or("invalid image for aesthetic scoring")?;
            let tensor = Tensor::from_array(([1usize, 3, SIDE, SIDE], chw))
                .map_err(|e| format!("ort tensor: {e}"))?;
            let mut sess = self.session.lock().map_err(|_| "ort session poisoned")?;
            let input_name = sess.inputs()[0].name().to_string();
            let outputs = sess
                .run(ort::inputs![input_name => tensor])
                .map_err(|e| format!("ort run: {e}"))?;
            let (_, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("ort extract: {e}"))?;
            // 출력 (1,2): [P(good), P(bad)]. index 0 = P(good).
            if data.len() < 2 {
                return Err(format!("unexpected CLIP-IQA output len {}", data.len()));
            }
            Ok(data[0].clamp(0.0, 1.0))
        }
    }

    /// RGBA8 → 224² RGB → CHW f32(CLIP 정규화). 실패 시 None.
    /// 리샘플은 Triangle(쌍선형) — CLIP 표준 전처리(바이큐빅)에 가장 가깝다. (면적평균으로
    /// 바꿔봤으나 추론이 병목이라 속도 이득이 없고 점수만 달라져 되돌림 — preprocess 비병목.)
    fn preprocess(img: &DecodedImage) -> Option<Vec<f32>> {
        if !valid(img) {
            return None;
        }
        let buf = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())?;
        let rgb = image::DynamicImage::ImageRgba8(buf)
            .resize_exact(SIDE as u32, SIDE as u32, image::imageops::FilterType::Triangle)
            .to_rgb8();
        let mut out = vec![0f32; 3 * SIDE * SIDE];
        for (i, px) in rgb.pixels().enumerate() {
            for c in 0..3 {
                let v = px[c] as f32 / 255.0;
                out[c * SIDE * SIDE + i] = (v - MEAN[c]) / STD[c];
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RGBA8 DecodedImage 합성(클로저로 픽셀 휘도 지정).
    fn synth(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> DecodedImage {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y);
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        DecodedImage {
            width: w,
            height: h,
            rgba,
            color_managed: false,
            full_raw: false,
        }
    }

    #[test]
    fn exposure_midgray_is_good() {
        let img = synth(64, 64, |_, _| 128);
        let e = exposure_report(&img);
        assert!((e.mean - 0.5).abs() < 0.02, "mean {}", e.mean);
        assert_eq!(e.highlight_clip, 0.0);
        assert_eq!(e.shadow_clip, 0.0);
        assert!(e.score > 0.95, "score {}", e.score);
    }

    #[test]
    fn exposure_blown_highlights_penalized() {
        // 절반이 완전히 날아간 흰색.
        let img = synth(64, 64, |x, _| if x < 32 { 128 } else { 255 });
        let e = exposure_report(&img);
        assert!(e.highlight_clip > 0.4, "hi {}", e.highlight_clip);
        assert!(e.score < 0.2, "score {}", e.score);
    }

    #[test]
    fn exposure_crushed_shadows_penalized() {
        let img = synth(64, 64, |x, _| if x < 32 { 128 } else { 0 });
        let e = exposure_report(&img);
        assert!(e.shadow_clip > 0.4, "sh {}", e.shadow_clip);
        assert!(e.score < 0.4, "score {}", e.score);
    }

    #[test]
    fn focus_sharp_pattern_in_focus() {
        // 1px 체커보드 = 고주파 → 매우 선명.
        let img = synth(200, 200, |x, y| if (x + y) % 2 == 0 { 220 } else { 30 });
        let f = focus_report(&img);
        assert!(f.in_focus, "sharpness {}", f.sharpness);
        assert!(f.sharpness > 0.8, "sharpness {}", f.sharpness);
    }

    #[test]
    fn focus_smooth_ramp_is_blurry() {
        // 매끈한 그래디언트 = 디테일 없음 → 흐림 판정.
        let img = synth(200, 200, |x, _| (x as f32 / 200.0 * 255.0) as u8);
        let f = focus_report(&img);
        assert!(!f.in_focus, "sharpness {}", f.sharpness);
    }

    #[test]
    fn focus_region_isolates_sharp_area() {
        // 좌반부만 선명 패턴, 우반부 평탄 → 좌측 영역 점수 > 우측.
        let img = synth(200, 200, |x, y| {
            if x < 100 {
                if (x + y) % 2 == 0 { 220 } else { 30 }
            } else {
                128
            }
        });
        let left = focus_report_regions(&img, &[(0.25, 0.5, 0.4, 0.9)]);
        let right = focus_report_regions(&img, &[(0.75, 0.5, 0.4, 0.9)]);
        assert!(left.sharpness > right.sharpness, "L {} R {}", left.sharpness, right.sharpness);
        assert!(left.in_focus && !right.in_focus);
    }

    /// 각도 alpha(도)로 기울어진 매끈한 엣지(밴드형 그래디언트). 엣지 방향 = alpha.
    fn tilted_edge(w: u32, h: u32, alpha_deg: f32) -> DecodedImage {
        let a = alpha_deg.to_radians();
        // 법선 방향(alpha+90)으로의 부호거리 d에 비례한 밴드. 중심 통과.
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let k = 16.0_f32;
        synth(w, h, |x, y| {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            // 법선 = (-sin a, cos a) 방향 거리.
            let d = -dx * a.sin() + dy * a.cos();
            (128.0 + k * d).clamp(0.0, 255.0) as u8
        })
    }

    #[test]
    fn tilt_level_image_near_zero() {
        let img = tilted_edge(240, 240, 0.0);
        let t = tilt_report(&img);
        assert!(t.degrees.abs() < 1.0, "degrees {}", t.degrees);
        assert!(t.confidence > 0.3, "confidence {}", t.confidence);
    }

    #[test]
    fn tilt_detects_six_degrees() {
        let img = tilted_edge(240, 240, 6.0);
        let t = tilt_report(&img);
        assert!((t.degrees.abs() - 6.0).abs() < 1.5, "degrees {}", t.degrees);
        assert!(t.confidence > 0.3, "confidence {}", t.confidence);
    }

    fn report_with(focus: f32, expo: f32, tilt_deg: f32, tilt_conf: f32) -> QualityReport {
        let mut q = QualityReport::empty();
        q.focus.sharpness = focus;
        q.exposure.score = expo;
        q.tilt.degrees = tilt_deg;
        q.tilt.confidence = tilt_conf;
        q
    }

    #[test]
    fn verdict_good_when_all_pass() {
        let c = CullCriteria::default();
        let q = report_with(0.8, 0.9, 0.5, 0.8);
        assert_eq!(c.verdict(&q), Verdict::Good);
    }

    #[test]
    fn verdict_bad_on_blur() {
        let c = CullCriteria::default();
        let q = report_with(0.2, 0.9, 0.0, 0.8);
        assert_eq!(c.verdict(&q), Verdict::Bad);
    }

    #[test]
    fn verdict_bad_on_tilt_only_when_confident() {
        let c = CullCriteria::default(); // tilt_max 3°
        // 크게 기울었지만 신뢰도 낮음 → 거르지 않음.
        assert_eq!(c.verdict(&report_with(0.8, 0.9, 9.0, 0.05)), Verdict::Good);
        // 신뢰도 충분 → 탈락.
        assert_eq!(c.verdict(&report_with(0.8, 0.9, 9.0, 0.6)), Verdict::Bad);
    }

    #[test]
    fn verdict_ignores_disabled_checks() {
        let c = CullCriteria {
            use_focus: false,
            use_tilt: false,
            ..CullCriteria::default()
        };
        // 흐림·기울기 둘 다 나쁘지만 검사 꺼짐, 노출만 통과 → Good.
        assert_eq!(c.verdict(&report_with(0.0, 0.9, 20.0, 0.9)), Verdict::Good);
    }

    #[test]
    fn verdict_aesthetic_gated_by_model() {
        // use_aesthetic=true but aesthetic=None → 모델 없음으로 간주, 판정에서 건너뜀.
        let c = CullCriteria { use_aesthetic: true, aesthetic_min: 0.5, ..CullCriteria::default() };
        let mut q = report_with(0.8, 0.9, 0.0, 0.0);
        q.aesthetic = None;
        assert_eq!(c.verdict(&q), Verdict::Good, "모델 없으면 미적 판정 건너뜀");
        // 모델 점수가 하한 미만이면 탈락.
        q.aesthetic = Some(0.3);
        assert_eq!(c.verdict(&q), Verdict::Bad, "P(good) < aesthetic_min → Bad");
        // 하한 이상이면 통과.
        q.aesthetic = Some(0.7);
        assert_eq!(c.verdict(&q), Verdict::Good, "P(good) >= aesthetic_min → Good");
    }

    #[test]
    fn analyze_handles_empty_image() {
        let img = DecodedImage {
            width: 0,
            height: 0,
            rgba: Vec::new(),
            color_managed: false,
            full_raw: false,
        };
        let q = analyze(&img);
        assert_eq!(q.tilt.confidence, 0.0);
        assert!(q.aesthetic.is_none());
    }
}
