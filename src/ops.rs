//! Selection edits, retouch filters and geometry that the panels call into.

use crate::document::{Document, LevelRange, Selection};
use crate::raster::Raster;
use crate::render;

pub fn expand_selection(selection: &Selection, radius: i32, width: u32, height: u32) -> Selection {
    morph(selection, radius, width, height, true)
}

pub fn contract_selection(selection: &Selection, radius: i32, width: u32, height: u32) -> Selection {
    morph(selection, radius, width, height, false)
}

pub fn offset_selection(selection: Selection, dx: f32, dy: f32) -> Selection {
    match selection {
        Selection::None => Selection::None,
        Selection::Rect { x, y, w, h } => Selection::Rect { x: x + dx, y: y + dy, w, h },
        Selection::Mask { x, y, width, height, coverage } => Selection::Mask {
            x: x + dx.round() as i32,
            y: y + dy.round() as i32,
            width,
            height,
            coverage,
        },
    }
}

fn morph(selection: &Selection, radius: i32, width: u32, height: u32, grow: bool) -> Selection {
    let radius = radius.abs().clamp(1, 64);
    if width as u64 * height as u64 > 8_000_000 {
        return match selection {
            Selection::Rect { x, y, w, h } if grow => Selection::Rect {
                x: x - radius as f32,
                y: y - radius as f32,
                w: w + radius as f32 * 2.0,
                h: h + radius as f32 * 2.0,
            },
            Selection::Rect { x, y, w, h } => Selection::Rect {
                x: x + radius as f32,
                y: y + radius as f32,
                w: (w - radius as f32 * 2.0).max(1.0),
                h: (h - radius as f32 * 2.0).max(1.0),
            },
            other => other.clone(),
        };
    }
    let mut coverage = vec![0u8; width as usize * height as usize];
    for y in 0..height {
        for x in 0..width {
            coverage[(y * width + x) as usize] = (selection.coverage_at(x as f32 + 0.5, y as f32 + 0.5) * 255.0).round() as u8;
        }
    }
    let src = coverage.clone();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut value = if grow { 0u8 } else { 255 };
            for ky in -radius..=radius {
                for kx in -radius..=radius {
                    if kx * kx + ky * ky > radius * radius {
                        continue;
                    }
                    let sx = x + kx;
                    let sy = y + ky;
                    let sample = if sx < 0 || sy < 0 || sx >= width as i32 || sy >= height as i32 {
                        0
                    } else {
                        src[(sy as u32 * width + sx as u32) as usize]
                    };
                    value = if grow { value.max(sample) } else { value.min(sample) };
                }
            }
            coverage[(y as u32 * width + x as u32) as usize] = value;
        }
    }
    Selection::Mask { x: 0, y: 0, width, height, coverage }
}

/// Subject selection without a neural net.
///
/// Border pixels train a small background color model. A pixel is foreground
/// when it is unlike that model and closer to the center than to the edge.
/// A close/open pass fills holes and drops specks.
pub fn select_subject(doc: &Document) -> Selection {
    let scale = (480.0 / doc.width.max(doc.height).max(1) as f32).min(1.0);
    let flat = render::composite_document(doc, scale).image;
    let w = flat.width;
    let h = flat.height;
    let model = border_colors(&flat);
    let mut fg = vec![0u8; w as usize * h as usize];
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..h {
        for x in 0..w {
            let px = flat.pixel(x as i32, y as i32);
            let dist = color_distance(px, &model);
            let center = 1.0 - ((x as f32 - cx).hypot(y as f32 - cy) / max_r);
            if dist > 28.0 + (1.0 - center) * 18.0 {
                fg[(y * w + x) as usize] = 255;
            }
        }
    }
    fg = close_mask(&fg, w, h);
    let coverage = scale_mask(&fg, w, h, doc.width, doc.height);
    Selection::Mask { x: 0, y: 0, width: doc.width, height: doc.height, coverage }
}

fn border_colors(image: &Raster) -> Vec<[u8; 3]> {
    let mut colors = Vec::new();
    let step = (image.width.max(image.height) / 32).max(1);
    let w = image.width;
    let h = image.height;
    for x in (0..w).step_by(step as usize) {
        colors.push(rgb(image.pixel(x as i32, 0)));
        colors.push(rgb(image.pixel(x as i32, h as i32 - 1)));
    }
    for y in (0..h).step_by(step as usize) {
        colors.push(rgb(image.pixel(0, y as i32)));
        colors.push(rgb(image.pixel(w as i32 - 1, y as i32)));
    }
    if colors.is_empty() {
        colors.push([0, 0, 0]);
    }
    colors
}

fn rgb(px: [u8; 4]) -> [u8; 3] {
    [px[0], px[1], px[2]]
}

fn color_distance(px: [u8; 4], model: &[[u8; 3]]) -> f32 {
    model.iter().map(|color| {
        let dr = px[0] as f32 - color[0] as f32;
        let dg = px[1] as f32 - color[1] as f32;
        let db = px[2] as f32 - color[2] as f32;
        (dr * dr + dg * dg + db * db).sqrt()
    }).fold(f32::MAX, f32::min)
}

fn close_mask(src: &[u8], width: u32, height: u32) -> Vec<u8> {
    let dilate = morph_bin(src, width, height, true);
    morph_bin(&dilate, width, height, false)
}

fn morph_bin(src: &[u8], width: u32, height: u32, grow: bool) -> Vec<u8> {
    let mut out = src.to_vec();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut value = if grow { 0 } else { 255 };
            for ky in -1..=1 {
                for kx in -1..=1 {
                    let sx = x + kx;
                    let sy = y + ky;
                    let sample = if sx < 0 || sy < 0 || sx >= width as i32 || sy >= height as i32 {
                        0
                    } else {
                        src[(sy as u32 * width + sx as u32) as usize]
                    };
                    value = if grow { value.max(sample) } else { value.min(sample) };
                }
            }
            out[(y as u32 * width + x as u32) as usize] = value;
        }
    }
    out
}

fn scale_mask(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; dw as usize * dh as usize];
    for y in 0..dh {
        for x in 0..dw {
            let sx = (x as f32 / dw as f32 * sw as f32) as u32;
            let sy = (y as f32 / dh as f32 * sh as f32) as u32;
            out[(y * dw + x) as usize] = src[(sy.min(sh - 1) * sw + sx.min(sw - 1)) as usize];
        }
    }
    out
}

fn sample(image: &Raster, x: u32, y: u32) -> [u8; 4] {
    image.pixel(x as i32, y as i32)
}

fn close_color(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= tol)
}

fn near_edge_color(image: &Raster, x: u32, y: u32, tol: u8) -> bool {
    let px = sample(image, x, y);
    let w = image.width.saturating_sub(1);
    let h = image.height.saturating_sub(1);
    close_color(px, sample(image, 0, y.min(h)), tol)
        || close_color(px, sample(image, w, y.min(h)), tol)
        || close_color(px, sample(image, x.min(w), 0), tol)
        || close_color(px, sample(image, x.min(w), h), tol)
}

pub fn trim_bounds(doc: &Document) -> Option<(f32, f32, f32, f32)> {
    let image = render::composite_document(doc, 1.0).image;
    let mut min_x = image.width;
    let mut min_y = image.height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..image.height {
        for x in 0..image.width {
            if image.pixel(x as i32, y as i32)[3] > 0 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    if !any {
        None
    } else {
        Some((min_x as f32, min_y as f32, (max_x - min_x) as f32, (max_y - min_y) as f32))
    }
}

pub fn auto_levels(doc: &Document) -> LevelRange {
    let image = render::composite_document(doc, (512.0 / doc.width.max(doc.height).max(1) as f32).min(1.0)).image;
    let mut hist = [0u32; 256];
    let mut total = 0u32;
    for px in image.pixels().chunks_exact(4) {
        if px[3] == 0 {
            continue;
        }
        let y = (0.3 * px[0] as f32 + 0.59 * px[1] as f32 + 0.11 * px[2] as f32).round() as usize;
        hist[y.min(255)] += 1;
        total += 1;
    }
    if total == 0 {
        return LevelRange::default();
    }
    let low = (total as f32 * 0.001) as u32;
    let high = (total as f32 * 0.999) as u32;
    let mut seen = 0u32;
    let mut black = 0.0;
    let mut white = 255.0;
    for (i, count) in hist.iter().enumerate() {
        seen += count;
        if seen >= low && black == 0.0 {
            black = i as f32;
        }
        if seen >= high {
            white = i as f32;
            break;
        }
    }
    LevelRange { black, white: white.max(black + 1.0), ..LevelRange::default() }
}

pub fn black_white(rgb: [f32; 3], weights: [f32; 6]) -> [f32; 3] {
    let (h, s, l) = hsl(rgb);
    let sector = (h * 6.0).floor() as usize % 6;
    let next = (sector + 1) % 6;
    let t = h * 6.0 - sector as f32;
    let weight = weights[sector] * (1.0 - t) + weights[next] * t;
    let y = (l * (weight / 100.0).clamp(0.05, 3.0) * (0.4 + s * 0.6)).clamp(0.0, 1.0);
    [y, y, y]
}

fn hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let l = (max + min) / 2.0;
    if (max - min) < 1e-5 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == rgb[0] {
        ((rgb[1] - rgb[2]) / d).rem_euclid(6.0)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.0
    } else {
        (rgb[0] - rgb[1]) / d + 4.0
    } / 6.0;
    (h.rem_euclid(1.0), s, l)
}

pub fn bloom(raster: &mut Raster, amount: f32) {
    let width = raster.width;
    let height = raster.height;
    let mut glow = raster.clone();
    crate::raster::box_blur(&mut glow, 8);
    let src = glow.pixels();
    let dest = raster.pixels_mut();
    for (px, glow_px) in dest.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let y = 0.3 * px[0] as f32 + 0.59 * px[1] as f32 + 0.11 * px[2] as f32;
        let strength = ((y - 140.0) / 115.0).clamp(0.0, 1.0) * amount;
        for c in 0..3 {
            let mixed = px[c] as f32 + glow_px[c] as f32 * strength * 0.45;
            px[c] = mixed.min(255.0) as u8;
        }
    }
    let _ = (width, height);
}

pub fn tonal_contrast(raster: &mut Raster, amount: f32) {
    let mut low = raster.clone();
    crate::raster::box_blur(&mut low, 6);
    let base = low.pixels().to_vec();
    let dest = raster.pixels_mut();
    for (px, blur) in dest.chunks_exact_mut(4).zip(base.chunks_exact(4)) {
        for c in 0..3 {
            let detail = px[c] as f32 - blur[c] as f32;
            px[c] = (px[c] as f32 + detail * amount).round().clamp(0.0, 255.0) as u8;
        }
    }
}

pub fn lens_correct(raster: &mut Raster, amount: f32) {
    let width = raster.width;
    let height = raster.height;
    let src = raster.pixels().to_vec();
    let dest = raster.pixels_mut();
    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let r = (dx * dx + dy * dy).sqrt() / max_r;
            let scale = 1.0 + amount * r * r;
            let sx = (cx + dx / scale).clamp(0.0, width as f32 - 1.0);
            let sy = (cy + dy / scale).clamp(0.0, height as f32 - 1.0);
            let sample = sample_bilinear(&src, width, sx, sy);
            let i = (y as usize * width as usize + x as usize) * 4;
            dest[i..i + 4].copy_from_slice(&sample);
        }
    }
}

fn sample_bilinear(src: &[u8], width: u32, x: f32, y: f32) -> [u8; 4] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let at = |x: i32, y: i32| {
        let i = (y as usize * width as usize + x as usize) * 4;
        if i + 3 >= src.len() { [0, 0, 0, 0] } else { [src[i], src[i + 1], src[i + 2], src[i + 3]] }
    };
    let c00 = at(x0, y0);
    let c10 = at(x0 + 1, y0);
    let c01 = at(x0, y0 + 1);
    let c11 = at(x0 + 1, y0 + 1);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let a = c00[c] as f32 + (c10[c] as f32 - c00[c] as f32) * tx;
        let b = c01[c] as f32 + (c11[c] as f32 - c01[c] as f32) * tx;
        out[c] = (a + (b - a) * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}

pub fn remove_background(raster: &mut Raster) {
    let width = raster.width;
    let height = raster.height;
    let mut bg = vec![false; width as usize * height as usize];
    let mut stack = Vec::new();
    for x in 0..width {
        stack.push((x, 0));
        if height > 1 {
            stack.push((x, height - 1));
        }
    }
    for y in 0..height {
        stack.push((0, y));
        if width > 1 {
            stack.push((width - 1, y));
        }
    }
    let seed = raster.pixel(0, 0);
    while let Some((x, y)) = stack.pop() {
        if x >= width || y >= height {
            continue;
        }
        let index = (y * width + x) as usize;
        if bg[index] {
            continue;
        }
        let px = raster.pixel(x as i32, y as i32);
        if !close_color(px, seed, 32) {
            continue;
        }
        bg[index] = true;
        stack.push((x.saturating_add(1), y));
        if x > 0 {
            stack.push((x - 1, y));
        }
        stack.push((x, y.saturating_add(1)));
        if y > 0 {
            stack.push((x, y - 1));
        }
    }
    let pixels = raster.pixels_mut();
    for (index, is_bg) in bg.into_iter().enumerate() {
        if is_bg {
            pixels[index * 4 + 3] = 0;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DevelopSettings {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
    pub clarity: f32,
    pub vignette: f32,
    pub sharpen: f32,
    pub denoise: f32,
}

impl Default for DevelopSettings {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            temperature: 0.0,
            tint: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            clarity: 0.0,
            vignette: 0.0,
            sharpen: 0.0,
            denoise: 0.0,
        }
    }
}

/// A stand-in for Camera Raw: tone, color, clarity, vignette, sharpen and denoise.
pub fn develop(raster: &mut Raster, settings: DevelopSettings) {
    if settings.denoise > 0.05 {
        crate::raster::box_blur(raster, settings.denoise.round().clamp(1.0, 4.0) as u32);
    }
    if settings.clarity.abs() > 0.01 || settings.sharpen.abs() > 0.01 {
        tonal_contrast(raster, settings.clarity + settings.sharpen * 0.6);
    }
    let width = raster.width;
    let height = raster.height;
    let pixels = raster.pixels_mut();
    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..height {
        for x in 0..width {
            let i = (y as usize * width as usize + x as usize) * 4;
            let mut rgb = [pixels[i] as f32 / 255.0, pixels[i + 1] as f32 / 255.0, pixels[i + 2] as f32 / 255.0];
            let y_luma = 0.3 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2];
            let gain = 2.0f32.powf(settings.exposure);
            for c in &mut rgb {
                *c *= gain;
                *c = (*c - 0.5) * (1.0 + settings.contrast) + 0.5;
            }
            let hi = y_luma.powf(2.0);
            let sh = (1.0 - y_luma).powf(2.0);
            for c in &mut rgb {
                *c += settings.highlights * 0.35 * hi * (*c - 1.0);
                *c += settings.shadows * 0.35 * sh * *c;
            }
            rgb[0] += settings.temperature * 0.08;
            rgb[2] -= settings.temperature * 0.08;
            rgb[1] += settings.tint * 0.06;
            let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
            let sat = 1.0 + settings.saturation;
            let vib = 1.0 + settings.vibrance * (1.0 - (rgb[0] - mean).abs().max((rgb[1] - mean).abs()).max((rgb[2] - mean).abs()));
            for c in &mut rgb {
                *c = mean + (*c - mean) * sat * vib;
            }
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let vig = ((dx * dx + dy * dy).sqrt() / max_r).powf(1.5) * settings.vignette;
            for c in 0..3 {
                pixels[i + c] = ((rgb[c] * (1.0 - vig)).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

/// Push pixels along a stroke. `source` is the image before this dab.
pub fn liquify(dest: &mut Raster, source: &Raster, from: (f32, f32), to: (f32, f32), radius: f32) {
    let width = dest.width;
    let height = dest.height;
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let r = radius.ceil() as i32 + 1;
    let cx = to.0;
    let cy = to.1;
    for y in (cy as i32 - r).max(0)..=(cy as i32 + r).min(height as i32 - 1) {
        for x in (cx as i32 - r).max(0)..=(cx as i32 + r).min(width as i32 - 1) {
            let ox = x as f32 + 0.5 - cx;
            let oy = y as f32 + 0.5 - cy;
            let d = (ox * ox + oy * oy).sqrt() / radius.max(1.0);
            if d > 1.0 {
                continue;
            }
            let falloff = 1.0 - d;
            let sample = source.sample(x as f32 + 0.5 - dx * falloff, y as f32 + 0.5 - dy * falloff);
            let bytes = [
                (sample[0] * 255.0).round() as u8,
                (sample[1] * 255.0).round() as u8,
                (sample[2] * 255.0).round() as u8,
                (sample[3] * 255.0).round() as u8,
            ];
            dest.set_pixel(x, y, bytes);
        }
    }
}

pub fn blur_dab(dest: &mut Raster, source: &Raster, cx: f32, cy: f32, radius: f32) {
    let r = radius.ceil() as i32;
    for y in (cy as i32 - r).max(0)..=(cy as i32 + r).min(dest.height as i32 - 1) {
        for x in (cx as i32 - r).max(0)..=(cx as i32 + r).min(dest.width as i32 - 1) {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d > radius {
                continue;
            }
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for ky in -2..=2 {
                for kx in -2..=2 {
                    let s = source.sample(x as f32 + kx as f32, y as f32 + ky as f32);
                    for c in 0..4 {
                        acc[c] += s[c];
                    }
                    n += 1.0;
                }
            }
            dest.set_pixel(x, y, [
                (acc[0] / n * 255.0).round() as u8,
                (acc[1] / n * 255.0).round() as u8,
                (acc[2] / n * 255.0).round() as u8,
                (acc[3] / n * 255.0).round() as u8,
            ]);
        }
    }
}

/// Bilinear map of `source` into the destination quad (document pixels of the image's own box).
pub fn warp_quad(source: &Raster, corners: [(f32, f32); 4]) -> Raster {
    let min_x = corners.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let min_y = corners.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let max_x = corners.iter().map(|p| p.0).fold(0.0, f32::max).ceil() as u32;
    let max_y = corners.iter().map(|p| p.1).fold(0.0, f32::max).ceil() as u32;
    let width = (max_x.saturating_sub(min_x)).max(1).min(crate::document::MAX_SIDE);
    let height = (max_y.saturating_sub(min_y)).max(1).min(crate::document::MAX_SIDE);
    let mut image = Raster::new(width, height, [0, 0, 0, 0]);
    let local = [
        (corners[0].0 - min_x as f32, corners[0].1 - min_y as f32),
        (corners[1].0 - min_x as f32, corners[1].1 - min_y as f32),
        (corners[2].0 - min_x as f32, corners[2].1 - min_y as f32),
        (corners[3].0 - min_x as f32, corners[3].1 - min_y as f32),
    ];
    for y in 0..height {
        for x in 0..width {
            if let Some((u, v)) = inverse_bilinear(x as f32 + 0.5, y as f32 + 0.5, local) {
                let sample = source.sample(u * source.width as f32, v * source.height as f32);
                image.set_pixel(x as i32, y as i32, [
                    (sample[0] * 255.0).round() as u8,
                    (sample[1] * 255.0).round() as u8,
                    (sample[2] * 255.0).round() as u8,
                    (sample[3] * 255.0).round() as u8,
                ]);
            }
        }
    }
    image
}

fn inverse_bilinear(x: f32, y: f32, q: [(f32, f32); 4]) -> Option<(f32, f32)> {
    // q: top-left, top-right, bottom-right, bottom-left
    let mut u = 0.5;
    let mut v = 0.5;
    for _ in 0..8 {
        let p = bilinear(u, v, q);
        let dx = x - p.0;
        let dy = y - p.1;
        if dx * dx + dy * dy < 0.05 {
            return Some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)));
        }
        u = (u + dx / (q[1].0 - q[0].0).abs().max(1.0)).clamp(-0.05, 1.05);
        v = (v + dy / (q[3].1 - q[0].1).abs().max(1.0)).clamp(-0.05, 1.05);
    }
    let p = bilinear(u, v, q);
    if (p.0 - x).powi(2) + (p.1 - y).powi(2) < 4.0 {
        Some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
    } else {
        None
    }
}

fn bilinear(u: f32, v: f32, q: [(f32, f32); 4]) -> (f32, f32) {
    let a = (q[0].0 * (1.0 - u) + q[1].0 * u, q[0].1 * (1.0 - u) + q[1].1 * u);
    let b = (q[3].0 * (1.0 - u) + q[2].0 * u, q[3].1 * (1.0 - u) + q[2].1 * u);
    (a.0 * (1.0 - v) + b.0 * v, a.1 * (1.0 - v) + b.1 * v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn expand_grows_a_rect() {
        let sel = Selection::Rect { x: 4.0, y: 4.0, w: 4.0, h: 4.0 };
        let next = expand_selection(&sel, 2, 64, 64);
        assert!(next.coverage_at(3.0, 5.0) > 0.5);
        assert!(next.coverage_at(0.0, 0.0) < 0.5);
    }

    #[test]
    fn black_white_is_gray() {
        let y = black_white([1.0, 0.0, 0.0], [40.0, 60.0, 40.0, 60.0, 20.0, 80.0]);
        assert!((y[0] - y[1]).abs() < 1e-4 && (y[1] - y[2]).abs() < 1e-4);
    }

    #[test]
    fn trim_finds_opaque_bounds() {
        let mut doc = Document::new(8, 8, None);
        doc.layers[0].image = Some(Raster::new(8, 8, [0, 0, 0, 0]));
        doc.layers[0].image.as_mut().unwrap().set_pixel(2, 3, [1, 2, 3, 255]);
        let (x, y, w, h) = trim_bounds(&doc).unwrap();
        assert_eq!((x, y, w, h), (2.0, 3.0, 1.0, 1.0));
    }
}
