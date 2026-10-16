//! 표시 전용 3D LUT(#125).
//!
//! `.cube`의 `LUT_3D_SIZE` 2~64만 받는다. 1D, 범위 밖 크기, 데이터가 짧거나
//! 길면 거절한다. 빌트인은 Warm, Tungsten, Chrome, Pan, Vivid, Snap뿐.
//! 룩이 없거나 비교 키를 누르고 있으면 입력 바이트를 그대로 돌려준다.

use serde::{Deserialize, Serialize};

/// 마지막 룩. 빈 `name`은 룩 없음. `"file"`이면 `cube_path`의 `.cube`.
/// 사진별 값은 두지 않는다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct LookPref {
    pub name: String,
    pub cube_path: String,
}

impl Default for LookPref {
    fn default() -> Self {
        Self {
            name: String::new(),
            cube_path: String::new(),
        }
    }
}

pub const BUILTINS: [&str; 6] = ["Warm", "Tungsten", "Chrome", "Pan", "Vivid", "Snap"];

#[derive(Clone, Debug)]
pub struct Lut3d {
    size: u32,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    /// R이 가장 빨리 변하고, 그다음 G, B. 길이 `size³ × 3`.
    rgb: Vec<f32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CubeError {
    /// `LUT_3D_SIZE`가 없거나 1D다.
    Not3d,
    /// 크기가 2~64가 아니다.
    Size,
    /// 격자 데이터가 크기와 맞지 않는다.
    Short,
    /// 숫자나 도메인이 아니다.
    Value,
}

/// 화면에 올릴 RGBA. 룩이 없거나 `compare_held`이면 `src`와 바이트가 같다.
pub fn display_rgba(src: &[u8], lut: Option<&Lut3d>, compare_held: bool) -> Vec<u8> {
    let Some(lut) = lut else {
        return src.to_vec();
    };
    if compare_held {
        return src.to_vec();
    }
    let mut out = src.to_vec();
    let n = out.len() / 4;
    for i in 0..n {
        let o = i * 4;
        let s = sample(
            lut,
            [
                out[o] as f32 / 255.0,
                out[o + 1] as f32 / 255.0,
                out[o + 2] as f32 / 255.0,
            ],
        );
        out[o] = to_u8(s[0]);
        out[o + 1] = to_u8(s[1]);
        out[o + 2] = to_u8(s[2]);
    }
    out
}

pub fn builtin(name: &str) -> Option<Lut3d> {
    let f: fn(f32, f32, f32) -> [f32; 3] = match name {
        "Warm" => warm,
        "Tungsten" => tungsten,
        "Chrome" => chrome,
        "Pan" => pan,
        "Vivid" => vivid,
        "Snap" => snap,
        _ => return None,
    };
    Some(bake(f))
}

pub fn parse_cube(text: &str) -> Result<Lut3d, CubeError> {
    let mut size: Option<u32> = None;
    let mut domain_min = [0.0_f32, 0.0, 0.0];
    let mut domain_max = [1.0_f32, 1.0, 1.0];
    let mut data: Vec<f32> = Vec::new();
    let mut in_data = false;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let first = parts.next().ok_or(CubeError::Value)?;
        if !in_data && first.parse::<f32>().is_err() {
            let key = first.to_ascii_uppercase();
            if key.starts_with("LUT_1D") {
                return Err(CubeError::Not3d);
            }
            match key.as_str() {
                "TITLE" => {}
                "LUT_3D_SIZE" => {
                    if size.is_some() {
                        return Err(CubeError::Size);
                    }
                    let n: u32 = parts.next().ok_or(CubeError::Size)?.parse().map_err(|_| CubeError::Size)?;
                    if parts.next().is_some() || !(2..=64).contains(&n) {
                        return Err(CubeError::Size);
                    }
                    size = Some(n);
                }
                "DOMAIN_MIN" => domain_min = three(&mut parts)?,
                "DOMAIN_MAX" => domain_max = three(&mut parts)?,
                _ => {}
            }
            continue;
        }
        if size.is_none() {
            return Err(CubeError::Not3d);
        }
        in_data = true;
        let mut vals = [parse_comp(first)?, 0.0, 0.0];
        for slot in &mut vals[1..] {
            *slot = parse_comp(parts.next().ok_or(CubeError::Value)?)?;
        }
        if parts.next().is_some() {
            return Err(CubeError::Value);
        }
        data.extend_from_slice(&vals);
    }
    let size = size.ok_or(CubeError::Not3d)?;
    let need = (size as usize)
        .checked_pow(3)
        .and_then(|n| n.checked_mul(3))
        .ok_or(CubeError::Size)?;
    if data.len() != need {
        return Err(CubeError::Short);
    }
    for c in 0..3 {
        if !domain_min[c].is_finite() || !domain_max[c].is_finite() || domain_max[c] <= domain_min[c] {
            return Err(CubeError::Value);
        }
    }
    Ok(Lut3d {
        size,
        domain_min,
        domain_max,
        rgb: data,
    })
}

fn three<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Result<[f32; 3], CubeError> {
    let mut out = [0.0; 3];
    for slot in &mut out {
        *slot = parse_comp(parts.next().ok_or(CubeError::Value)?)?;
    }
    if parts.next().is_some() {
        return Err(CubeError::Value);
    }
    Ok(out)
}

fn parse_comp(tok: &str) -> Result<f32, CubeError> {
    let v: f32 = tok.parse().map_err(|_| CubeError::Value)?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(CubeError::Value)
    }
}

fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn sample(lut: &Lut3d, rgb: [f32; 3]) -> [f32; 3] {
    let s = lut.size;
    let max_i = (s - 1) as f32;
    let mut p = [0.0_f32; 3];
    for c in 0..3 {
        let span = lut.domain_max[c] - lut.domain_min[c];
        let t = ((rgb[c] - lut.domain_min[c]) / span).clamp(0.0, 1.0);
        p[c] = t * max_i;
    }
    let r0 = (p[0] as u32).min(s - 1);
    let g0 = (p[1] as u32).min(s - 1);
    let b0 = (p[2] as u32).min(s - 1);
    let r1 = (r0 + 1).min(s - 1);
    let g1 = (g0 + 1).min(s - 1);
    let b1 = (b0 + 1).min(s - 1);
    let fr = p[0] - r0 as f32;
    let fg = p[1] - g0 as f32;
    let fb = p[2] - b0 as f32;
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    let c00 = lerp(at(lut, r0, g0, b0), at(lut, r1, g0, b0), fr);
    let c10 = lerp(at(lut, r0, g1, b0), at(lut, r1, g1, b0), fr);
    let c01 = lerp(at(lut, r0, g0, b1), at(lut, r1, g0, b1), fr);
    let c11 = lerp(at(lut, r0, g1, b1), at(lut, r1, g1, b1), fr);
    lerp(lerp(c00, c10, fg), lerp(c01, c11, fg), fb)
}

fn at(lut: &Lut3d, r: u32, g: u32, b: u32) -> [f32; 3] {
    let s = lut.size;
    let i = ((r + s * (g + s * b)) * 3) as usize;
    [lut.rgb[i], lut.rgb[i + 1], lut.rgb[i + 2]]
}

const BAKE_N: u32 = 17;

fn bake(f: fn(f32, f32, f32) -> [f32; 3]) -> Lut3d {
    let n = BAKE_N;
    let mut rgb = Vec::with_capacity((n * n * n * 3) as usize);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let rf = r as f32 / (n - 1) as f32;
                let gf = g as f32 / (n - 1) as f32;
                let bf = b as f32 / (n - 1) as f32;
                let o = f(rf, gf, bf);
                rgb.extend_from_slice(&o);
            }
        }
    }
    Lut3d {
        size: n,
        domain_min: [0.0, 0.0, 0.0],
        domain_max: [1.0, 1.0, 1.0],
        rgb,
    }
}

fn warm(r: f32, g: f32, b: f32) -> [f32; 3] {
    [(r * 1.15).clamp(0.0, 1.0), g.clamp(0.0, 1.0), (b * 0.85).clamp(0.0, 1.0)]
}

fn tungsten(r: f32, g: f32, b: f32) -> [f32; 3] {
    [(r * 1.28).clamp(0.0, 1.0), (g * 1.04).clamp(0.0, 1.0), (b * 0.70).clamp(0.0, 1.0)]
}

fn chrome(r: f32, g: f32, b: f32) -> [f32; 3] {
    let c = |x: f32| ((x - 0.5) * 1.45 + 0.5).clamp(0.0, 1.0);
    [(c(r) * 0.96).clamp(0.0, 1.0), c(g), (c(b) * 1.06).clamp(0.0, 1.0)]
}

fn pan(r: f32, g: f32, b: f32) -> [f32; 3] {
    let y = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
    [y, y, y]
}

fn sat(r: f32, g: f32, b: f32, amount: f32) -> [f32; 3] {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let s = |c: f32| (y + (c - y) * amount).clamp(0.0, 1.0);
    [s(r), s(g), s(b)]
}

fn vivid(r: f32, g: f32, b: f32) -> [f32; 3] {
    sat(r, g, b, 1.55)
}

fn snap(r: f32, g: f32, b: f32) -> [f32; 3] {
    let c = |x: f32| ((x - 0.5) * 1.3 + 0.5).clamp(0.0, 1.0);
    sat(c(r), c(g), c(b), 1.35)
}
