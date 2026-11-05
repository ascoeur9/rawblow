//! 표시용 색 변환(#122).
//!
//! 기본은 임베디드 ICC를 sRGB로 바꾼다. 광색역 표시가 켜지면 목적지가 OS별로 바뀐다.
//! macOS는 Display P3, Windows는 모니터 ICC, Linux와 실패는 sRGB.
//! `color` 피처가 없으면 픽셀을 바꾸지 않는다.

/// 입력 색. 임베디드 ICC가 있으면 그것이 우선이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputColor<'a> {
    Icc(&'a [u8]),
    /// EXIF `ColorSpace` 0xFFFF + `InteroperabilityIndex` R03. ICC 바이트는 없다.
    AdobeRgb,
    Srgb,
}

/// 표시 목적지. 모니터 ICC 파싱이 실패하면 sRGB로 다시 변환한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputColor {
    Srgb,
    DisplayP3,
    Icc(Vec<u8>),
}

/// 이 창에서 쓸 수 있는 출력. 앱이 OS를 조사해 넘긴다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsColor {
    Linux,
    MacOs { layer_ok: bool },
    Windows { icc: Option<Vec<u8>>, acm_or_hdr: bool },
}

/// 임베디드 ICC, 없으면 R03(Adobe RGB), 그 외 sRGB.
pub fn choose_input<'a>(
    icc: Option<&'a [u8]>,
    color_space: Option<u16>,
    interop: Option<&str>,
) -> InputColor<'a> {
    if let Some(profile) = icc.filter(|p| !p.is_empty()) {
        return InputColor::Icc(profile);
    }
    if color_space == Some(0xFFFF) && interop_is_r03(interop) {
        return InputColor::AdobeRgb;
    }
    InputColor::Srgb
}

fn interop_is_r03(interop: Option<&str>) -> bool {
    let Some(raw) = interop else { return false };
    let s = raw.trim().trim_matches('\0').trim();
    s.eq_ignore_ascii_case("R03")
}

/// 설정과 OS 상태를 표시 목적지로 접는다. 끄거나 폴백이면 sRGB.
pub fn effective_output(wide_gamut: bool, os: &OsColor) -> OutputColor {
    if !wide_gamut {
        return OutputColor::Srgb;
    }
    match os {
        OsColor::Linux => OutputColor::Srgb,
        OsColor::MacOs { layer_ok: false } => OutputColor::Srgb,
        OsColor::MacOs { layer_ok: true } => OutputColor::DisplayP3,
        OsColor::Windows { acm_or_hdr: true, .. } => OutputColor::Srgb,
        OsColor::Windows { icc: Some(bytes), acm_or_hdr: false } if !bytes.is_empty() => {
            OutputColor::Icc(bytes.clone())
        }
        OsColor::Windows { .. } => OutputColor::Srgb,
    }
}

impl OutputColor {
    /// 디스크 캐시 키에 섞을 값. sRGB는 0이라 기존 캐시 키를 유지한다.
    pub fn cache_tag(&self) -> u64 {
        match self {
            OutputColor::Srgb => 0,
            OutputColor::DisplayP3 => 1,
            OutputColor::Icc(bytes) => {
                let mut h = 0xcbf2_9ce4_8422_2325u64;
                for &b in bytes {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
                h | 0x8000_0000_0000_0000
            }
        }
    }
}

/// 모니터 변환 전 버퍼의 RGB 히스토그램(채널당 64칸). 앱의 기존 계산과 같다.
pub fn source_histogram(rgba: &[u8]) -> [[u32; 64]; 3] {
    let mut bins = [[0u32; 64]; 3];
    let px = rgba.len() / 4;
    if px == 0 {
        return bins;
    }
    let step = (px / 200_000).max(1);
    let mut i = 0;
    while i < px {
        let o = i * 4;
        bins[0][rgba[o] as usize >> 2] += 1;
        bins[1][rgba[o + 1] as usize >> 2] += 1;
        bins[2][rgba[o + 2] as usize >> 2] += 1;
        i += step;
    }
    bins
}

/// RGBA8을 표시 색으로 바꾼다. 바꿨으면 true. 실패·항등이면 false이고 버퍼는 그대로다.
pub fn convert_display(rgba: &mut [u8], input: InputColor<'_>, output: &OutputColor) -> bool {
    convert_display_inner(rgba, input, output, true)
}

/// RGBA8을 임베디드 ICC 기준으로 sRGB로 바꾼다. 수행했으면 true.
pub fn convert_to_srgb(rgba: &mut [u8], icc: &[u8]) -> bool {
    convert_display(rgba, InputColor::Icc(icc), &OutputColor::Srgb)
}

#[cfg(feature = "color")]
fn convert_display_inner(
    rgba: &mut [u8],
    input: InputColor<'_>,
    output: &OutputColor,
    fallback_output: bool,
) -> bool {
    if rgba.len() < 4 || rgba.len() % 4 != 0 {
        return false;
    }
    if matches!(input, InputColor::Srgb) && matches!(output, OutputColor::Srgb) {
        return false;
    }
    let mut input_profile = match input_profile(input) {
        Some(p) => p,
        None => return false,
    };
    let mut output_profile = match output_profile(output) {
        Some(p) => p,
        None if fallback_output && !matches!(output, OutputColor::Srgb) => {
            return convert_display_inner(rgba, input, &OutputColor::Srgb, false);
        }
        None => return false,
    };
    output_profile.precache_output_transform();
    input_profile.precache_output_transform();
    let transform = match qcms::Transform::new(
        &input_profile,
        &output_profile,
        qcms::DataType::RGBA8,
        qcms::Intent::Perceptual,
    ) {
        Some(t) => t,
        None if fallback_output && !matches!(output, OutputColor::Srgb) => {
            return convert_display_inner(rgba, input, &OutputColor::Srgb, false);
        }
        None => return false,
    };
    transform.apply(rgba);
    true
}

#[cfg(not(feature = "color"))]
fn convert_display_inner(
    _rgba: &mut [u8],
    _input: InputColor<'_>,
    _output: &OutputColor,
    _fallback_output: bool,
) -> bool {
    false
}

#[cfg(feature = "color")]
fn input_profile(input: InputColor<'_>) -> Option<Box<qcms::Profile>> {
    match input {
        InputColor::Srgb => Some(qcms::Profile::new_sRGB()),
        InputColor::AdobeRgb => adobe_rgb(),
        InputColor::Icc(bytes) => qcms::Profile::new_from_slice(bytes, false),
    }
}

#[cfg(feature = "color")]
fn output_profile(output: &OutputColor) -> Option<Box<qcms::Profile>> {
    match output {
        OutputColor::Srgb => Some(qcms::Profile::new_sRGB()),
        OutputColor::DisplayP3 => display_p3(),
        OutputColor::Icc(bytes) => qcms::Profile::new_from_slice(bytes, false),
    }
}

#[cfg(feature = "color")]
fn xy(x: f64, y: f64) -> qcms::CIE_xyY {
    qcms::CIE_xyY { x, y, Y: 1.0 }
}

/// Display P3 프라이머리. qcms는 감마 곡선만 공개라 전달 특성은 2.2로 둔다.
#[cfg(feature = "color")]
fn display_p3() -> Option<Box<qcms::Profile>> {
    qcms::Profile::new_rgb_with_gamma_set(
        xy(0.3127, 0.3290),
        qcms::CIE_xyYTRIPLE {
            red: xy(0.680, 0.320),
            green: xy(0.265, 0.690),
            blue: xy(0.150, 0.060),
        },
        2.2,
        2.2,
        2.2,
    )
}

/// Adobe RGB (1998). D65, 감마 2.2.
#[cfg(feature = "color")]
fn adobe_rgb() -> Option<Box<qcms::Profile>> {
    qcms::Profile::new_rgb_with_gamma_set(
        xy(0.3127, 0.3290),
        qcms::CIE_xyYTRIPLE {
            red: xy(0.6400, 0.3300),
            green: xy(0.2100, 0.7100),
            blue: xy(0.1500, 0.0600),
        },
        2.2,
        2.2,
        2.2,
    )
}
