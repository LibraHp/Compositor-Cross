//! Layer effects stored in a `.comp` manifest and drawn on the CPU.
//!
//! Sizes are document pixels. Shadow angle follows the macOS app: 90° is light
//! from above, so the shadow falls straight down.

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct StrokeEffect {
    pub enabled: bool,
    pub size: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub inside: bool,
}

#[derive(Clone, Debug)]
pub struct ShadowEffect {
    pub enabled: bool,
    pub angle: f32,
    pub distance: f32,
    pub blur: f32,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct OverlayEffect {
    pub enabled: bool,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct GlowEffect {
    pub enabled: bool,
    pub size: f32,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug, Default)]
pub struct LayerEffects {
    pub stroke: Option<StrokeEffect>,
    pub shadow: Option<ShadowEffect>,
    pub color_overlay: Option<OverlayEffect>,
    pub inner_shadow: Option<ShadowEffect>,
    pub outer_glow: Option<GlowEffect>,
    pub inner_glow: Option<GlowEffect>,
}

impl LayerEffects {
    pub fn is_empty(&self) -> bool {
        self.stroke.is_none()
            && self.shadow.is_none()
            && self.color_overlay.is_none()
            && self.inner_shadow.is_none()
            && self.outer_glow.is_none()
            && self.inner_glow.is_none()
    }

    pub fn from_value(value: &Value) -> Self {
        let Some(obj) = value.as_object() else {
            return Self::default();
        };
        Self {
            stroke: obj.get("stroke").and_then(parse_stroke),
            shadow: obj.get("shadow").and_then(parse_shadow),
            color_overlay: obj.get("colorOverlay").and_then(parse_overlay),
            inner_shadow: obj.get("innerShadow").and_then(parse_shadow),
            outer_glow: obj.get("outerGlow").and_then(parse_glow),
            inner_glow: obj.get("innerGlow").and_then(parse_glow),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut obj = serde_json::Map::new();
        if let Some(effect) = &self.stroke {
            obj.insert("stroke".into(), stroke_value(effect));
        }
        if let Some(effect) = &self.shadow {
            obj.insert("shadow".into(), shadow_value(effect));
        }
        if let Some(effect) = &self.color_overlay {
            obj.insert("colorOverlay".into(), overlay_value(effect));
        }
        if let Some(effect) = &self.inner_shadow {
            obj.insert("innerShadow".into(), shadow_value(effect));
        }
        if let Some(effect) = &self.outer_glow {
            obj.insert("outerGlow".into(), glow_value(effect));
        }
        if let Some(effect) = &self.inner_glow {
            obj.insert("innerGlow".into(), glow_value(effect));
        }
        Value::Object(obj)
    }

    pub fn margin(&self) -> f32 {
        let mut margin = 0.0f32;
        if let Some(effect) = &self.shadow {
            if effect.enabled {
                margin = margin.max(effect.distance + effect.blur);
            }
        }
        if let Some(effect) = &self.outer_glow {
            if effect.enabled {
                margin = margin.max(effect.size);
            }
        }
        if let Some(effect) = &self.stroke {
            if effect.enabled && !effect.inside {
                margin = margin.max(effect.size);
            }
        }
        margin
    }
}

impl ShadowEffect {
    pub fn offset(self_angle: f32, distance: f32) -> (f32, f32) {
        let rad = self_angle.to_radians();
        (-rad.cos() * distance, rad.sin() * distance)
    }
}

fn enabled(value: &Value) -> bool {
    value.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

fn color_of(value: &Value) -> [f32; 3] {
    [
        value.get("red").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        value.get("green").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        value.get("blue").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    ]
}

fn parse_stroke(value: &Value) -> Option<StrokeEffect> {
    Some(StrokeEffect {
        enabled: enabled(value),
        size: value.get("size").and_then(|v| v.as_f64()).unwrap_or(4.0) as f32,
        color: color_of(value),
        opacity: value.get("opacity").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        inside: value.get("inside").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

fn parse_shadow(value: &Value) -> Option<ShadowEffect> {
    Some(ShadowEffect {
        enabled: enabled(value),
        angle: value.get("angle").and_then(|v| v.as_f64()).unwrap_or(90.0) as f32,
        distance: value.get("distance").and_then(|v| v.as_f64()).unwrap_or(12.0) as f32,
        blur: value.get("blur").and_then(|v| v.as_f64()).unwrap_or(8.0) as f32,
        color: color_of(value),
        opacity: value.get("opacity").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
    })
}

fn parse_overlay(value: &Value) -> Option<OverlayEffect> {
    Some(OverlayEffect {
        enabled: enabled(value),
        color: color_of(value),
        opacity: value.get("opacity").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
    })
}

fn parse_glow(value: &Value) -> Option<GlowEffect> {
    Some(GlowEffect {
        enabled: enabled(value),
        size: value.get("size").and_then(|v| v.as_f64()).unwrap_or(16.0) as f32,
        color: color_of(value),
        opacity: value.get("opacity").and_then(|v| v.as_f64()).unwrap_or(0.75) as f32,
    })
}

fn color_value(color: [f32; 3]) -> Value {
    json!({ "red": color[0], "green": color[1], "blue": color[2] })
}

fn stroke_value(effect: &StrokeEffect) -> Value {
    json!({
        "enabled": effect.enabled,
        "size": effect.size,
        "red": effect.color[0],
        "green": effect.color[1],
        "blue": effect.color[2],
        "opacity": effect.opacity,
        "inside": effect.inside,
    })
}

fn shadow_value(effect: &ShadowEffect) -> Value {
    json!({
        "enabled": effect.enabled,
        "angle": effect.angle,
        "distance": effect.distance,
        "blur": effect.blur,
        "red": effect.color[0],
        "green": effect.color[1],
        "blue": effect.color[2],
        "opacity": effect.opacity,
    })
}

fn overlay_value(effect: &OverlayEffect) -> Value {
    let mut value = color_value(effect.color);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("enabled".into(), json!(effect.enabled));
        obj.insert("opacity".into(), json!(effect.opacity));
    }
    value
}

fn glow_value(effect: &GlowEffect) -> Value {
    json!({
        "enabled": effect.enabled,
        "size": effect.size,
        "red": effect.color[0],
        "green": effect.color[1],
        "blue": effect.color[2],
        "opacity": effect.opacity,
    })
}

fn soft_alpha(sample: &impl Fn(f32, f32) -> f32, x: f32, y: f32, radius: f32) -> f32 {
    if radius < 0.8 {
        return sample(x, y);
    }
    let radius = radius.min(48.0);
    let mut acc = sample(x, y) * 6.0;
    let mut weight = 6.0;
    for ring in 1..=4 {
        let t = ring as f32 / 4.0;
        let falloff = (1.0 - t * t).max(0.08);
        let rad = radius * t;
        for i in 0..10 {
            let angle = (i as f32 + ring as f32 * 0.37) / 10.0 * std::f32::consts::TAU;
            acc += sample(x + angle.cos() * rad, y + angle.sin() * rad) * falloff;
            weight += falloff;
        }
    }
    acc / weight
}

fn dilate_alpha(sample: &impl Fn(f32, f32) -> f32, x: f32, y: f32, radius: f32) -> f32 {
    let mut max_a = sample(x, y);
    if radius < 0.5 {
        return max_a;
    }
    let radius = radius.min(64.0);
    for ring in [0.55, 1.0] {
        for i in 0..16 {
            let angle = i as f32 / 16.0 * std::f32::consts::TAU;
            max_a = max_a.max(sample(x + angle.cos() * radius * ring, y + angle.sin() * radius * ring));
        }
    }
    max_a
}

/// Color drawn behind the layer: outer glow, then drop shadow.
pub fn behind(effects: &LayerEffects, sample: &impl Fn(f32, f32) -> f32, x: f32, y: f32) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    if let Some(glow) = &effects.outer_glow {
        if glow.enabled && glow.opacity > 0.0 {
            let halo = soft_alpha(sample, x, y, glow.size.max(0.5));
            let a = (halo * glow.opacity).clamp(0.0, 1.0);
            out = mix_over(out, [glow.color[0], glow.color[1], glow.color[2], a]);
        }
    }
    if let Some(shadow) = &effects.shadow {
        if shadow.enabled && shadow.opacity > 0.0 {
            let (ox, oy) = ShadowEffect::offset(shadow.angle, shadow.distance);
            let a = soft_alpha(sample, x - ox, y - oy, shadow.blur) * shadow.opacity;
            out = mix_over(out, [shadow.color[0], shadow.color[1], shadow.color[2], a.clamp(0.0, 1.0)]);
        }
    }
    out
}

/// Outside stroke, drawn after the layer so it sits on the silhouette.
pub fn outside_stroke(effects: &LayerEffects, sample: &impl Fn(f32, f32) -> f32, x: f32, y: f32) -> [f32; 4] {
    let Some(stroke) = &effects.stroke else {
        return [0.0; 4];
    };
    if !stroke.enabled || stroke.inside || stroke.opacity <= 0.0 || stroke.size <= 0.0 {
        return [0.0; 4];
    }
    let here = sample(x, y);
    let dilated = dilate_alpha(sample, x, y, stroke.size);
    let a = ((dilated - here).max(0.0) * stroke.opacity).clamp(0.0, 1.0);
    [stroke.color[0], stroke.color[1], stroke.color[2], a]
}

/// Color overlay, inner shadow, inner glow and inside stroke, applied to the layer sample.
pub fn decorate(effects: &LayerEffects, mut color: [f32; 4], sample: &impl Fn(f32, f32) -> f32, x: f32, y: f32) -> [f32; 4] {
    if color[3] <= 0.0 {
        return color;
    }
    if let Some(overlay) = &effects.color_overlay {
        if overlay.enabled && overlay.opacity > 0.0 {
            let t = overlay.opacity.clamp(0.0, 1.0);
            for i in 0..3 {
                color[i] = color[i] + (overlay.color[i] - color[i]) * t;
            }
        }
    }
    if let Some(shadow) = &effects.inner_shadow {
        if shadow.enabled && shadow.opacity > 0.0 {
            let (ox, oy) = ShadowEffect::offset(shadow.angle, shadow.distance);
            let hole = 1.0 - soft_alpha(sample, x - ox, y - oy, shadow.blur);
            let a = (hole * shadow.opacity * color[3]).clamp(0.0, 1.0);
            color = mix_over(color, [shadow.color[0], shadow.color[1], shadow.color[2], a]);
        }
    }
    if let Some(glow) = &effects.inner_glow {
        if glow.enabled && glow.opacity > 0.0 {
            let interior = soft_alpha(sample, x, y, glow.size.max(0.5));
            let edge = (1.0 - interior).max(0.0);
            let a = (edge * glow.opacity * color[3]).clamp(0.0, 1.0);
            color = mix_over(color, [glow.color[0], glow.color[1], glow.color[2], a]);
        }
    }
    if let Some(stroke) = &effects.stroke {
        if stroke.enabled && stroke.inside && stroke.opacity > 0.0 && stroke.size > 0.0 {
            let eroded = 1.0 - dilate_alpha(&|px, py| 1.0 - sample(px, py), x, y, stroke.size);
            let ring = (color[3] - eroded.max(0.0)).max(0.0);
            let a = (ring * stroke.opacity).clamp(0.0, 1.0);
            color = mix_over(color, [stroke.color[0], stroke.color[1], stroke.color[2], a]);
        }
    }
    color
}

fn mix_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let src_a = src[3].clamp(0.0, 1.0);
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = dst[3].clamp(0.0, 1.0);
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 1e-5 {
        return [0.0; 4];
    }
    let inv = 1.0 - src_a;
    [
        (src[0] * src_a + dst[0] * dst_a * inv) / out_a,
        (src[1] * src_a + dst[1] * dst_a * inv) / out_a,
        (src[2] * src_a + dst[2] * dst_a * inv) / out_a,
        out_a,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_at_ninety_falls_down() {
        let (x, y) = ShadowEffect::offset(90.0, 20.0);
        assert!(x.abs() < 1e-3, "{x}");
        assert!((y - 20.0).abs() < 1e-3, "{y}");
    }

    #[test]
    fn effects_json_roundtrip() {
        let effects = LayerEffects {
            shadow: Some(ShadowEffect {
                enabled: true,
                angle: 90.0,
                distance: 12.0,
                blur: 4.0,
                color: [0.0, 0.0, 0.0],
                opacity: 0.4,
            }),
            ..LayerEffects::default()
        };
        let parsed = LayerEffects::from_value(&effects.to_value());
        assert!((parsed.shadow.unwrap().distance - 12.0).abs() < 1e-3);
    }
}
