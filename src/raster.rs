//! Straight RGBA8 images, shared across undo snapshots until a pixel changes.

use std::sync::Arc;

use crate::blend::{blend_pixel, BlendMode};

#[derive(Clone, Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    rgba: Arc<Vec<u8>>,
}

impl Raster {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let n = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        let mut rgba = vec![0u8; n];
        if fill != [0, 0, 0, 0] {
            for px in rgba.chunks_exact_mut(4) {
                px.copy_from_slice(&fill);
            }
        }
        Self {
            width,
            height,
            rgba: Arc::new(rgba),
        }
    }

    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            width,
            height,
            rgba: Arc::new(rgba),
        })
    }

    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        Arc::make_mut(&mut self.rgba).as_mut_slice()
    }

    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0, 0, 0, 0];
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[i..i + 4].try_into().unwrap_or([0, 0, 0, 0])
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, px: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels_mut()[i..i + 4].copy_from_slice(&px);
    }

    /// Bilinear sample. `x`/`y` are pixel coordinates (0 at the left/top edge of pixel 0).
    pub fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [0.0; 4];
        }
        let x = x - 0.5;
        let y = y - 0.5;
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let tx = x - x0 as f32;
        let ty = y - y0 as f32;
        let c00 = byte_to_f(self.pixel(x0, y0));
        let c10 = byte_to_f(self.pixel(x0 + 1, y0));
        let c01 = byte_to_f(self.pixel(x0, y0 + 1));
        let c11 = byte_to_f(self.pixel(x0 + 1, y0 + 1));
        let mut out = [0.0; 4];
        for i in 0..4 {
            let a = c00[i] + (c10[i] - c00[i]) * tx;
            let b = c01[i] + (c11[i] - c01[i]) * tx;
            out[i] = a + (b - a) * ty;
        }
        out
    }

    /// Coverage sample for a mask. A 1×1 mask is a uniform coverage.
    pub fn sample_coverage(&self, u: f32, v: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 1.0;
        }
        if self.width == 1 && self.height == 1 {
            return self.rgba[0] as f32 / 255.0;
        }
        let x = u.clamp(0.0, 1.0) * self.width as f32;
        let y = v.clamp(0.0, 1.0) * self.height as f32;
        self.sample(x, y)[0]
    }

    pub fn resize(&self, new_w: u32, new_h: u32) -> Self {
        let new_w = new_w.max(1);
        let new_h = new_h.max(1);
        let mut out = Self::new(new_w, new_h, [0, 0, 0, 0]);
        let dest = out.pixels_mut();
        for y in 0..new_h {
            for x in 0..new_w {
                let sx = (x as f32 + 0.5) * self.width as f32 / new_w as f32;
                let sy = (y as f32 + 0.5) * self.height as f32 / new_h as f32;
                let c = f_to_byte(self.sample(sx, sy));
                let i = (y as usize * new_w as usize + x as usize) * 4;
                dest[i..i + 4].copy_from_slice(&c);
            }
        }
        out
    }

    pub fn rotate_cw(&self) -> Self {
        let mut out = Self::new(self.height, self.width, [0, 0, 0, 0]);
        let src = self.pixels();
        let dest = out.pixels_mut();
        let w = self.width as usize;
        let h = self.height as usize;
        for y in 0..h {
            for x in 0..w {
                let nx = h - 1 - y;
                let ny = x;
                let si = (y * w + x) * 4;
                let di = (ny * h + nx) * 4;
                dest[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
        out
    }

    pub fn rotate_ccw(&self) -> Self {
        let mut out = Self::new(self.height, self.width, [0, 0, 0, 0]);
        let src = self.pixels();
        let dest = out.pixels_mut();
        let w = self.width as usize;
        let h = self.height as usize;
        for y in 0..h {
            for x in 0..w {
                let nx = y;
                let ny = w - 1 - x;
                let si = (y * w + x) * 4;
                let di = (ny * h + nx) * 4;
                dest[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
        out
    }

    pub fn flip_x(&self) -> Self {
        let mut out = self.clone();
        let w = self.width as usize;
        let h = self.height as usize;
        let dest = out.pixels_mut();
        for y in 0..h {
            for x in 0..w / 2 {
                let a = (y * w + x) * 4;
                let b = (y * w + (w - 1 - x)) * 4;
                for c in 0..4 {
                    dest.swap(a + c, b + c);
                }
            }
        }
        out
    }

    pub fn flip_y(&self) -> Self {
        let mut out = self.clone();
        let w = self.width as usize;
        let h = self.height as usize;
        let dest = out.pixels_mut();
        let row = w * 4;
        for y in 0..h / 2 {
            let a = y * row;
            let b = (h - 1 - y) * row;
            for i in 0..row {
                dest.swap(a + i, b + i);
            }
        }
        out
    }

    pub fn thumbnail(&self, max_side: u32) -> Self {
        let max_side = max_side.max(1);
        let scale = (max_side as f32 / self.width.max(self.height) as f32).min(1.0);
        let w = (self.width as f32 * scale).round().max(1.0) as u32;
        let h = (self.height as f32 * scale).round().max(1.0) as u32;
        if w == self.width && h == self.height {
            self.clone()
        } else {
            self.resize(w, h)
        }
    }
}

pub fn byte_to_f(px: [u8; 4]) -> [f32; 4] {
    [
        px[0] as f32 / 255.0,
        px[1] as f32 / 255.0,
        px[2] as f32 / 255.0,
        px[3] as f32 / 255.0,
    ]
}

pub fn f_to_byte(px: [f32; 4]) -> [u8; 4] {
    [
        (px[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (px[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (px[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (px[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// Paint a round dab into a layer image. `radius` is in image pixels.
pub fn paint_dab(
    raster: &mut Raster,
    cx: f32,
    cy: f32,
    radius: f32,
    hardness: f32,
    color: [u8; 4],
    opacity: f32,
    erase: bool,
    selection: Option<&dyn Fn(i32, i32) -> f32>,
) {
    if radius <= 0.0 || opacity <= 0.0 {
        return;
    }
    let hardness = hardness.clamp(0.0, 1.0);
    let width = raster.width;
    let height = raster.height;
    let r = radius.ceil() as i32 + 1;
    let x0 = (cx.floor() as i32 - r).max(0);
    let y0 = (cy.floor() as i32 - r).max(0);
    let x1 = (cx.ceil() as i32 + r).min(width as i32 - 1);
    let y1 = (cy.ceil() as i32 + r).min(height as i32 - 1);
    let src = byte_to_f(color);
    let pixels = raster.pixels_mut();
    let w = width as usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt() / radius;
            if d > 1.0 {
                continue;
            }
            let falloff = if d <= hardness || hardness >= 1.0 {
                1.0
            } else {
                1.0 - (d - hardness) / (1.0 - hardness)
            };
            let mut a = opacity * falloff;
            if let Some(sel) = selection {
                a *= sel(x, y);
            }
            if a <= 0.0 {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            if erase {
                let px = &mut pixels[i..i + 4];
                let keep = 1.0 - a;
                px[3] = (px[3] as f32 * keep).round() as u8;
                if px[3] == 0 {
                    px[0] = 0;
                    px[1] = 0;
                    px[2] = 0;
                }
            } else {
                let mut ink = src;
                ink[3] *= a;
                blend_pixel(&mut pixels[i..i + 4], ink, BlendMode::Normal);
            }
        }
    }
}

pub fn fill_rect(raster: &mut Raster, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    let x1 = x.max(0);
    let y1 = y.max(0);
    let x2 = (x + w).min(raster.width as i32);
    let y2 = (y + h).min(raster.height as i32);
    for py in y1..y2 {
        for px in x1..x2 {
            raster.set_pixel(px, py, color);
        }
    }
}

/// Scanline flood fill on an RGBA image, matching `src` within `tolerance` (0–255 per channel).
pub fn flood_fill(raster: &mut Raster, x: i32, y: i32, color: [u8; 4], tolerance: u8) {
    if x < 0 || y < 0 || x >= raster.width as i32 || y >= raster.height as i32 {
        return;
    }
    let target = raster.pixel(x, y);
    if target == color {
        return;
    }
    let w = raster.width as i32;
    let h = raster.height as i32;
    let mut stack = vec![(x, y)];
    let mut seen = vec![false; (w * h) as usize];
    let pixels = raster.pixels_mut();
    while let Some((sx, sy)) = stack.pop() {
        if sx < 0 || sy < 0 || sx >= w || sy >= h {
            continue;
        }
        let idx = (sy * w + sx) as usize;
        if seen[idx] {
            continue;
        }
        let i = idx * 4;
        let px = [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]];
        if !close(px, target, tolerance) {
            continue;
        }
        seen[idx] = true;
        pixels[i..i + 4].copy_from_slice(&color);
        stack.push((sx + 1, sy));
        stack.push((sx - 1, sy));
        stack.push((sx, sy + 1));
        stack.push((sx, sy - 1));
    }
}

fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter()
        .zip(b)
        .all(|(p, q)| (*p as i16 - q as i16).unsigned_abs() <= tolerance as u16)
}

/// Three-pass box blur, a fast stand-in for a gaussian of about `radius` pixels.
pub fn box_blur(raster: &mut Raster, radius: u32) {
    if radius == 0 || raster.width < 2 || raster.height < 2 {
        return;
    }
    let r = radius.min(raster.width.max(raster.height) / 2).max(1);
    for _ in 0..3 {
        blur_axis(raster, r, true);
        blur_axis(raster, r, false);
    }
}

fn blur_axis(raster: &mut Raster, radius: u32, horizontal: bool) {
    let w = raster.width as usize;
    let h = raster.height as usize;
    let src = raster.pixels().to_vec();
    let dest = raster.pixels_mut();
    let r = radius as i32;
    if horizontal {
        let mut prefix = vec![[0.0f32; 4]; w + 1];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    prefix[x + 1][c] = prefix[x][c] + src[i + c] as f32;
                }
            }
            for x in 0..w {
                let left = (x as i32 - r).max(0) as usize;
                let right = (x as i32 + r).min(w as i32 - 1) as usize;
                let count = (right - left + 1) as f32;
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    dest[i + c] = ((prefix[right + 1][c] - prefix[left][c]) / count).round() as u8;
                }
            }
            prefix.fill([0.0; 4]);
        }
    } else {
        let mut prefix = vec![[0.0f32; 4]; h + 1];
        for x in 0..w {
            for y in 0..h {
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    prefix[y + 1][c] = prefix[y][c] + src[i + c] as f32;
                }
            }
            for y in 0..h {
                let top = (y as i32 - r).max(0) as usize;
                let bottom = (y as i32 + r).min(h as i32 - 1) as usize;
                let count = (bottom - top + 1) as f32;
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    dest[i + c] = ((prefix[bottom + 1][c] - prefix[top][c]) / count).round() as u8;
                }
            }
            prefix.fill([0.0; 4]);
        }
    }
}

pub fn add_noise(raster: &mut Raster, amount: f32, seed: u32, monochrome: bool) {
    let amount = amount.clamp(0.0, 255.0);
    if amount <= 0.0 {
        return;
    }
    let pixels = raster.pixels_mut();
    let mut state = seed.max(1);
    for px in pixels.chunks_exact_mut(4) {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let n = ((state >> 16) as u8) as f32 / 255.0 * 2.0 - 1.0;
        if monochrome {
            for c in 0..3 {
                px[c] = (px[c] as f32 + n * amount).round().clamp(0.0, 255.0) as u8;
            }
        } else {
            for c in 0..3 {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let n = ((state >> 16) as u8) as f32 / 255.0 * 2.0 - 1.0;
                px[c] = (px[c] as f32 + n * amount).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

pub fn invert(raster: &mut Raster) {
    for px in raster.pixels_mut().chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_center_of_solid() {
        let raster = Raster::new(4, 4, [10, 20, 30, 255]);
        let s = raster.sample(2.0, 2.0);
        assert!((s[0] - 10.0 / 255.0).abs() < 1e-4);
        assert!((s[3] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn dab_paints_center() {
        let mut raster = Raster::new(16, 16, [0, 0, 0, 0]);
        paint_dab(&mut raster, 8.0, 8.0, 3.0, 1.0, [255, 0, 0, 255], 1.0, false, None);
        let px = raster.pixel(8, 8);
        assert_eq!(px, [255, 0, 0, 255]);
        assert_eq!(raster.pixel(0, 0)[3], 0);
    }
}
