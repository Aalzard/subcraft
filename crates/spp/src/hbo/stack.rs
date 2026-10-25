//! Build `subcraft-spp-stack-v1` JSON from a decoded `DataDocument` root.

use serde::Serialize;

use super::decode::{
    array_items, field, object_val, prim_bool, prim_bytes, prim_int, string_val, UNode, Value,
};
use crate::stack::{
    SppChannelBlend, SppContentEffect, SppDecalHit, SppLayerNode, SppMaskEffect, SppMaskEffectKind,
    SppMaterialStack, SppPaintStroke, SppProceduralChannel, SppStackFile, SppTextureSetStack,
};

#[derive(Serialize)]
struct StackExport {
    format: &'static str,
    hbo_format: String,
    data_version: u32,
    materials: Vec<SppMaterialStack>,
}

pub fn build_stack_from_document(
    document_bin: &[u8],
) -> Result<(SppStackFile, String, u32), String> {
    let (root, hbo_format, data_version) = super::decode::decode(document_bin)?;
    if root.type_name != "DataDocument" {
        return Err(format!("expected DataDocument root, got {}", root.type_name));
    }
    let materials_out = materials_from_root(&root)?;
    let file = SppStackFile {
        format: "subcraft-spp-stack-v1".into(),
        hbo_format: Some(hbo_format.to_string()),
        data_version: Some(data_version),
        materials: materials_out,
    };
    Ok((file, hbo_format.to_string(), data_version))
}

pub fn write_stack_json(document_bin: &[u8], path: &std::path::Path) -> Result<SppStackFile, String> {
    let (file, hbo_format, data_version) = build_stack_from_document(document_bin)?;
    let export = StackExport {
        format: "subcraft-spp-stack-v1",
        hbo_format,
        data_version,
        materials: file.materials.clone(),
    };
    let text = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file)
}

/// Decode a Substance Painter `.spsm` Smart Material HDF5 file into a `SppLayerNode` tree.
pub fn decode_spsm(path: &std::path::Path) -> Result<SppLayerNode, String> {
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Smart Material");
    let h5 = hdf5_reader::Hdf5File::open(path)
        .map_err(|e| format!("{}: HDF5 open: {e}", path.display()))?;
    let mut last_err = String::from("no preset dataset found in .spsm");
    for ds_name in preset_dataset_names(&h5) {
        if let Ok(ds) = h5.dataset(&ds_name) {
            if let Ok(raw) = ds.read_raw_bytes() {
                match decode_spsm_from_preset(&raw, fallback_name) {
                    Ok(node) => return Ok(node),
                    Err(e) => last_err = format!("{ds_name}: {e}"),
                }
            }
        }
    }
    Err(format!("{}: {last_err}", path.display()))
}

/// Decode a `preset*.bin` HBO buffer from a `.spsm` Smart Material into a `SppLayerNode`.
pub fn decode_spsm_from_preset(preset_bin: &[u8], fallback_name: &str) -> Result<SppLayerNode, String> {
    let (root, _, _) = super::decode::decode(preset_bin)?;
    let group = if root.type_name == "DataLayersPreset" {
        field(&root, "layerGroup")
            .and_then(object_val)
            .ok_or_else(|| "DataLayersPreset.layerGroup missing".to_string())?
    } else if root.type_name == "DataLayerGroup" {
        &root
    } else {
        return Err(format!(
            "expected DataLayersPreset or DataLayerGroup root, got {}",
            root.type_name
        ));
    };
    let mut node = layer_export(group, 0);
    if node.name.trim().is_empty() || node.name == "DataLayerGroup" {
        node.name = fallback_name.to_string();
    }
    Ok(node)
}

/// Decode a Substance Painter `.spmsk` Smart Mask HDF5 file into a list of `SppMaskEffect`s.
pub fn decode_spmsk(path: &std::path::Path) -> Result<Vec<SppMaskEffect>, String> {
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Smart Mask");
    let h5 = hdf5_reader::Hdf5File::open(path)
        .map_err(|e| format!("{}: HDF5 open: {e}", path.display()))?;
    let mut last_err = String::from("no preset dataset found in .spmsk");
    for ds_name in preset_dataset_names(&h5) {
        if let Ok(ds) = h5.dataset(&ds_name) {
            if let Ok(raw) = ds.read_raw_bytes() {
                match decode_spmsk_from_preset(&raw, fallback_name) {
                    Ok(effects) if !effects.is_empty() => return Ok(effects),
                    Ok(_) => last_err = format!("{ds_name}: no decodable mask effects"),
                    Err(e) => last_err = format!("{ds_name}: {e}"),
                }
            }
        }
    }
    Err(format!("{}: {last_err}", path.display()))
}

fn preset_dataset_names(h5: &hdf5_reader::Hdf5File) -> Vec<String> {
    let mut names: Vec<String> = ["preset_2.bin", "preset_1.bin", "preset.bin"]
        .into_iter()
        .map(str::to_string)
        .collect();
    if let Ok(root) = h5.root_group() {
        if let Ok((_, datasets)) = root.members() {
            for ds in datasets {
                let n = ds.name().trim_start_matches('/').to_string();
                if n.contains("preset") && n.ends_with(".bin") && !names.iter().any(|x| x == &n) {
                    names.push(n);
                }
            }
        }
    }
    names
}

/// Decode a `preset*.bin` HBO buffer from a `.spmsk` Smart Mask into `Vec<SppMaskEffect>`.
pub fn decode_spmsk_from_preset(
    preset_bin: &[u8],
    fallback_name: &str,
) -> Result<Vec<SppMaskEffect>, String> {
    let (root, _, _) = super::decode::decode(preset_bin)?;
    let actions = field(&root, "actionsStack")
        .or_else(|| field(&root, "maskActions"))
        .and_then(object_val)
        .unwrap_or(&root);
    let mut out = Vec::new();
    collect_mask_action_nodes(actions, fallback_name, &mut out);
    Ok(out)
}

fn materials_from_root(root: &UNode) -> Result<Vec<SppMaterialStack>, String> {
    let mut materials_out = Vec::new();
    let materials_val = field(root, "materials").ok_or("DataDocument.materials missing")?;
    for el in array_items(materials_val) {
        let mat = object_val(el).filter(|n| n.type_name == "DataMaterial").ok_or("expected DataMaterial")?;
        let slot_name = string_val(field(mat, "sceneMaterialName").unwrap_or(&Value::Null))
            .unwrap_or_else(|| "Material".into());
        let rlog = prim_int(field(mat, "resolutionLog2").unwrap_or(&Value::Null));
        let resolution = resolution_from_log2(rlog);
        let mut texture_sets = Vec::new();
        let empty_stacks: Vec<Value> = Vec::new();
        let stacks_slice = field(mat, "stacks")
            .map(array_items)
            .unwrap_or(empty_stacks.as_slice());
        for st_el in stacks_slice {
            let st = object_val(st_el)
                .filter(|n| n.type_name == "DataMaterialStack")
                .ok_or("expected DataMaterialStack")?;
            let stack_label = string_val(field(st, "label").unwrap_or(&Value::Null))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| slot_name.clone());
            let stack_obj = field(st, "stack").and_then(object_val);
            let layers = stack_obj
                .map(|s| stack_layer_items(s))
                .unwrap_or_default()
                .into_iter()
                .map(|n| layer_export(&n, 0))
                .collect();
            texture_sets.push(SppTextureSetStack {
                name: stack_label,
                material_slot_name: slot_name.clone(),
                resolution,
                layers,
            });
        }
        materials_out.push(SppMaterialStack {
            scene_material: slot_name,
            resolution,
            texture_sets,
        });
    }
    Ok(materials_out)
}

fn resolution_from_log2(rlog: Option<i64>) -> u32 {
    let exp = rlog.unwrap_or(10).clamp(7, 13) as u32;
    let r = 1u32 << exp;
    r.clamp(128, 8192)
}

fn stack_layer_items(stack_layers: &UNode) -> Vec<UNode> {
    let mut out = Vec::new();
    let empty: &[Value] = &[];
    let items = field(stack_layers, "items").map(array_items).unwrap_or(empty);
    for el in items {
        if let Some(node) = object_val(el) {
            out.push(node.clone());
        }
    }
    out
}

fn layer_children(group: &UNode) -> Vec<UNode> {
    if group.type_name != "DataLayerGroup" {
        return Vec::new();
    }
    let stack = field(group, "subStack")
        .or_else(|| field(group, "stack"))
        .and_then(object_val);
    stack.map(stack_layer_items).unwrap_or_default()
}

fn layer_export(node: &UNode, depth: u32) -> SppLayerNode {
    if depth > 64 {
        return SppLayerNode {
            kind: "fill".into(),
            name: node.type_name.clone(),
            visible: true,
            children: vec![],
            painter_type: None,
            base_color: None,
            roughness: None,
            metallic: None,
            height: None,
            base_color_proc: None,
            roughness_proc: None,
            height_proc: None,
            triplanar: false,
            uv_scale: None,
            has_black_mask: false,
            channel_blends: vec![],
            content_effects: vec![],
            mask_effects: vec![],
            mask_decal_hits: vec![],
            mask_paint_strokes: vec![],
        };
    }
    let label = string_val(field(node, "label").unwrap_or(&Value::Null))
        .unwrap_or_else(|| node.type_name.clone());
    let enabled = prim_bool(field(node, "enabled").unwrap_or(&Value::Null));
    let visible = enabled.unwrap_or(true);
    let mask_enabled = prim_bool(field(node, "maskEnabled").unwrap_or(&Value::Null)).unwrap_or(true);
    let has_black_mask = mask_enabled && prim_int(field(node, "maskInitial").unwrap_or(&Value::Null)) == Some(0);
    let channel_blends = extract_layer_channel_blends(node);
    let mask_effects = if mask_enabled {
        extract_mask_effects(node, &label)
    } else {
        vec![]
    };
    let mask_decal_hits = if mask_enabled {
        extract_mask_decal_hits(node)
    } else {
        vec![]
    };
    let mask_paint_strokes = if mask_enabled {
        extract_mask_paint_strokes(node)
    } else {
        vec![]
    };
    match node.type_name.as_str() {
        "DataLayerGroup" => SppLayerNode {
            kind: "folder".into(),
            name: label,
            visible,
            children: layer_children(node).into_iter().map(|c| layer_export(&c, depth + 1)).collect(),
            painter_type: None,
            base_color: None,
            roughness: None,
            metallic: None,
            height: None,
            base_color_proc: None,
            roughness_proc: None,
            height_proc: None,
            triplanar: false,
            uv_scale: None,
            has_black_mask,
            channel_blends,
            content_effects: vec![],
            mask_effects,
            mask_decal_hits,
            mask_paint_strokes,
        },
        "DataLayerPaint" => {
            let fill = extract_fill_channels(node, &label, &channel_blends);
            SppLayerNode {
                kind: "paint".into(),
                name: label,
                visible,
                children: vec![],
                painter_type: None,
                base_color: fill.base_color,
                roughness: fill.roughness,
                metallic: fill.metallic,
                height: fill.height,
                base_color_proc: fill.base_color_proc,
                roughness_proc: fill.roughness_proc,
                height_proc: fill.height_proc,
                triplanar: fill.triplanar,
                uv_scale: fill.uv_scale,
                has_black_mask,
                channel_blends,
                content_effects: fill.content_effects,
                mask_effects,
                mask_decal_hits,
                mask_paint_strokes,
            }
        }
        other => {
            let fill = extract_fill_channels(node, &label, &channel_blends);
            SppLayerNode {
                kind: "fill".into(),
                name: label,
                visible,
                children: vec![],
                painter_type: Some(other.to_string()),
                base_color: fill.base_color,
                roughness: fill.roughness,
                metallic: fill.metallic,
                height: fill.height,
                base_color_proc: fill.base_color_proc,
                roughness_proc: fill.roughness_proc,
                height_proc: fill.height_proc,
                triplanar: fill.triplanar,
                uv_scale: fill.uv_scale,
                has_black_mask,
                channel_blends,
                content_effects: fill.content_effects,
                mask_effects,
                mask_decal_hits,
                mask_paint_strokes,
            }
        }
    }
}

const CH_BASE_COLOR: u64 = 0x0000_0001;
const CH_HEIGHT: u64 = 0x0000_0002;
const CH_SPECULAR: u64 = 0x0000_0004;
const CH_GLOSSINESS: u64 = 0x0000_0040;
const CH_ROUGHNESS: u64 = 0x0000_0080;
const CH_METALLIC: u64 = 0x0000_2000;
const CH_NORMAL: u64 = 0x0040_0000;
const CH_DIFFUSE: u64 = 0x0100_0000;

fn collect_fill_action_blends<'a>(node: &'a UNode, out: &mut Vec<&'a UNode>) {
    if prim_bool(field(node, "enabled").unwrap_or(&Value::Null)) == Some(false) {
        return;
    }
    if node.type_name == "DataActionFill"
        || node.type_name == "DataActionPaint"
        || node.type_name == "DataActionFilterProcedural"
    {
        for (_, val) in &node.fields {
            match val {
                Value::Object(Some(child)) => collect_nodes_of_type(child, "DataBlending", out),
                Value::Array(items) => {
                    for item_val in items {
                        if let Value::Object(Some(child)) = item_val {
                            collect_nodes_of_type(child, "DataBlending", out);
                        }
                    }
                }
                _ => {}
            }
        }
        return;
    }
    if node.type_name == "DataActionGroup" || node.type_name == "DataStackActions" {
        for (_, val) in &node.fields {
            match val {
                Value::Object(Some(child)) => collect_fill_action_blends(child, out),
                Value::Array(items) => {
                    for item_val in items {
                        if let Value::Object(Some(child)) = item_val {
                            collect_fill_action_blends(child, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn extract_layer_channel_blends(layer: &UNode) -> Vec<SppChannelBlend> {
    let mut out = Vec::new();
    let mut blend_nodes = Vec::new();
    let is_folder = layer.type_name == "DataLayerGroup";

    // 1. Collect perChannelBlending from the primary DataActionFill node only when the layer has a
    // single DataActionFill (additional DataActionFill nodes in a layer's actions stack are sub-fills
    // like anisotropic fiber overlays whose blends apply only to that sub-fill, not the whole layer).
    if !is_folder {
        if let Some(Value::Object(Some(actions))) = field(layer, "actions") {
            let mut fill_actions = Vec::new();
            collect_nodes_of_type(actions, "DataActionFill", &mut fill_actions);
            if fill_actions.len() <= 1 {
                collect_fill_action_blends(actions, &mut blend_nodes);
            }
        }
    }
    // 2. Collect layer-level perChannelBlending last so it overrides action defaults.
    if let Some(val) = field(layer, "perChannelBlending") {
        match val {
            Value::Object(Some(node)) => collect_nodes_of_type(node, "DataBlending", &mut blend_nodes),
            Value::Array(items) => {
                for item_val in items {
                    if let Value::Object(Some(child)) = item_val {
                        collect_nodes_of_type(child, "DataBlending", &mut blend_nodes);
                    }
                }
            }
            _ => {}
        }
    }

    let mut has_explicit_base_color = false;
    let mut has_explicit_roughness = false;
    for b in blend_nodes {
        let ch = field(b, "channelTypes").and_then(prim_u64).unwrap_or(u64::MAX);
        let bm = field(b, "blendingMode").and_then(prim_int).unwrap_or(1);
        let opacity = field(b, "opacity").and_then(prim_f32).unwrap_or(1.0).clamp(0.0, 1.0);
        if ch == 0 {
            let mode = if bm == 2 || bm == 25 {
                "passthrough"
            } else if bm == 0 {
                "disable"
            } else {
                painter_blend_mode_name(bm)
            };
            out.push(SppChannelBlend {
                channel: "folder".into(),
                mode: mode.into(),
                opacity,
            });
            continue;
        }
        if is_folder {
            // Keep folder channels in passthrough mode unless overridden at folder level (ch == 0).
            continue;
        }
        let mode = painter_blend_mode_name(bm);
        if ch & CH_BASE_COLOR != 0 {
            has_explicit_base_color = true;
            out.retain(|x: &SppChannelBlend| x.channel != "base_color");
            out.push(SppChannelBlend {
                channel: "base_color".into(),
                mode: mode.into(),
                opacity,
            });
        } else if ch & CH_DIFFUSE != 0 && !has_explicit_base_color {
            out.push(SppChannelBlend {
                channel: "base_color".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_HEIGHT != 0 {
            let height_mode = if bm == 2 {
                "add"
            } else {
                mode
            };
            out.retain(|x: &SppChannelBlend| x.channel != "height");
            out.push(SppChannelBlend {
                channel: "height".into(),
                mode: height_mode.into(),
                opacity,
            });
        }
        if ch & CH_ROUGHNESS != 0 {
            has_explicit_roughness = true;
            out.retain(|x: &SppChannelBlend| x.channel != "roughness");
            out.push(SppChannelBlend {
                channel: "roughness".into(),
                mode: mode.into(),
                opacity,
            });
        } else if ch & CH_GLOSSINESS != 0 && !has_explicit_roughness {
            out.push(SppChannelBlend {
                channel: "roughness".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_METALLIC != 0 {
            out.push(SppChannelBlend {
                channel: "metallic".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_NORMAL != 0 {
            // Painter writes mode=2 (LinearDodge) on ch=0xc00002 (Height|Normal) as default normal combine.
            let norm_mode = if bm == 2 { "normal" } else { mode };
            out.push(SppChannelBlend {
                channel: "normal".into(),
                mode: norm_mode.into(),
                opacity,
            });
        }
    }
    out
}

#[derive(Default)]
struct ExtractedFill {
    base_color: Option<[f32; 3]>,
    roughness: Option<f32>,
    metallic: Option<f32>,
    height: Option<f32>,
    base_color_proc: Option<SppProceduralChannel>,
    roughness_proc: Option<SppProceduralChannel>,
    height_proc: Option<SppProceduralChannel>,
    triplanar: bool,
    uv_scale: Option<f32>,
    content_effects: Vec<SppContentEffect>,
}

fn hash_url_seed(url: &str, random_seed: i64) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in url.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let s = h ^ (random_seed as u32).wrapping_mul(0x9e37_79b9);
    if s == 0 { 1 } else { s }
}

fn extract_action_channel_blends(act: &UNode) -> Vec<SppChannelBlend> {
    let mut blend_nodes = Vec::new();
    collect_nodes_of_type(act, "DataBlending", &mut blend_nodes);
    let mut out = Vec::new();
    let mut has_explicit_base_color = false;
    let mut has_explicit_roughness = false;
    for b in blend_nodes {
        let ch = field(b, "channelTypes").and_then(prim_u64).unwrap_or(u64::MAX);
        let bm = field(b, "blendingMode").and_then(prim_int).unwrap_or(2);
        let opacity = field(b, "opacity").and_then(prim_f32).unwrap_or(1.0).clamp(0.0, 1.0);
        if ch == 0 {
            continue;
        }
        let mode = painter_blend_mode_name(bm);
        if ch & CH_BASE_COLOR != 0 {
            has_explicit_base_color = true;
            out.retain(|x: &SppChannelBlend| x.channel != "base_color");
            out.push(SppChannelBlend {
                channel: "base_color".into(),
                mode: mode.into(),
                opacity,
            });
        } else if ch & CH_DIFFUSE != 0 && !has_explicit_base_color {
            out.push(SppChannelBlend {
                channel: "base_color".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_HEIGHT != 0 {
            let height_mode = if bm == 2 {
                "add"
            } else {
                mode
            };
            out.retain(|x: &SppChannelBlend| x.channel != "height");
            out.push(SppChannelBlend {
                channel: "height".into(),
                mode: height_mode.into(),
                opacity,
            });
        }
        if ch & CH_ROUGHNESS != 0 {
            has_explicit_roughness = true;
            out.retain(|x: &SppChannelBlend| x.channel != "roughness");
            out.push(SppChannelBlend {
                channel: "roughness".into(),
                mode: mode.into(),
                opacity,
            });
        } else if ch & CH_GLOSSINESS != 0 && !has_explicit_roughness {
            out.push(SppChannelBlend {
                channel: "roughness".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_METALLIC != 0 {
            out.push(SppChannelBlend {
                channel: "metallic".into(),
                mode: mode.into(),
                opacity,
            });
        }
        if ch & CH_NORMAL != 0 {
            let norm_mode = if bm == 2 { "normal" } else { mode };
            out.push(SppChannelBlend {
                channel: "normal".into(),
                mode: norm_mode.into(),
                opacity,
            });
        }
    }
    out
}

fn fill_action_display_name(act: &UNode) -> String {
    if let Some(lbl) = field(act, "label").and_then(string_val) {
        if !lbl.trim().is_empty() {
            return lbl.trim().to_string();
        }
    }
    let mut procs = Vec::new();
    collect_nodes_of_type(act, "DataProcedural", &mut procs);
    if let Some(&primary) = procs.first() {
        let url = string_val(field(primary, "urlToSbsRes").unwrap_or(&Value::Null))
            .unwrap_or_default();
        if let Some(stem) = url
            .split('?')
            .next()
            .and_then(|p| p.rsplit('/').next())
            .filter(|s| !s.is_empty())
        {
            let pretty = stem
                .replace('_', " ")
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            return format!("Fill ({pretty})");
        }
    }
    "Fill".into()
}

fn populate_fill_from_action(
    act: &UNode,
    layer_label: &str,
    out: &mut ExtractedFill,
    diffuse_fallback: &mut Option<[f32; 3]>,
) -> Option<f32> {
    let mut rotation_deg = None;
    if let Some(proj) = field(act, "projection").and_then(prim_int) {
        if proj != 0 {
            out.triplanar = true;
        }
    }
    if let Some(uv_tr) = field(act, "uvTransformation").and_then(object_val) {
        if let Some(s) = field(uv_tr, "scale")
            .or_else(|| field(uv_tr, "scaleFactors"))
            .and_then(prim_f32x2)
        {
            let avg = ((s[0].abs() + s[1].abs()) * 0.5).clamp(0.1, 256.0);
            if (avg - 1.0).abs() > 0.05 {
                out.uv_scale = Some(avg);
            }
        }
        if let Some(rot_rad) = field(uv_tr, "rotation").and_then(prim_f32) {
            let deg = rot_rad.to_degrees();
            if deg.abs() > 0.1 {
                rotation_deg = Some(deg);
            }
        }
    }
    for src_val in array_items(field(act, "sources").unwrap_or(&Value::Null)) {
        let Some(src) = object_val(src_val) else { continue };
        let mut mask = prim_u64(field(src, "channelTypes").unwrap_or(&Value::Null)).unwrap_or(0);
        match src.type_name.as_str() {
            "DataSourceUniform" => {
                if let Some(rgb) = prim_f32x3(field(src, "color").unwrap_or(&Value::Null)) {
                    if mask & CH_BASE_COLOR != 0 {
                        out.base_color = Some(rgb);
                    } else if mask & CH_DIFFUSE != 0 {
                        *diffuse_fallback = Some(rgb);
                    }
                    if mask & CH_ROUGHNESS != 0 {
                        out.roughness = Some(rgb[0].clamp(0.0, 1.0));
                    }
                    if mask & CH_METALLIC != 0 {
                        out.metallic = Some(rgb[0].clamp(0.0, 1.0));
                    }
                    if mask & CH_HEIGHT != 0 {
                        out.height = Some(rgb[0]);
                    }
                }
            }
            "DataSourceProcedural" => {
                if let Some(proc_node) = field(src, "procedural").and_then(object_val) {
                    for out_val in array_items(field(proc_node, "outputs").unwrap_or(&Value::Null)) {
                        if let Some(out_node) = object_val(out_val) {
                            if let Some(ch) = field(out_node, "channelTypes")
                                .or_else(|| field(out_node, "channelType"))
                                .and_then(prim_u64)
                            {
                                mask |= ch;
                            }
                        }
                    }
                    extract_procedural_tweaks(proc_node, mask, layer_label, out);
                }
            }
            _ => {}
        }
    }
    rotation_deg
}

fn extract_fill_channels(
    layer: &UNode,
    layer_label: &str,
    channel_blends: &[SppChannelBlend],
) -> ExtractedFill {
    let mut out = ExtractedFill::default();
    let mut diffuse_fallback: Option<[f32; 3]> = None;

    let Some(actions) = field(layer, "actions").and_then(object_val) else {
        return out;
    };
    let mut seen_primary_fill = false;
    for act_val in array_items(field(actions, "items").unwrap_or(&Value::Null)) {
        let Some(act) = object_val(act_val) else { continue };
        if prim_bool(field(act, "enabled").unwrap_or(&Value::Null)) == Some(false) {
            continue;
        }
        if act.type_name == "DataActionFilterProcedural" {
            extract_content_filter_procedural(act, layer_label, &mut out);
            continue;
        }
        if act.type_name != "DataActionFill" {
            continue;
        }
        if !seen_primary_fill {
            seen_primary_fill = true;
            populate_fill_from_action(act, layer_label, &mut out, &mut diffuse_fallback);
        } else {
            let mut sub_fill = ExtractedFill::default();
            let mut sub_diffuse = None;
            let rotation_deg = populate_fill_from_action(act, layer_label, &mut sub_fill, &mut sub_diffuse);
            if sub_fill.base_color.is_none() {
                sub_fill.base_color = sub_diffuse;
            }
            let sub_blends = extract_action_channel_blends(act);
            let name = fill_action_display_name(act);
            out.content_effects.push(SppContentEffect {
                name,
                base_color: sub_fill.base_color,
                roughness: sub_fill.roughness,
                metallic: sub_fill.metallic,
                height: sub_fill.height,
                base_color_proc: sub_fill.base_color_proc,
                roughness_proc: sub_fill.roughness_proc,
                height_proc: sub_fill.height_proc,
                triplanar: sub_fill.triplanar,
                uv_scale: sub_fill.uv_scale,
                rotation_deg,
                channel_blends: sub_blends,
            });
        }
    }
    if out.base_color.is_none() {
        out.base_color = diffuse_fallback;
    }
    // If a procedural layer (e.g. "Scratches") set roughness_proc AND explicitly blends BaseColor,
    // also populate base_color_proc so the scratches affect both BaseColor and Roughness as in Painter.
    if out.base_color.is_none()
        && out.base_color_proc.is_none()
        && channel_blends.iter().any(|b| b.channel == "base_color")
    {
        if let Some(rp) = &out.roughness_proc {
            let mut bp = rp.clone();
            bp.low = 0.0;
            bp.high = 1.0;
            out.base_color_proc = Some(bp);
        }
    }
    out
}

fn extract_content_filter_procedural(
    act: &UNode,
    _layer_label: &str,
    out: &mut ExtractedFill,
) {
    let mut procs = Vec::new();
    collect_nodes_of_type(act, "DataProcedural", &mut procs);
    let Some(&primary) = procs.first() else {
        return;
    };
    let url = string_val(field(primary, "urlToSbsRes").unwrap_or(&Value::Null))
        .unwrap_or_default()
        .to_lowercase();
    let mut tweaks = std::collections::HashMap::<String, &Value>::new();
    collect_direct_procedural_tweaks(primary, &mut tweaks);
    if !(url.contains("finish_")
        || url.contains("matfinish_")
        || url.contains("leather_rough")
        || url.contains("wood_rough"))
    {
        return;
    }

    let get_f32 = |keys: &[&str], default: f32| -> f32 {
        for k in keys {
            if let Some(v) = tweaks.get(&k.to_lowercase()).and_then(|v| prim_f32(v)) {
                return v;
            }
        }
        default
    };
    let get_int = |keys: &[&str], default: i64| -> i64 {
        for k in keys {
            if let Some(v) = tweaks.get(&k.to_lowercase()).and_then(|v| prim_int(v)) {
                return v;
            }
        }
        default
    };

    let intensity = get_f32(&["intensity", "bump_intensity", "grunge_intensity"], 0.07).clamp(0.01, 0.50);
    let tile = get_int(&["tile", "scale", "tiling", "tile_grunge"], 3).clamp(1, 32) as f32;
    let rseed = get_int(&["$randomseed"], 0);
    let seed = hash_url_seed(&url, rseed);
    if get_int(&["triplanar"], 1) != 0 {
        out.triplanar = true;
    }
    if url.contains("powder_coated") || url.contains("powder") {
        let scale = (tile * 8.0).clamp(8.0, 48.0);
        let amp = (intensity * 0.45).clamp(0.015, 0.08);
        if out.height_proc.is_none() {
            out.height_proc = Some(SppProceduralChannel {
                pattern: "clouds".into(),
                seed,
                scale,
                balance: 0.5,
                contrast: 0.15,
                invert: false,
                low: -amp,
                high: amp,
            });
        }
        if out.roughness_proc.is_none() {
            let base_r = out.roughness.unwrap_or(0.32);
            let r_span = (intensity * 0.25).clamp(0.01, 0.06);
            out.roughness_proc = Some(SppProceduralChannel {
                pattern: "clouds".into(),
                seed: seed ^ 0x5bd1_e995,
                scale,
                balance: 0.5,
                contrast: 0.15,
                invert: false,
                low: (base_r - r_span).clamp(0.02, 1.0),
                high: (base_r + r_span).clamp(0.02, 1.0),
            });
        }
        return;
    }

    let pattern = if url.contains("brushed") || url.contains("grinded") {
        "stripes"
    } else if url.contains("grainy") || url.contains("galvanized") || url.contains("hammered") || url.contains("perforated") {
        "cells"
    } else {
        "grunge"
    };
    let scale = (tile * 4.0).clamp(4.0, 32.0);
    let amp = (intensity * 0.65).clamp(0.02, 0.16);
    if out.height_proc.is_none() {
        out.height_proc = Some(SppProceduralChannel {
            pattern: pattern.into(),
            seed,
            scale,
            balance: 0.5,
            contrast: 0.20,
            invert: false,
            low: -amp,
            high: amp,
        });
    }
    if out.roughness_proc.is_none() {
        let base_r = out.roughness.unwrap_or(0.35);
        let r_span = (intensity * 0.40).clamp(0.015, 0.10);
        out.roughness_proc = Some(SppProceduralChannel {
            pattern: pattern.into(),
            seed: seed ^ 0x5bd1_e995,
            scale,
            balance: 0.5,
            contrast: 0.20,
            invert: false,
            low: (base_r - r_span).clamp(0.02, 1.0),
            high: (base_r + r_span).clamp(0.02, 1.0),
        });
    }
}

fn procedural_pattern_from_url(url: &str, layer_label: &str) -> Option<&'static str> {
    let lname = layer_label.to_lowercase();
    if url.contains("scratch") || url.contains("grunge_map_012") || lname.contains("scratch") {
        Some("scratches")
    } else if url.contains("wood_fiber")
        || url.contains("wood_pattern")
        || url.contains("wood_01")
        || url.contains("directional_noise")
        || url.contains("messy_fibers")
    {
        Some("wood_rings")
    } else if url.contains("weave")
        || url.contains("stripe")
        || url.contains("anisotropic")
        || url.contains("waveform")
    {
        Some("stripes")
    } else if url.contains("checker") {
        Some("checker")
    } else if url.contains("brick") || url.contains("tile_generator") {
        Some("brick")
    } else if url.contains("cell")
        || url.contains("crystal")
        || url.contains("hexagon")
        || url.contains("spot")
        || url.contains("zits")
    {
        Some("cells")
    } else if url.contains("grunge")
        || url.contains("dirt")
        || url.contains("stain")
        || url.contains("leak")
        || url.contains("crease")
        || url.contains("fold")
        || url.contains("vein")
        || url.contains("charcoal")
        || url.contains("concrete")
        || url.contains("rust")
    {
        Some("grunge")
    } else if url.contains("perlin") {
        Some("perlin")
    } else if url.contains("cloud")
        || url.contains("noise")
        || url.contains("fractal")
        || url.contains("moisture")
    {
        Some("clouds")
    } else {
        None
    }
}

fn extract_procedural_tweaks(
    proc_node: &UNode,
    mask: u64,
    layer_label: &str,
    out: &mut ExtractedFill,
) {
    let url = string_val(field(proc_node, "urlToSbsRes").unwrap_or(&Value::Null))
        .unwrap_or_default()
        .to_lowercase();
    let mut direct_tweaks = std::collections::HashMap::<String, &Value>::new();
    collect_direct_procedural_tweaks(proc_node, &mut direct_tweaks);

    let get_f32 = |keys: &[&str], default: f32| -> f32 {
        for k in keys {
            if let Some(v) = direct_tweaks.get(&k.to_lowercase()).and_then(|v| prim_f32(v)) {
                return v;
            }
        }
        default
    };
    let get_int = |keys: &[&str], default: i64| -> i64 {
        for k in keys {
            if let Some(v) = direct_tweaks.get(&k.to_lowercase()).and_then(|v| prim_int(v)) {
                return v;
            }
        }
        default
    };

    for (name, &val) in &direct_tweaks {
        if (name == "color"
            || name == "basecolor"
            || name == "base_color"
            || name == "main_color"
            || name == "metal_color"
            || name == "paint_color"
            || name == "wood_color"
            || name == "leather_color"
            || name == "fabric_color"
            || name == "plastic_color"
            || name == "skin_color")
            && (mask & (CH_BASE_COLOR | CH_DIFFUSE) != 0)
        {
            if let Some(rgb) = prim_f32x3(val) {
                if out.base_color.is_none() {
                    out.base_color = Some(rgb);
                }
            }
        } else if (name == "roughness"
            || name == "rough"
            || name == "metal_roughness"
            || name == "surface_roughness"
            || name == "wood_roughness"
            || name == "leather_roughness"
            || name == "plastic_roughness"
            || name == "rust_roughness")
            && (mask & CH_ROUGHNESS != 0)
        {
            if let Some(v) = prim_f32(val) {
                if out.roughness.is_none() {
                    out.roughness = Some(v.clamp(0.0, 1.0));
                }
            }
        } else if (name == "metallic" || name == "metalness") && (mask & CH_METALLIC != 0) {
            if let Some(v) = prim_f32(val) {
                if out.metallic.is_none() {
                    out.metallic = Some(v.clamp(0.0, 1.0));
                }
            }
        }
    }

    // Check if this is a single-channel procedural pattern (grunge, noise, scratches, cells, etc.)
    let is_multi_channel_mat = (mask & (CH_BASE_COLOR | CH_DIFFUSE) != 0)
        && ((mask & CH_ROUGHNESS != 0) || (mask & CH_METALLIC != 0));
    if !is_multi_channel_mat {
        if let Some(pattern) = procedural_pattern_from_url(&url, layer_label) {
            let painter_balance = get_f32(
                &["balance", "histogram_position", "disorder", "scratch_quantity"],
                0.50,
            )
            .clamp(0.0, 1.0);
            // Painter balance > 0.5 brightens (lowers remap threshold); < 0.5 darkens (raises threshold).
            let balance = (1.0 - painter_balance).clamp(0.05, 0.95);
            let default_contrast = if pattern == "stripes" { 0.50 } else { 0.30 };
            let contrast = get_f32(
                &["contrast", "historgram_contrast", "scratch_contrast"],
                default_contrast,
            )
            .clamp(0.0, 1.0);
            let raw_scale = get_int(
                &[
                    "scale",
                    "tile",
                    "tiling",
                    "tile_ratio",
                    "scratch_tiling",
                    "nb_2",
                    "x_amount",
                ],
                1,
            )
            .clamp(1, 64) as f32;
            // Pattern frequency only — UV tiling lives on `ExtractedFill::uv_scale` / Projection.
            // Do not multiply by 5 (that was a fudge when UV scale was not applied to projection).
            let scale = if pattern == "stripes" {
                raw_scale.clamp(2.0, 32.0)
            } else if pattern == "cells" {
                raw_scale.clamp(1.0, 64.0)
            } else {
                raw_scale.clamp(1.0, 64.0)
            };
            let invert = get_int(&["invert", "global_invert"], 0) != 0;
            let rseed = get_int(&["$randomseed"], 0);
            let seed = hash_url_seed(&url, rseed);

            if mask & CH_ROUGHNESS != 0 {
                out.roughness_proc = Some(SppProceduralChannel {
                    pattern: pattern.into(),
                    seed,
                    scale,
                    balance,
                    contrast,
                    invert,
                    low: 0.0,
                    high: 1.0,
                });
                if out.roughness.is_none() {
                    out.roughness = Some(0.50);
                }
            } else if mask & CH_GLOSSINESS != 0 && out.roughness_proc.is_none() {
                out.roughness_proc = Some(SppProceduralChannel {
                    pattern: pattern.into(),
                    seed,
                    scale,
                    balance,
                    contrast,
                    invert: !invert,
                    low: 0.0,
                    high: 1.0,
                });
                if out.roughness.is_none() {
                    out.roughness = Some(0.50);
                }
            }
            if mask & CH_HEIGHT != 0 {
                let amp = if pattern == "stripes" { 0.45 } else { 0.15 };
                out.height_proc = Some(SppProceduralChannel {
                    pattern: pattern.into(),
                    seed,
                    scale,
                    balance,
                    contrast,
                    invert,
                    low: -amp,
                    high: amp,
                });
                if out.height.is_none() {
                    out.height = Some(0.0);
                }
            }
            if mask & (CH_BASE_COLOR | CH_DIFFUSE | CH_SPECULAR) != 0 && out.base_color.is_none() {
                if (mask & CH_BASE_COLOR != 0) || out.base_color_proc.is_none() {
                    out.base_color_proc = Some(SppProceduralChannel {
                        pattern: pattern.into(),
                        seed,
                        scale,
                        balance,
                        contrast,
                        invert,
                        low: 0.0,
                        high: 1.0,
                    });
                }
                out.base_color = Some([0.5, 0.5, 0.5]);
            }
            if pattern == "scratches" && out.height.is_none() && out.height_proc.is_none() {
                out.height_proc = Some(SppProceduralChannel {
                    pattern: pattern.into(),
                    seed,
                    scale,
                    balance,
                    contrast,
                    invert,
                    low: -0.025,
                    high: 0.0,
                });
            }
            return;
        }
    }

    let hint = format!("{url} {}", layer_label.to_lowercase());

    if out.metallic.is_none()
        && (mask & CH_METALLIC != 0)
        && (hint.contains("metal")
            || hint.contains("steel")
            || hint.contains("iron")
            || hint.contains("aluminium")
            || hint.contains("aluminum")
            || hint.contains("alloy")
            || hint.contains("bronze")
            || hint.contains("copper")
            || hint.contains("gold")
            || hint.contains("brass")
            || hint.contains("chrome")
            || hint.contains("silver")
            || hint.contains("titanium"))
    {
        out.metallic = Some(1.0);
    }
    if out.base_color.is_none() && (mask & (CH_BASE_COLOR | CH_DIFFUSE) != 0) {
        if hint.contains("rust") {
            out.base_color = Some([0.24, 0.10, 0.05]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.85);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("gold") || hint.contains("brass") {
            out.base_color = Some([0.88, 0.68, 0.24]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.25);
            }
        } else if hint.contains("bronze") {
            out.base_color = Some([0.58, 0.38, 0.20]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.36);
            }
        } else if hint.contains("copper") {
            out.base_color = Some([0.85, 0.45, 0.30]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.28);
            }
        } else if hint.contains("chrome") || hint.contains("silver") {
            out.base_color = Some([0.84, 0.85, 0.87]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.14);
            }
        } else if hint.contains("aluminium") || hint.contains("aluminum") {
            out.base_color = Some([0.76, 0.77, 0.79]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.28);
            }
        } else if hint.contains("rubber") || hint.contains("tire") {
            out.base_color = Some([0.055, 0.055, 0.058]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.58);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("wood") || hint.contains("walnut") || hint.contains("mahogany") || hint.contains("oak") {
            out.base_color = Some([0.36, 0.20, 0.10]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.52);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("leather") {
            out.base_color = Some([0.24, 0.13, 0.08]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.55);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("fabric") || hint.contains("cloth") || hint.contains("denim") || hint.contains("canvas") {
            out.base_color = Some([0.22, 0.26, 0.34]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.78);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("concrete") || hint.contains("cement") || hint.contains("rock") || hint.contains("stone") {
            out.base_color = Some([0.46, 0.45, 0.43]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.82);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("marble") || hint.contains("jade") || hint.contains("porcelain") || hint.contains("ceramic") {
            out.base_color = Some([0.85, 0.84, 0.81]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.18);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("skin") {
            out.base_color = Some([0.76, 0.54, 0.44]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.55);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("bone") {
            out.base_color = Some([0.78, 0.72, 0.60]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.52);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("plastic") || hint.contains("pvc") {
            out.base_color = Some([0.18, 0.20, 0.24]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.32);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.0);
            }
        } else if hint.contains("car_paint") {
            out.base_color = Some([0.78, 0.42, 0.08]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.18);
            }
            if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
                out.metallic = Some(0.05);
            }
        } else if hint.contains("iron") {
            out.base_color = Some([0.42, 0.43, 0.44]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.48);
            }
        } else if hint.contains("steel") || hint.contains("metal") || hint.contains("alloy") || hint.contains("titanium") {
            out.base_color = Some([0.55, 0.56, 0.58]);
            if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
                out.roughness = Some(0.35);
            }
        }
    } else if url.contains("car_paint") {
        if out.roughness.is_none() && (mask & CH_ROUGHNESS != 0) {
            out.roughness = Some(0.18);
        }
        if out.metallic.is_none() && (mask & CH_METALLIC != 0) {
            out.metallic = Some(0.05);
        }
    }
}

fn extract_mask_effects(layer: &UNode, layer_label: &str) -> Vec<SppMaskEffect> {
    let Some(mask_actions) = field(layer, "maskActions").and_then(object_val) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_mask_action_nodes(mask_actions, layer_label, &mut out);
    out
}

fn collect_mask_action_nodes(
    node: &UNode,
    layer_label: &str,
    out: &mut Vec<SppMaskEffect>,
) {
    for item_val in array_items(field(node, "items").unwrap_or(&Value::Null)) {
        let Some(act) = object_val(item_val) else { continue };
        if prim_bool(field(act, "enabled").unwrap_or(&Value::Null)) == Some(false) {
            continue;
        }
        match act.type_name.as_str() {
            "DataActionGroup" | "DataStackActions" => {
                if let Some(sub) = field(act, "subStack")
                    .or_else(|| field(act, "stack"))
                    .or_else(|| field(act, "actions"))
                    .and_then(object_val)
                {
                    collect_mask_action_nodes(sub, layer_label, out);
                } else {
                    collect_mask_action_nodes(act, layer_label, out);
                }
            }
            "DataActionFill" => {
                extract_mask_fill_action(act, layer_label, out);
            }
            "DataActionFilterLevels" => {
                if let Some(lvl) = extract_mask_levels_action(act) {
                    out.push(lvl);
                }
            }
            "DataActionFilterProcedural" => {
                extract_mask_filter_procedural_action(act, out);
            }
            _ => {}
        }
    }
}

fn extract_action_blending(act: &UNode) -> (f32, String) {
    let mut opacity = 1.0f32;
    let mut blend = "normal".to_string();
    let mut blends = Vec::new();
    collect_nodes_of_type(act, "DataBlending", &mut blends);
    // In maskActions, Painter attaches a default DataBlending with channelTypes=0xc00002 (Height|Normal)
    // and blendingMode=2 (LinearDodge). Only DataBlending with channelTypes == 0 or u64::MAX
    // applies to the mask value itself!
    for b in blends {
        let ch = field(b, "channelTypes").and_then(prim_u64).unwrap_or(0);
        if ch == 0 || ch == u64::MAX {
            if let Some(op) = field(b, "opacity").and_then(prim_f32) {
                opacity = op.clamp(0.0, 1.0);
            }
            if let Some(bm) = field(b, "blendingMode").and_then(prim_int) {
                blend = painter_blend_mode_name(bm).into();
            }
            break;
        }
    }
    (opacity, blend)
}

fn painter_blend_mode_name(code: i64) -> &'static str {
    // Substance Painter DataBlending.blendingMode enum mapping to SubCraft BlendMode names:
    //  0: Disable
    //  1: Normal (or Replace)
    //  2: Normal (Norm) / LinearDodge on Height
    //  3: Subtract (Sub)
    //  4: Multiply (Mul)
    //  5: AddSub / SignedAddition
    //  6: Min (Darken)
    //  7: Max (Lighten)
    //  8: Divide (Div)
    //  9: Screen (Scr)
    // 10: Linear Dodge (Add / Ldge)
    // 11: Overlay (Ovrl)
    // 12: Color Burn (CBrn)
    // 13: Color Dodge (CDdg)
    // 14: Soft Light (SLgt)
    // 15: Hard Light (HLgt)
    // 16: Vivid Light (VLgt)
    // 17: Linear Light (LLgt)
    // 18: Pin Light (PLgt)
    // 19: Linear Burn (LBrn)
    // 20: Difference (Diff)
    // 21: Exclusion (Excl)
    // 22: NormalMapCombine / InverseSubtract
    // 23: NormalMapDetail / InverseDivide
    // 24: NormalMapInverseDetail
    // 25: Passthrough (Pass)
    match code {
        0 => "disable",
        1 => "normal",
        2 => "normal",
        3 => "subtract",
        4 => "multiply",
        5 => "add_sub",
        6 => "min",
        7 => "max",
        8 => "divide",
        9 => "screen",
        10 => "add", // Linear Dodge (Add) — shown as "Ldge" in Substance Painter
        11 => "overlay",
        12 => "color_burn",
        13 => "color_dodge",
        14 => "soft_light",
        15 => "hard_light",
        16 => "vivid_light",
        17 => "linear_light",
        18 => "pin_light",
        19 => "linear_burn",
        20 => "difference",
        21 => "exclusion",
        22 => "normal_combine",
        23 => "normal_detail",
        24 => "normal_inverse_detail",
        25 => "passthrough",
        26 => "linear_burn",
        28 => "color",
        29 => "tint",
        30 => "saturation",
        31 => "value",
        _ => "normal",
    }
}

fn extract_mask_filter_procedural_action(act: &UNode, out: &mut Vec<SppMaskEffect>) {
    let (opacity, blend) = extract_action_blending(act);
    let mut procs = Vec::new();
    collect_nodes_of_type(act, "DataProcedural", &mut procs);
    let Some(&primary) = procs.first() else {
        return;
    };
    let url = string_val(field(primary, "urlToSbsRes").unwrap_or(&Value::Null))
        .unwrap_or_default()
        .to_lowercase();
    if url.contains("histogram_scan") {
        let mut tweaks = std::collections::HashMap::<String, &Value>::new();
        collect_direct_procedural_tweaks(primary, &mut tweaks);
        let position = tweaks
            .get("position")
            .and_then(|v| prim_f32(v))
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        let contrast = tweaks
            .get("contrast")
            .and_then(|v| prim_f32(v))
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        let invert = tweaks
            .get("invert_position")
            .or_else(|| tweaks.get("invert"))
            .and_then(|v| prim_int(v))
            .unwrap_or(0)
            != 0;
        let center = (1.0 - position).clamp(0.05, 0.95);
        let half_span = ((1.0 - contrast) * 0.5).clamp(0.02, 0.5);
        let in_low = (center - half_span).clamp(0.0, 0.96);
        let in_high = (center + half_span).clamp(in_low + 0.02, 1.0);
        let (out_low, out_high) = if invert { (1.0, 0.0) } else { (0.0, 1.0) };
        out.push(SppMaskEffect {
            name: "Histogram Scan (Levels)".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Levels {
                in_low,
                in_high,
                gamma: 1.0,
                out_low,
                out_high,
            },
        });
    } else if url.contains("/invert") {
        out.push(SppMaskEffect {
            name: "Invert".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Invert,
        });
    } else if url.contains("/contrast_luminosity") || url.contains("/contrast") {
        let mut tweaks = std::collections::HashMap::<String, &Value>::new();
        collect_direct_procedural_tweaks(primary, &mut tweaks);
        let luminosity = tweaks
            .get("luminosity")
            .and_then(|v| prim_f32(v))
            .unwrap_or(0.0)
            .clamp(-1.0, 1.0);
        let contrast = tweaks
            .get("contrast")
            .and_then(|v| prim_f32(v))
            .unwrap_or(0.0)
            .clamp(-1.0, 1.0);
        let half_span = (0.5 * (1.0 - contrast * 0.7)).clamp(0.05, 0.95);
        let center = (0.5 - luminosity * 0.45).clamp(0.05, 0.95);
        let in_low = (center - half_span).clamp(0.0, 0.95);
        let in_high = (center + half_span).clamp(in_low + 0.04, 1.0);
        out.push(SppMaskEffect {
            name: "Contrast/Luminosity (Levels)".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Levels {
                in_low,
                in_high,
                gamma: 1.0,
                out_low: 0.0,
                out_high: 1.0,
            },
        });
    }
}

fn extract_mask_fill_action(
    act: &UNode,
    layer_label: &str,
    out: &mut Vec<SppMaskEffect>,
) {
    let (opacity, blend) = extract_action_blending(act);
    let mut procs = Vec::new();
    collect_nodes_of_type(act, "DataProcedural", &mut procs);
    let Some(&primary) = procs.first() else {
        // Uniform grayscale fill inside maskActions
        let mut uniforms = Vec::new();
        collect_nodes_of_type(act, "DataSourceUniform", &mut uniforms);
        if let Some(u) = uniforms.first() {
            if let Some(rgb) = field(u, "color").and_then(prim_f32x3) {
                out.push(SppMaskEffect {
                    name: "Fill (Uniform)".into(),
                    opacity,
                    blend,
                    kind: SppMaskEffectKind::UniformFill {
                        value: rgb[0].clamp(0.0, 1.0),
                    },
                });
            }
        }
        return;
    };
    let url_raw = string_val(field(primary, "urlToSbsRes").unwrap_or(&Value::Null))
        .unwrap_or_default()
        .to_string();
    let url = url_raw.to_lowercase();

    // Collect tweaks belonging directly to the primary DataProcedural (excluding nested input procedurals).
    let mut tweaks = std::collections::HashMap::<String, &Value>::new();
    collect_direct_procedural_tweaks(primary, &mut tweaks);

    let get_f32 = |keys: &[&str], default: f32| -> f32 {
        for k in keys {
            if let Some(v) = tweaks.get(&k.to_lowercase()).and_then(|v| prim_f32(v)) {
                return v;
            }
        }
        default
    };
    let get_int = |keys: &[&str], default: i64| -> i64 {
        for k in keys {
            if let Some(v) = tweaks.get(&k.to_lowercase()).and_then(|v| prim_int(v)) {
                return v;
            }
        }
        default
    };

    if url.contains("metal_edge_wear")
        || url.contains("edge_wear")
        || url.contains("mg_edge_damages")
        || url.contains("mg_edge_select")
        || url.contains("mg_curvature")
        || url.contains("/curvature")
    {
        let wear_level = get_f32(&["wear_level", "balance", "level"], 0.65).clamp(0.0, 1.0);
        let curv_weight = get_f32(&["curvature_weight"], 0.85).clamp(0.15, 1.0);
        let wear_contrast = get_f32(&["wear_contrast", "contrast"], 0.45).clamp(0.0, 1.0);
        let grunge_amount = get_f32(&["grunge_amount", "grunge"], 0.30).clamp(0.0, 1.0);
        let grunge_scale = get_int(&["grunge_scale", "scale"], 4).clamp(1, 32) as f32;
        let invert = get_int(&["invert", "global_invert", "invert_invert"], 0) != 0;

        // Curvature map has flat=0.5, convex edges > 0.5.
        // In Painter, effective edge wear depends on wear_level * curvature_weight, and flat faces
        // (curvature = 0.50) must never be crossed by `level - half - 0.5 * noise_amount`.
        let effective_wear = (wear_level * (0.45 + 0.55 * curv_weight)).clamp(0.0, 1.0);
        let softness = ((1.0 - wear_contrast) * 0.14).clamp(0.03, 0.16);
        let noise_amount = (grunge_amount * 0.22).clamp(0.0, 0.20);
        let min_safe_level = 0.525 + 0.5 * softness + 0.5 * noise_amount;
        let level = (0.76 - effective_wear * 0.20).clamp(min_safe_level, 0.82);
        let gen_name = if url.contains("fiber_glass") {
            "Generator (Fiber Glass Edge Wear)"
        } else {
            "Generator (Edges)"
        };

        out.push(SppMaskEffect {
            name: gen_name.into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Generator {
                generator: "edges".into(),
                level,
                softness,
                invert,
                noise_amount,
                noise_scale: (grunge_scale * 2.5).clamp(2.0, 24.0),
            },
        });
    } else if url.contains("mg_dirt")
        || url.contains("dirt_generator")
        || url.contains("mg_ambient_occlusion")
        || url.contains("ambient_occlusion")
        || url.contains("mg_driping_rust")
        || url.contains("mg_drip")
        || url.contains("mg_grease")
    {
        let dirt_level = get_f32(&["dirt_level", "rust_level", "ao_balance", "balance", "level"], 0.55).clamp(0.0, 1.0);
        let dirt_contrast = get_f32(&["dirt_contrast", "rust_contrast", "ao_contrast", "contrast"], 0.40).clamp(0.0, 1.0);
        let grunge_amount = get_f32(&["grunge_amount", "rust_spreading"], 0.30).clamp(0.0, 1.0);
        let grunge_scale = get_int(&["grunge_scale", "scale"], 4).clamp(1, 32) as f32;
        let invert = get_int(&["invert", "global_invert", "invert_invert"], 0) != 0;

        let level = (0.78 - dirt_level * 0.48).clamp(0.22, 0.80);
        let softness = ((1.0 - dirt_contrast) * 0.30).clamp(0.04, 0.35);

        out.push(SppMaskEffect {
            name: "Generator (Cavity)".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Generator {
                generator: "cavity".into(),
                level,
                softness,
                invert,
                noise_amount: (grunge_amount * 0.45).clamp(0.0, 0.5),
                noise_scale: (grunge_scale * 2.5).clamp(2.0, 24.0),
            },
        });
    } else if url.contains("mg_mask_editor")
        || url.contains("mask_editor")
        || url.contains("maskeditor")
        || url.contains("mg_mask_builder")
        || url.contains("mask_builder")
        || url.contains("maskbuilder")
    {
        let curv_w = get_f32(&["curv", "curvature_opacity", "curvature"], 0.0);
        let ao_w = get_f32(&["ao", "ao_opacity", "ambient_occlusion"], 0.0);
        let ws_w = get_f32(&["ws", "ws_opacity", "world_space_normals"], 0.0);
        let pos_w = get_f32(&["position_gradient", "position_opacity", "position"], 0.0);
        let tex_w = get_f32(&["tex", "texture_opacity", "grunge_amount", "grunge", "grunge2", "scratches"], 0.0);
        let global_bal = get_f32(&["global_balance", "level"], 0.5).clamp(0.0, 1.0);
        let global_con = get_f32(&["global_contrast", "contrast"], 0.25).clamp(0.0, 1.0);
        let mut global_inv = get_int(&["global_invert", "invert_invert", "invert"], 0) != 0;
        let tex_scale = get_f32(&["scale_tex", "texture_scale", "grunge_scale", "triplanar_scale"], 4.0).clamp(1.0, 32.0);

        let convex_range = get_f32(&["curvature_convex_range"], 1.0);
        let concave_range = get_f32(&["curvature_concave_range"], 0.0);
        let concave_only = convex_range <= 0.01 && concave_range > 0.1;

        let lname = layer_label.to_lowercase();
        let is_edge_layer = lname.contains("edge") || lname.contains("wear") || lname.contains("highlight") || lname.contains("border");
        let is_cavity_layer = lname.contains("dirt") || lname.contains("occlusion") || lname.contains("cavity") || lname.contains("crease") || concave_only;
        let is_dust_layer = lname.contains("dust") || get_f32(&["world_space_normal_top_intensity"], 0.0) > 0.5;

        let generator = if is_dust_layer && ws_w > 0.05 {
            "directional"
        } else if is_cavity_layer || (ao_w > curv_w && ao_w > 0.05) {
            "cavity"
        } else if is_edge_layer || (curv_w >= ao_w && curv_w >= ws_w && curv_w > 0.05) {
            "edges"
        } else if pos_w > 0.05 {
            "position_gradient"
        } else if ws_w > 0.05 {
            "directional"
        } else {
            "edges"
        };

        if concave_only && generator == "cavity" {
            global_inv = false;
        }

        let effective_bal = if generator == "cavity" {
            get_f32(&["ao_position", "ao_balance", "ao_range", "global_balance", "level"], global_bal).clamp(0.0, 1.0)
        } else {
            global_bal
        };

        let softness = ((1.0 - global_con) * 0.16).clamp(0.04, 0.22);
        let noise_amount = if tex_w > 0.01 {
            (tex_w * 0.22).clamp(0.05, 0.22)
        } else {
            0.10
        };
        let level = if generator == "edges" {
            let min_safe = 0.525 + 0.5 * softness + 0.5 * noise_amount;
            (0.75 - effective_bal * 0.20).clamp(min_safe, 0.82)
        } else {
            (0.75 - effective_bal * 0.45).clamp(0.25, 0.80)
        };

        out.push(SppMaskEffect {
            name: format!("Mask Editor ({generator})"),
            opacity,
            blend,
            kind: SppMaskEffectKind::Generator {
                generator: generator.into(),
                level,
                softness,
                invert: global_inv,
                noise_amount,
                noise_scale: (tex_scale * 2.0).clamp(2.0, 24.0),
            },
        });
    } else if url.contains("mg_light")
        || url.contains("/light/light")
        || url.contains("light_generator")
        || url.contains("mg_dust")
    {
        let balance = get_f32(&["highlight_level", "dust_level", "balance", "level"], 0.55).clamp(0.0, 1.0);
        let contrast = get_f32(&["highlight_contrast", "dust_contrast", "contrast"], 0.40).clamp(0.0, 1.0);
        let invert = get_int(&["invert", "global_invert", "invert_invert"], 0) != 0;
        let level = (0.75 - balance * 0.45).clamp(0.25, 0.80);
        let softness = ((1.0 - contrast) * 0.28).clamp(0.04, 0.34);
        out.push(SppMaskEffect {
            name: "Generator (Directional)".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Generator {
                generator: "directional".into(),
                level,
                softness,
                invert,
                noise_amount: 0.15,
                noise_scale: 8.0,
            },
        });
    } else if url.contains("/position") || url.contains("gradient_linear") || url.contains("mg_position") {
        let balance = get_f32(&["balance", "position", "level"], 0.50).clamp(0.0, 1.0);
        let contrast = get_f32(&["contrast"], 0.25).clamp(0.0, 1.0);
        let invert = get_int(&["invert", "global_invert", "invert_invert"], 0) != 0;
        let level = (0.75 - balance * 0.50).clamp(0.20, 0.80);
        let softness = ((1.0 - contrast) * 0.35).clamp(0.05, 0.40);
        out.push(SppMaskEffect {
            name: "Generator (Position Gradient)".into(),
            opacity,
            blend,
            kind: SppMaskEffectKind::Generator {
                generator: "position_gradient".into(),
                level,
                softness,
                invert,
                noise_amount: 0.10,
                noise_scale: 6.0,
            },
        });
    } else {
        // Standalone procedural fill (grunge, dirt, clouds, spots, scratches, stains, cells, fibers, stripes)
        let painter_balance = get_f32(&["balance", "histogram_position", "disorder", "scratch_quantity"], 0.50).clamp(0.0, 1.0);
        // Painter balance > 0.5 brightens (lowers remap threshold); < 0.5 darkens (raises threshold).
        let mut balance = (1.0 - painter_balance).clamp(0.05, 0.95);
        let painter_contrast = get_f32(&["contrast", "historgram_contrast", "scratch_contrast"], 0.25).clamp(0.0, 1.0);
        let contrast = painter_contrast;
        let disorder = get_f32(&["disorder"], 0.0).clamp(0.0, 1.0);
        let invert = get_int(&["invert", "global_invert", "invert_invert"], 0) != 0;
        // Painter's UV Transformation "Skalierung / Wiederholen" — this is the main density control
        // users tweak in the Fill properties (e.g. Plastic Grainy Soft Cells 1 = 10). Keep it on
        // `uv_scale` → Projection.tiling; do NOT fold it into the procedural pattern frequency.
        let uv_scale = field(act, "uvTransformation")
            .and_then(object_val)
            .and_then(|u| field(u, "scale").or_else(|| field(u, "scaleFactors")))
            .and_then(prim_f32x2)
            .map(|s| ((s[0].abs() + s[1].abs()) * 0.5).clamp(0.25, 256.0));
        // Substance graph tile / scale (Ratio Cells "X Amount", grunge tile, …). Independent of UV tiling.
        let raw_scale = get_int(
            &["scale", "tile", "tiling", "tile_ratio", "tile_by_ratio", "scratch_tiling", "nb_2", "x_amount"],
            0,
        );
        let pattern = procedural_pattern_from_url(&url, layer_label).unwrap_or("grunge");
        let scale = if raw_scale > 0 {
            let rs = raw_scale as f32;
            if pattern == "stripes" {
                rs.clamp(2.0, 32.0)
            } else if pattern == "cells" {
                // Ratio Cells internal amount is already a dense frequency; keep it close to Painter.
                rs.clamp(1.0, 64.0)
            } else {
                rs.clamp(1.0, 64.0)
            }
        } else if uv_scale.is_some() {
            // UV tiling carries the density; use a moderate default pattern frequency.
            if pattern == "cells" { 8.0 } else { 4.0 }
        } else {
            if pattern == "cells" { 8.0 } else { 5.0 }
        };
        let rseed = get_int(&["$randomseed"], 0);
        let seed = hash_url_seed(&url, rseed);
        if pattern == "grunge" || pattern == "cells" {
            // Grunge and Cells have skewed raw distributions (mean ~0.2..0.4), so lower the
            // remap threshold slightly so stacked Multiply fills (like Surface Damages) preserve bright peaks.
            balance = (balance * 0.65).clamp(0.10, 0.60);
        }
        let sbsar = bakeable_noise_sbsar_ref(
            &url_raw,
            &url,
            if raw_scale > 0 { raw_scale as i32 } else { scale.round() as i32 },
            painter_balance,
            painter_contrast,
            disorder,
            invert,
            rseed,
        );
        let fx_name = fill_action_display_name(act);
        let name = if fx_name == "Fill" {
            "Fill (Procedural)".into()
        } else {
            fx_name
        };
        out.push(SppMaskEffect {
            name,
            opacity,
            blend,
            kind: SppMaskEffectKind::ProceduralFill {
                pattern: pattern.into(),
                seed,
                scale,
                balance,
                contrast,
                invert,
                triplanar: true,
                uv_scale,
                sbsar,
            },
        });
    }
}

/// True when `urlToSbsRes` points at a standalone Painter noise/grunge `.sbsar` we can bake
/// (Ratio Cells, BnW Spots, Fractal Sum, …) — not a mesh-map generator (`mg_*`).
fn bakeable_noise_sbsar_ref(
    url_raw: &str,
    url_lower: &str,
    scale: i32,
    balance: f32,
    contrast: f32,
    disorder: f32,
    invert: bool,
    seed: i64,
) -> Option<crate::SppSbsarRef> {
    if url_raw.is_empty() {
        return None;
    }
    // Mask / mesh-map generators are handled above as Generator effects.
    if url_lower.contains("mg_")
        || url_lower.contains("mask_editor")
        || url_lower.contains("mask_builder")
        || url_lower.contains("edge_wear")
        || url_lower.contains("dirt_generator")
    {
        return None;
    }
    let stem = url_lower
        .trim_start_matches('/')
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    let bakeable = stem.starts_with("noise_")
        || stem.starts_with("grunge_")
        || stem.starts_with("pattern_")
        || url_lower.contains("/noise_")
        || url_lower.contains("cells")
        || url_lower.contains("spots")
        || url_lower.contains("fractal")
        || url_lower.contains("cloud")
        || url_lower.contains("perlin");
    if !bakeable {
        return None;
    }
    Some(crate::SppSbsarRef {
        url: url_raw.to_string(),
        scale: scale.clamp(1, 256),
        balance: balance.clamp(0.0, 1.0),
        contrast: contrast.clamp(0.0, 1.0),
        disorder: disorder.clamp(0.0, 1.0),
        invert,
        seed,
    })
}

fn extract_mask_decal_hits(layer: &UNode) -> Vec<SppDecalHit> {
    let Some(mask_actions) = field(layer, "maskActions").and_then(object_val) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_mask_decal_nodes(mask_actions, &mut out);
    out
}

fn collect_mask_decal_nodes(node: &UNode, out: &mut Vec<SppDecalHit>) {
    if prim_bool(field(node, "enabled").unwrap_or(&Value::Null)) == Some(false) {
        return;
    }
    if node.type_name == "DataActionDecal" {
        let act_vt = field(node, "viewType").and_then(prim_int);
        let act_gran = field(node, "granularity").and_then(prim_int);
        let act_bfc = field(node, "backfaceCulling").and_then(prim_bool);
        let act_angle = field(node, "maxAngle").and_then(prim_f32);

        for hit_val in array_items(field(node, "hits").unwrap_or(&Value::Null)) {
            let Some(hit) = object_val(hit_val) else { continue };
            let view_type = field(hit, "viewType").and_then(prim_int).or(act_vt).unwrap_or(1);
            let granularity = field(hit, "granularity").and_then(prim_int).or(act_gran).unwrap_or(2);
            let backface_culling = field(hit, "backfaceCulling")
                .and_then(prim_bool)
                .or(act_bfc)
                .unwrap_or(true);
            let max_angle = field(hit, "maxAngle").and_then(prim_f32).or(act_angle).unwrap_or(180.0);

            let mut value = field(hit, "color")
                .and_then(prim_f32x3)
                .map(|c| c[0].clamp(0.0, 1.0))
                .unwrap_or(1.0);
            for src_val in array_items(field(hit, "sources").unwrap_or(&Value::Null)) {
                if let Some(src) = object_val(src_val) {
                    if let Some(c) = field(src, "color").and_then(prim_f32x3) {
                        value = c[0].clamp(0.0, 1.0);
                    }
                }
            }
            let pvi = field(hit, "projViewInvMatrix")
                .and_then(prim_f32x16)
                .unwrap_or([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ]);
            if hit.type_name == "DataDecalHitRect" {
                let rect = if let (Some(p0), Some(p1)) = (
                    field(hit, "position0").and_then(prim_f32x2),
                    field(hit, "position1").and_then(prim_f32x2),
                ) {
                    Some([p0[0], p0[1], p1[0], p1[1]])
                } else {
                    field(hit, "rect").and_then(prim_f32x4)
                };
                if let Some(rect) = rect {
                    out.push(SppDecalHit {
                        view_type,
                        granularity,
                        backface_culling,
                        max_angle,
                        value,
                        is_rect: true,
                        pos: [0.0, 0.0],
                        rect,
                        proj_view_inv: pvi,
                    });
                }
            } else if let Some(pos) = field(hit, "position")
                .or_else(|| field(hit, "pos"))
                .and_then(prim_f32x2)
            {
                out.push(SppDecalHit {
                    view_type,
                    granularity,
                    backface_culling,
                    max_angle,
                    value,
                    is_rect: false,
                    pos,
                    rect: [0.0; 4],
                    proj_view_inv: pvi,
                });
            }
        }
        return;
    }

    for (_, val) in &node.fields {
        match val {
            Value::Object(Some(child)) => collect_mask_decal_nodes(child, out),
            Value::Array(items) => {
                for el in items {
                    if let Value::Object(Some(child)) = el {
                        collect_mask_decal_nodes(child, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_mask_paint_strokes(layer: &UNode) -> Vec<SppPaintStroke> {
    let Some(mask_actions) = field(layer, "maskActions").and_then(object_val) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_mask_stroke_nodes(mask_actions, &mut out);
    out
}

fn collect_mask_stroke_nodes(node: &UNode, out: &mut Vec<SppPaintStroke>) {
    if prim_bool(field(node, "enabled").unwrap_or(&Value::Null)) == Some(false) {
        return;
    }
    if node.type_name == "DataBasicStroke" {
        let mut value = field(node, "color")
            .and_then(prim_f32x3)
            .map(|c| c[0].clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let mut sources = Vec::new();
        collect_nodes_of_type(node, "DataSourceUniform", &mut sources);
        if let Some(src) = sources.first() {
            if let Some(c) = field(src, "color").and_then(prim_f32x3) {
                value = c[0].clamp(0.0, 1.0);
            }
        }
        let size = field(node, "overrideParams")
            .and_then(object_val)
            .and_then(|ov| field(ov, "size"))
            .and_then(prim_f32)
            .unwrap_or(1.0);
        let mut pt_nodes = Vec::new();
        collect_nodes_of_type(node, "DataStrokePoint", &mut pt_nodes);
        let mut points = Vec::with_capacity(pt_nodes.len());
        for pt in pt_nodes {
            if let Some(pos) = field(pt, "position").and_then(prim_f32x3) {
                points.push(pos);
            }
        }
        if !points.is_empty() {
            out.push(SppPaintStroke {
                value,
                size,
                points,
            });
        }
        return;
    }
    for (_, val) in &node.fields {
        match val {
            Value::Object(Some(child)) => collect_mask_stroke_nodes(child, out),
            Value::Array(items) => {
                for el in items {
                    if let Value::Object(Some(child)) = el {
                        collect_mask_stroke_nodes(child, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_mask_levels_action(act: &UNode) -> Option<SppMaskEffect> {
    let mut lvls = Vec::new();
    collect_nodes_of_type(act, "DataLevels", &mut lvls);
    let lvl = lvls.first()?;
    let in_low = field(lvl, "inputMinimum")
        .and_then(prim_f32x3)
        .map(|v| v[0])
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let in_high = field(lvl, "inputMaximum")
        .and_then(prim_f32x3)
        .map(|v| v[0])
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let gamma = field(lvl, "gamma")
        .and_then(prim_f32x3)
        .map(|v| v[0])
        .unwrap_or(1.0)
        .clamp(0.05, 10.0);
    let out_low = field(lvl, "outputMinimum")
        .and_then(prim_f32x3)
        .map(|v| v[0])
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let out_high = field(lvl, "outputMaximum")
        .and_then(prim_f32x3)
        .map(|v| v[0])
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    Some(SppMaskEffect {
        name: "Levels".into(),
        opacity: 1.0,
        blend: "normal".into(),
        kind: SppMaskEffectKind::Levels {
            in_low,
            in_high,
            gamma,
            out_low,
            out_high,
        },
    })
}

fn collect_nodes_of_type<'a>(node: &'a UNode, type_name: &str, out: &mut Vec<&'a UNode>) {
    if node.type_name == type_name {
        out.push(node);
    }
    for (_, val) in &node.fields {
        match val {
            Value::Object(Some(child)) => collect_nodes_of_type(child, type_name, out),
            Value::Array(items) => {
                for el in items {
                    if let Value::Object(Some(child)) = el {
                        collect_nodes_of_type(child, type_name, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_direct_procedural_tweaks<'a>(
    proc_node: &'a UNode,
    out: &mut std::collections::HashMap<String, &'a Value>,
) {
    for (fname, val) in &proc_node.fields {
        if fname == "inputs" || fname == "inputsSource" {
            continue;
        }
        match val {
            Value::Object(Some(child)) => {
                if child.type_name.starts_with("DataTweak") {
                    if let Some(id) = field(child, "identifier")
                        .or_else(|| field(child, "name"))
                        .and_then(string_val)
                    {
                        if let Some(v) = field(child, "value") {
                            out.insert(id.to_lowercase(), v);
                        }
                    }
                } else if child.type_name != "DataProcedural" {
                    collect_direct_procedural_tweaks(child, out);
                }
            }
            Value::Array(items) => {
                for el in items {
                    if let Value::Object(Some(child)) = el {
                        if child.type_name.starts_with("DataTweak") {
                            if let Some(id) = field(child, "identifier")
                                .or_else(|| field(child, "name"))
                                .and_then(string_val)
                            {
                                if let Some(v) = field(child, "value") {
                                    out.insert(id.to_lowercase(), v);
                                }
                            }
                        } else if child.type_name != "DataProcedural" {
                            collect_direct_procedural_tweaks(child, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn prim_u64(val: &Value) -> Option<u64> {
    let b = prim_bytes(val)?;
    if b.len() >= 8 {
        Some(u64::from_le_bytes(b[..8].try_into().unwrap()))
    } else if b.len() >= 4 {
        Some(u32::from_le_bytes(b[..4].try_into().unwrap()) as u64)
    } else {
        None
    }
}

fn prim_f32(val: &Value) -> Option<f32> {
    let b = prim_bytes(val)?;
    if b.len() >= 4 {
        Some(f32::from_le_bytes(b[..4].try_into().unwrap()))
    } else {
        None
    }
}

fn prim_f32x2(val: &Value) -> Option<[f32; 2]> {
    let b = prim_bytes(val)?;
    if b.len() >= 8 {
        Some([
            f32::from_le_bytes(b[0..4].try_into().unwrap()),
            f32::from_le_bytes(b[4..8].try_into().unwrap()),
        ])
    } else {
        None
    }
}

fn prim_f32x3(val: &Value) -> Option<[f32; 3]> {
    let b = prim_bytes(val)?;
    if b.len() >= 12 {
        Some([
            f32::from_le_bytes(b[0..4].try_into().unwrap()),
            f32::from_le_bytes(b[4..8].try_into().unwrap()),
            f32::from_le_bytes(b[8..12].try_into().unwrap()),
        ])
    } else if b.len() >= 4 {
        let v = f32::from_le_bytes(b[0..4].try_into().unwrap());
        Some([v, v, v])
    } else {
        None
    }
}

fn prim_f32x4(val: &Value) -> Option<[f32; 4]> {
    let b = prim_bytes(val)?;
    if b.len() >= 16 {
        Some([
            f32::from_le_bytes(b[0..4].try_into().unwrap()),
            f32::from_le_bytes(b[4..8].try_into().unwrap()),
            f32::from_le_bytes(b[8..12].try_into().unwrap()),
            f32::from_le_bytes(b[12..16].try_into().unwrap()),
        ])
    } else {
        None
    }
}

fn prim_f32x16(val: &Value) -> Option<[f32; 16]> {
    let b = prim_bytes(val)?;
    if b.len() >= 64 {
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            out[i] = f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Some(out)
    } else {
        None
    }
}
