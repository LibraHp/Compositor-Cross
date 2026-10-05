//! Destructive edits, shapes, gradients and text rasterization.

use ab_glyph::{Font, FontVec, Glyph, PxScale, ScaleFont};

use crate::document::{Document, Layer, Selection, Transform};
use crate::raster::{add_noise, box_blur, flood_fill, invert, Raster};
use crate::render::composite_document;

pub fn merge_down(doc: &mut Document, id: uuid::Uuid) -> Result<(), String> {
    let index = doc.index_of(id).ok_or("没有可合并的图层")?;
    if index == 0 {
        return Err("下面没有图层".into());
    }
    let below_id = doc.layers[index - 1].id;
    if doc.layers[index].parent_id != doc.layers[index - 1].parent_id {
        return Err("只能合并同一组里相邻的图层".into());
    }
    if doc.layers[index - 1].is_group || doc.layers[index].is_group {
        return Err("组不能这样合并，请先栅格化".into());
    }
    let flat = flatten_ids(doc, &[below_id, id])?;
    let width = doc.width as f32;
    let height = doc.height as f32;
    let below = doc.layer_mut(below_id).ok_or("图层已消失")?;
    below.image = Some(flat);
    below.transform = Transform::identity(width, height);
    below.mask = None;
    below.adjustment = None;
    below.mask_source_id = None;
    below.rasterize_metadata();
    below.opacity = 1.0;
    below.blend_mode = crate::blend::BlendMode::Normal;
    doc.delete_layer(id);
    doc.active_layer_id = Some(below_id);
    Ok(())
}

pub fn flatten(doc: &mut Document) -> Result<(), String> {
    let flat = composite_document(doc, 1.0).image;
    let mut layer = Layer::with_image("Background", flat);
    layer.transform = Transform::identity(doc.width as f32, doc.height as f32);
    doc.layers = vec![layer.clone()];
    doc.active_layer_id = Some(layer.id);
    Ok(())
}

pub fn merge_visible(doc: &mut Document) -> Result<(), String> {
    let ids: Vec<_> = doc
        .layers
        .iter()
        .filter(|layer| layer.visible && !layer.is_group)
        .map(|layer| layer.id)
        .collect();
    if ids.is_empty() {
        return Err("没有可见图层".into());
    }
    let flat = flatten_ids(doc, &ids)?;
    doc.layers.retain(|layer| !layer.visible || layer.is_group);
    let mut layer = Layer::with_image("Merged", flat);
    layer.transform = Transform::identity(doc.width as f32, doc.height as f32);
    let id = layer.id;
    doc.layers.push(layer);
    doc.active_layer_id = Some(id);
    Ok(())
}

fn flatten_ids(doc: &Document, ids: &[uuid::Uuid]) -> Result<Raster, String> {
    let mut ghost = doc.clone();
    for layer in &mut ghost.layers {
        layer.visible = ids.contains(&layer.id);
    }
    Ok(composite_document(&ghost, 1.0).image)
}

pub fn apply_to_active(doc: &mut Document, selection: &Selection, op: impl FnOnce(&mut Raster)) -> Result<(), String> {
    let width = doc.width;
    let height = doc.height;
    let layer = doc.active_mut().ok_or("没有活动图层")?;
    if layer.is_group {
        return Err("组没有像素".into());
    }
    if layer.adjustment.is_some() {
        return Err("调整图层请用图层面板编辑，或先栅格化".into());
    }
    if layer.image.is_none() {
        layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
        layer.transform = Transform::identity(width as f32, height as f32);
    }
    let transform = layer.transform;
    let image = layer.image.as_mut().ok_or("图层没有像素")?;
    if selection.is_empty() || matches!(selection, Selection::None) {
        op(image);
    } else {
        let before = image.clone();
        op(image);
        let w = image.width;
        let h = image.height;
        for y in 0..h {
            for x in 0..w {
                let u = (x as f32 + 0.5) / w as f32;
                let v = (y as f32 + 0.5) / h as f32;
                let (dx, dy) = transform.unit_to_doc(u, v);
                if selection.coverage_at(dx, dy) <= 0.5 {
                    let old = before.pixel(x as i32, y as i32);
                    image.set_pixel(x as i32, y as i32, old);
                }
            }
        }
    }
    layer.rasterize_metadata();
    Ok(())
}

pub fn fill_active(doc: &mut Document, selection: &Selection, color: [u8; 4]) -> Result<(), String> {
    apply_to_active(doc, selection, |image| {
        let w = image.width as i32;
        let h = image.height as i32;
        for y in 0..h {
            for x in 0..w {
                image.set_pixel(x, y, color);
            }
        }
    })
}

pub fn clear_selection(doc: &mut Document, selection: &Selection) -> Result<(), String> {
    fill_active(doc, selection, [0, 0, 0, 0])
}

pub fn invert_active(doc: &mut Document, selection: &Selection) -> Result<(), String> {
    apply_to_active(doc, selection, invert)
}

pub fn blur_active(doc: &mut Document, selection: &Selection, radius: u32) -> Result<(), String> {
    apply_to_active(doc, selection, |image| box_blur(image, radius))
}

pub fn noise_active(doc: &mut Document, selection: &Selection, amount: f32) -> Result<(), String> {
    apply_to_active(doc, selection, |image| add_noise(image, amount, 1, false))
}

pub fn desaturate_active(doc: &mut Document, selection: &Selection) -> Result<(), String> {
    apply_to_active(doc, selection, |image| {
        for px in image.pixels_mut().chunks_exact_mut(4) {
            let y = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32).round() as u8;
            px[0] = y;
            px[1] = y;
            px[2] = y;
        }
    })
}

pub fn bucket(doc: &mut Document, doc_x: f32, doc_y: f32, color: [u8; 4], tolerance: u8) -> Result<(), String> {
    let layer = doc.active_mut().ok_or("没有活动图层")?;
    if layer.is_group || layer.adjustment.is_some() {
        return Err("不能在组或调整图层上填充".into());
    }
    let (u, v) = layer.transform.doc_to_unit(doc_x, doc_y);
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return Ok(());
    }
    let width = doc.width;
    let height = doc.height;
    let layer = doc.active_mut().unwrap();
    if layer.image.is_none() {
        layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
        layer.transform = Transform::identity(width as f32, height as f32);
    }
    let image = layer.image.as_mut().unwrap();
    let x = (u * image.width as f32) as i32;
    let y = (v * image.height as f32) as i32;
    flood_fill(image, x, y, color, tolerance);
    layer.rasterize_metadata();
    Ok(())
}

pub fn magic_select(doc: &Document, doc_x: f32, doc_y: f32, tolerance: u8) -> Selection {
    let w = doc.width;
    let h = doc.height;
    if w as u64 * h as u64 > 6_000_000 {
        return Selection::None;
    }
    let flat = composite_document(doc, 1.0).image;
    let sx = doc_x.floor() as i32;
    let sy = doc_y.floor() as i32;
    let target = flat.pixel(sx, sy);
    let mut coverage = vec![0u8; w as usize * h as usize];
    let mut stack = vec![(sx, sy)];
    let mut seen = vec![false; w as usize * h as usize];
    let pixels = flat.pixels();
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            continue;
        }
        let idx = (y as u32 * w + x as u32) as usize;
        if seen[idx] {
            continue;
        }
        seen[idx] = true;
        let i = idx * 4;
        let px = [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]];
        if px.iter().zip(target).any(|(a, b)| (*a as i16 - b as i16).unsigned_abs() > tolerance as u16) {
            continue;
        }
        coverage[idx] = 255;
        stack.push((x + 1, y));
        stack.push((x - 1, y));
        stack.push((x, y + 1));
        stack.push((x, y - 1));
    }
    Selection::Mask {
        x: 0,
        y: 0,
        width: w,
        height: h,
        coverage,
    }
}

pub fn polygon_selection(points: &[(f32, f32)]) -> Selection {
    if points.len() < 3 {
        return Selection::None;
    }
    let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
    let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
    let max_x = points.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
    let max_y = points.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
    let w = (max_x - min_x).max(1) as u32;
    let h = (max_y - min_y).max(1) as u32;
    if w as u64 * h as u64 > 16_000_000 {
        return Selection::None;
    }
    let mut coverage = vec![0u8; w as usize * h as usize];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if point_in_poly(x as f32 + min_x as f32 + 0.5, y as f32 + min_y as f32 + 0.5, points) {
                coverage[(y as u32 * w + x as u32) as usize] = 255;
            }
        }
    }
    Selection::Mask {
        x: min_x,
        y: min_y,
        width: w,
        height: h,
        coverage,
    }
}

fn point_in_poly(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi + 1e-6) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn ellipse_selection(x: f32, y: f32, w: f32, h: f32) -> Selection {
    let min_x = x.min(x + w);
    let min_y = y.min(y + h);
    let rw = w.abs().max(1.0);
    let rh = h.abs().max(1.0);
    let width = rw.ceil() as u32;
    let height = rh.ceil() as u32;
    let mut coverage = vec![0u8; width as usize * height as usize];
    let cx = rw / 2.0;
    let cy = rh / 2.0;
    for py in 0..height {
        for px in 0..width {
            let nx = (px as f32 + 0.5 - cx) / (rw / 2.0);
            let ny = (py as f32 + 0.5 - cy) / (rh / 2.0);
            if nx * nx + ny * ny <= 1.0 {
                coverage[(py * width + px) as usize] = 255;
            }
        }
    }
    Selection::Mask {
        x: min_x.floor() as i32,
        y: min_y.floor() as i32,
        width,
        height,
        coverage,
    }
}

pub fn rasterize_shape(kind: ShapeKind, w: u32, h: u32, color: [u8; 4], radius: f32, line_width: f32) -> Raster {
    let mut image = Raster::new(w.max(1), h.max(1), [0, 0, 0, 0]);
    let wf = image.width as f32;
    let hf = image.height as f32;
    for y in 0..image.height {
        for x in 0..image.width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let inside = match kind {
                ShapeKind::Rectangle => rounded_rect(px, py, wf, hf, radius),
                ShapeKind::Ellipse => {
                    let nx = (px - wf / 2.0) / (wf / 2.0);
                    let ny = (py - hf / 2.0) / (hf / 2.0);
                    nx * nx + ny * ny <= 1.0
                }
                ShapeKind::Line => {
                    let dist = point_segment_dist(px, py, 0.0, hf, wf, 0.0);
                    dist <= line_width.max(1.0) / 2.0
                }
            };
            if inside {
                image.set_pixel(x as i32, y as i32, color);
            }
        }
    }
    image
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> bool {
    let r = radius.clamp(0.0, w.min(h) / 2.0);
    if x < 0.0 || y < 0.0 || x > w || y > h {
        return false;
    }
    let cx = if x < r { r } else if x > w - r { w - r } else { x };
    let cy = if y < r { r } else if y > h - r { h - r } else { y };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r + 0.01
}

fn point_segment_dist(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-4 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len2).clamp(0.0, 1.0);
    let x = x1 + t * dx;
    let y = y1 + t * dy;
    ((px - x).powi(2) + (py - y).powi(2)).sqrt()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
}

pub fn system_font_bytes() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\calibri.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    None
}

pub fn rasterize_text(text: &str, font_size: f32, color: [u8; 4]) -> Result<Raster, String> {
    let bytes = system_font_bytes().ok_or("找不到系统字体（Arial / Segoe UI / DejaVu）")?;
    let font = FontVec::try_from_vec(bytes).map_err(|_| "字体无法解析")?;
    let scale = PxScale::from(font_size.max(8.0));
    let scaled = font.as_scaled(scale);
    let line_height = scaled.height() + scaled.line_gap();
    let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.split('\n').collect() };
    let mut width = 8.0f32;
    for line in &lines {
        let mut cursor = 0.0f32;
        let mut last: Option<ab_glyph::GlyphId> = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = last {
                cursor += scaled.kern(prev, id);
            }
            cursor += scaled.h_advance(id);
            last = Some(id);
        }
        width = width.max(cursor + 8.0);
    }
    let height = (line_height * lines.len() as f32 + 8.0).ceil().max(8.0);
    let mut image = Raster::new(width.ceil() as u32, height.ceil() as u32, [0, 0, 0, 0]);
    let mut baseline = scaled.ascent() + 4.0;
    for line in lines {
        let mut cursor = 4.0f32;
        let mut last = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = last {
                cursor += scaled.kern(prev, id);
            }
            let glyph = Glyph {
                id,
                scale,
                position: ab_glyph::point(cursor, baseline),
            };
            if let Some(outlined) = scaled.outline_glyph(glyph) {
                outlined.draw(|x, y, c| {
                    let px = outlined.px_bounds().min.x as i32 + x as i32;
                    let py = outlined.px_bounds().min.y as i32 + y as i32;
                    let mut ink = color;
                    ink[3] = (ink[3] as f32 * c).round() as u8;
                    let dst = image.pixel(px, py);
                    let src_a = ink[3] as f32 / 255.0;
                    let dst_a = dst[3] as f32 / 255.0;
                    let out_a = src_a + dst_a * (1.0 - src_a);
                    if out_a > 0.0 {
                        let mix = |s: u8, d: u8| {
                            ((s as f32 * src_a + d as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8
                        };
                        image.set_pixel(
                            px,
                            py,
                            [mix(ink[0], dst[0]), mix(ink[1], dst[1]), mix(ink[2], dst[2]), (out_a * 255.0).round() as u8],
                        );
                    }
                });
            }
            cursor += scaled.h_advance(id);
            last = Some(id);
        }
        baseline += line_height;
    }
    Ok(image)
}

pub fn feather_selection(selection: &Selection, radius: i32, width: u32, height: u32) -> Selection {
    if selection.is_empty() || width as u64 * height as u64 > 8_000_000 {
        return selection.clone();
    }
    let radius = radius.clamp(1, 64);
    let mut coverage = vec![0u8; width as usize * height as usize];
    for y in 0..height {
        for x in 0..width {
            coverage[(y * width + x) as usize] = (selection.coverage_at(x as f32 + 0.5, y as f32 + 0.5) * 255.0).round() as u8;
        }
    }
    for _ in 0..2 {
        coverage = blur_mask(&coverage, width, height, radius, true);
        coverage = blur_mask(&coverage, width, height, radius, false);
    }
    Selection::Mask { x: 0, y: 0, width, height, coverage }
}

fn blur_mask(src: &[u8], width: u32, height: u32, radius: i32, horizontal: bool) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    let w = width as i32;
    let h = height as i32;
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0i32;
            let mut n = 0i32;
            for k in -radius..=radius {
                let sx = if horizontal { (x + k).clamp(0, w - 1) } else { x };
                let sy = if horizontal { y } else { (y + k).clamp(0, h - 1) };
                acc += src[(sy as u32 * width + sx as u32) as usize] as i32;
                n += 1;
            }
            out[(y as u32 * width + x as u32) as usize] = (acc / n.max(1)) as u8;
        }
    }
    out
}

/// Fill selected pixels from the nearest already-known neighbors, walking inward from the edge.
pub fn content_aware_fill(raster: &mut Raster, hole: &[bool]) {
    let w = raster.width as i32;
    let h = raster.height as i32;
    if hole.len() != (w * h) as usize || w <= 0 || h <= 0 {
        return;
    }
    let mut known = vec![false; hole.len()];
    let mut queue = std::collections::VecDeque::new();
    for y in 0..h {
        for x in 0..w {
            let index = (y * w + x) as usize;
            if !hole[index] {
                known[index] = true;
                continue;
            }
            if neighbors(x, y, w, h).any(|(nx, ny)| !hole[(ny * w + nx) as usize]) {
                queue.push_back((x, y));
            }
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        let index = (y * w + x) as usize;
        if known[index] {
            continue;
        }
        let mut acc = [0u32; 4];
        let mut n = 0u32;
        for (nx, ny) in neighbors(x, y, w, h) {
            let nindex = (ny * w + nx) as usize;
            if !known[nindex] {
                continue;
            }
            let px = raster.pixel(nx, ny);
            for (slot, channel) in acc.iter_mut().zip(px) {
                *slot += channel as u32;
            }
            n += 1;
        }
        if n == 0 {
            continue;
        }
        raster.set_pixel(x, y, [
            (acc[0] / n) as u8,
            (acc[1] / n) as u8,
            (acc[2] / n) as u8,
            (acc[3] / n) as u8,
        ]);
        known[index] = true;
        for (nx, ny) in neighbors(x, y, w, h) {
            let nindex = (ny * w + nx) as usize;
            if hole[nindex] && !known[nindex] {
                queue.push_back((nx, ny));
            }
        }
    }
}

fn neighbors(x: i32, y: i32, w: i32, h: i32) -> impl Iterator<Item = (i32, i32)> {
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .map(move |(dx, dy)| (x + dx, y + dy))
        .filter(move |(nx, ny)| *nx >= 0 && *ny >= 0 && *nx < w && *ny < h)
}

/// Color sampled from a ring just outside a spot-heal brush.
pub fn heal_sample(source: &Raster, x: i32, y: i32, radius: i32) -> [u8; 4] {
    let radius = radius.max(1);
    let mut acc = [0u32; 4];
    let mut n = 0u32;
    for i in 0..12 {
        let angle = i as f32 / 12.0 * std::f32::consts::TAU;
        let sx = x + (angle.cos() * (radius as f32 + 2.0)) as i32;
        let sy = y + (angle.sin() * (radius as f32 + 2.0)) as i32;
        let px = source.pixel(sx, sy);
        if px[3] == 0 {
            continue;
        }
        for (slot, channel) in acc.iter_mut().zip(px) {
            *slot += channel as u32;
        }
        n += 1;
    }
    if n == 0 {
        return source.pixel(x, y);
    }
    [
        (acc[0] / n) as u8,
        (acc[1] / n) as u8,
        (acc[2] / n) as u8,
        (acc[3] / n) as u8,
    ]
}

pub fn vignette(raster: &mut Raster, amount: f32) {
    let width = raster.width;
    let height = raster.height;
    let w = width as f32;
    let h = height as f32;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    let pixels = raster.pixels_mut();
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let t = ((dx * dx + dy * dy).sqrt() / max_r).clamp(0.0, 1.0);
            let falloff = t.powf(1.6) * amount.clamp(0.0, 1.0);
            let i = (y as usize * width as usize + x as usize) * 4;
            for c in 0..3 {
                pixels[i + c] = (pixels[i + c] as f32 * (1.0 - falloff)).round() as u8;
            }
        }
    }
}

pub fn copy_merged_rgba(doc: &Document) -> Raster {
    composite_document(doc, 1.0).image
}
