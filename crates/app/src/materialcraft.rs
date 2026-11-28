//! MaterialCraft materials as graph-driven fill layers.
//!
//! `subcraft-materialcraft` runs MaterialCraft's own engine; this module turns its results into
//! SubCraft data: parameters into slider descriptions, cooked outputs into source bitmaps.
//! Bitmaps keep encoded values like an imported PNG (color usages sRGB, data usages raw), and
//! normals are turned into SubCraft's OpenGL (+Y) convention.

use std::collections::BTreeMap;

use subcraft_core::ChannelKind;
use subcraft_core::color::linear_to_srgb;
use subcraft_doc::graph_material::{channel_for_usage, usage_inverts};
use subcraft_doc::{GraphParam, GraphParamKind};
use subcraft_image::{Bitmap, BitmapData};
use subcraft_core::MeshMapKind;
use subcraft_materials::MeshMaps;
pub use subcraft_materialcraft::{EXTENSIONS, Error, ImageInput, InputImage, Material, Output, Param, ParamValue, is_materialcraft};

use crate::graph_layer::{GraphBackend, GraphDesc, RenderedMap};

/// Usages whose values are colors (written through sRGB); everything else is data.
pub fn is_color_usage(usage: &str) -> bool {
    matches!(
        usage,
        "baseColor" | "diffuse" | "emissive" | "specular" | "specularEdgeColor" | "scatteringColor" | "absorptionColor" | "coatColor" | "sheenColor" | "reflection"
    )
}

/// A cooked output as a 16-bit source bitmap: sRGB for color usages, raw for data, OpenGL normals.
pub fn output_bitmap(output: &Output) -> Bitmap {
    let color = is_color_usage(&output.usage);
    let flip_green = output.usage == "normal" && output.normal_y_down;
    let channels = output.channels.max(1);
    let data = output
        .texels
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let c = i % channels;
            let alpha = channels == 4 && c == 3;
            let mut v = if v.is_finite() { v } else { 0.0 };
            if flip_green && c == 1 {
                v = 1.0 - v;
            }
            if color && !alpha {
                v = linear_to_srgb(v.max(0.0));
            }
            (v.clamp(0.0, 1.0) * 65535.0).round() as u16
        })
        .collect();
    Bitmap { width: output.width, height: output.height, channels: channels as u8, data: BitmapData::U16(data) }
}

/// Values as plain numbers (how the layer saves them) for one parameter, by its kind.
pub fn value_to_numbers(value: &ParamValue) -> Vec<f64> {
    match value {
        ParamValue::Float(v) => vec![f64::from(*v)],
        ParamValue::Float2(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Float3(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Float4(v) | ParamValue::Color(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Int(v) => vec![f64::from(*v)],
        ParamValue::Int2(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Int3(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Int4(v) => v.iter().map(|&x| f64::from(x)).collect(),
        ParamValue::Bool(v) => vec![if *v { 1.0 } else { 0.0 }],
        ParamValue::Enum(v) => vec![f64::from(*v)],
        ParamValue::Text(_) => Vec::new(),
    }
}

/// Saved numbers back into a value shaped like `default`. Missing numbers keep the default's.
pub fn numbers_to_value(default: &ParamValue, numbers: &[f64]) -> ParamValue {
    let f = |i: usize, d: f32| numbers.get(i).map_or(d, |&x| x as f32);
    let n = |i: usize, d: i32| numbers.get(i).map_or(d, |&x| x.round() as i32);
    match default {
        ParamValue::Float(d) => ParamValue::Float(f(0, *d)),
        ParamValue::Float2(d) => ParamValue::Float2(std::array::from_fn(|i| f(i, d[i]))),
        ParamValue::Float3(d) => ParamValue::Float3(std::array::from_fn(|i| f(i, d[i]))),
        ParamValue::Float4(d) => ParamValue::Float4(std::array::from_fn(|i| f(i, d[i]))),
        ParamValue::Color(d) => ParamValue::Color(std::array::from_fn(|i| f(i, d[i]))),
        ParamValue::Int(d) => ParamValue::Int(n(0, *d)),
        ParamValue::Int2(d) => ParamValue::Int2(std::array::from_fn(|i| n(i, d[i]))),
        ParamValue::Int3(d) => ParamValue::Int3(std::array::from_fn(|i| n(i, d[i]))),
        ParamValue::Int4(d) => ParamValue::Int4(std::array::from_fn(|i| n(i, d[i]))),
        ParamValue::Bool(d) => ParamValue::Bool(numbers.first().map_or(*d, |&x| x >= 0.5)),
        ParamValue::Enum(d) => ParamValue::Enum(numbers.first().map_or(*d, |&x| x.max(0.0).round() as u32)),
        ParamValue::Text(d) => ParamValue::Text(d.clone()),
    }
}

/// Saved values for a graph, converted against its parameters. Unknown ids and text params are
/// dropped (a newer file version may have removed them), so an old layer still cooks.
pub fn overrides(params: &[Param], saved: &BTreeMap<String, Vec<f64>>) -> BTreeMap<String, ParamValue> {
    params
        .iter()
        .filter(|param| !matches!(param.default, ParamValue::Text(_)))
        .filter_map(|param| Some((param.id.clone(), numbers_to_value(&param.default, saved.get(&param.id)?))))
        .collect()
}

/// A slider description for one MaterialCraft parameter. Text parameters have no control.
pub fn graph_param(param: &Param) -> Option<GraphParam> {
    let (lo, hi) = (f64::from(param.min.min(param.max)), f64::from(param.min.max(param.max)));
    let float = |components: usize| GraphParamKind::Float {
        components,
        min: vec![lo; components],
        max: vec![hi; components],
        step: param.step.map_or(0.0, f64::from),
    };
    let int = |components: usize| GraphParamKind::Int { components, min: vec![lo.round(); components], max: vec![hi.round(); components] };
    let kind = match &param.default {
        ParamValue::Float(_) => float(1),
        ParamValue::Float2(_) => float(2),
        ParamValue::Float3(_) => float(3),
        ParamValue::Float4(_) => float(4),
        ParamValue::Int(_) => int(1),
        ParamValue::Int2(_) => int(2),
        ParamValue::Int3(_) => int(3),
        ParamValue::Int4(_) => int(4),
        ParamValue::Bool(_) => GraphParamKind::Bool,
        ParamValue::Color(_) => GraphParamKind::Color { alpha: true, linear: true },
        ParamValue::Enum(_) => GraphParamKind::Enum(param.options.iter().enumerate().map(|(i, label)| (i as i32, label.clone())).collect()),
        ParamValue::Text(_) => return None,
    };
    Some(GraphParam { id: param.id.clone(), label: param.label.clone(), group: param.group.clone(), kind, default: value_to_numbers(&param.default) })
}

/// Cooked outputs as channel maps: the best-ranked usage per channel (base color over diffuse,
/// roughness over glossiness), inverted where the shared mapping says so.
pub fn channel_maps(outputs: &[Output]) -> Vec<RenderedMap> {
    let mut best: BTreeMap<ChannelKind, (u8, &Output)> = BTreeMap::new();
    for output in outputs {
        let Some((channel, rank)) = channel_for_usage(&output.usage) else { continue };
        if best.get(&channel).is_none_or(|(r, _)| rank < *r) {
            best.insert(channel, (rank, output));
        }
    }
    best.into_iter()
        .map(|(channel, (rank, output))| {
            let mut bitmap = output_bitmap(output);
            if usage_inverts(channel, rank)
                && let BitmapData::U16(data) = &mut bitmap.data
            {
                let channels = bitmap.channels as usize;
                for (i, v) in data.iter_mut().enumerate() {
                    if !(channels == 4 && i % 4 == 3) {
                        *v = 65535 - *v;
                    }
                }
            }
            RenderedMap { channel, bitmap }
        })
        .collect()
}

/// The baked mesh map an image input wants, from its identifier (the names TextureCraft hands
/// MaterialCraft: `ambientOcclusion`, `curvature`, `worldNormal`...) or, failing that, its usage.
/// Case, `_`, `-` and spaces are ignored, and the common Substance spellings are accepted.
pub fn mesh_map_for_input(input: &ImageInput) -> Option<MeshMapKind> {
    mesh_map_for_name(&input.identifier).or(match input.usage.as_str() {
        "ambientOcclusion" => Some(MeshMapKind::AmbientOcclusion),
        "normal" => Some(MeshMapKind::Normal),
        "height" => Some(MeshMapKind::Height),
        _ => None,
    })
}

/// The baked mesh map an image input named `name` wants (`ambient_occlusion`, `curvature`,
/// `world_space_normals`, `Position map`...). Case, `_`, `-` and spaces are ignored, and a
/// trailing "map" or "input" is too.
pub fn mesh_map_for_name(name: &str) -> Option<MeshMapKind> {
    let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let by_key = |key: &str| {
        Some(match key {
            "ambientocclusion" | "ao" | "occlusion" => MeshMapKind::AmbientOcclusion,
            "curvature" | "curv" => MeshMapKind::Curvature,
            "thickness" => MeshMapKind::Thickness,
            "position" | "pos" => MeshMapKind::Position,
            "worldnormal" | "worldnormals" | "worldspacenormal" | "worldspacenormals" | "wsnormal" | "wsnormals" => MeshMapKind::WorldSpaceNormal,
            "normal" | "normals" | "tangentnormal" | "normalmap" => MeshMapKind::Normal,
            "height" | "heightmap" => MeshMapKind::Height,
            "id" | "idmap" | "colorid" | "materialid" => MeshMapKind::Id,
            "bentnormal" | "bentnormals" => MeshMapKind::BentNormal,
            _ => return None,
        })
    };
    by_key(&key).or_else(|| ["map", "input"].iter().find_map(|suffix| key.strip_suffix(suffix).filter(|k| !k.is_empty()).and_then(by_key)))
}

/// Baked maps for the graph's image inputs, as raw values (mesh maps are data, never
/// color-transformed). An input whose map is not baked yet gets that map's neutral value (no
/// occlusion, flat curvature, flat normal), as SubCraft's own generators read it, instead of
/// MaterialCraft's 50% gray placeholder.
pub fn input_images(inputs: &[ImageInput], maps: &MeshMaps) -> BTreeMap<String, InputImage> {
    inputs
        .iter()
        .filter_map(|input| {
            let kind = mesh_map_for_input(input)?;
            let image = match maps.get(kind) {
                Some(image) if matches!(image.comps(), 1 | 3 | 4) => InputImage { width: image.width(), height: image.height(), channels: image.comps(), texels: image.to_dense() },
                _ => InputImage { width: 1, height: 1, channels: 3, texels: kind.neutral_value().to_vec() },
            };
            Some((input.identifier.clone(), image))
        })
        .collect()
}

/// An opened MaterialCraft file for graph layers.
pub struct MaterialCraftBackend {
    material: Material,
    graphs: Vec<GraphDesc>,
}

impl MaterialCraftBackend {
    pub fn open(bytes: &[u8], name: &str) -> Result<Self, String> {
        let material = Material::open(bytes, name).map_err(|e| format!("{name}: {e}"))?;
        for warning in material.warnings() {
            log::info!("MaterialCraft {name}: {warning}");
        }
        let mut graphs = Vec::new();
        // The entry graph first, so a layer without a graph name opens what the author published.
        let mut infos = material.graphs();
        infos.sort_by_key(|info| info.index != material.entry);
        for info in infos {
            let params = material.params(info.index).map_err(|e| e.to_string())?;
            let mut channels: Vec<ChannelKind> = info.usages.iter().filter_map(|u| channel_for_usage(u)).map(|(c, _)| c).collect();
            channels.sort();
            channels.dedup();
            if channels.is_empty() {
                continue;
            }
            let skipped: Vec<&str> = params.iter().filter(|p| graph_param(p).is_none()).map(|p| p.label.as_str()).collect();
            let inputs = material.image_inputs(info.index).map_err(|e| e.to_string())?;
            let unmatched: Vec<&str> = inputs.iter().filter(|i| mesh_map_for_input(i).is_none()).map(|i| i.identifier.as_str()).collect();
            let mut notes = Vec::new();
            if !skipped.is_empty() {
                notes.push(format!("text parameters keep the file's value: {}", skipped.join(", ")));
            }
            if inputs.len() > unmatched.len() {
                notes.push("baked mesh maps feed the graph's inputs; bake the texture set for them to show".to_string());
            }
            if !unmatched.is_empty() {
                notes.push(format!("image inputs with no mesh map: {}", unmatched.join(", ")));
            }
            graphs.push(GraphDesc {
                graph: info.name.clone(),
                label: info.name.clone(),
                params: params.iter().filter_map(graph_param).collect(),
                channels,
                notes,
                mesh_inputs: {
                    let mut kinds: Vec<MeshMapKind> = inputs.iter().filter_map(mesh_map_for_input).collect();
                    kinds.sort();
                    kinds.dedup();
                    kinds
                },
                mask: false,
                filter_inputs: Vec::new(),
            });
        }
        if graphs.is_empty() {
            return Err(format!("{name}: no material graph with base color, normal, roughness or other channel outputs"));
        }
        Ok(Self { material, graphs })
    }
}

impl GraphBackend for MaterialCraftBackend {
    fn graphs(&self) -> &[GraphDesc] {
        &self.graphs
    }

    fn render(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32) -> Result<Vec<RenderedMap>, String> {
        self.render_with_maps(graph, values, size_log2, &MeshMaps::new())
    }

    fn render_with_maps(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps) -> Result<Vec<RenderedMap>, String> {
        let name = if graph.is_empty() { self.graphs.first().map_or("", |g| g.graph.as_str()) } else { graph };
        let index = self.material.graph_index(name).ok_or_else(|| format!("graph `{name}` is not in this MaterialCraft file"))?;
        let params = self.material.params(index).map_err(|e| e.to_string())?;
        let size = 1u32 << size_log2.clamp(subcraft_materialcraft::MIN_SIZE.ilog2(), subcraft_materialcraft::MAX_SIZE.ilog2());
        let inputs = input_images(&self.material.image_inputs(index).map_err(|e| e.to_string())?, maps);
        let outputs = self.material.cook_with_inputs(index, &overrides(&params, values), size, &inputs).map_err(|e| e.to_string())?;
        Ok(channel_maps(&outputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(usage: &str, channels: usize, texels: Vec<f32>, y_down: bool) -> Output {
        Output { identifier: usage.into(), usage: usage.into(), width: 1, height: 1, channels, texels, normal_y_down: y_down }
    }

    fn u16s(bitmap: &Bitmap) -> &[u16] {
        match &bitmap.data {
            BitmapData::U16(v) => v,
            other => panic!("expected u16, got {other:?}"),
        }
    }

    #[test]
    fn color_usages_are_srgb_encoded_and_data_stays_raw() {
        let base = output_bitmap(&output("baseColor", 4, vec![0.5, 0.5, 0.5, 0.5], false));
        let encoded = (linear_to_srgb(0.5) * 65535.0).round() as u16;
        assert_eq!(u16s(&base), [encoded, encoded, encoded, 32768], "alpha stays linear");
        // Decoding the bitmap like an imported PNG gives the cooked linear value back.
        assert!((base.texel(0, 0, true)[0] - 0.5).abs() < 1e-4);

        let rough = output_bitmap(&output("roughness", 1, vec![0.5], false));
        assert_eq!(u16s(&rough), [32768]);
        assert_eq!(rough.channels, 1);
    }

    #[test]
    fn directx_normals_become_opengl() {
        let gl = output_bitmap(&output("normal", 4, vec![0.5, 0.8, 1.0, 1.0], false));
        let dx = output_bitmap(&output("normal", 4, vec![0.5, 0.2, 1.0, 1.0], true));
        assert_eq!(u16s(&gl), u16s(&dx));
    }

    #[test]
    fn non_finite_and_out_of_range_values_are_clamped() {
        let b = output_bitmap(&output("height", 1, vec![f32::NAN], false));
        assert_eq!(u16s(&b), [0]);
        let b = output_bitmap(&output("height", 1, vec![3.0], false));
        assert_eq!(u16s(&b), [65535]);
    }

    #[test]
    fn values_round_trip_through_numbers() {
        for value in [
            ParamValue::Float(0.25),
            ParamValue::Float3([0.1, 0.2, 0.3]),
            ParamValue::Color([0.5, 0.25, 0.0, 1.0]),
            ParamValue::Int(7),
            ParamValue::Int2([3, -4]),
            ParamValue::Bool(true),
            ParamValue::Enum(2),
        ] {
            assert_eq!(numbers_to_value(&value, &value_to_numbers(&value)), value);
        }
        // Too few numbers keep the default's components.
        assert_eq!(numbers_to_value(&ParamValue::Float2([1.0, 2.0]), &[5.0]), ParamValue::Float2([5.0, 2.0]));
    }

    #[test]
    fn inputs_find_their_mesh_maps() {
        let input = |identifier: &str, usage: &str| ImageInput { identifier: identifier.into(), usage: usage.into(), gray: true };
        for (id, kind) in [
            ("ambientOcclusion", MeshMapKind::AmbientOcclusion),
            ("ambient_occlusion", MeshMapKind::AmbientOcclusion),
            ("curvature", MeshMapKind::Curvature),
            ("World Space Normals", MeshMapKind::WorldSpaceNormal),
            ("worldNormal", MeshMapKind::WorldSpaceNormal),
            ("position", MeshMapKind::Position),
            ("thickness", MeshMapKind::Thickness),
            ("bentNormal", MeshMapKind::BentNormal),
            ("id", MeshMapKind::Id),
        ] {
            assert_eq!(mesh_map_for_input(&input(id, "any")), Some(kind), "{id}");
        }
        assert_eq!(mesh_map_for_input(&input("dirt_mask", "ambientOcclusion")), Some(MeshMapKind::AmbientOcclusion), "usage is the fallback");
        assert_eq!(mesh_map_for_input(&input("colorMap", "any")), None);

        let mut maps = MeshMaps::new();
        maps.insert(MeshMapKind::Curvature, subcraft_image::TiledImage::new(4, 4, 1, [0.75, 0.0, 0.0, 0.0]));
        let images = input_images(&[input("curvature", "any"), input("thickness", "any")], &maps);
        assert_eq!(images["curvature"].texels, vec![0.75; 16]);
        let neutral = MeshMapKind::Thickness.neutral_value();
        assert_eq!((images["thickness"].width, &images["thickness"].texels[..]), (1, &neutral[..]), "an unbaked map is its neutral value");
        assert!(!input_images(&[input("colorMap", "any")], &maps).contains_key("colorMap"), "inputs that are not mesh maps are left alone");
    }

    #[test]
    fn stale_saved_values_are_ignored() {
        let param = Param {
            id: "amount".into(),
            label: "Amount".into(),
            group: String::new(),
            description: String::new(),
            default: ParamValue::Float(0.5),
            min: 0.0,
            max: 1.0,
            step: None,
            clamp: true,
            options: Vec::new(),
        };
        let saved = BTreeMap::from([("amount".to_string(), vec![0.75]), ("removed".to_string(), vec![1.0])]);
        let values = overrides(&[param], &saved);
        assert_eq!(values.len(), 1);
        assert_eq!(values["amount"], ParamValue::Float(0.75));
    }
}
