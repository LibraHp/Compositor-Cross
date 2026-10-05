//! `.comp` packages: a directory with `manifest.json` and `images/<UUID>.png`.
//!
//! New saves use format version 11. Fields this build does not edit are kept
//! so a project can go back to the macOS app without losing text, effects or
//! an unsupported adjustment.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use uuid::Uuid;

use crate::blend::BlendMode;
use crate::effects::LayerEffects;
use crate::document::{
    Adjustment, Document, Guide, GuideAxis, Layer, LevelRange, Sampling, Transform, MAX_LAYERS,
    MAX_SIDE,
};
use crate::raster::Raster;

const FORMAT: &str = "com.compositor.project";

pub fn load(path: &Path) -> Result<Document, String> {
    let path = resolve_package(path)?;
    let manifest_path = path.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).map_err(|err| format!("无法读取清单: {err}"))?;
    let mut root: Value = serde_json::from_str(&text).map_err(|err| format!("清单不是有效的 JSON: {err}"))?;
    let obj = root.as_object_mut().ok_or("清单必须是 JSON 对象")?;
    let format = obj.get("format").and_then(|v| v.as_str()).unwrap_or("");
    if format != FORMAT {
        return Err("这不是 Compositor 工程（format 不是 com.compositor.project）".into());
    }
    let version = obj.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if !(1..=11).contains(&version) {
        return Err(format!("不支持的工程版本 {version}，此版本可读 1–11"));
    }
    let color = obj.get("colorSpace").and_then(|v| v.as_str()).unwrap_or("");
    if color != "sRGB" {
        return Err("仅支持 sRGB 工程".into());
    }
    let width = obj.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = obj.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err("画布尺寸无效".into());
    }
    let document_id = parse_uuid(obj.get("documentID").and_then(|v| v.as_str()).unwrap_or(""))
        .ok_or("documentID 无效")?;
    let resolution = obj
        .get("resolution")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && (1.0..=9600.0).contains(v))
        .unwrap_or(72.0);
    let active = obj
        .get("activeLayerID")
        .and_then(|v| v.as_str())
        .and_then(parse_uuid);
    let layers_value = obj
        .remove("layers")
        .ok_or("清单缺少 layers")?;
    let layer_values = layers_value.as_array().ok_or("layers 必须是数组")?;
    if layer_values.len() > MAX_LAYERS {
        return Err("图层数量超过上限".into());
    }
    let mut layers = Vec::with_capacity(layer_values.len());
    let mut pixels = 0u64;
    for value in layer_values {
        let layer = load_layer(path.as_path(), value, &mut pixels)?;
        layers.push(layer);
    }
    validate_hierarchy(&layers)?;
    let guides = obj
        .remove("guides")
        .map(load_guides)
        .transpose()?
        .unwrap_or_default();
    obj.remove("format");
    obj.remove("version");
    obj.remove("colorSpace");
    obj.remove("resolution");
    obj.remove("documentID");
    obj.remove("width");
    obj.remove("height");
    obj.remove("activeLayerID");
    Ok(Document {
        id: document_id,
        width,
        height,
        resolution,
        layers,
        active_layer_id: active,
        guides,
        extras: obj.clone(),
    })
}

fn load_layer(package: &Path, value: &Value, pixels: &mut u64) -> Result<Layer, String> {
    let obj = value.as_object().ok_or("图层记录必须是对象")?;
    let id = parse_uuid(obj.get("id").and_then(|v| v.as_str()).unwrap_or("")).ok_or("图层 id 无效")?;
    let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err("图层名称为空".into());
    }
    let visible = obj.get("isVisible").and_then(|v| v.as_bool()).unwrap_or(true);
    let is_group = obj.get("isGroup").and_then(|v| v.as_bool()).unwrap_or(false);
    let opacity = obj
        .get("opacity")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(format!("图层 {name} 的不透明度无效"));
    }
    let blend = obj
        .get("blendMode")
        .and_then(|v| v.as_str())
        .and_then(BlendMode::parse)
        .unwrap_or(BlendMode::Normal);
    let transform = parse_transform(obj.get("transform").ok_or("图层缺少 transform")?)?;
    let parent_id = obj
        .get("parentID")
        .and_then(|v| v.as_str())
        .and_then(parse_uuid);
    let mask_source_id = obj
        .get("maskSourceID")
        .and_then(|v| v.as_str())
        .and_then(parse_uuid);
    let image = match obj.get("imageFile").and_then(|v| v.as_str()) {
        Some(file) => Some(read_png(package, file, pixels, false)?),
        None => None,
    };
    if is_group && image.is_some() {
        return Err(format!("组 {name} 不能带图像"));
    }
    let mask = match obj.get("maskFile").and_then(|v| v.as_str()) {
        Some(file) => Some(read_png(package, file, pixels, true)?),
        None => None,
    };
    let mask_enabled = obj.get("maskEnabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let mask_linked = obj.get("maskLinked").and_then(|v| v.as_bool()).unwrap_or(true);
    let mask_placement = obj
        .get("maskPlacement")
        .map(parse_transform)
        .transpose()?;
    let adjustment = obj.get("adjustment").cloned().map(parse_adjustment);
    let effects = obj.get("effects").map(LayerEffects::from_value).unwrap_or_default();
    let mut extras = obj.clone();
    for key in [
        "id",
        "name",
        "isVisible",
        "isGroup",
        "opacity",
        "blendMode",
        "transform",
        "imageFile",
        "parentID",
        "maskFile",
        "maskEnabled",
        "maskLinked",
        "maskPlacement",
        "maskSourceID",
        "adjustment",
        "effects",
    ] {
        extras.remove(key);
    }
    Ok(Layer {
        id,
        name,
        visible,
        opacity,
        blend_mode: blend,
        transform,
        image,
        mask,
        mask_enabled,
        mask_linked,
        mask_placement,
        parent_id,
        is_group,
        mask_source_id,
        adjustment,
        effects,
        extras,
        revision: 1,
    })
}

fn read_png(package: &Path, file: &str, pixels: &mut u64, mask: bool) -> Result<Raster, String> {
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return Err(format!("不安全的图像路径 {file}"));
    }
    let path = package.join("images").join(file);
    let bytes = fs::read(&path).map_err(|_| format!("缺少图像 {file}"))?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(format!("{file} 超过 512 MiB"));
    }
    let image = image::load_from_memory(&bytes).map_err(|err| format!("{file} 无法解码: {err}"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 || w > 30_000 || h > 30_000 {
        return Err(format!("{file} 尺寸无效"));
    }
    *pixels += w as u64 * h as u64;
    if *pixels > 200_000_000 {
        return Err("工程像素总量超过上限".into());
    }
    let mut raw = rgba.into_raw();
    if mask {
        for px in raw.chunks_exact_mut(4) {
            let coverage = px[0];
            px[0] = coverage;
            px[1] = coverage;
            px[2] = coverage;
            px[3] = 255;
        }
    }
    Raster::from_rgba(w, h, raw).ok_or_else(|| format!("{file} 像素无效"))
}

fn parse_transform(value: &Value) -> Result<Transform, String> {
    let obj = value.as_object().ok_or("transform 必须是对象")?;
    let origin = parse_pair(obj.get("origin").ok_or("transform.origin 缺失")?)?;
    let size = parse_pair(obj.get("size").ok_or("transform.size 缺失")?)?;
    if !size.0.is_finite() || !size.1.is_finite() || size.0 < 1.0 || size.1 < 1.0 {
        return Err("transform.size 无效".into());
    }
    Ok(Transform {
        origin_x: origin.0,
        origin_y: origin.1,
        width: size.0,
        height: size.1,
        rotation: obj.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        flip_x: obj.get("flipX").and_then(|v| v.as_bool()).unwrap_or(false),
        flip_y: obj.get("flipY").and_then(|v| v.as_bool()).unwrap_or(false),
        sampling: Sampling::parse(obj.get("sampling").and_then(|v| v.as_str()).unwrap_or("")),
    })
}

fn parse_pair(value: &Value) -> Result<(f32, f32), String> {
    if let Some(arr) = value.as_array() {
        if arr.len() >= 2 {
            let x = arr[0].as_f64().ok_or("坐标不是数字")? as f32;
            let y = arr[1].as_f64().ok_or("坐标不是数字")? as f32;
            return Ok((x, y));
        }
    }
    if let Some(obj) = value.as_object() {
        let x = obj
            .get("x")
            .or_else(|| obj.get("width"))
            .and_then(|v| v.as_f64())
            .ok_or("坐标缺少 x")? as f32;
        let y = obj
            .get("y")
            .or_else(|| obj.get("height"))
            .and_then(|v| v.as_f64())
            .ok_or("坐标缺少 y")? as f32;
        return Ok((x, y));
    }
    Err("坐标格式无效".into())
}

fn parse_adjustment(value: Value) -> Adjustment {
    let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "Invert" => Adjustment::Invert,
        "Hue/Saturation" => Adjustment::HueSat {
            hue: value.get("hue").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            saturation: value.get("saturation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            lightness: value.get("lightness").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        },
        "Exposure" => {
            let settings = value.get("exposureSettings");
            Adjustment::Exposure {
                exposure: settings
                    .and_then(|s| s.get("exposure"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
                offset: settings
                    .and_then(|s| s.get("offset"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
                gamma: settings
                    .and_then(|s| s.get("gamma"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0) as f32,
            }
        }
        "Levels" => Adjustment::Levels {
            ranges: parse_levels(value.get("levels")),
        },
        "Black & White" => Adjustment::BlackWhite {
            weights: value
                .get("blackWhiteSettings")
                .map(|settings| {
                    [
                        settings.get("reds").and_then(|v| v.as_f64()).unwrap_or(40.0) as f32,
                        settings.get("yellows").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32,
                        settings.get("greens").and_then(|v| v.as_f64()).unwrap_or(40.0) as f32,
                        settings.get("cyans").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32,
                        settings.get("blues").and_then(|v| v.as_f64()).unwrap_or(20.0) as f32,
                        settings.get("magentas").and_then(|v| v.as_f64()).unwrap_or(80.0) as f32,
                    ]
                })
                .unwrap_or([40.0, 60.0, 40.0, 60.0, 20.0, 80.0]),
        },
        "Gaussian Blur" => Adjustment::GaussianBlur {
            radius: value.get("blurRadius").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32,
        },
        "Motion Blur" => Adjustment::MotionBlur {
            angle: value.get("motionAngle").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            distance: value.get("motionDistance").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32,
        },
        "Curves" => Adjustment::Curves {
            channels: parse_curves(value.get("curves")),
        },
        "Grain" => {
            let settings = value.get("grainSettings");
            Adjustment::Grain {
                amount: settings.and_then(|s| s.get("amount")).and_then(|v| v.as_f64()).unwrap_or(25.0) as f32,
                size: settings.and_then(|s| s.get("size")).and_then(|v| v.as_f64()).unwrap_or(1.5) as f32,
                roughness: settings.and_then(|s| s.get("roughness")).and_then(|v| v.as_f64()).unwrap_or(50.0) as f32,
                seed: settings.and_then(|s| s.get("seed")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            }
        }
        "Color Balance" => {
            let settings = value.get("colorBalanceSettings");
            let axis = |prefix: &str| {
                [
                    settings.and_then(|s| s.get(&format!("{prefix}CyanRed"))).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    settings.and_then(|s| s.get(&format!("{prefix}MagentaGreen"))).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    settings.and_then(|s| s.get(&format!("{prefix}YellowBlue"))).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                ]
            };
            Adjustment::ColorBalance {
                shadow: axis("shadow"),
                midtone: axis("mid"),
                highlight: axis("highlight"),
                preserve_luminosity: settings.and_then(|s| s.get("preserveLuminosity")).and_then(|v| v.as_bool()).unwrap_or(true),
            }
        }
        "Gradient Map" => {
            let settings = value.get("gradientMapSettings");
            Adjustment::GradientMap {
                shadow: rgb_of(settings.and_then(|s| s.get("shadows"))),
                highlight: rgb_of(settings.and_then(|s| s.get("highlights"))),
                reversed: settings.and_then(|s| s.get("reversed")).and_then(|v| v.as_bool()).unwrap_or(false),
            }
        }
        _ => Adjustment::Other(value),
    }
}

fn rgb_of(value: Option<&Value>) -> [f32; 3] {
    let Some(value) = value else {
        return [0.0, 0.0, 0.0];
    };
    [
        value.get("red").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        value.get("green").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        value.get("blue").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    ]
}

fn parse_curves(value: Option<&Value>) -> [Vec<(f32, f32)>; 4] {
    let mut channels = crate::document::identity_curves();
    let Some(list) = value.and_then(|v| v.get("channels")).and_then(|v| v.as_array()) else {
        return channels;
    };
    for (i, channel) in list.iter().take(4).enumerate() {
        let Some(points) = channel.as_array() else { continue };
        let parsed: Vec<(f32, f32)> = points
            .iter()
            .filter_map(|point| {
                Some((
                    point.get("x")?.as_f64()? as f32,
                    point.get("y")?.as_f64()? as f32,
                ))
            })
            .collect();
        if parsed.len() >= 2 {
            channels[i] = parsed;
        }
    }
    channels
}

fn parse_levels(value: Option<&Value>) -> [LevelRange; 4] {
    let mut ranges = [LevelRange::default(); 4];
    let Some(list) = value.and_then(|v| v.get("ranges")).and_then(|v| v.as_array()) else {
        return ranges;
    };
    for (i, item) in list.iter().take(4).enumerate() {
        ranges[i] = LevelRange {
            black: item.get("black").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            gamma: item.get("gamma").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            white: item.get("white").and_then(|v| v.as_f64()).unwrap_or(255.0) as f32,
            output_black: item.get("outputBlack").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            output_white: item.get("outputWhite").and_then(|v| v.as_f64()).unwrap_or(255.0) as f32,
        };
    }
    ranges
}

fn load_guides(value: Value) -> Result<Vec<Guide>, String> {
    let list = value.as_array().ok_or("guides 必须是数组")?;
    if list.len() > 1000 {
        return Err("参考线超过 1000 条".into());
    }
    let mut guides = Vec::new();
    for item in list {
        let id = parse_uuid(item.get("id").and_then(|v| v.as_str()).unwrap_or("")).ok_or("参考线 id 无效")?;
        let axis = match item.get("axis").and_then(|v| v.as_str()).unwrap_or("") {
            "horizontal" => GuideAxis::Horizontal,
            "vertical" => GuideAxis::Vertical,
            _ => return Err("参考线方向无效".into()),
        };
        let position = item.get("position").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        guides.push(Guide { id, axis, position });
    }
    Ok(guides)
}

fn validate_hierarchy(layers: &[Layer]) -> Result<(), String> {
    let ids: Vec<Uuid> = layers.iter().map(|layer| layer.id).collect();
    if ids.len() != ids.iter().collect::<std::collections::HashSet<_>>().len() {
        return Err("图层 id 重复".into());
    }
    for layer in layers {
        if let Some(parent) = layer.parent_id {
            let parent_layer = layers.iter().find(|item| item.id == parent);
            if !parent_layer.is_some_and(|item| item.is_group) {
                return Err(format!("图层 {} 的父级不是组", layer.name));
            }
        }
    }
    Ok(())
}

pub fn save(doc: &Document, path: &Path) -> Result<(), String> {
    if doc.width == 0 || doc.height == 0 || doc.width > MAX_SIDE || doc.height > MAX_SIDE {
        return Err("画布尺寸超出可保存范围".into());
    }
    let path = ensure_comp_extension(path);
    let staging = staging_path(&path);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|err| format!("无法清理临时目录: {err}"))?;
    }
    fs::create_dir_all(staging.join("images")).map_err(|err| format!("无法创建工程目录: {err}"))?;
    let manifest = build_manifest(doc)?;
    for layer in &doc.layers {
        if let Some(image) = &layer.image {
            write_png(&staging.join("images").join(format!("{}.png", uuid_string(layer.id))), image, false)?;
        }
        if let Some(mask) = &layer.mask {
            write_png(
                &staging.join("images").join(format!("{}.mask.png", uuid_string(layer.id))),
                mask,
                true,
            )?;
        }
    }
    let json = serde_json::to_string_pretty(&manifest).map_err(|err| format!("无法编码清单: {err}"))?;
    let tmp_manifest = staging.join(".manifest.json.tmp");
    fs::write(&tmp_manifest, json.as_bytes()).map_err(|err| format!("无法写入清单: {err}"))?;
    fs::rename(&tmp_manifest, staging.join("manifest.json")).map_err(|err| format!("无法完成清单: {err}"))?;
    replace_dir(&staging, &path)?;
    Ok(())
}

fn build_manifest(doc: &Document) -> Result<Value, String> {
    let mut root = doc.extras.clone();
    root.insert("format".into(), json!(FORMAT));
    root.insert("version".into(), json!(11));
    root.insert("colorSpace".into(), json!("sRGB"));
    root.insert("resolution".into(), json!(doc.resolution));
    root.insert("documentID".into(), json!(uuid_string(doc.id)));
    root.insert("width".into(), json!(doc.width));
    root.insert("height".into(), json!(doc.height));
    if let Some(id) = doc.active_layer_id {
        root.insert("activeLayerID".into(), json!(uuid_string(id)));
    }
    let mut layers = Vec::new();
    for layer in &doc.layers {
        layers.push(layer_value(layer)?);
    }
    root.insert("layers".into(), Value::Array(layers));
    if !doc.guides.is_empty() {
        root.insert(
            "guides".into(),
            json!(doc
                .guides
                .iter()
                .map(|guide| {
                    json!({
                        "id": uuid_string(guide.id),
                        "axis": match guide.axis {
                            GuideAxis::Horizontal => "horizontal",
                            GuideAxis::Vertical => "vertical",
                        },
                        "position": guide.position,
                    })
                })
                .collect::<Vec<_>>()),
        );
    }
    Ok(Value::Object(root))
}

fn layer_value(layer: &Layer) -> Result<Value, String> {
    let mut obj = layer.extras.clone();
    obj.insert("id".into(), json!(uuid_string(layer.id)));
    obj.insert("name".into(), json!(layer.name));
    obj.insert("isVisible".into(), json!(layer.visible));
    obj.insert("isGroup".into(), json!(layer.is_group));
    obj.insert("opacity".into(), json!(layer.opacity as f64));
    obj.insert("blendMode".into(), json!(layer.blend_mode.as_str()));
    obj.insert("transform".into(), transform_value(layer.transform));
    if let Some(parent) = layer.parent_id {
        obj.insert("parentID".into(), json!(uuid_string(parent)));
    } else {
        obj.remove("parentID");
    }
    if layer.image.is_some() && !layer.is_group {
        obj.insert("imageFile".into(), json!(format!("{}.png", uuid_string(layer.id))));
    } else {
        obj.remove("imageFile");
    }
    if layer.mask.is_some() {
        obj.insert("maskFile".into(), json!(format!("{}.mask.png", uuid_string(layer.id))));
        obj.insert("maskEnabled".into(), json!(layer.mask_enabled));
        obj.insert("maskLinked".into(), json!(layer.mask_linked));
        if let Some(place) = layer.mask_placement {
            obj.insert("maskPlacement".into(), transform_value(place));
        }
    } else {
        obj.remove("maskFile");
        obj.remove("maskEnabled");
        obj.remove("maskLinked");
        obj.remove("maskPlacement");
    }
    if let Some(source) = layer.mask_source_id {
        obj.insert("maskSourceID".into(), json!(uuid_string(source)));
    } else {
        obj.remove("maskSourceID");
    }
    if let Some(adjustment) = &layer.adjustment {
        obj.insert("adjustment".into(), adjustment_value(adjustment));
    } else {
        obj.remove("adjustment");
    }
    if layer.effects.is_empty() {
        obj.remove("effects");
    } else {
        obj.insert("effects".into(), layer.effects.to_value());
    }
    Ok(Value::Object(obj))
}

fn transform_value(transform: Transform) -> Value {
    json!({
        "origin": [transform.origin_x, transform.origin_y],
        "size": [transform.width, transform.height],
        "rotation": transform.rotation,
        "flipX": transform.flip_x,
        "flipY": transform.flip_y,
        "sampling": transform.sampling.as_str(),
    })
}

fn adjustment_value(adjustment: &Adjustment) -> Value {
    match adjustment {
        Adjustment::Invert => json!({ "kind": "Invert" }),
        Adjustment::HueSat {
            hue,
            saturation,
            lightness,
        } => json!({
            "kind": "Hue/Saturation",
            "hue": hue,
            "saturation": saturation,
            "lightness": lightness,
        }),
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => json!({
            "kind": "Exposure",
            "exposureSettings": { "exposure": exposure, "offset": offset, "gamma": gamma }
        }),
        Adjustment::Levels { ranges } => json!({
            "kind": "Levels",
            "levels": {
                "ranges": ranges.iter().map(|range| json!({
                    "black": range.black,
                    "gamma": range.gamma,
                    "white": range.white,
                    "outputBlack": range.output_black,
                    "outputWhite": range.output_white,
                })).collect::<Vec<_>>()
            }
        }),
        Adjustment::BlackWhite { weights } => json!({
            "kind": "Black & White",
            "blackWhiteSettings": {
                "reds": weights[0], "yellows": weights[1], "greens": weights[2],
                "cyans": weights[3], "blues": weights[4], "magentas": weights[5],
            }
        }),
        Adjustment::GaussianBlur { radius } => json!({
            "kind": "Gaussian Blur",
            "blurRadius": radius,
        }),
        Adjustment::MotionBlur { angle, distance } => json!({
            "kind": "Motion Blur",
            "motionAngle": angle,
            "motionDistance": distance,
        }),
        Adjustment::Curves { channels } => json!({
            "kind": "Curves",
            "curves": {
                "channel": "RGB",
                "channels": channels.iter().map(|points| {
                    points.iter().map(|(x, y)| json!({"x": x, "y": y})).collect::<Vec<_>>()
                }).collect::<Vec<_>>()
            }
        }),
        Adjustment::Grain { amount, size, roughness, seed } => json!({
            "kind": "Grain",
            "grainSettings": { "amount": amount, "size": size, "roughness": roughness, "seed": seed }
        }),
        Adjustment::ColorBalance { shadow, midtone, highlight, preserve_luminosity } => json!({
            "kind": "Color Balance",
            "colorBalanceSettings": {
                "shadowCyanRed": shadow[0],
                "shadowMagentaGreen": shadow[1],
                "shadowYellowBlue": shadow[2],
                "midCyanRed": midtone[0],
                "midMagentaGreen": midtone[1],
                "midYellowBlue": midtone[2],
                "highlightCyanRed": highlight[0],
                "highlightMagentaGreen": highlight[1],
                "highlightYellowBlue": highlight[2],
                "preserveLuminosity": preserve_luminosity,
            }
        }),
        Adjustment::GradientMap { shadow, highlight, reversed } => json!({
            "kind": "Gradient Map",
            "gradientMapSettings": {
                "shadows": { "red": shadow[0], "green": shadow[1], "blue": shadow[2] },
                "highlights": { "red": highlight[0], "green": highlight[1], "blue": highlight[2] },
                "reversed": reversed,
            }
        }),
        Adjustment::Other(value) => value.clone(),
    }
}

fn write_png(path: &Path, raster: &Raster, mask: bool) -> Result<(), String> {
    let mut bytes = raster.pixels().to_vec();
    if mask {
        for px in bytes.chunks_exact_mut(4) {
            let coverage = px[0];
            px[0] = coverage;
            px[1] = coverage;
            px[2] = coverage;
            px[3] = 255;
        }
    }
    image::save_buffer(path, &bytes, raster.width, raster.height, image::ColorType::Rgba8)
        .map_err(|err| format!("无法写入 {}: {err}", path.display()))
}

fn replace_dir(staging: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        let mut backup_name = dest.file_name().unwrap_or_default().to_os_string();
        backup_name.push(".bak");
        let backup = dest.with_file_name(backup_name);
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
            let _ = fs::remove_file(&backup);
        }
        fs::rename(dest, &backup).map_err(|err| format!("无法替换已有工程: {err}"))?;
        if let Err(err) = fs::rename(staging, dest) {
            let _ = fs::rename(&backup, dest);
            return Err(format!("无法完成保存: {err}"));
        }
        let _ = fs::remove_dir_all(&backup);
        let _ = fs::remove_file(&backup);
    } else if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("无法创建目录: {err}"))?;
        fs::rename(staging, dest).map_err(|err| format!("无法完成保存: {err}"))?;
    } else {
        fs::rename(staging, dest).map_err(|err| format!("无法完成保存: {err}"))?;
    }
    Ok(())
}

fn resolve_package(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_dir() {
        path.to_path_buf()
    } else if path.extension().and_then(|ext| ext.to_str()) == Some("comp") && path.is_dir() {
        path.to_path_buf()
    } else if path.join("manifest.json").is_file() {
        path.to_path_buf()
    } else {
        return Err("请选择 .comp 工程文件夹".into());
    };
    if !path.join("manifest.json").is_file() {
        return Err("工程文件夹里没有 manifest.json".into());
    }
    Ok(path)
}

fn ensure_comp_extension(path: &Path) -> PathBuf {
    if path.extension().and_then(|ext| ext.to_str()) == Some("comp") {
        path.to_path_buf()
    } else {
        path.with_extension("comp")
    }
}

fn staging_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project.comp");
    path.with_file_name(format!("{name}.saving"))
}

pub fn uuid_string(id: Uuid) -> String {
    id.hyphenated()
        .encode_upper(&mut Uuid::encode_buffer())
        .to_string()
}

pub fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

pub fn export_png(raster: &Raster, path: &Path) -> Result<(), String> {
    image::save_buffer(
        path,
        raster.pixels(),
        raster.width,
        raster.height,
        image::ColorType::Rgba8,
    )
    .map_err(|err| format!("无法导出 PNG: {err}"))
}

pub fn export_jpeg(raster: &Raster, path: &Path, quality: u8) -> Result<(), String> {
    let mut rgb = Vec::with_capacity(raster.width as usize * raster.height as usize * 3);
    for px in raster.pixels().chunks_exact(4) {
        let a = px[3] as f32 / 255.0;
        for c in 0..3 {
            let v = px[c] as f32 * a + 255.0 * (1.0 - a);
            rgb.push(v.round() as u8);
        }
    }
    let file = fs::File::create(path).map_err(|err| format!("无法创建 JPEG: {err}"))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality.clamp(1, 100));
    encoder
        .encode(&rgb, raster.width, raster.height, image::ExtendedColorType::Rgb8)
        .map_err(|err| format!("无法编码 JPEG: {err}"))
}

pub fn import_image(path: &Path) -> Result<(String, Raster), String> {
    let bytes = fs::read(path).map_err(|err| format!("无法读取图像: {err}"))?;
    let image = image::load_from_memory(&bytes).map_err(|err| format!("无法解码图像: {err}"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err("图像尺寸超出支持范围".into());
    }
    if w as u64 * h as u64 > 80_000_000 {
        return Err("图像像素过多，无法作为图层载入".into());
    }
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Image")
        .to_string();
    let raster = Raster::from_rgba(w, h, rgba.into_raw()).ok_or("图像像素无效")?;
    Ok((name, raster))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;

    #[test]
    fn roundtrip_package() {
        let dir = std::env::temp_dir().join(format!("comp-test-{}", Uuid::new_v4()));
        let path = dir.join("demo.comp");
        let mut doc = Document::new(16, 8, Some([10, 20, 30, 255]));
        doc.resolution = 144.0;
        doc.add_layer(Layer::with_image("Cutout", Raster::new(4, 4, [255, 0, 0, 128])));
        save(&doc, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.width, 16);
        assert_eq!(loaded.height, 8);
        assert!((loaded.resolution - 144.0).abs() < 0.01);
        assert_eq!(loaded.layers.len(), 2);
        assert_eq!(loaded.layers[0].image.as_ref().unwrap().pixel(0, 0), [10, 20, 30, 255]);
        let manifest = std::fs::read_to_string(path.join("manifest.json")).unwrap();
        assert!(manifest.contains("com.compositor.project"));
        assert!(manifest.contains(&uuid_string(doc.id)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
