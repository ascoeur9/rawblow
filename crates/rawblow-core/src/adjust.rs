//! 폴더 공통 표시 조정(#130).
//!
//! 디코드된 RGBA8에 노출·색온도·대비·그레인을 입힌다. 네 값이 모두 0이거나
//! 비교 키를 누르고 있으면 입력 바이트를 그대로 돌려준다. 알파와 4바이트에
//! 못 미치는 꼬리는 건드리지 않는다.
//!
//! 적용 순서: 노출(EV 게인) → 색온도(양수면 R을 올리고 B를 내림) → 대비(0.5 기준) → 그레인.
//! 그레인은 픽셀 인덱스로 정해지는 값이라 같은 버퍼는 항상 같은 결과가 된다.

use serde::{Deserialize, Serialize};

/// 폴더에 공통으로 거는 표시 조정. 사진마다 다른 값은 두지 않는다.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Adjust {
    /// −2~+2 EV. 양수는 밝게.
    pub exposure: f32,
    /// −1~+1. 양수는 따뜻하게(R↑ B↓), 음수는 차갑게.
    pub temperature: f32,
    /// −1~+1. 0.5를 축으로 벌리거나 모은다.
    pub contrast: f32,
    /// 0~1. 0이면 그레인 없음.
    pub grain: f32,
}

impl Default for Adjust {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            temperature: 0.0,
            contrast: 0.0,
            grain: 0.0,
        }
    }
}

impl Adjust {
    /// 네 값이 모두 정확히 0이면 픽셀을 바꾸지 않는다.
    pub fn is_neutral(self) -> bool {
        self.exposure == 0.0
            && self.temperature == 0.0
            && self.contrast == 0.0
            && self.grain == 0.0
    }

    /// 슬라이더 범위 밖·비정상 값은 범위 안으로 모은다. 비정상은 0.
    pub fn clamped(self) -> Self {
        Self {
            exposure: finite(self.exposure, -2.0, 2.0),
            temperature: finite(self.temperature, -1.0, 1.0),
            contrast: finite(self.contrast, -1.0, 1.0),
            grain: finite(self.grain, 0.0, 1.0),
        }
    }
}

/// 화면에 올릴 RGBA. `compare_held`이거나 조정이 0이면 `src`와 바이트가 같다.
pub fn display_rgba(src: &[u8], adj: Adjust, compare_held: bool) -> Vec<u8> {
    if compare_held || adj.is_neutral() {
        return src.to_vec();
    }
    let adj = adj.clamped();
    if adj.is_neutral() {
        return src.to_vec();
    }
    let mut out = src.to_vec();
    apply_pixels(&mut out, adj);
    out
}

fn finite(x: f32, lo: f32, hi: f32) -> f32 {
    if x.is_finite() {
        x.clamp(lo, hi)
    } else {
        0.0
    }
}

fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 그레인 진폭(0..1 공간). 1일 때 약 ±0.12.
const GRAIN_AMP: f32 = 0.12;

fn grain_signed(pixel: u32) -> f32 {
    let mut x = pixel.wrapping_mul(0x9E37_79B1).wrapping_add(0xA5A5_A5A5);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    let u = (x & 0xFFFF) as f32 / 65535.0;
    u * 2.0 - 1.0
}

fn apply_pixels(buf: &mut [u8], adj: Adjust) {
    let exp = finite(adj.exposure, -2.0, 2.0);
    let temp = finite(adj.temperature, -1.0, 1.0);
    let contrast = finite(adj.contrast, -1.0, 1.0);
    let grain = finite(adj.grain, 0.0, 1.0);
    let gain = 2f32.powf(exp);
    let rk = 1.0 + 0.25 * temp;
    let bk = 1.0 - 0.25 * temp;
    let ck = 1.0 + contrast;
    let n = buf.len() / 4;
    for i in 0..n {
        let o = i * 4;
        let mut r = buf[o] as f32 / 255.0 * gain * rk;
        let mut g = buf[o + 1] as f32 / 255.0 * gain;
        let mut b = buf[o + 2] as f32 / 255.0 * gain * bk;
        r = (r - 0.5) * ck + 0.5;
        g = (g - 0.5) * ck + 0.5;
        b = (b - 0.5) * ck + 0.5;
        if grain > 0.0 {
            let off = grain_signed(i as u32) * grain * GRAIN_AMP;
            r += off;
            g += off;
            b += off;
        }
        buf[o] = to_u8(r);
        buf[o + 1] = to_u8(g);
        buf[o + 2] = to_u8(b);
    }
}
