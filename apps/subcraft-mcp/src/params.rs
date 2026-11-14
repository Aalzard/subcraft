//! Turning tool arguments (loose JSON from the agent) into engine values, and back.
//!
//! Most engine parameter types are serde types with snake_case names, so the agent can pass
//! their fields directly. [`with_defaults`] lays a partial JSON object over a type's defaults,
//! which lets a call name only the fields it changes.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};

pub type Res<T> = Result<T, String>;

/// Tool arguments with typed, error-reporting getters.
pub struct Args<'a>(pub &'a Map<String, Value>);

impl<'a> Args<'a> {
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key).filter(|v| !v.is_null())
    }
    pub fn str(&self, key: &str) -> Res<Option<&'a str>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(v) => Err(format!("`{key}` must be a string, got {v}")),
        }
    }
    pub fn req_str(&self, key: &str) -> Res<&'a str> {
        self.str(key)?.ok_or_else(|| format!("missing required argument `{key}`"))
    }
    pub fn f32(&self, key: &str) -> Res<Option<f32>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.as_f64().map(|f| Some(f as f32)).ok_or_else(|| format!("`{key}` must be a number, got {v}")),
        }
    }
    pub fn u32(&self, key: &str) -> Res<Option<u32>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| format!("`{key}` must be a non-negative integer, got {v}")),
        }
    }
    pub fn bool(&self, key: &str) -> Res<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(v) => Err(format!("`{key}` must be true or false, got {v}")),
        }
    }
    pub fn obj(&self, key: &str) -> Res<Option<&'a Map<String, Value>>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Object(m)) => Ok(Some(m)),
            Some(v) => Err(format!("`{key}` must be an object, got {v}")),
        }
    }
    pub fn array(&self, key: &str) -> Res<Option<&'a Vec<Value>>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Array(a)) => Ok(Some(a)),
            Some(v) => Err(format!("`{key}` must be an array, got {v}")),
        }
    }
    /// A string or a list of strings.
    pub fn strings(&self, key: &str) -> Res<Option<Vec<&'a str>>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(vec![s.as_str()])),
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().ok_or_else(|| format!("`{key}` must list strings"))).collect::<Res<Vec<_>>>().map(Some),
            Some(v) => Err(format!("`{key}` must be a string or a list of strings, got {v}")),
        }
    }
}

/// `Base Color`, `base-color`, `BaseColor` -> `base_color`.
pub fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.trim().chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c == ' ' || c == '-' || c == '_' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        } else {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    out.trim_end_matches('_').to_string()
}

/// Parse a snake_case serde enum from a loosely written name.
pub fn enum_from<T: DeserializeOwned>(what: &str, s: &str, valid: &[String]) -> Res<T> {
    let key = snake(s);
    serde_json::from_value(Value::String(key.clone()))
        .map_err(|_| format!("unknown {what} `{s}`; valid: {}", valid.join(", ")))
}

pub fn names<T: Serialize>(all: &[T]) -> Vec<String> {
    all.iter().filter_map(|v| serde_json::to_value(v).ok()).filter_map(|v| v.as_str().map(str::to_string)).collect()
}

pub fn name_of<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

pub fn channel(s: &str) -> Res<ChannelKind> {
    let alias = match snake(s).as_str() {
        "color" | "colour" | "albedo" | "diffuse" | "basecolor" | "base_colour" => "base_color",
        "rough" => "roughness",
        "metal" | "metalness" => "metallic",
        "ao" | "occlusion" => "ambient_occlusion",
        "emission" | "emissive_color" | "emiss" => "emissive",
        "alpha" | "opacity_map" => "opacity",
        "spec" | "specular_level" => "specular",
        "bump" | "displacement" => "height",
        "nrm" | "normal_map" => "normal",
        other => return enum_from("channel", other, &names(&ChannelKind::ALL)),
    };
    enum_from("channel", alias, &names(&ChannelKind::ALL))
}

pub fn mesh_map(s: &str) -> Res<MeshMapKind> {
    let alias = match snake(s).as_str() {
        "ao" | "occlusion" => "ambient_occlusion",
        "world_normal" | "world_space_normal" | "wsn" => "world_space_normal",
        "bent_normal" | "bent" => "bent_normal",
        "normal" | "tangent_normal" => "normal",
        other => return enum_from("mesh map", other, &names(&MeshMapKind::ALL)),
    };
    enum_from("mesh map", alias, &names(&MeshMapKind::ALL))
}

/// Deep-merge `patch` into `base` (objects merge key by key; anything else replaces).
pub fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Agents often wrap settings (`{"params": {...}}`, `{"filter": {...}}`, `{"settings": {...}}`):
/// lift a single wrapper object's fields to the top level.
pub fn unwrap_params(patch: &Value, wrappers: &[&str]) -> Value {
    let mut out = patch.clone();
    if let Some(o) = out.as_object_mut() {
        for w in wrappers {
            if let Some(Value::Object(inner)) = o.remove(*w) {
                for (k, v) in inner {
                    o.entry(k).or_insert(v);
                }
            }
        }
    }
    out
}

/// A projection from `patch` over `default`, refused when a value is out of the range the project
/// file accepts (a zero scale, an angle over 180, too many symmetry copies). Accepted, the project
/// saved and then failed to open with "invalid fill projection".
pub fn projection_with_defaults(default: &subcraft_doc::Projection, patch: &Value) -> Res<subcraft_doc::Projection> {
    let p: subcraft_doc::Projection = with_defaults("projection", default, patch)?;
    if !p.valid() {
        return Err("`projection` has a value out of range: scale and 3D scale 0.01 to 100, 3D offset within ±8, rotations within ±360°, backface angle 0 to 180°, hardness 0 to 1, at most 64 symmetry copies".into());
    }
    Ok(p)
}

/// `default` with the fields of `patch` laid over it. Unknown field names, at any depth, are an
/// error that lists the valid ones (serde would otherwise ignore them silently).
pub fn with_defaults<T: Serialize + DeserializeOwned>(what: &str, default: &T, patch: &Value) -> Res<T> {
    let base = serde_json::to_value(default).map_err(|e| e.to_string())?;
    let mut patch = unwrap_params(patch, &["params", "settings", "parameters"]);
    snake_keys(&mut patch);
    let mut v = base.clone();
    merge(&mut v, &patch);
    let result: T = serde_json::from_value(v.clone()).map_err(|e| format!("invalid {what}: {e}"))?;
    let shown = serde_json::to_value(&result).map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    absent_keys(&patch, &shown, &base, &mut Vec::new(), &mut candidates);
    // A key missing from the result is either ignored by serde (a typo: refuse it) or a real
    // field that is not written while it holds its default (`cylinder_angle: 360`: accept it).
    // Only the first reads a deliberately wrong value without complaint.
    let unknown: Vec<String> = candidates.into_iter().filter(|path| ignored::<T>(&v, path)).map(|path| path.join(".")).collect();
    if let Some(first) = unknown.first() {
        let parent: Vec<String> = first.split('.').map(str::to_string).collect();
        let parent = &parent[..parent.len() - 1];
        let mut valid: Vec<String> = [&base, &shown].iter().filter_map(|root| at(root, parent)?.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>())).flatten().collect();
        valid.sort();
        valid.dedup();
        return Err(format!("unknown {what} field(s) {unknown:?}; valid{}: {}", if parent.is_empty() { String::new() } else { format!(" in `{}`", parent.join(".")) }, valid.join(", ")));
    }
    Ok(result)
}

/// Paths of keys in `patch` that neither the deserialized result nor the default has. Nested
/// objects are followed where the result has the same key, so a typo inside `backface` or
/// `symmetry` is found as well as one at the top.
fn absent_keys(patch: &Value, result: &Value, default: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let (Some(p), Some(r)) = (patch.as_object(), result.as_object()) else { return };
    let d = default.as_object();
    for (k, pv) in p {
        path.push(k.clone());
        match r.get(k) {
            Some(rv) => absent_keys(pv, rv, d.and_then(|d| d.get(k)).unwrap_or(&Value::Null), path, out),
            None if d.is_none_or(|d| !d.contains_key(k)) => out.push(path.clone()),
            None => {}
        }
        path.pop();
    }
}

fn at<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(root, |v, k| v.get(k))
}

/// Whether `T` still deserializes with the value at `path` replaced by one no real field accepts:
/// then serde skipped the key unread.
fn ignored<T: DeserializeOwned>(merged: &Value, path: &[String]) -> bool {
    let mut v = merged.clone();
    let Some((last, parents)) = path.split_last() else { return false };
    let mut cur = &mut v;
    for k in parents {
        match cur.get_mut(k) {
            Some(next) => cur = next,
            None => return false,
        }
    }
    let Some(obj) = cur.as_object_mut() else { return false };
    obj.insert(last.clone(), serde_json::json!([[[{ "\u{1}": [true, "x", 1.5] }]]]));
    serde_json::from_value::<T>(v).is_ok()
}

/// Object keys to snake_case (agents write `noiseAmount` as often as `noise_amount`).
fn snake_keys(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let old = std::mem::take(m);
            for (k, mut val) in old {
                snake_keys(&mut val);
                m.insert(snake(&k), val);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(snake_keys),
        _ => {}
    }
}

fn hex(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    let h = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect::<String>() } else { h.to_string() };
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(&h, 16).ok()?;
    Some([((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0])
}

/// A channel value from JSON. Color channels take sRGB (`"#ff8800"`, `[1, 0.5, 0]` or a gray
/// number) unless `linear` is set; other channels take raw values (a number, or `[x,y,z]` for
/// normals).
pub fn channel_value(kind: ChannelKind, v: &Value, linear: bool) -> Res<ChannelValue> {
    let rgb: [f32; 3] = match v {
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0) as f32;
            [f, f, f]
        }
        Value::String(s) => hex(s).ok_or_else(|| format!("`{s}` is not a #rrggbb color"))?,
        Value::Array(a) if a.len() == 3 => {
            let mut c = [0.0; 3];
            for (i, x) in a.iter().enumerate() {
                c[i] = x.as_f64().ok_or_else(|| format!("color component {x} is not a number"))? as f32;
            }
            c
        }
        Value::Object(m) if m.contains_key("value") => {
            let lin = m.get("linear").and_then(Value::as_bool).unwrap_or(linear);
            return channel_value(kind, &m["value"], lin);
        }
        other => return Err(format!("{} value must be a number, '#rrggbb' or [r,g,b]; got {other}", kind.display_name())),
    };
    let rgb = if kind.is_color() && !linear { srgb3_to_linear(rgb) } else { rgb };
    Ok(ChannelValue(rgb))
}

/// A channel value for display: sRGB hex plus the components for color channels, the number
/// for scalar channels.
pub fn show_value(kind: ChannelKind, v: ChannelValue) -> Value {
    if kind.is_color() {
        let s = linear3_to_srgb(v.0);
        let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
        Value::String(format!("#{:02x}{:02x}{:02x}", b(s[0]), b(s[1]), b(s[2])))
    } else if kind.components() == 3 {
        serde_json::json!([round(v.0[0]), round(v.0[1]), round(v.0[2])])
    } else {
        serde_json::json!(round(v.0[0]))
    }
}

pub fn round(x: f32) -> f64 {
    ((x as f64) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_values() {
        assert_eq!(snake("Base Color"), "base_color");
        assert_eq!(snake("BaseColor"), "base_color");
        assert_eq!(channel("rough").unwrap(), ChannelKind::Roughness);
        assert_eq!(channel("AO").unwrap(), ChannelKind::AmbientOcclusion);
        assert!(channel("bogus").is_err());
        let red = channel_value(ChannelKind::BaseColor, &serde_json::json!("#ff0000"), false).unwrap();
        assert_eq!(red.0, [1.0, 0.0, 0.0]);
        let gray = channel_value(ChannelKind::BaseColor, &serde_json::json!(0.5), false).unwrap();
        assert!((gray.0[0] - 0.214).abs() < 0.01, "sRGB 0.5 is linear ~0.214");
        assert_eq!(show_value(ChannelKind::BaseColor, red), serde_json::json!("#ff0000"));
        let r = channel_value(ChannelKind::Roughness, &serde_json::json!(0.7), false).unwrap();
        assert!((r.0[0] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn defaults_merge() {
        let g: subcraft_doc::Generator =
            with_defaults("generator", &subcraft_doc::Generator::new(subcraft_doc::GeneratorKind::Edges), &serde_json::json!({"level": 0.8, "noiseAmount": 0.1})).unwrap();
        assert_eq!(g.level, 0.8);
        assert_eq!(g.noise_amount, 0.1);
        assert_eq!(g.kind, subcraft_doc::GeneratorKind::Edges);
    }
}
