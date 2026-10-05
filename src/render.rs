//! CPU compositor. A destination pixel maps to a document point, samples each
//! visible layer, and blends bottom to top. Point adjustments run on the
//! buffer already drawn; blur adjustments run on that same buffer inside the
//! layer's bounds.

use rayon::prelude::*;

use crate::blend::{blend_pixel, BlendMode};
use crate::document::{Adjustment, Document, Layer, LevelRange};
use crate::effects;
use crate::raster::{byte_to_f, f_to_byte, Raster};

pub struct Composite {
    pub image: Raster,
    /// Destination pixels per document pixel.
    pub scale: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

pub fn preview_scale(width: u32, height: u32) -> f32 {
    let longest = width.max(height).max(1) as f32;
    (2048.0 / longest).min(1.0)
}

pub fn composite_document(doc: &Document, scale: f32) -> Composite {
    let scale = scale.clamp(0.02, 4.0);
    let w = (doc.width as f32 * scale).round().max(1.0) as u32;
    let h = (doc.height as f32 * scale).round().max(1.0) as u32;
    Composite {
        image: composite_region(doc, 0.0, 0.0, scale, w, h),
        scale,
        origin_x: 0.0,
        origin_y: 0.0,
    }
}

pub fn composite_region(
    doc: &Document,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
    width: u32,
    height: u32,
) -> Raster {
    let mut image = Raster::new(width, height, [0, 0, 0, 0]);
    composite_into(doc, &mut image, origin_x, origin_y, scale, None);
    image
}

/// Repaint `clip` (x, y, w, h in destination pixels) of an existing preview.
pub fn composite_into(
    doc: &Document,
    dest: &mut Raster,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
    clip: Option<(u32, u32, u32, u32)>,
) {
    let (cx, cy, cw, ch) = clip.unwrap_or((0, 0, dest.width, dest.height));
    let x1 = cx.min(dest.width);
    let y1 = cy.min(dest.height);
    let x2 = (cx + cw).min(dest.width);
    let y2 = (cy + ch).min(dest.height);
    if x1 >= x2 || y1 >= y2 {
        return;
    }
    clear_rect(dest, x1, y1, x2, y2);
    for id in doc.paint_order() {
        if !doc.effectively_visible(id) {
            continue;
        }
        let Some(layer) = doc.layer(id) else {
            continue;
        };
        if let Some(adjustment) = layer.adjustment.clone() {
            apply_adjustment(doc, layer, &adjustment, dest, origin_x, origin_y, scale, x1, y1, x2, y2);
        } else if layer.image.is_some() {
            blend_layer(doc, layer, dest, origin_x, origin_y, scale, x1, y1, x2, y2);
        }
    }
}

fn clear_rect(dest: &mut Raster, x1: u32, y1: u32, x2: u32, y2: u32) {
    let w = dest.width as usize;
    let pixels = dest.pixels_mut();
    for y in y1..y2 {
        let start = (y as usize * w + x1 as usize) * 4;
        let end = (y as usize * w + x2 as usize) * 4;
        pixels[start..end].fill(0);
    }
}

fn blend_layer(
    doc: &Document,
    layer: &Layer,
    dest: &mut Raster,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
) {
    let Some(image) = layer.image.as_ref() else {
        return;
    };
    let opacity = doc.effective_opacity(layer.id);
    if opacity <= 0.0 {
        return;
    }
    let mode = if layer.is_group {
        BlendMode::Normal
    } else {
        layer.blend_mode
    };
    let span = 1.0 / scale.max(0.02);
    let w = dest.width;
    let effects = layer.effects.clone();
    let has_effects = !effects.is_empty();
    let (min_x, min_y, max_x, max_y) = layer.transform.axis_bounds();
    let margin = effects.margin() + 2.0;
    let pixels = dest.pixels_mut();
    let row_bytes = w as usize * 4;
    pixels
        .par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(y, row)| {
            if (y as u32) < y1 || (y as u32) >= y2 {
                return;
            }
            for x in x1..x2 {
                let doc_x = origin_x + (x as f32 + 0.5) / scale;
                let doc_y = origin_y + (y as f32 + 0.5) / scale;
                if doc_x < min_x - margin || doc_x > max_x + margin || doc_y < min_y - margin || doc_y > max_y + margin {
                    continue;
                }
                let i = x as usize * 4;
                if has_effects {
                    let mut behind = effects::behind(&effects, &|px, py| layer_alpha(layer, image, px, py), doc_x, doc_y);
                    behind[3] *= opacity;
                    if behind[3] > 0.004 {
                        blend_pixel(&mut row[i..i + 4], behind, BlendMode::Normal);
                    }
                }
                let mut color = sample_layer(doc, layer, image, doc_x, doc_y, span, opacity);
                if has_effects && color[3] > 0.0 {
                    color = effects::decorate(&effects, color, &|px, py| layer_alpha(layer, image, px, py), doc_x, doc_y);
                }
                if color[3] > 0.004 {
                    blend_pixel(&mut row[i..i + 4], color, mode);
                }
                if has_effects {
                    let mut stroke = effects::outside_stroke(&effects, &|px, py| layer_alpha(layer, image, px, py), doc_x, doc_y);
                    stroke[3] *= opacity;
                    if stroke[3] > 0.004 {
                        blend_pixel(&mut row[i..i + 4], stroke, BlendMode::Normal);
                    }
                }
            }
        });
}

fn layer_alpha(layer: &Layer, image: &Raster, doc_x: f32, doc_y: f32) -> f32 {
    let (u, v) = layer.transform.doc_to_unit(doc_x, doc_y);
    if !(u >= -0.02 && v >= -0.02 && u <= 1.02 && v <= 1.02) {
        return 0.0;
    }
    let sample = image.sample(u * image.width as f32, v * image.height as f32);
    sample[3] * mask_coverage(layer, doc_x, doc_y)
}

fn sample_layer(
    doc: &Document,
    layer: &Layer,
    image: &Raster,
    doc_x: f32,
    doc_y: f32,
    span: f32,
    opacity: f32,
) -> [f32; 4] {
    let (u, v) = layer.transform.doc_to_unit(doc_x, doc_y);
    if !(u >= -0.01 && v >= -0.01 && u <= 1.01 && v <= 1.01) {
        return [0.0; 4];
    }
    let mut color = if span > 1.6 && layer.transform.sampling != crate::document::Sampling::Nearest {
        let du = span / layer.transform.width.max(1.0);
        let dv = span / layer.transform.height.max(1.0);
        let mut acc = [0.0; 4];
        let steps = if span > 3.0 { 3 } else { 2 };
        let n = steps * steps;
        for iy in 0..steps {
            for ix in 0..steps {
                let uu = u + (ix as f32 - (steps as f32 - 1.0) / 2.0) * du;
                let vv = v + (iy as f32 - (steps as f32 - 1.0) / 2.0) * dv;
                let s = image.sample(uu * image.width as f32, vv * image.height as f32);
                for c in 0..4 {
                    acc[c] += s[c];
                }
            }
        }
        acc.map(|c| c / n as f32)
    } else if layer.transform.sampling == crate::document::Sampling::Nearest {
        let px = (u * image.width as f32).floor().clamp(0.0, image.width as f32 - 1.0) as i32;
        let py = (v * image.height as f32).floor().clamp(0.0, image.height as f32 - 1.0) as i32;
        byte_to_f(image.pixel(px, py))
    } else {
        image.sample(u * image.width as f32, v * image.height as f32)
    };
    if color[3] <= 0.0 {
        return [0.0; 4];
    }
    color[3] *= mask_coverage(layer, doc_x, doc_y);
    color[3] *= ancestor_mask(doc, layer.id, doc_x, doc_y);
    color[3] *= clip_coverage(doc, layer, doc_x, doc_y);
    color[3] *= opacity;
    color
}

fn mask_coverage(layer: &Layer, doc_x: f32, doc_y: f32) -> f32 {
    let Some(mask) = layer.mask.as_ref() else {
        return 1.0;
    };
    if !layer.mask_enabled {
        return 1.0;
    }
    let place = if layer.mask_linked {
        layer.transform
    } else {
        layer.mask_placement.unwrap_or(layer.transform)
    };
    let (u, v) = place.doc_to_unit(doc_x, doc_y);
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return 0.0;
    }
    mask.sample_coverage(u, v)
}

fn ancestor_mask(doc: &Document, id: uuid::Uuid, doc_x: f32, doc_y: f32) -> f32 {
    let mut coverage = 1.0;
    for parent in doc.ancestor_ids(id) {
        if let Some(group) = doc.layer(parent) {
            coverage *= mask_coverage(group, doc_x, doc_y);
        }
    }
    coverage
}

fn clip_coverage(doc: &Document, layer: &Layer, doc_x: f32, doc_y: f32) -> f32 {
    let Some(source_id) = layer.mask_source_id else {
        return 1.0;
    };
    let Some(source) = doc.layer(source_id) else {
        return 1.0;
    };
    let Some(image) = source.image.as_ref() else {
        return 0.0;
    };
    let (u, v) = source.transform.doc_to_unit(doc_x, doc_y);
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return 0.0;
    }
    let sample = image.sample(u * image.width as f32, v * image.height as f32);
    sample[3] * mask_coverage(source, doc_x, doc_y) * source.opacity.clamp(0.0, 1.0)
}

fn apply_adjustment(
    doc: &Document,
    layer: &Layer,
    adjustment: &Adjustment,
    dest: &mut Raster,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
) {
    if matches!(adjustment, Adjustment::Other(_)) {
        return;
    }
    let opacity = doc.effective_opacity(layer.id);
    if opacity <= 0.0 {
        return;
    }
    if let Adjustment::GaussianBlur { radius } = adjustment {
        let grow = (*radius * scale).ceil() as u32;
        blur_region(dest, (*radius * scale).max(0.5), x1.saturating_sub(grow), y1.saturating_sub(grow), (x2 + grow).min(dest.width), (y2 + grow).min(dest.height), |x, y| {
            cover_expanded(doc, layer, origin_x, origin_y, scale, opacity, x, y, *radius)
        });
        return;
    }
    if let Adjustment::MotionBlur { angle, distance } = adjustment {
        motion_blur(dest, *angle, *distance * scale, x1, y1, x2, y2, |x, y| {
            cover_at(doc, layer, origin_x, origin_y, scale, opacity, x, y)
        });
        return;
    }
    let w = dest.width as usize;
    let pixels = dest.pixels_mut();
    for y in y1..y2 {
        for x in x1..x2 {
            let doc_x = origin_x + (x as f32 + 0.5) / scale;
            let doc_y = origin_y + (y as f32 + 0.5) / scale;
            if !layer.transform.contains(doc_x, doc_y) {
                continue;
            }
            let cover = mask_coverage(layer, doc_x, doc_y)
                * ancestor_mask(doc, layer.id, doc_x, doc_y)
                * opacity;
            if cover <= 0.0 {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            let mut px = byte_to_f([pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]);
            if px[3] <= 0.0 {
                continue;
            }
            let adjusted = adjust_at(adjustment, px, doc_x, doc_y);
            px[0] = px[0] + (adjusted[0] - px[0]) * cover;
            px[1] = px[1] + (adjusted[1] - px[1]) * cover;
            px[2] = px[2] + (adjusted[2] - px[2]) * cover;
            let bytes = f_to_byte(px);
            pixels[i..i + 4].copy_from_slice(&bytes);
        }
    }
}

fn adjust_color(adjustment: &Adjustment, px: [f32; 4]) -> [f32; 4] {
    let mut rgb = [px[0], px[1], px[2]];
    match adjustment {
        Adjustment::Invert => {
            rgb = [1.0 - rgb[0], 1.0 - rgb[1], 1.0 - rgb[2]];
        }
        Adjustment::HueSat {
            hue,
            saturation,
            lightness,
        } => {
            rgb = apply_hsl(rgb, *hue, *saturation, *lightness);
        }
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            for c in &mut rgb {
                let mut v = *c;
                v = (v * 2.0f32.powf(*exposure) + *offset).clamp(0.0, 1.0);
                if (*gamma - 1.0).abs() > 1e-3 {
                    v = v.powf(1.0 / gamma.clamp(0.01, 9.99));
                }
                *c = v.clamp(0.0, 1.0);
            }
        }
        Adjustment::Levels { ranges } => {
            for (i, c) in rgb.iter_mut().enumerate() {
                *c = ranges[0].apply(ranges[i + 1].apply(*c));
            }
        }
        Adjustment::BlackWhite { weights } => {
            rgb = crate::ops::black_white(rgb, *weights);
        }
        Adjustment::Curves { channels } => {
            for (i, channel) in rgb.iter_mut().enumerate() {
                let mapped = curve_map(&channels[i + 1], *channel * 255.0) / 255.0;
                *channel = curve_map(&channels[0], mapped * 255.0) / 255.0;
            }
        }
        Adjustment::GradientMap { shadow, highlight, reversed } => {
            let y = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]).clamp(0.0, 1.0);
            let t = if *reversed { 1.0 - y } else { y };
            rgb = [
                shadow[0] + (highlight[0] - shadow[0]) * t,
                shadow[1] + (highlight[1] - shadow[1]) * t,
                shadow[2] + (highlight[2] - shadow[2]) * t,
            ];
        }
        Adjustment::Grain { .. } | Adjustment::ColorBalance { .. } | Adjustment::GaussianBlur { .. } | Adjustment::MotionBlur { .. } | Adjustment::Other(_) => {}
    }
    [rgb[0], rgb[1], rgb[2], px[3]]
}

/// Bake a point adjustment into layer pixels. Blur adjustments are skipped.
pub fn bake_adjustment(image: &mut Raster, adjustment: &Adjustment) {
    if matches!(
        adjustment,
        Adjustment::Other(_) | Adjustment::GaussianBlur { .. } | Adjustment::MotionBlur { .. } | Adjustment::Grain { .. }
    ) {
        return;
    }
    for px in image.pixels_mut().chunks_exact_mut(4) {
        let src = byte_to_f([px[0], px[1], px[2], px[3]]);
        if src[3] <= 0.0 {
            continue;
        }
        let bytes = f_to_byte(adjust_color(adjustment, src));
        px.copy_from_slice(&bytes);
    }
}

fn adjust_at(adjustment: &Adjustment, px: [f32; 4], doc_x: f32, doc_y: f32) -> [f32; 4] {
    match adjustment {
        Adjustment::Grain { .. } | Adjustment::ColorBalance { .. } => adjust_color_at(adjustment, px, doc_x, doc_y),
        other => adjust_color(other, px),
    }
}

fn adjust_color_at(adjustment: &Adjustment, px: [f32; 4], doc_x: f32, doc_y: f32) -> [f32; 4] {
    let mut rgb = [px[0], px[1], px[2]];
    match adjustment {
        Adjustment::Grain { amount, size, roughness, seed } => {
            rgb = grain_rgb(rgb, doc_x, doc_y, *amount, *size, *roughness, *seed);
        }
        Adjustment::ColorBalance { shadow, midtone, highlight, preserve_luminosity } => {
            rgb = color_balance(rgb, *shadow, *midtone, *highlight, *preserve_luminosity);
        }
        _ => return adjust_color(adjustment, px),
    }
    [rgb[0], rgb[1], rgb[2], px[3]]
}

fn hash_noise(x: i32, y: i32, seed: u32) -> f32 {
    let mut n = seed
        .wrapping_add(x as u32)
        .wrapping_mul(1664525)
        .wrapping_add(y as u32)
        .wrapping_mul(1013904223);
    n ^= n >> 16;
    (n & 255) as f32 / 255.0 * 2.0 - 1.0
}

fn grain_rgb(mut rgb: [f32; 3], x: f32, y: f32, amount: f32, size: f32, roughness: f32, seed: u32) -> [f32; 3] {
    let cell = size.max(0.5);
    let ix = (x / cell).floor() as i32;
    let iy = (y / cell).floor() as i32;
    let coarse = hash_noise(ix, iy, seed);
    let fine = hash_noise((x * 3.0) as i32, (y * 3.0) as i32, seed.wrapping_add(99));
    let n = coarse + fine * (roughness / 100.0);
    let delta = n * (amount / 100.0) * 0.35;
    for c in &mut rgb {
        *c = (*c + delta).clamp(0.0, 1.0);
    }
    rgb
}

fn color_balance(mut rgb: [f32; 3], shadow: [f32; 3], midtone: [f32; 3], highlight: [f32; 3], preserve: bool) -> [f32; 3] {
    let y = 0.3 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2];
    let shadow_w = (1.0 - y).powi(2);
    let high_w = y.powi(2);
    let mid_w = (1.0 - (y - 0.5).abs() * 2.0).max(0.0);
    let axis = |shift: [f32; 3], weight: f32, rgb: &mut [f32; 3]| {
        let scale = weight / 100.0;
        rgb[0] += shift[0] * scale;
        rgb[1] -= shift[0] * scale * 0.5;
        rgb[2] -= shift[0] * scale * 0.5;
        rgb[1] += shift[1] * scale;
        rgb[0] -= shift[1] * scale * 0.5;
        rgb[2] -= shift[1] * scale * 0.5;
        rgb[2] += shift[2] * scale;
        rgb[0] -= shift[2] * scale * 0.5;
        rgb[1] -= shift[2] * scale * 0.5;
    };
    axis(shadow, shadow_w, &mut rgb);
    axis(midtone, mid_w, &mut rgb);
    axis(highlight, high_w, &mut rgb);
    if preserve {
        let next = 0.3 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2];
        let delta = y - next;
        for c in &mut rgb {
            *c += delta;
        }
    }
    rgb.map(|c| c.clamp(0.0, 1.0))
}

fn apply_hsl(rgb: [f32; 3], hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let (h, s, l) = rgb_to_hsl(rgb);
    let h = (h + hue / 360.0).rem_euclid(1.0);
    let s = (s + saturation / 100.0).clamp(0.0, 1.0);
    let l = (l + lightness / 100.0).clamp(0.0, 1.0);
    hsl_to_rgb(h, s, l)
}

fn rgb_to_hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let l = (max + min) / 2.0;
    if (max - min) < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == rgb[0] {
        ((rgb[1] - rgb[2]) / d + if rgb[1] < rgb[2] { 6.0 } else { 0.0 }) / 6.0
    } else if max == rgb[1] {
        ((rgb[2] - rgb[0]) / d + 2.0) / 6.0
    } else {
        ((rgb[0] - rgb[1]) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 1e-6 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    [
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    ]
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

fn cover_at(doc: &Document, layer: &Layer, origin_x: f32, origin_y: f32, scale: f32, opacity: f32, x: u32, y: u32) -> f32 {
    cover_expanded(doc, layer, origin_x, origin_y, scale, opacity, x, y, 0.0)
}

fn cover_expanded(doc: &Document, layer: &Layer, origin_x: f32, origin_y: f32, scale: f32, opacity: f32, x: u32, y: u32, expand: f32) -> f32 {
    let doc_x = origin_x + (x as f32 + 0.5) / scale;
    let doc_y = origin_y + (y as f32 + 0.5) / scale;
    let (u, v) = layer.transform.doc_to_unit(doc_x, doc_y);
    let eu = expand / layer.transform.width.max(1.0);
    let ev = expand / layer.transform.height.max(1.0);
    if u < -eu || v < -ev || u > 1.0 + eu || v > 1.0 + ev {
        return 0.0;
    }
    mask_coverage(layer, doc_x, doc_y) * ancestor_mask(doc, layer.id, doc_x, doc_y) * opacity
}

fn curve_map(points: &[(f32, f32)], x: f32) -> f32 {
    if points.is_empty() {
        return x;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    for pair in points.windows(2) {
        if x <= pair[1].0 {
            let span = (pair[1].0 - pair[0].0).max(1e-4);
            let t = (x - pair[0].0) / span;
            return pair[0].1 + (pair[1].1 - pair[0].1) * t;
        }
    }
    points.last().map(|point| point.1).unwrap_or(x)
}

fn motion_blur(
    dest: &mut Raster,
    angle: f32,
    distance: f32,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
    cover: impl Fn(u32, u32) -> f32,
) {
    let distance = distance.abs().clamp(1.0, 64.0);
    let steps = (distance as i32).clamp(2, 16);
    let rad = angle.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let w = dest.width as i32;
    let h = dest.height as i32;
    let src = dest.pixels().to_vec();
    let pixels = dest.pixels_mut();
    for y in y1..y2 {
        for x in x1..x2 {
            let amount = cover(x, y);
            if amount <= 0.0 {
                continue;
            }
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for step in -steps..=steps {
                let t = step as f32 / steps as f32 * distance * 0.5;
                let sx = (x as f32 + dx * t).round() as i32;
                let sy = (y as f32 + dy * t).round() as i32;
                if sx < 0 || sy < 0 || sx >= w || sy >= h {
                    continue;
                }
                let i = (sy as usize * w as usize + sx as usize) * 4;
                for c in 0..4 {
                    acc[c] += src[i + c] as f32;
                }
                n += 1.0;
            }
            if n <= 0.0 {
                continue;
            }
            let i = (y as usize * w as usize + x as usize) * 4;
            for c in 0..4 {
                let blurred = acc[c] / n;
                let orig = src[i + c] as f32;
                pixels[i + c] = (orig + (blurred - orig) * amount).round() as u8;
            }
        }
    }
}

fn blur_region(
    dest: &mut Raster,
    radius: f32,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
    cover: impl Fn(u32, u32) -> f32,
) {
    let r = radius.round().clamp(1.0, 24.0) as i32;
    let w = dest.width as i32;
    let h = dest.height as i32;
    let src = dest.pixels().to_vec();
    let pixels = dest.pixels_mut();
    let span = (r * 2 + 1) as f32;
    for y in y1..y2 {
        for x in x1..x2 {
            let amount = cover(x, y);
            if amount <= 0.0 {
                continue;
            }
            let mut acc = [0.0f32; 4];
            let mut n = 0.0f32;
            for ky in -r..=r {
                for kx in -r..=r {
                    let sx = (x as i32 + kx).clamp(0, w - 1) as usize;
                    let sy = (y as i32 + ky).clamp(0, h - 1) as usize;
                    let i = (sy * w as usize + sx) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as f32;
                    }
                    n += 1.0;
                }
            }
            let i = (y as usize * w as usize + x as usize) * 4;
            for c in 0..4 {
                let blurred = acc[c] / n.max(1.0);
                let orig = src[i + c] as f32;
                pixels[i + c] = (orig + (blurred - orig) * amount).round() as u8;
            }
            let _ = span;
        }
    }
}

pub fn sample_document(doc: &Document, x: f32, y: f32) -> [u8; 4] {
    let mut px = [0.0f32; 4];
    for id in doc.paint_order() {
        if !doc.effectively_visible(id) {
            continue;
        }
        let Some(layer) = doc.layer(id) else {
            continue;
        };
        if let Some(adjustment) = &layer.adjustment {
            if px[3] > 0.0 && layer.transform.contains(x, y) {
                let cover = mask_coverage(layer, x, y)
                    * ancestor_mask(doc, layer.id, x, y)
                    * doc.effective_opacity(layer.id);
                let adjusted = adjust_at(adjustment, px, x, y);
                for c in 0..3 {
                    px[c] += (adjusted[c] - px[c]) * cover;
                }
            }
            continue;
        }
        let Some(image) = layer.image.as_ref() else {
            continue;
        };
        let src = sample_layer(doc, layer, image, x, y, 1.0, doc.effective_opacity(layer.id));
        if src[3] > 0.0 {
            let bytes = f_to_byte(px);
            let mut slot = bytes;
            blend_pixel(&mut slot, src, layer.blend_mode);
            px = byte_to_f(slot);
        }
    }
    f_to_byte(px)
}

pub fn apply_levels_pixel(value: f32, range: LevelRange) -> f32 {
    range.apply(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::BlendMode;
    use crate::document::{Layer, Transform};
    use crate::raster::Raster;

    #[test]
    fn top_layer_covers_background() {
        let mut doc = Document::new(8, 8, Some([255, 0, 0, 255]));
        let mut top = Layer::with_image("top", Raster::new(8, 8, [0, 0, 255, 255]));
        top.transform = Transform::identity(8.0, 8.0);
        doc.add_layer(top);
        let image = composite_document(&doc, 1.0).image;
        let px = image.pixel(3, 3);
        assert_eq!(px, [0, 0, 255, 255]);
    }

    #[test]
    fn drop_shadow_falls_below_the_layer() {
        use crate::effects::{LayerEffects, ShadowEffect};
        let mut doc = Document::new(32, 32, None);
        let mut layer = Layer::with_image("box", Raster::new(8, 8, [255, 0, 0, 255]));
        layer.transform.origin_x = 4.0;
        layer.transform.origin_y = 4.0;
        layer.transform.width = 8.0;
        layer.transform.height = 8.0;
        layer.effects = LayerEffects {
            shadow: Some(ShadowEffect {
                enabled: true,
                angle: 90.0,
                distance: 10.0,
                blur: 0.0,
                color: [0.0, 0.0, 0.0],
                opacity: 1.0,
            }),
            ..LayerEffects::default()
        };
        doc.layers = vec![layer];
        let image = composite_document(&doc, 1.0).image;
        let shadow = image.pixel(8, 18);
        assert!(shadow[3] > 10, "{shadow:?}");
        assert_eq!(image.pixel(1, 1)[3], 0);
    }

    #[test]
    fn opacity_blends() {
        let mut doc = Document::new(4, 4, Some([0, 0, 0, 255]));
        let mut top = Layer::with_image("top", Raster::new(4, 4, [255, 255, 255, 255]));
        top.opacity = 0.5;
        top.blend_mode = BlendMode::Normal;
        doc.add_layer(top);
        let image = composite_document(&doc, 1.0).image;
        let px = image.pixel(1, 1);
        assert!((px[0] as i16 - 128).abs() <= 1, "{px:?}");
    }
}
