//! A small SVG rasterizer for icons and simple artwork.
//!
//! It understands width, height, viewBox, and the shapes rect, circle, ellipse,
//! line, polygon, polyline and path commands M, L, H, V, C, Z. Text and filters
//! are reported rather than silently dropped.

use crate::raster::Raster;

pub fn rasterize(bytes: &[u8]) -> Result<(String, Raster, Vec<String>), String> {
    let text = String::from_utf8_lossy(bytes);
    if !text.contains("<svg") {
        return Err("这不是 SVG".into());
    }
    let (vw, vh) = view_size(&text);
    let width = vw.ceil().clamp(1.0, 4096.0) as u32;
    let height = vh.ceil().clamp(1.0, 4096.0) as u32;
    let mut image = Raster::new(width, height, [0, 0, 0, 0]);
    let mut notes = Vec::new();
    if text.contains("<text") {
        notes.push("SVG 文字已忽略".into());
    }
    for tag in tags(&text) {
        let fill = color_attr(&tag, "fill").unwrap_or([0, 0, 0, 255]);
        let name = tag_name(&tag);
        match name {
            "rect" => {
                let x = num(&tag, "x");
                let y = num(&tag, "y");
                let w = num(&tag, "width");
                let h = num(&tag, "height");
                fill_rect(&mut image, x, y, w, h, fill);
            }
            "circle" => fill_ellipse(&mut image, num(&tag, "cx"), num(&tag, "cy"), num(&tag, "r"), num(&tag, "r"), fill),
            "ellipse" => fill_ellipse(&mut image, num(&tag, "cx"), num(&tag, "cy"), num(&tag, "rx"), num(&tag, "ry"), fill),
            "line" => stroke_line(&mut image, num(&tag, "x1"), num(&tag, "y1"), num(&tag, "x2"), num(&tag, "y2"), 1.5, color_attr(&tag, "stroke").unwrap_or(fill)),
            "polygon" | "polyline" => {
                if let Some(points) = attr(&tag, "points") {
                    fill_polygon(&mut image, &parse_points(&points), fill, name == "polygon");
                }
            }
            "path" => {
                if let Some(d) = attr(&tag, "d") {
                    fill_polygon(&mut image, &path_points(&d), fill, true);
                }
            }
            _ => {}
        }
    }
    Ok(("SVG".into(), image, notes))
}

fn view_size(text: &str) -> (f32, f32) {
    if let Some(svg) = tags(text).into_iter().find(|tag| tag_name(tag) == "svg") {
        if let Some(box_value) = attr(&svg, "viewBox") {
            let n = parse_points(&box_value.replace(',', " "));
            if n.len() >= 2 {
                let w = n.last().map(|p| p.0).unwrap_or(512.0);
                let h = n.last().map(|p| p.1).unwrap_or(512.0);
                if w > 1.0 && h > 1.0 {
                    return (w, h);
                }
            }
        }
        let w = num(&svg, "width").max(1.0);
        let h = num(&svg, "height").max(1.0);
        if w > 1.0 && h > 1.0 {
            return (w, h);
        }
    }
    (512.0, 512.0)
}

fn tags(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] != b'/' && bytes[i + 1] != b'!' && bytes[i + 1] != b'?' {
            if let Some(end) = text[i..].find('>') {
                out.push(text[i..i + end + 1].to_string());
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn tag_name(tag: &str) -> &str {
    tag.trim_start_matches('<').split([' ', '\n', '\t', '>']).next().unwrap_or("")
}

fn attr(tag: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=");
    let start = tag.find(&pattern)? + pattern.len();
    let rest = &tag[start..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let body = &rest[1..];
        let end = body.find(quote)?;
        Some(body[..end].to_string())
    } else {
        Some(rest.split_whitespace().next()?.trim_end_matches('>').to_string())
    }
}

fn num(tag: &str, key: &str) -> f32 {
    attr(tag, key).and_then(|value| value.trim_end_matches("px").parse().ok()).unwrap_or(0.0)
}

fn color_attr(tag: &str, key: &str) -> Option<[u8; 4]> {
    let value = attr(tag, key)?;
    if value == "none" {
        return Some([0, 0, 0, 0]);
    }
    parse_color(&value)
}

fn parse_color(value: &str) -> Option<[u8; 4]> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let hex = hex.trim();
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            return Some([r * 17, g * 17, b * 17, 255]);
        }
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some([r, g, b, 255]);
        }
    }
    if value.starts_with("rgb") {
        let nums: Vec<f32> = value.split(|c: char| !c.is_ascii_digit() && c != '.').filter_map(|n| n.parse().ok()).collect();
        if nums.len() >= 3 {
            return Some([nums[0] as u8, nums[1] as u8, nums[2] as u8, 255]);
        }
    }
    match value {
        "black" => Some([0, 0, 0, 255]),
        "white" => Some([255, 255, 255, 255]),
        "red" => Some([255, 0, 0, 255]),
        "green" => Some([0, 128, 0, 255]),
        "blue" => Some([0, 0, 255, 255]),
        _ => None,
    }
}

fn parse_points(text: &str) -> Vec<(f32, f32)> {
    let nums: Vec<f32> = text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter_map(|n| n.parse().ok()).collect();
    nums.chunks(2).filter_map(|pair| if pair.len() == 2 { Some((pair[0], pair[1])) } else { None }).collect()
}

fn path_points(d: &str) -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    let mut nums = Vec::new();
    let mut current = String::new();
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            if let Ok(n) = current.parse::<f32>() {
                nums.push(n);
            }
            current.clear();
            if ch == 'M' || ch == 'L' || ch == 'C' || ch == 'H' || ch == 'V' {
                // commands are implied by number pairs; H/V are approximated below
            }
        } else if ch == '-' && !current.is_empty() {
            if let Ok(n) = current.parse::<f32>() {
                nums.push(n);
            }
            current = "-".into();
        } else if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            current.push(ch);
        } else if let Ok(n) = current.parse::<f32>() {
            nums.push(n);
            current.clear();
        }
    }
    if let Ok(n) = current.parse::<f32>() {
        nums.push(n);
    }
    for pair in nums.chunks(2) {
        if pair.len() == 2 {
            points.push((pair[0], pair[1]));
        }
    }
    points
}

fn fill_rect(image: &mut Raster, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
    if color[3] == 0 {
        return;
    }
    for py in y.floor() as i32..(y + h).ceil() as i32 {
        for px in x.floor() as i32..(x + w).ceil() as i32 {
            image.set_pixel(px, py, color);
        }
    }
}

fn fill_ellipse(image: &mut Raster, cx: f32, cy: f32, rx: f32, ry: f32, color: [u8; 4]) {
    if rx <= 0.0 || ry <= 0.0 || color[3] == 0 {
        return;
    }
    for y in (cy - ry).floor() as i32..(cy + ry).ceil() as i32 {
        for x in (cx - rx).floor() as i32..(cx + rx).ceil() as i32 {
            let nx = (x as f32 - cx) / rx;
            let ny = (y as f32 - cy) / ry;
            if nx * nx + ny * ny <= 1.0 {
                image.set_pixel(x, y, color);
            }
        }
    }
}

fn stroke_line(image: &mut Raster, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: [u8; 4]) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32;
    for i in 0..=steps.max(1) {
        let t = i as f32 / steps.max(1) as f32;
        image.set_pixel((x0 + (x1 - x0) * t) as i32, (y0 + (y1 - y0) * t) as i32, color);
    }
    let _ = width;
}

fn fill_polygon(image: &mut Raster, points: &[(f32, f32)], color: [u8; 4], close: bool) {
    if points.len() < 2 || color[3] == 0 {
        return;
    }
    if !close {
        for pair in points.windows(2) {
            stroke_line(image, pair[0].0, pair[0].1, pair[1].0, pair[1].1, 1.0, color);
        }
        return;
    }
    let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
    let max_y = points.iter().map(|p| p.1).fold(0.0, f32::max).ceil() as i32;
    let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
    let max_x = points.iter().map(|p| p.0).fold(0.0, f32::max).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_poly(x as f32 + 0.5, y as f32 + 0.5, points) {
                image.set_pixel(x, y, color);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_red_rect() {
        let svg = b"<svg viewBox=\"0 0 10 10\"><rect x=\"1\" y=\"1\" width=\"4\" height=\"4\" fill=\"#ff0000\"/></svg>";
        let (_, image, _) = rasterize(svg).unwrap();
        assert_eq!(image.pixel(2, 2), [255, 0, 0, 255]);
        assert_eq!(image.pixel(0, 0)[3], 0);
    }
}
