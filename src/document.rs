//! Document model: layers, groups, selections and canvas edits.
//!
//! Layer order in `layers` is bottom to top, matching a `.comp` manifest.
//! Folders are pass-through: a folder's opacity and mask multiply into each
//! descendant, and children blend directly onto what is below them.

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::effects::LayerEffects;
use crate::raster::Raster;

pub const MAX_SIDE: u32 = 16_000;

pub fn identity_curves() -> [Vec<(f32, f32)>; 4] {
    std::array::from_fn(|_| vec![(0.0, 0.0), (255.0, 255.0)])
}
pub const MAX_LAYERS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Nearest,
    Smooth,
    High,
}

impl Sampling {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nearest => "Nearest",
            Self::Smooth => "Smooth",
            Self::High => "High quality",
        }
    }

    pub fn parse(name: &str) -> Self {
        match name {
            "Nearest" => Self::Nearest,
            "Smooth" => Self::Smooth,
            _ => Self::High,
        }
    }
}

/// Unrotated bounds in document pixels. Rotation is clockwise around the center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub origin_x: f32,
    pub origin_y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub sampling: Sampling,
}

impl Transform {
    pub fn identity(width: f32, height: f32) -> Self {
        Self {
            origin_x: 0.0,
            origin_y: 0.0,
            width: width.max(1.0),
            height: height.max(1.0),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            sampling: Sampling::High,
        }
    }

    pub fn center(self) -> (f32, f32) {
        (self.origin_x + self.width / 2.0, self.origin_y + self.height / 2.0)
    }

    pub fn doc_to_unit(self, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = self.center();
        let dx = x - cx;
        let dy = y - cy;
        let rad = self.rotation.to_radians();
        let (cos, sin) = (rad.cos(), rad.sin());
        let local_x = dx * cos + dy * sin;
        let local_y = -dx * sin + dy * cos;
        let mut u = local_x / self.width + 0.5;
        let mut v = local_y / self.height + 0.5;
        if self.flip_x {
            u = 1.0 - u;
        }
        if self.flip_y {
            v = 1.0 - v;
        }
        (u, v)
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        let (u, v) = self.doc_to_unit(x, y);
        (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
    }

    pub fn corners(self) -> [(f32, f32); 4] {
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(u, v)| self.unit_to_doc(u, v))
    }

    pub fn unit_to_doc(self, u: f32, v: f32) -> (f32, f32) {
        let mut uu = u;
        let mut vv = v;
        if self.flip_x {
            uu = 1.0 - uu;
        }
        if self.flip_y {
            vv = 1.0 - vv;
        }
        let x = (uu - 0.5) * self.width;
        let y = (vv - 0.5) * self.height;
        let rad = self.rotation.to_radians();
        let (cos, sin) = (rad.cos(), rad.sin());
        let (cx, cy) = self.center();
        (cx + x * cos - y * sin, cy + x * sin + y * cos)
    }

    pub fn axis_bounds(self) -> (f32, f32, f32, f32) {
        let corners = self.corners();
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for (x, y) in corners {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        (min_x, min_y, max_x, max_y)
    }
}

#[derive(Clone, Debug)]
pub enum Adjustment {
    Invert,
    HueSat {
        hue: f32,
        saturation: f32,
        lightness: f32,
    },
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    Levels {
        ranges: [LevelRange; 4],
    },
    BlackWhite {
        weights: [f32; 6],
    },
    GaussianBlur {
        radius: f32,
    },
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    Curves {
        channels: [Vec<(f32, f32)>; 4],
    },
    GradientMap {
        shadow: [f32; 3],
        highlight: [f32; 3],
        reversed: bool,
    },
    Grain {
        amount: f32,
        size: f32,
        roughness: f32,
        seed: u32,
    },
    ColorBalance {
        shadow: [f32; 3],
        midtone: [f32; 3],
        highlight: [f32; 3],
        preserve_luminosity: bool,
    },
    /// Kept so a round-trip does not drop a kind this build does not apply.
    Other(Value),
}

impl Adjustment {
    pub fn kind_name(&self) -> &str {
        match self {
            Self::Invert => "Invert",
            Self::HueSat { .. } => "Hue/Saturation",
            Self::Exposure { .. } => "Exposure",
            Self::Levels { .. } => "Levels",
            Self::BlackWhite { .. } => "Black & White",
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::MotionBlur { .. } => "Motion Blur",
            Self::Curves { .. } => "Curves",
            Self::GradientMap { .. } => "Gradient Map",
            Self::Grain { .. } => "Grain",
            Self::ColorBalance { .. } => "Color Balance",
            Self::Other(value) => value
                .get("kind")
                .and_then(|k| k.as_str())
                .unwrap_or("Unknown"),
        }
    }

    pub fn label(&self, zh: bool) -> String {
        if !zh {
            return self.kind_name().to_string();
        }
        match self {
            Self::Invert => "反相".into(),
            Self::HueSat { .. } => "色相/饱和度".into(),
            Self::Exposure { .. } => "曝光".into(),
            Self::Levels { .. } => "色阶".into(),
            Self::BlackWhite { .. } => "黑白".into(),
            Self::GaussianBlur { .. } => "高斯模糊".into(),
            Self::MotionBlur { .. } => "动感模糊".into(),
            Self::Curves { .. } => "曲线".into(),
            Self::GradientMap { .. } => "渐变映射".into(),
            Self::Grain { .. } => "颗粒".into(),
            Self::ColorBalance { .. } => "色彩平衡".into(),
            Self::Other(_) => format!("未支持 ({})", self.kind_name()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelRange {
    pub black: f32,
    pub gamma: f32,
    pub white: f32,
    pub output_black: f32,
    pub output_white: f32,
}

impl Default for LevelRange {
    fn default() -> Self {
        Self {
            black: 0.0,
            gamma: 1.0,
            white: 255.0,
            output_black: 0.0,
            output_white: 255.0,
        }
    }
}

impl LevelRange {
    pub fn apply(self, value: f32) -> f32 {
        let black = self.black.clamp(0.0, 254.0);
        let white = self.white.clamp(black + 1.0, 255.0);
        let gamma = self.gamma.clamp(0.1, 9.99);
        let input = ((value * 255.0 - black) / (white - black)).clamp(0.0, 1.0);
        let mid = input.powf(1.0 / gamma);
        let out = self.output_black + mid * (self.output_white - self.output_black);
        (out / 255.0).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
pub struct Guide {
    pub id: Uuid,
    pub axis: GuideAxis,
    pub position: f32,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: crate::blend::BlendMode,
    pub transform: Transform,
    pub image: Option<Raster>,
    pub mask: Option<Raster>,
    pub mask_enabled: bool,
    pub mask_linked: bool,
    pub mask_placement: Option<Transform>,
    pub parent_id: Option<Uuid>,
    pub is_group: bool,
    pub mask_source_id: Option<Uuid>,
    pub adjustment: Option<Adjustment>,
    pub effects: LayerEffects,
    /// Fields this build stores but does not edit (text, shape, …).
    pub extras: Map<String, Value>,
    pub revision: u64,
}

impl Layer {
    pub fn blank(name: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend_mode: crate::blend::BlendMode::Normal,
            transform: Transform::identity(width, height),
            image: None,
            mask: None,
            mask_enabled: true,
            mask_linked: true,
            mask_placement: None,
            parent_id: None,
            is_group: false,
            mask_source_id: None,
            adjustment: None,
            effects: LayerEffects::default(),
            extras: Map::new(),
            revision: 1,
        }
    }

    pub fn group(name: impl Into<String>, width: f32, height: f32) -> Self {
        let mut layer = Self::blank(name, width, height);
        layer.is_group = true;
        layer
    }

    pub fn with_image(name: impl Into<String>, image: Raster) -> Self {
        let mut layer = Self::blank(name, image.width as f32, image.height as f32);
        layer.image = Some(image);
        layer
    }

    pub fn is_adjustment(&self) -> bool {
        self.adjustment.is_some()
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Painting or a filter replaces the pixels, so editable text/shape metadata no longer matches.
    pub fn rasterize_metadata(&mut self) {
        self.extras.remove("text");
        self.extras.remove("shape");
        self.touch();
    }
}

#[derive(Clone, Debug)]
pub enum Selection {
    None,
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Coverage in document pixels. `mask[y * width + x]` is 0–255.
    Mask {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        coverage: Vec<u8>,
    },
}

impl Default for Selection {
    fn default() -> Self {
        Self::None
    }
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Rect { w, h, .. } => *w < 0.5 || *h < 0.5,
            Self::Mask { coverage, .. } => coverage.iter().all(|c| *c == 0),
        }
    }

    pub fn coverage_at(&self, x: f32, y: f32) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Rect { x: rx, y: ry, w, h } => {
                if x >= *rx && y >= *ry && x < rx + w && y < ry + h {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Mask {
                x: ox,
                y: oy,
                width,
                height,
                coverage,
            } => {
                let px = x.floor() as i32 - ox;
                let py = y.floor() as i32 - oy;
                if px < 0 || py < 0 || px >= *width as i32 || py >= *height as i32 {
                    0.0
                } else {
                    coverage[(py as u32 * width + px as u32) as usize] as f32 / 255.0
                }
            }
        }
    }

    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        match self {
            Self::None => None,
            Self::Rect { x, y, w, h } => Some((*x, *y, x + w, y + h)),
            Self::Mask {
                x,
                y,
                width,
                height,
                coverage,
            } => {
                let mut min_x = *width;
                let mut min_y = *height;
                let mut max_x = 0u32;
                let mut max_y = 0u32;
                let mut any = false;
                for py in 0..*height {
                    for px in 0..*width {
                        if coverage[(py * width + px) as usize] > 0 {
                            any = true;
                            min_x = min_x.min(px);
                            min_y = min_y.min(py);
                            max_x = max_x.max(px + 1);
                            max_y = max_y.max(py + 1);
                        }
                    }
                }
                if !any {
                    None
                } else {
                    Some((
                        *x as f32 + min_x as f32,
                        *y as f32 + min_y as f32,
                        *x as f32 + max_x as f32,
                        *y as f32 + max_y as f32,
                    ))
                }
            }
        }
    }

    /// Screen-ready edges of the selection. Masks follow the covered pixels, not the bounding box.
    pub fn outline_edges(&self) -> Vec<((f32, f32), (f32, f32))> {
        match self {
            Self::None => Vec::new(),
            Self::Rect { x, y, w, h } => vec![
                ((*x, *y), (*x + *w, *y)),
                ((*x + *w, *y), (*x + *w, *y + *h)),
                ((*x + *w, *y + *h), (*x, *y + *h)),
                ((*x, *y + *h), (*x, *y)),
            ],
            Self::Mask { x, y, width, height, coverage } => mask_outline(*x, *y, *width, *height, coverage),
        }
    }

    /// Closed paths that follow the selection, smoothed enough to draw as a marquee.
    pub fn contours(&self) -> Vec<Vec<(f32, f32)>> {
        match self {
            Self::None => Vec::new(),
            Self::Rect { x, y, w, h } => vec![vec![(*x, *y), (*x + *w, *y), (*x + *w, *y + *h), (*x, *y + *h)]],
            Self::Mask { x, y, width, height, coverage } => mask_contours(*x, *y, *width, *height, coverage),
        }
    }

    pub fn select_all(width: u32, height: u32) -> Self {
        Self::Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
        }
    }

    pub fn invert(self, width: u32, height: u32) -> Self {
        let mut mask = vec![255u8; width as usize * height as usize];
        match self {
            Self::None => return Self::select_all(width, height),
            Self::Rect { x, y, w, h } => {
                for py in 0..height {
                    for px in 0..width {
                        let inside = px as f32 >= x
                            && py as f32 >= y
                            && (px as f32) < x + w
                            && (py as f32) < y + h;
                        if inside {
                            mask[(py * width + px) as usize] = 0;
                        }
                    }
                }
            }
            Self::Mask {
                x,
                y,
                width: mw,
                height: mh,
                coverage,
            } => {
                for py in 0..height {
                    for px in 0..width {
                        let lx = px as i32 - x;
                        let ly = py as i32 - y;
                        let covered = if lx >= 0 && ly >= 0 && lx < mw as i32 && ly < mh as i32 {
                            coverage[(ly as u32 * mw + lx as u32) as usize]
                        } else {
                            0
                        };
                        mask[(py * width + px) as usize] = 255 - covered;
                    }
                }
            }
        }
        Self::Mask {
            x: 0,
            y: 0,
            width,
            height,
            coverage: mask,
        }
    }
}

fn mask_outline(ox: i32, oy: i32, width: u32, height: u32, coverage: &[u8]) -> Vec<((f32, f32), (f32, f32))> {
    if width == 0 || height == 0 || coverage.len() < (width as usize) * (height as usize) {
        return Vec::new();
    }
    let cells = width as u64 * height as u64;
    let stride = if cells <= 400_000 {
        1
    } else {
        ((cells / 400_000) as u32).max(2)
    };
    let cols = (width + stride - 1) / stride;
    let rows = (height + stride - 1) / stride;
    let on = |cx: u32, cy: u32| -> bool {
        if cx >= cols || cy >= rows {
            return false;
        }
        let px = (cx * stride + stride / 2).min(width - 1);
        let py = (cy * stride + stride / 2).min(height - 1);
        coverage[(py * width + px) as usize] > 127
    };
    let mut edges = Vec::new();
    for cy in 0..=rows {
        let mut run: Option<u32> = None;
        for cx in 0..=cols {
            let above = cy > 0 && cx < cols && on(cx, cy - 1);
            let below = cy < rows && cx < cols && on(cx, cy);
            if above != below {
                if run.is_none() {
                    run = Some(cx);
                }
            } else if let Some(start) = run.take() {
                let y = oy as f32 + cy as f32 * stride as f32;
                edges.push((
                    (ox as f32 + start as f32 * stride as f32, y),
                    (ox as f32 + cx as f32 * stride as f32, y),
                ));
            }
        }
    }
    for cx in 0..=cols {
        let mut run: Option<u32> = None;
        for cy in 0..=rows {
            let left = cx > 0 && cy < rows && on(cx - 1, cy);
            let right = cx < cols && cy < rows && on(cx, cy);
            if left != right {
                if run.is_none() {
                    run = Some(cy);
                }
            } else if let Some(start) = run.take() {
                let x = ox as f32 + cx as f32 * stride as f32;
                edges.push((
                    (x, oy as f32 + start as f32 * stride as f32),
                    (x, oy as f32 + cy as f32 * stride as f32),
                ));
            }
        }
    }
    edges
}

fn mask_contours(ox: i32, oy: i32, width: u32, height: u32, coverage: &[u8]) -> Vec<Vec<(f32, f32)>> {
    if width == 0 || height == 0 || coverage.len() < width as usize * height as usize {
        return Vec::new();
    }
    let stride = if width as u64 * height as u64 > 900_000 { 2 } else { 1 };
    let cols = ((width + stride - 1) / stride) as i32;
    let rows = ((height + stride - 1) / stride) as i32;
    let on = |cx: i32, cy: i32| -> bool {
        if cx < 0 || cy < 0 || cx >= cols || cy >= rows {
            return false;
        }
        let px = (cx as u32 * stride + stride / 2).min(width - 1);
        let py = (cy as u32 * stride + stride / 2).min(height - 1);
        coverage[(py * width + px) as usize] > 127
    };
    let mut seen = vec![false; (cols * rows) as usize];
    let index = |cx: i32, cy: i32| (cy * cols + cx) as usize;
    const STEP: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let mut paths = Vec::new();
    for cy in 0..rows {
        for cx in 0..cols {
            if !on(cx, cy) || seen[index(cx, cy)] {
                continue;
            }
            let edge = !on(cx - 1, cy) || !on(cx + 1, cy) || !on(cx, cy - 1) || !on(cx, cy + 1);
            if !edge {
                seen[index(cx, cy)] = true;
                continue;
            }
            let mut path = Vec::new();
            let mut x = cx;
            let mut y = cy;
            let mut dir = 0usize;
            let start = (cx, cy);
            for _ in 0..16000 {
                seen[index(x, y)] = true;
                path.push((
                    ox as f32 + (x as f32 + 0.5) * stride as f32,
                    oy as f32 + (y as f32 + 0.5) * stride as f32,
                ));
                let mut found = false;
                for k in 0..8 {
                    let next = (dir + 5 + k) % 8;
                    let nx = x + STEP[next].0;
                    let ny = y + STEP[next].1;
                    if on(nx, ny) {
                        x = nx;
                        y = ny;
                        dir = next;
                        found = true;
                        break;
                    }
                }
                if !found || (path.len() > 2 && x == start.0 && y == start.1) {
                    break;
                }
            }
            if path.len() >= 3 {
                paths.push(smooth_loop(path));
            }
            if paths.len() >= 8 {
                return paths;
            }
        }
    }
    paths
}

fn smooth_loop(path: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    if path.len() < 6 {
        return path;
    }
    let n = path.len();
    (0..n)
        .map(|i| {
            let a = path[(i + n - 1) % n];
            let b = path[i];
            let c = path[(i + 1) % n];
            ((a.0 + b.0 * 2.0 + c.0) / 4.0, (a.1 + b.1 * 2.0 + c.1) / 4.0)
        })
        .step_by(2)
        .collect()
}

#[derive(Clone, Debug)]
pub struct Document {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    pub resolution: f64,
    pub layers: Vec<Layer>,
    pub active_layer_id: Option<Uuid>,
    pub guides: Vec<Guide>,
    /// Unknown top-level manifest fields, preserved on save.
    pub extras: Map<String, Value>,
}

impl Document {
    pub fn new(width: u32, height: u32, background: Option<[u8; 4]>) -> Self {
        let width = width.clamp(1, MAX_SIDE);
        let height = height.clamp(1, MAX_SIDE);
        let mut doc = Self {
            id: Uuid::new_v4(),
            width,
            height,
            resolution: 72.0,
            layers: Vec::new(),
            active_layer_id: None,
            guides: Vec::new(),
            extras: Map::new(),
        };
        let layer = match background {
            Some(color) => {
                let mut layer = Layer::with_image(
                    "Background",
                    Raster::new(width, height, color),
                );
                layer.name = "Background".into();
                layer
            }
            None => Layer::blank("Layer 1", width as f32, height as f32),
        };
        doc.active_layer_id = Some(layer.id);
        doc.layers.push(layer);
        doc
    }

    pub fn from_image(name: impl Into<String>, image: Raster) -> Self {
        let mut doc = Self {
            id: Uuid::new_v4(),
            width: image.width.clamp(1, MAX_SIDE),
            height: image.height.clamp(1, MAX_SIDE),
            resolution: 72.0,
            layers: Vec::new(),
            active_layer_id: None,
            guides: Vec::new(),
            extras: Map::new(),
        };
        let mut layer = Layer::with_image(name, image);
        if layer.transform.width > doc.width as f32 {
            layer.transform.width = doc.width as f32;
        }
        doc.active_layer_id = Some(layer.id);
        doc.layers.push(layer);
        doc
    }

    pub fn index_of(&self, id: Uuid) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id == id)
    }

    pub fn layer(&self, id: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn layer_mut(&mut self, id: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == id)
    }

    pub fn active(&self) -> Option<&Layer> {
        self.active_layer_id.and_then(|id| self.layer(id))
    }

    pub fn active_mut(&mut self) -> Option<&mut Layer> {
        let id = self.active_layer_id?;
        self.layer_mut(id)
    }

    pub fn ensure_active_image(&mut self) -> Option<&mut Raster> {
        let width = self.width;
        let height = self.height;
        let layer = self.active_mut()?;
        if layer.is_group || layer.adjustment.is_some() {
            return None;
        }
        if layer.image.is_none() {
            layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
            layer.transform = Transform::identity(width as f32, height as f32);
            layer.touch();
        }
        layer.image.as_mut()
    }

    pub fn add_layer(&mut self, mut layer: Layer) -> Uuid {
        layer.parent_id = self.active().and_then(|active| {
            if active.is_group {
                Some(active.id)
            } else {
                active.parent_id
            }
        });
        let id = layer.id;
        self.layers.push(layer);
        self.active_layer_id = Some(id);
        id
    }

    pub fn delete_layer(&mut self, id: Uuid) {
        let children: Vec<Uuid> = self
            .layers
            .iter()
            .filter(|layer| layer.parent_id == Some(id))
            .map(|layer| layer.id)
            .collect();
        for child in children {
            self.delete_layer(child);
        }
        self.layers.retain(|layer| layer.id != id);
        if self.active_layer_id == Some(id) {
            self.active_layer_id = self.layers.last().map(|layer| layer.id);
        }
        for layer in &mut self.layers {
            if layer.mask_source_id == Some(id) {
                layer.mask_source_id = None;
                layer.touch();
            }
        }
    }

    pub fn duplicate_layer(&mut self, id: Uuid) -> Option<Uuid> {
        let mut copy = self.layer(id)?.clone();
        copy.id = Uuid::new_v4();
        copy.name = format!("{} copy", copy.name);
        copy.transform.origin_x += 16.0;
        copy.transform.origin_y += 16.0;
        copy.touch();
        let new_id = copy.id;
        let index = self.index_of(id)?;
        self.layers.insert(index + 1, copy);
        self.active_layer_id = Some(new_id);
        Some(new_id)
    }

    /// `up` moves the layer toward the top of the stack, among its siblings.
    pub fn reorder_sibling(&mut self, id: Uuid, up: bool) {
        let Some(parent) = self.layer(id).map(|layer| layer.parent_id) else {
            return;
        };
        let siblings: Vec<usize> = self
            .layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.parent_id == parent)
            .map(|(index, _)| index)
            .collect();
        let Some(pos) = siblings.iter().position(|&index| self.layers[index].id == id) else {
            return;
        };
        if up && pos + 1 < siblings.len() {
            let a = siblings[pos];
            let b = siblings[pos + 1];
            self.layers.swap(a, b);
        } else if !up && pos > 0 {
            let a = siblings[pos];
            let b = siblings[pos - 1];
            self.layers.swap(a, b);
        }
    }

    pub fn ancestor_ids(&self, id: Uuid) -> Vec<Uuid> {
        let mut out = Vec::new();
        let mut current = self.layer(id).and_then(|layer| layer.parent_id);
        let mut guard = 0;
        while let Some(parent) = current {
            if guard > 64 || out.contains(&parent) {
                break;
            }
            out.push(parent);
            current = self.layer(parent).and_then(|layer| layer.parent_id);
            guard += 1;
        }
        out
    }

    pub fn effectively_visible(&self, id: Uuid) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        if !layer.visible {
            return false;
        }
        self.ancestor_ids(id)
            .into_iter()
            .all(|parent| self.layer(parent).is_some_and(|layer| layer.visible))
    }

    pub fn effective_opacity(&self, id: Uuid) -> f32 {
        let Some(layer) = self.layer(id) else {
            return 0.0;
        };
        let mut opacity = layer.opacity.clamp(0.0, 1.0);
        for parent in self.ancestor_ids(id) {
            if let Some(group) = self.layer(parent) {
                opacity *= group.opacity.clamp(0.0, 1.0);
            }
        }
        opacity
    }

    /// Panel rows, top of the stack first. Groups are followed by their children.
    pub fn panel_rows(&self) -> Vec<(Uuid, usize)> {
        fn visit(doc: &Document, parent: Option<Uuid>, depth: usize, out: &mut Vec<(Uuid, usize)>) {
            if depth > 64 {
                return;
            }
            let mut siblings: Vec<Uuid> = doc
                .layers
                .iter()
                .filter(|layer| layer.parent_id == parent)
                .map(|layer| layer.id)
                .collect();
            siblings.reverse();
            for id in siblings {
                out.push((id, depth));
                if doc.layer(id).is_some_and(|layer| layer.is_group) {
                    visit(doc, Some(id), depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        visit(self, None, 0, &mut out);
        out
    }

    /// Leaf and adjustment layers in compositing order (bottom to top), groups omitted.
    pub fn descendant_ids(&self, id: Uuid) -> Vec<Uuid> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        let mut guard = 0;
        while let Some(current) = stack.pop() {
            if guard > 10_000 {
                break;
            }
            guard += 1;
            for layer in &self.layers {
                if layer.parent_id == Some(current) {
                    out.push(layer.id);
                    stack.push(layer.id);
                }
            }
        }
        out
    }

    /// Place `child` as it sat inside `old_parent` after that parent becomes `new_parent`.
    pub fn follow_child(old_parent: Transform, new_parent: Transform, child: Transform) -> Transform {
        let (cx, cy) = child.center();
        let (u, v) = old_parent.doc_to_unit(cx, cy);
        let (nx, ny) = new_parent.unit_to_doc(u, v);
        let sx = new_parent.width / old_parent.width.max(1.0);
        let sy = new_parent.height / old_parent.height.max(1.0);
        let mut out = child;
        out.width = (child.width * sx).abs().max(1.0);
        out.height = (child.height * sy).abs().max(1.0);
        out.origin_x = nx - out.width / 2.0;
        out.origin_y = ny - out.height / 2.0;
        out.rotation += new_parent.rotation - old_parent.rotation;
        if new_parent.flip_x != old_parent.flip_x {
            out.flip_x = !out.flip_x;
        }
        if new_parent.flip_y != old_parent.flip_y {
            out.flip_y = !out.flip_y;
        }
        out
    }

    pub fn translate(&mut self, id: Uuid, dx: f32, dy: f32) {
        let ids = std::iter::once(id).chain(self.descendant_ids(id).into_iter());
        for layer_id in ids {
            if let Some(layer) = self.layer_mut(layer_id) {
                layer.transform.origin_x += dx;
                layer.transform.origin_y += dy;
                if let Some(place) = layer.mask_placement.as_mut() {
                    place.origin_x += dx;
                    place.origin_y += dy;
                }
                layer.touch();
            }
        }
    }

    pub fn paint_order(&self) -> Vec<Uuid> {
        fn visit(doc: &Document, parent: Option<Uuid>, out: &mut Vec<Uuid>) {
            for layer in doc.layers.iter().filter(|layer| layer.parent_id == parent) {
                if layer.is_group {
                    visit(doc, Some(layer.id), out);
                } else {
                    out.push(layer.id);
                }
            }
        }
        let mut out = Vec::new();
        visit(self, None, &mut out);
        out
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Uuid> {
        for id in self.paint_order().into_iter().rev() {
            if !self.effectively_visible(id) {
                continue;
            }
            let Some(layer) = self.layer(id) else {
                continue;
            };
            if layer.adjustment.is_some() {
                continue;
            }
            if layer.transform.contains(x, y) {
                let (u, v) = layer.transform.doc_to_unit(x, y);
                if let Some(image) = &layer.image {
                    let sample = image.sample(u * image.width as f32, v * image.height as f32);
                    if sample[3] > 0.04 {
                        return Some(id);
                    }
                } else if layer.transform.contains(x, y) {
                    return Some(id);
                }
            }
        }
        None
    }

    pub fn crop(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let w = w.round().clamp(1.0, MAX_SIDE as f32);
        let h = h.round().clamp(1.0, MAX_SIDE as f32);
        for layer in &mut self.layers {
            layer.transform.origin_x -= x;
            layer.transform.origin_y -= y;
            if let Some(place) = layer.mask_placement.as_mut() {
                place.origin_x -= x;
                place.origin_y -= y;
            }
        }
        for guide in &mut self.guides {
            match guide.axis {
                GuideAxis::Vertical => guide.position -= x,
                GuideAxis::Horizontal => guide.position -= y,
            }
        }
        self.width = w as u32;
        self.height = h as u32;
    }

    pub fn resize_canvas(&mut self, width: u32, height: u32, anchor_x: f32, anchor_y: f32) {
        let width = width.clamp(1, MAX_SIDE);
        let height = height.clamp(1, MAX_SIDE);
        let dx = (width as f32 - self.width as f32) * anchor_x;
        let dy = (height as f32 - self.height as f32) * anchor_y;
        for layer in &mut self.layers {
            layer.transform.origin_x += dx;
            layer.transform.origin_y += dy;
            if let Some(place) = layer.mask_placement.as_mut() {
                place.origin_x += dx;
                place.origin_y += dy;
            }
        }
        for guide in &mut self.guides {
            match guide.axis {
                GuideAxis::Vertical => guide.position += dx,
                GuideAxis::Horizontal => guide.position += dy,
            }
        }
        self.width = width;
        self.height = height;
    }

    pub fn resize_image(&mut self, width: u32, height: u32) {
        let width = width.clamp(1, MAX_SIDE);
        let height = height.clamp(1, MAX_SIDE);
        let sx = width as f32 / self.width.max(1) as f32;
        let sy = height as f32 / self.height.max(1) as f32;
        for layer in &mut self.layers {
            if let Some(image) = layer.image.as_mut() {
                let nw = (image.width as f32 * sx).round().clamp(1.0, MAX_SIDE as f32) as u32;
                let nh = (image.height as f32 * sy).round().clamp(1.0, MAX_SIDE as f32) as u32;
                *image = image.resize(nw, nh);
            }
            if let Some(mask) = layer.mask.as_mut() {
                let nw = (mask.width as f32 * sx).round().clamp(1.0, MAX_SIDE as f32) as u32;
                let nh = (mask.height as f32 * sy).round().clamp(1.0, MAX_SIDE as f32) as u32;
                *mask = mask.resize(nw, nh);
            }
            layer.transform.origin_x *= sx;
            layer.transform.origin_y *= sy;
            layer.transform.width = (layer.transform.width * sx).max(1.0);
            layer.transform.height = (layer.transform.height * sy).max(1.0);
            layer.touch();
        }
        for guide in &mut self.guides {
            match guide.axis {
                GuideAxis::Vertical => guide.position *= sx,
                GuideAxis::Horizontal => guide.position *= sy,
            }
        }
        self.width = width;
        self.height = height;
    }

    pub fn flip_canvas(&mut self, horizontal: bool) {
        let w = self.width as f32;
        let h = self.height as f32;
        for layer in &mut self.layers {
            if horizontal {
                let right = layer.transform.origin_x + layer.transform.width;
                layer.transform.origin_x = w - right;
                layer.transform.flip_x = !layer.transform.flip_x;
            } else {
                let bottom = layer.transform.origin_y + layer.transform.height;
                layer.transform.origin_y = h - bottom;
                layer.transform.flip_y = !layer.transform.flip_y;
            }
            layer.touch();
        }
        for guide in &mut self.guides {
            match (horizontal, guide.axis) {
                (true, GuideAxis::Vertical) => guide.position = w - guide.position,
                (false, GuideAxis::Horizontal) => guide.position = h - guide.position,
                _ => {}
            }
        }
    }

    pub fn rotate_canvas(&mut self, clockwise: bool) {
        let old_w = self.width as f32;
        let old_h = self.height as f32;
        for layer in &mut self.layers {
            let (cx, cy) = layer.transform.center();
            let (nx, ny) = if clockwise {
                (old_h - cy, cx)
            } else {
                (cy, old_w - cx)
            };
            layer.transform.origin_x = nx - layer.transform.width / 2.0;
            layer.transform.origin_y = ny - layer.transform.height / 2.0;
            layer.transform.rotation += if clockwise { 90.0 } else { -90.0 };
            layer.touch();
        }
        for guide in &mut self.guides {
            let pos = guide.position;
            if clockwise {
                match guide.axis {
                    GuideAxis::Horizontal => {
                        guide.axis = GuideAxis::Vertical;
                        guide.position = old_h - pos;
                    }
                    GuideAxis::Vertical => {
                        guide.axis = GuideAxis::Horizontal;
                        guide.position = pos;
                    }
                }
            } else {
                match guide.axis {
                    GuideAxis::Horizontal => {
                        guide.axis = GuideAxis::Vertical;
                        guide.position = pos;
                    }
                    GuideAxis::Vertical => {
                        guide.axis = GuideAxis::Horizontal;
                        guide.position = old_w - pos;
                    }
                }
            }
        }
        self.width = old_h as u32;
        self.height = old_w as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipse_outline_is_not_the_bounding_box() {
        let selection = crate::edit::ellipse_selection(2.0, 3.0, 48.0, 32.0);
        let edges = selection.outline_edges();
        let (x0, y0, x1, y1) = selection.bounds().unwrap();
        let off_box = edges.iter().any(|((ax, ay), (bx, by))| {
            let vertical = (ax - bx).abs() < 0.01;
            let horizontal = (ay - by).abs() < 0.01;
            let on_side = (vertical && ((ax - x0).abs() < 0.01 || (ax - x1).abs() < 0.01))
                || (horizontal && ((ay - y0).abs() < 0.01 || (ay - y1).abs() < 0.01));
            !on_side
        });
        assert!(edges.len() > 8);
        assert!(off_box);
    }

    #[test]
    fn transform_roundtrip_corners() {
        let mut t = Transform::identity(100.0, 40.0);
        t.origin_x = 10.0;
        t.origin_y = 20.0;
        let (u, v) = t.doc_to_unit(10.0, 20.0);
        assert!((u - 0.0).abs() < 1e-4 && (v - 0.0).abs() < 1e-4);
        let (x, y) = t.unit_to_doc(1.0, 1.0);
        assert!((x - 110.0).abs() < 1e-3 && (y - 60.0).abs() < 1e-3);
    }

    #[test]
    fn group_opacity_multiplies() {
        let mut doc = Document::new(32, 32, None);
        let group = Layer::group("Group", 32.0, 32.0);
        let gid = group.id;
        doc.layers.insert(0, group);
        doc.layers[1].parent_id = Some(gid);
        doc.layers[1].opacity = 0.5;
        doc.layer_mut(gid).unwrap().opacity = 0.5;
        assert!((doc.effective_opacity(doc.layers[1].id) - 0.25).abs() < 1e-4);
    }

    #[test]
    fn child_follows_a_scaled_group() {
        let old = Transform::identity(100.0, 100.0);
        let mut new = old;
        new.width = 200.0;
        new.height = 200.0;
        new.origin_x = -50.0;
        new.origin_y = -50.0;
        let mut child = Transform::identity(10.0, 10.0);
        child.origin_x = 10.0;
        child.origin_y = 20.0;
        let followed = Document::follow_child(old, new, child);
        assert!((followed.width - 20.0).abs() < 0.2, "{}", followed.width);
        assert!(followed.origin_x < child.origin_x);
    }

    #[test]
    fn moving_a_group_moves_its_children() {
        let mut doc = Document::new(32, 32, None);
        let group = Layer::group("Group", 32.0, 32.0);
        let gid = group.id;
        doc.layers.insert(0, group);
        doc.layers[1].parent_id = Some(gid);
        doc.layers[1].transform.origin_x = 4.0;
        doc.translate(gid, 10.0, 6.0);
        assert!((doc.layer(gid).unwrap().transform.origin_x - 10.0).abs() < 1e-3);
        assert!((doc.layers[1].transform.origin_x - 14.0).abs() < 1e-3);
        assert!((doc.layers[1].transform.origin_y - 6.0).abs() < 1e-3);
    }

    #[test]
    fn hidden_group_hides_children() {
        let mut doc = Document::new(8, 8, Some([255, 255, 255, 255]));
        let group = Layer::group("Group", 8.0, 8.0);
        let gid = group.id;
        doc.layers.insert(0, group);
        let child = doc.layers[1].id;
        doc.layers[1].parent_id = Some(gid);
        doc.layer_mut(gid).unwrap().visible = false;
        assert!(!doc.effectively_visible(child));
    }
}
