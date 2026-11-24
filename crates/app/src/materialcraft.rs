//! MaterialCraft materials as graph-driven fill layers.
//!
//! `subcraft-materialcraft` runs MaterialCraft's own engine; this module turns its results into
//! SubCraft data: parameters into slider descriptions, cooked outputs into source bitmaps.
//! Bitmaps keep encoded values like an imported PNG (color usages sRGB, data usages raw), and
//! normals are turned into SubCraft's OpenGL (+Y) convention.

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;
use subcraft_core::color::linear_to_srgb;
use subcraft_image::{Bitmap, BitmapData};
pub use subcraft_materialcraft::{EXTENSIONS, Error, Material, Output, Param, ParamValue, is_materialcraft};

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

/// Opening a file parses its whole SBS graph; slider edits re-cook the same file many times, so
/// the last few opened files are kept, keyed by a hash of their bytes.
#[derive(Default)]
pub struct MaterialCache {
    open: Mutex<Vec<(u64, Arc<Material>)>>,
}

const CACHED_FILES: usize = 4;

fn hash(bytes: &[u8], name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl MaterialCache {
    pub fn open(&self, bytes: &[u8], name: &str) -> Result<Arc<Material>, Error> {
        let key = hash(bytes, name);
        if let Some(material) = self.open.lock().iter().find(|(k, _)| *k == key).map(|(_, m)| m.clone()) {
            return Ok(material);
        }
        let material = Arc::new(Material::open(bytes, name)?);
        let mut open = self.open.lock();
        open.insert(0, (key, material.clone()));
        open.truncate(CACHED_FILES);
        Ok(material)
    }
}

/// Parameters of graph `graph` (a name, or empty for the file's entry graph).
pub fn params(cache: &MaterialCache, bytes: &[u8], name: &str, graph: &str) -> Result<Vec<Param>, Error> {
    let material = cache.open(bytes, name)?;
    let index = material.graph_index(graph).ok_or(Error::NoMaterial)?;
    material.params(index)
}

/// Cook graph `graph` at `2^size_log2` pixels with saved values; returns (usage, bitmap) pairs.
pub fn render(
    cache: &MaterialCache,
    bytes: &[u8],
    name: &str,
    graph: &str,
    saved: &BTreeMap<String, Vec<f64>>,
    size_log2: u32,
) -> Result<Vec<(String, Bitmap)>, Error> {
    let material = cache.open(bytes, name)?;
    let index = material.graph_index(graph).ok_or(Error::NoMaterial)?;
    let values = overrides(&material.params(index)?, saved);
    let size = 1u32 << size_log2.min(13);
    Ok(material.cook(index, &values, size)?.iter().map(|output| (output.usage.clone(), output_bitmap(output))).collect())
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
