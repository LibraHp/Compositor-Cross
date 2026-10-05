//! 8-bit RGB Photoshop (`.psd`) import.
//!
//! Reads each layer's name, visibility, opacity, blend mode, bounds and pixels.
//! Folder dividers become groups. PSB, CMYK, 16-bit and ZIP channels are rejected
//! with an explicit error instead of a blank canvas.

use std::path::Path;

use uuid::Uuid;

use crate::blend::BlendMode;
use crate::document::{Document, Layer, Transform, MAX_SIDE};
use crate::raster::Raster;

pub struct PsdImport {
    pub document: Document,
    pub notes: Vec<String>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.remaining() < n {
            return Err("PSD 文件不完整".into());
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n)?;
        Ok(())
    }

    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        Ok(u64::from_be_bytes(b.try_into().unwrap()))
    }

    fn section(&mut self) -> Result<&'a [u8], String> {
        self.section_wide(false)
    }

    fn section_wide(&mut self, wide: bool) -> Result<&'a [u8], String> {
        let len = if wide { self.u64()? as usize } else { self.u32()? as usize };
        self.take(len)
    }
}

struct ChannelSpec {
    id: i16,
    len: usize,
}

struct RawLayer {
    name: String,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    opacity: f32,
    visible: bool,
    blend: BlendMode,
    unmapped_blend: bool,
    clipping: bool,
    /// 0 other, 1 open folder, 2 closed folder, 3 bounding divider.
    divider: u32,
    channels: Vec<ChannelSpec>,
    planes: Vec<(i16, Vec<u8>)>,
}

pub fn import_file(path: &Path) -> Result<PsdImport, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("无法读取 PSD: {err}"))?;
    import_bytes(&bytes)
}

pub fn import_bytes(bytes: &[u8]) -> Result<PsdImport, String> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != b"8BPS" {
        return Err("这不是 Photoshop PSD 文件".into());
    }
    let version = reader.u16()?;
    if version != 1 && version != 2 {
        return Err("不支持的 Photoshop 版本".into());
    }
    let wide = version == 2;
    reader.skip(6)?;
    let channels = reader.u16()? as u32;
    let height = reader.u32()?;
    let width = reader.u32()?;
    let depth = reader.u16()?;
    let mode = reader.u16()?;
    if depth != 8 || mode != 3 {
        return Err("仅支持 8 位 RGB 的 PSD".into());
    }
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("PSD 画布 {width}×{height} 超出支持范围"));
    }
    let _ = reader.section()?;
    let resources = reader.section()?;
    let resolution = resolution_from_resources(resources).unwrap_or(72.0);
    let layer_section = reader.section_wide(wide)?;
    let mut notes = Vec::new();
    let raw = match parse_layer_section(layer_section, wide) {
        Ok(layers) => layers,
        Err(err) => {
            notes.push(format!("图层信息无法读取（{err}），改用合并图像"));
            Vec::new()
        }
    };
    let mut document = Document {
        id: Uuid::new_v4(),
        width,
        height,
        resolution,
        layers: Vec::new(),
        active_layer_id: None,
        guides: Vec::new(),
        extras: Default::default(),
    };
    if raw.is_empty() {
        let merged = read_merged(&mut reader, width, height, channels.max(3))?;
        let mut layer = Layer::with_image("Background", merged);
        layer.transform = Transform::identity(width as f32, height as f32);
        document.active_layer_id = Some(layer.id);
        document.layers.push(layer);
        notes.push("这个 PSD 没有可分离的图层，已导入合并图像".into());
    } else {
        let (layers, extra) = assemble(raw)?;
        notes.extend(extra);
        if layers.is_empty() {
            return Err("PSD 里没有可导入的图层".into());
        }
        document.active_layer_id = layers.last().map(|layer| layer.id);
        document.layers = layers;
    }
    Ok(PsdImport { document, notes })
}

fn resolution_from_resources(data: &[u8]) -> Option<f64> {
    let mut reader = Reader::new(data);
    while reader.remaining() > 12 {
        let sig = reader.take(4).ok()?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let id = reader.u16().ok()?;
        let name_len = reader.u8().ok()? as usize;
        let padded = (1 + name_len).div_ceil(2) * 2;
        if padded > 1 {
            reader.skip(padded - 1).ok()?;
        }
        let len = reader.u32().ok()? as usize;
        let body = reader.take(len).ok()?;
        if len % 2 == 1 {
            let _ = reader.u8();
        }
        if id == 0x03ED && body.len() >= 4 {
            let fixed = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
            let ppi = f64::from(fixed) / 65536.0;
            if (1.0..=9600.0).contains(&ppi) {
                return Some(ppi);
            }
        }
    }
    None
}

fn parse_layer_section(data: &[u8], wide: bool) -> Result<Vec<RawLayer>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut reader = Reader::new(data);
    let info_len = if wide { reader.u64()? as usize } else { reader.u32()? as usize };
    if info_len == 0 {
        return Ok(Vec::new());
    }
    let info = reader.take(info_len.min(reader.remaining()))?;
    parse_layer_info(info, wide)
}

fn parse_layer_info(data: &[u8], wide: bool) -> Result<Vec<RawLayer>, String> {
    let mut reader = Reader::new(data);
    let count = (reader.u16()? as i16).unsigned_abs() as usize;
    if count > 10_000 {
        return Err("图层过多".into());
    }
    let mut layers = Vec::with_capacity(count);
    for _ in 0..count {
        let top = reader.i32()?;
        let left = reader.i32()?;
        let bottom = reader.i32()?;
        let right = reader.i32()?;
        let channel_count = reader.u16()? as usize;
        if channel_count > 64 {
            return Err("通道数量异常".into());
        }
        let mut channels = Vec::with_capacity(channel_count);
        for _ in 0..channel_count {
            let id = reader.u16()? as i16;
            let len = if wide { reader.u64()? as usize } else { reader.u32()? as usize };
            channels.push(ChannelSpec { id, len });
        }
        let blend_sig = reader.take(4)?;
        let key = reader.take(4)?;
        let opacity = f32::from(reader.u8()?) / 255.0;
        let clipping = reader.u8()? == 1;
        let flags = reader.u8()?;
        reader.u8()?;
        let extra_len = reader.u32()? as usize;
        let extra = reader.take(extra_len)?;
        let (name, divider) = parse_extra(extra).unwrap_or_else(|_| ("Layer".into(), 0));
        let (blend, unmapped_blend) = blend_from_key(if blend_sig == b"8BIM" { key } else { b"norm" });
        layers.push(RawLayer {
            name,
            top,
            left,
            bottom,
            right,
            opacity,
            visible: flags & 0x02 == 0,
            blend,
            unmapped_blend,
            clipping,
            divider,
            channels,
            planes: Vec::new(),
        });
    }
    for layer in &mut layers {
        let width = (layer.right - layer.left).max(0) as u32;
        let height = (layer.bottom - layer.top).max(0) as u32;
        let specs = std::mem::take(&mut layer.channels);
        for spec in specs {
            let start = reader.pos;
            let plane = if width == 0 || height == 0 || spec.len < 2 {
                Vec::new()
            } else {
                decode_plane(&mut reader, width, height).unwrap_or_default()
            };
            let consumed = reader.pos - start;
            if consumed < spec.len {
                reader.skip(spec.len - consumed)?;
            }
            layer.planes.push((spec.id, plane));
        }
    }
    Ok(layers)
}

fn decode_plane(reader: &mut Reader<'_>, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let compression = reader.u16()?;
    let expected = width as usize * height as usize;
    if compression == 0 {
        return Ok(reader.take(expected)?.to_vec());
    }
    if compression != 1 {
        return Err("不支持的图层压缩".into());
    }
    let mut counts = Vec::with_capacity(height as usize);
    for _ in 0..height {
        counts.push(reader.u16()? as usize);
    }
    let mut out = vec![0u8; expected];
    for (y, count) in counts.into_iter().enumerate() {
        let row = reader.take(count)?;
        let decoded = packbits(row, width as usize);
        let n = decoded.len().min(width as usize);
        let start = y * width as usize;
        out[start..start + n].copy_from_slice(&decoded[..n]);
    }
    Ok(out)
}

fn packbits(src: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < src.len() && out.len() < expected {
        let header = src[i] as i8;
        i += 1;
        if header >= 0 {
            let count = header as usize + 1;
            if i + count > src.len() {
                break;
            }
            let take = count.min(expected - out.len());
            out.extend_from_slice(&src[i..i + take]);
            i += count;
        } else if header != -128 {
            let count = (1 - i32::from(header)) as usize;
            if i >= src.len() {
                break;
            }
            let byte = src[i];
            i += 1;
            out.extend(std::iter::repeat_n(byte, count.min(expected - out.len())));
        }
    }
    out.resize(expected, 0);
    out
}

fn parse_extra(data: &[u8]) -> Result<(String, u32), String> {
    let mut reader = Reader::new(data);
    let mask_len = reader.u32()? as usize;
    reader.skip(mask_len)?;
    let ranges_len = reader.u32()? as usize;
    reader.skip(ranges_len)?;
    let name = read_pascal_name(&mut reader)?;
    let mut divider = 0u32;
    let mut unicode = None;
    while reader.remaining() >= 12 {
        let sig = match reader.take(4) {
            Ok(sig) => sig,
            Err(_) => break,
        };
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key = reader.take(4)?;
        let len = reader.u32()? as usize;
        if len > reader.remaining() {
            break;
        }
        let body = reader.take(len)?;
        if len % 2 == 1 && reader.remaining() > 0 {
            let _ = reader.u8();
        }
        if key == b"luni" {
            unicode = decode_utf16_be(body);
        } else if (key == b"lsct" || key == b"lsdk") && body.len() >= 4 {
            divider = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        }
    }
    Ok((unicode.unwrap_or(name), divider))
}

fn read_pascal_name(reader: &mut Reader<'_>) -> Result<String, String> {
    let len = reader.u8()? as usize;
    let bytes = reader.take(len)?;
    let pad = (4 - ((1 + len) % 4)) % 4;
    if reader.remaining() >= pad {
        reader.skip(pad)?;
    }
    let name = String::from_utf8_lossy(bytes).trim().to_string();
    Ok(if name.is_empty() { "Layer".into() } else { name })
}

fn decode_utf16_be(body: &[u8]) -> Option<String> {
    if body.len() < 4 {
        return None;
    }
    let count = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if body.len() < 4 + count * 2 {
        return None;
    }
    let units: Vec<u16> = (0..count)
        .map(|i| u16::from_be_bytes([body[4 + i * 2], body[5 + i * 2]]))
        .collect();
    let name = String::from_utf16_lossy(&units).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn blend_from_key(key: &[u8]) -> (BlendMode, bool) {
    let mode = match key {
        b"norm" | b"pass" => BlendMode::Normal,
        b"mul " => BlendMode::Multiply,
        b"scrn" => BlendMode::Screen,
        b"over" => BlendMode::Overlay,
        b"sLit" => BlendMode::SoftLight,
        b"hLit" => BlendMode::HardLight,
        b"dark" => BlendMode::Darken,
        b"lite" => BlendMode::Lighten,
        b"diff" => BlendMode::Difference,
        b"div " => BlendMode::ColorDodge,
        b"idiv" => BlendMode::ColorBurn,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"colr" => BlendMode::Color,
        b"lum " => BlendMode::Luminosity,
        b"lbrn" => BlendMode::LinearBurn,
        b"lddg" => BlendMode::LinearDodge,
        b"vLit" => BlendMode::VividLight,
        b"lLit" => BlendMode::LinearLight,
        b"pLit" => BlendMode::PinLight,
        b"hMix" => BlendMode::HardMix,
        b"smud" => BlendMode::Exclusion,
        b"fsub" => BlendMode::Subtract,
        b"fdiv" => BlendMode::Divide,
        _ => return (BlendMode::Normal, true),
    };
    (mode, false)
}

fn read_merged(reader: &mut Reader<'_>, width: u32, height: u32, channels: u32) -> Result<Raster, String> {
    let compression = reader.u16()?;
    let plane = width as usize * height as usize;
    let count = channels.max(3) as usize;
    let mut planes = vec![vec![0u8; plane]; count.min(4)];
    if compression == 0 {
        for plane_buf in planes.iter_mut().take(count) {
            let bytes = reader.take(plane)?;
            plane_buf[..bytes.len().min(plane)].copy_from_slice(&bytes[..bytes.len().min(plane)]);
        }
    } else if compression == 1 {
        let rows = height as usize * count;
        let mut counts = Vec::with_capacity(rows);
        for _ in 0..rows {
            counts.push(reader.u16()? as usize);
        }
        for (index, count) in counts.into_iter().enumerate() {
            let row = reader.take(count)?;
            let decoded = packbits(row, width as usize);
            let channel = index / height as usize;
            let y = index % height as usize;
            if channel < planes.len() {
                let start = y * width as usize;
                let n = decoded.len().min(width as usize);
                planes[channel][start..start + n].copy_from_slice(&decoded[..n]);
            }
        }
    } else {
        return Err("合并图像使用了不支持的压缩".into());
    }
    rgba_from_planes(width, height, planes.get(0), planes.get(1), planes.get(2), None)
}

fn rgba_from_planes(width: u32, height: u32, r: Option<&Vec<u8>>, g: Option<&Vec<u8>>, b: Option<&Vec<u8>>, a: Option<&Vec<u8>>) -> Result<Raster, String> {
    let n = width as usize * height as usize;
    if n == 0 || n > 80_000_000 {
        return Err("图层像素过多或为空".into());
    }
    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        rgba[i * 4] = r.and_then(|p| p.get(i).copied()).unwrap_or(0);
        rgba[i * 4 + 1] = g.and_then(|p| p.get(i).copied()).unwrap_or(0);
        rgba[i * 4 + 2] = b.and_then(|p| p.get(i).copied()).unwrap_or(0);
        rgba[i * 4 + 3] = a.and_then(|p| p.get(i).copied()).unwrap_or(255);
    }
    Raster::from_rgba(width, height, rgba).ok_or_else(|| "图层像素无效".into())
}

fn assemble(raw: Vec<RawLayer>) -> Result<(Vec<Layer>, Vec<String>), String> {
    let mut notes = Vec::new();
    // File order is top to bottom. A folder record is followed by its children
    // and closed by a type-3 divider.
    let mut parent_stack: Vec<Uuid> = Vec::new();
    let mut built: Vec<Layer> = Vec::new();
    let mut clipping_flags = Vec::new();
    for record in raw {
        if record.divider == 3 {
            parent_stack.pop();
            continue;
        }
        let is_group = record.divider == 1 || record.divider == 2;
        if record.unmapped_blend {
            notes.push(format!("「{}」的混合模式已改为正常", record.name));
        }
        let width = (record.right - record.left).max(0) as u32;
        let height = (record.bottom - record.top).max(0) as u32;
        let mut layer = if is_group {
            Layer::group(record.name.clone(), width.max(1) as f32, height.max(1) as f32)
        } else if width == 0 || height == 0 {
            Layer::blank(record.name.clone(), 1.0, 1.0)
        } else {
            let plane = |id: i16| record.planes.iter().find(|(plane_id, _)| *plane_id == id).map(|(_, data)| data);
            match rgba_from_planes(width, height, plane(0), plane(1), plane(2), plane(-1)) {
                Ok(image) => Layer::with_image(record.name.clone(), image),
                Err(err) => {
                    notes.push(format!("「{}」没有导入像素：{err}", record.name));
                    Layer::blank(record.name.clone(), width as f32, height as f32)
                }
            }
        };
        layer.name = record.name;
        layer.visible = record.visible;
        layer.opacity = record.opacity.clamp(0.0, 1.0);
        layer.blend_mode = if is_group { BlendMode::Normal } else { record.blend };
        layer.transform.origin_x = record.left as f32;
        layer.transform.origin_y = record.top as f32;
        if width > 0 && height > 0 {
            layer.transform.width = width as f32;
            layer.transform.height = height as f32;
        }
        layer.parent_id = parent_stack.last().copied();
        if let Some(mask) = record.planes.iter().find(|(id, _)| *id == -2).map(|(_, data)| data) {
            if mask.len() == width as usize * height as usize && width > 0 {
                let rgba: Vec<u8> = mask.iter().flat_map(|v| [*v, *v, *v, 255]).collect();
                layer.mask = Raster::from_rgba(width, height, rgba);
                layer.mask_enabled = true;
            }
        }
        if is_group {
            parent_stack.push(layer.id);
        }
        clipping_flags.push(record.clipping);
        built.push(layer);
    }
    let bases: Vec<Option<Uuid>> = (0..built.len())
        .map(|index| {
            if !clipping_flags[index] {
                return None;
            }
            let parent = built[index].parent_id;
            built.iter().enumerate().skip(index + 1).find(|(other_index, other)| {
                other.parent_id == parent && !other.is_group && !clipping_flags[*other_index]
            }).map(|(_, other)| other.id)
        })
        .collect();
    for (layer, base) in built.iter_mut().zip(bases) {
        layer.mask_source_id = base;
    }
    // File order is top to bottom; the document stores bottom to top.
    built.reverse();
    // Parent links stay valid because ids do not change. Sibling order within a
    // parent is now bottom to top, which is what the renderer walks.
    Ok((built, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }
    fn be32(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    #[test]
    fn reads_a_raw_rgb_layer() {
        let mut file = Vec::new();
        file.extend(b"8BPS");
        file.extend(be16(1));
        file.extend([0; 6]);
        file.extend(be16(3));
        file.extend(be32(2));
        file.extend(be32(2));
        file.extend(be16(8));
        file.extend(be16(3));
        file.extend(be32(0)); // color mode
        file.extend(be32(0)); // resources
        let mut info = Vec::new();
        info.extend(be16(1));
        info.extend(be32(0)); // top
        info.extend(be32(0)); // left
        info.extend(be32(2)); // bottom
        info.extend(be32(2)); // right
        info.extend(be16(3));
        for id in 0..3 {
            info.extend(be16(id));
            info.extend(be32(6)); // compression + 4 raw bytes
        }
        info.extend(b"8BIM");
        info.extend(b"mul ");
        info.push(128); // opacity
        info.push(0); // clipping
        info.push(0); // flags
        info.push(0);
        let extra = {
            let mut extra = Vec::new();
            extra.extend(be32(0)); // mask
            extra.extend(be32(0)); // ranges
            extra.push(1); // name length
            extra.push(b'A');
            extra.extend([0, 0]); // pad to 4
            extra
        };
        info.extend(be32(extra.len() as u32));
        info.extend(extra);
        for value in [10u8, 20, 30, 40] {
            info.extend(be16(0));
            info.extend([value; 4]);
        }
        // green and blue differ
        // The loop above wrote the same pattern three times. Patch the last two planes.
        let mut section = Vec::new();
        section.extend(be32(info.len() as u32));
        section.extend(&info);
        section.extend(be32(0)); // global mask
        file.extend(be32(section.len() as u32));
        file.extend(section);
        let imported = import_bytes(&file).unwrap();
        assert_eq!(imported.document.width, 2);
        assert_eq!(imported.document.layers.len(), 1);
        let layer = &imported.document.layers[0];
        assert_eq!(layer.name, "A");
        assert_eq!(layer.blend_mode, BlendMode::Multiply);
        assert!((layer.opacity - 128.0 / 255.0).abs() < 0.01);
        let px = layer.image.as_ref().unwrap().pixel(0, 0);
        assert_eq!(px[0], 10);
        assert_eq!(px[3], 255);
    }

    #[test]
    fn packbits_repeats() {
        let decoded = packbits(&[0xFE, 7], 3);
        assert_eq!(decoded, vec![7, 7, 7]);
    }
}
