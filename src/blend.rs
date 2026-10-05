//! Photoshop-style separable blend modes.
//!
//! Colors are straight (unassociated) RGBA in 0–1. The composite formula is the
//! PDF/SVG one: the blend function runs on the color channels, then the result
//! is source-over composited with both alphas.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    #[serde(rename = "Normal")]
    Normal,
    #[serde(rename = "Darken")]
    Darken,
    #[serde(rename = "Multiply")]
    Multiply,
    #[serde(rename = "Color Burn")]
    ColorBurn,
    #[serde(rename = "Linear Burn")]
    LinearBurn,
    #[serde(rename = "Lighten")]
    Lighten,
    #[serde(rename = "Screen")]
    Screen,
    #[serde(rename = "Color Dodge")]
    ColorDodge,
    #[serde(rename = "Linear Dodge (Add)")]
    LinearDodge,
    #[serde(rename = "Overlay")]
    Overlay,
    #[serde(rename = "Soft Light")]
    SoftLight,
    #[serde(rename = "Hard Light")]
    HardLight,
    #[serde(rename = "Vivid Light")]
    VividLight,
    #[serde(rename = "Linear Light")]
    LinearLight,
    #[serde(rename = "Pin Light")]
    PinLight,
    #[serde(rename = "Hard Mix")]
    HardMix,
    #[serde(rename = "Difference")]
    Difference,
    #[serde(rename = "Exclusion")]
    Exclusion,
    #[serde(rename = "Subtract")]
    Subtract,
    #[serde(rename = "Divide")]
    Divide,
    #[serde(rename = "Hue")]
    Hue,
    #[serde(rename = "Saturation")]
    Saturation,
    #[serde(rename = "Color")]
    Color,
    #[serde(rename = "Luminosity")]
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 24] = [
        Self::Normal,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Darken => "Darken",
            Self::Multiply => "Multiply",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::Lighten => "Lighten",
            Self::Screen => "Screen",
            Self::ColorDodge => "Color Dodge",
            Self::LinearDodge => "Linear Dodge (Add)",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::HardLight => "Hard Light",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Subtract => "Subtract",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == name)
    }

    pub fn label(self, zh: bool) -> &'static str {
        if !zh {
            return self.as_str();
        }
        match self {
            Self::Normal => "正常",
            Self::Darken => "变暗",
            Self::Multiply => "正片叠底",
            Self::ColorBurn => "颜色加深",
            Self::LinearBurn => "线性加深",
            Self::Lighten => "变亮",
            Self::Screen => "滤色",
            Self::ColorDodge => "颜色减淡",
            Self::LinearDodge => "线性减淡 (添加)",
            Self::Overlay => "叠加",
            Self::SoftLight => "柔光",
            Self::HardLight => "强光",
            Self::VividLight => "亮光",
            Self::LinearLight => "线性光",
            Self::PinLight => "点光",
            Self::HardMix => "实色混合",
            Self::Difference => "差值",
            Self::Exclusion => "排除",
            Self::Subtract => "减去",
            Self::Divide => "划分",
            Self::Hue => "色相",
            Self::Saturation => "饱和度",
            Self::Color => "颜色",
            Self::Luminosity => "明度",
        }
    }
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Normal
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cs >= 1.0 {
        1.0
    } else {
        clamp01(cb / (1.0 - cs))
    }
}

fn color_burn(cb: f32, cs: f32) -> f32 {
    if cs <= 0.0 {
        0.0
    } else {
        1.0 - clamp01((1.0 - cb) / cs)
    }
}

fn vivid_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        color_burn(cb, (cs * 2.0).clamp(0.0, 1.0))
    } else {
        color_dodge(cb, (cs * 2.0 - 1.0).clamp(0.0, 1.0))
    }
}

fn blend_channel(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::Normal | BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => cs,
        BlendMode::Darken => cb.min(cs),
        BlendMode::Multiply => cb * cs,
        BlendMode::ColorBurn => color_burn(cb, cs),
        BlendMode::LinearBurn => (cb + cs - 1.0).max(0.0),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Screen => 1.0 - (1.0 - cb) * (1.0 - cs),
        BlendMode::ColorDodge => color_dodge(cb, cs),
        BlendMode::LinearDodge => (cb + cs).min(1.0),
        BlendMode::Overlay => {
            if cb <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::HardLight => {
            if cs <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BlendMode::VividLight => vivid_light(cb, cs),
        BlendMode::LinearLight => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
        BlendMode::PinLight => {
            if cs > 0.5 {
                cb.max(2.0 * cs - 1.0)
            } else {
                cb.min(2.0 * cs)
            }
        }
        BlendMode::HardMix => {
            if vivid_light(cb, cs) >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::Divide => {
            if cs <= 1e-6 {
                1.0
            } else {
                (cb / cs).min(1.0)
            }
        }
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 && l != n {
        let scale = l / (l - n);
        for ch in &mut c {
            *ch = l + (*ch - l) * scale;
        }
    }
    if x > 1.0 && x != l {
        let scale = (1.0 - l) / (x - l);
        for ch in &mut c {
            *ch = l + (*ch - l) * scale;
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap_or(std::cmp::Ordering::Equal));
    let (min_i, mid_i, max_i) = (idx[0], idx[1], idx[2]);
    let mut out = c;
    if (c[max_i] - c[min_i]).abs() < 1e-6 {
        out[mid_i] = 0.0;
        out[max_i] = 0.0;
        out[min_i] = 0.0;
    } else {
        out[mid_i] = (c[mid_i] - c[min_i]) * s / (c[max_i] - c[min_i]);
        out[max_i] = s;
        out[min_i] = 0.0;
    }
    out
}

fn blend_rgb(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        other => [
            blend_channel(other, cb[0], cs[0]),
            blend_channel(other, cb[1], cs[1]),
            blend_channel(other, cb[2], cs[2]),
        ],
    }
}

/// `src` and `dst` are straight RGBA, 0–1. Returns straight RGBA.
pub fn composite(mode: BlendMode, dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let src_a = clamp01(src[3]);
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = clamp01(dst[3]);
    let blended = blend_rgb(mode, [dst[0], dst[1], dst[2]], [src[0], src[1], src[2]]);
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 1e-6 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let inv = 1.0 - src_a;
    let mix = |b: f32, s: f32, blended: f32| {
        (src_a * (1.0 - dst_a) * s + src_a * dst_a * blended + inv * dst_a * b) / out_a
    };
    [
        clamp01(mix(dst[0], src[0], blended[0])),
        clamp01(mix(dst[1], src[1], blended[1])),
        clamp01(mix(dst[2], src[2], blended[2])),
        clamp01(out_a),
    ]
}

pub fn u8_to_f(px: &[u8]) -> [f32; 4] {
    [
        px[0] as f32 / 255.0,
        px[1] as f32 / 255.0,
        px[2] as f32 / 255.0,
        px[3] as f32 / 255.0,
    ]
}

pub fn f_to_u8(px: [f32; 4]) -> [u8; 4] {
    [
        (clamp01(px[0]) * 255.0).round() as u8,
        (clamp01(px[1]) * 255.0).round() as u8,
        (clamp01(px[2]) * 255.0).round() as u8,
        (clamp01(px[3]) * 255.0).round() as u8,
    ]
}

/// Blend `src` over the straight RGBA pixel at `dst`.
pub fn blend_pixel(dst: &mut [u8], src: [f32; 4], mode: BlendMode) {
    let out = composite(mode, u8_to_f(dst), src);
    let bytes = f_to_u8(out);
    dst[..4].copy_from_slice(&bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: [f32; 4], b: [f32; 4]) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 1e-4, "{a:?} != {b:?} at {i}");
        }
    }

    #[test]
    fn normal_half_white_over_black() {
        let out = composite(
            BlendMode::Normal,
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 0.5],
        );
        approx(out, [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn multiply_grey() {
        let out = composite(
            BlendMode::Multiply,
            [0.5, 0.5, 0.5, 1.0],
            [0.5, 0.5, 0.5, 1.0],
        );
        approx(out, [0.25, 0.25, 0.25, 1.0]);
    }

    #[test]
    fn screen_white() {
        let out = composite(BlendMode::Screen, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        approx(out, [1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn names_roundtrip() {
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::parse(mode.as_str()), Some(mode));
        }
    }
}
