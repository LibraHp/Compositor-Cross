//! Alternatives for formats this program does not ship a codec for.
//!
//! HEIC and HEIF are opened from the largest JPEG preview embedded in the file.
//! Camera RAW is read with `rawloader` and finished with a small bilinear demosaic
//! and the camera white balance, instead of Adobe Camera Raw.

use std::path::Path;

use crate::document::MAX_SIDE;
use crate::raster::Raster;

pub fn import_heic(path: &Path) -> Result<Raster, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("无法读取 HEIC: {err}"))?;
    let jpeg = largest_jpeg(&bytes).ok_or("这个 HEIC 里没有可提取的 JPEG 预览")?;
    let image = image::load_from_memory(&jpeg).map_err(|err| format!("HEIC 预览无法解码: {err}"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    Raster::from_rgba(w, h, rgba.into_raw()).ok_or_else(|| "HEIC 预览像素无效".into())
}

pub fn largest_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut best: Option<Vec<u8>> = None;
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            if let Some(end) = jpeg_end(&bytes[i..]) {
                let jpeg = &bytes[i..i + end];
                if jpeg.len() > 2048 && best.as_ref().map(|old| jpeg.len() > old.len()).unwrap_or(true) {
                    best = Some(jpeg.to_vec());
                }
                i += end.max(2);
                continue;
            }
        }
        i += 1;
    }
    best
}

fn jpeg_end(bytes: &[u8]) -> Option<usize> {
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD9 {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

pub fn import_raw(path: &Path) -> Result<Raster, String> {
    let raw = rawloader::decode_file(path).map_err(|err| format!("无法读取 RAW: {err}"))?;
    let (width, height) = (raw.width, raw.height);
    if width == 0 || height == 0 || width > MAX_SIDE as usize || height > MAX_SIDE as usize {
        return Err(format!("RAW 尺寸 {width}×{height} 超出支持范围"));
    }
    let cfa = raw.cfa.to_string();
    let (samples, is_float) = match raw.data {
        rawloader::RawImageData::Integer(values) => (values.into_iter().map(|v| v as f32).collect::<Vec<_>>(), false),
        rawloader::RawImageData::Float(values) => (values, true),
    };
    let black = raw.blacklevels[0] as f32;
    let white = raw.whitelevels[0].max(raw.blacklevels[0] + 1) as f32;
    let wb = raw.wb_coeffs;
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let mut rgb = [0.0f32; 3];
            for channel in 0..3 {
                rgb[channel] = average_channel(&samples, width, height, x, y, channel, &cfa);
            }
            if !is_float {
                for c in &mut rgb {
                    *c = ((*c - black) / (white - black)).clamp(0.0, 1.0);
                }
            }
            rgb[0] *= wb[0].max(0.01);
            rgb[1] *= wb[1].max(0.01);
            rgb[2] *= wb[2].max(0.01);
            let peak = rgb[0].max(rgb[1]).max(rgb[2]).max(1.0);
            let i = (y * width + x) * 4;
            for c in 0..3 {
                rgba[i + c] = (rgb[c] / peak * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            rgba[i + 3] = 255;
        }
    }
    Raster::from_rgba(width as u32, height as u32, rgba).ok_or_else(|| "RAW 像素无效".into())
}

fn average_channel(samples: &[f32], width: usize, height: usize, x: usize, y: usize, channel: usize, cfa: &str) -> f32 {
    if cfa_at(cfa, x, y) == channel {
        return samples.get(y * width + x).copied().unwrap_or(0.0);
    }
    let mut acc = 0.0;
    let mut n = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let sx = x as i32 + dx;
            let sy = y as i32 + dy;
            if sx < 0 || sy < 0 || sx >= width as i32 || sy >= height as i32 {
                continue;
            }
            if cfa_at(cfa, sx as usize, sy as usize) == channel {
                acc += samples[sy as usize * width + sx as usize];
                n += 1.0;
            }
        }
    }
    if n == 0.0 { 0.0 } else { acc / n }
}

fn cfa_at(cfa: &str, x: usize, y: usize) -> usize {
    let bytes = cfa.as_bytes();
    let index = (y % 2) * 2 + (x % 2);
    match bytes.get(index).copied().unwrap_or(b'G') {
        b'R' => 0,
        b'B' => 2,
        _ => 1,
    }
}

pub fn is_raw(ext: &str) -> bool {
    matches!(
        ext,
        "cr2" | "cr3" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "raf" | "rw2" | "orf" | "pef" | "dng" | "raw"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_larger_jpeg() {
        let mut bytes = vec![0u8; 16];
        bytes.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        bytes.extend(vec![0u8; 8]);
        let mut big = vec![0xFF, 0xD8, 0xFF];
        big.extend(vec![1u8; 3000]);
        big.extend_from_slice(&[0xFF, 0xD9]);
        bytes.extend(big);
        let found = largest_jpeg(&bytes).unwrap();
        assert!(found.len() > 3000);
        assert_eq!(&found[..3], &[0xFF, 0xD8, 0xFF]);
    }
}
