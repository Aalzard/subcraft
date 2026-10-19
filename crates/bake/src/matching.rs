//! High-to-low matching by mesh name (Painter's "Match: By Mesh Name").
//!
//! Every object name is reduced to a base name: trimmed, lower-cased and stripped of its suffix
//! (`_low` on the low poly, `_high` on the high poly). Low triangles then only project onto high
//! triangles with the same base name, so touching parts bake without exploding the model and without
//! bleeding into each other. Triangles of meshes without object information use the mesh name.

use std::collections::BTreeMap;

use subcraft_mesh::Mesh;

/// Group id of triangles whose name matched nothing on the other side.
pub const UNMATCHED: u32 = u32::MAX;

/// Base name of `name`: trimmed, lower-cased, without `suffix` (case-insensitive).
pub fn base_name(name: &str, suffix: &str) -> String {
    let lower = name.trim().to_lowercase();
    let suffix = suffix.trim().to_lowercase();
    match lower.strip_suffix(suffix.as_str()) {
        Some(stripped) if !suffix.is_empty() && !stripped.is_empty() => stripped.trim_end().to_string(),
        _ => lower,
    }
}

/// Display name of a triangle's object (the mesh name when the mesh has no objects).
fn object_name(mesh: &Mesh, tri: usize) -> &str {
    match mesh.tri_object.get(tri).copied().flatten() {
        Some(o) => mesh.objects.get(o as usize).map_or(mesh.name.as_str(), |o| o.name.as_str()),
        None => mesh.name.as_str(),
    }
}

/// Base names per triangle and the matching result between a low and a high mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameGroups {
    /// Group per low triangle ([`UNMATCHED`] when no high object has the same base name).
    pub low: Vec<u32>,
    /// Group per high triangle ([`UNMATCHED`] when no low object has the same base name).
    pub high: Vec<u32>,
    pub report: MatchReport,
}

/// Which object names matched (for the bake window).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchReport {
    /// Base names present on both sides.
    pub matched: Vec<String>,
    /// Low-poly object names without a high-poly partner (baked without high-poly detail).
    pub unmatched_low: Vec<String>,
    /// High-poly object names without a low-poly partner (ignored).
    pub unmatched_high: Vec<String>,
}

/// Match `low` against `high` by object base name.
pub fn match_by_name(low: &Mesh, high: &Mesh, low_suffix: &str, high_suffix: &str) -> NameGroups {
    let low_base: Vec<String> = (0..low.triangle_count()).map(|t| base_name(object_name(low, t), low_suffix)).collect();
    let high_base: Vec<String> = (0..high.triangle_count()).map(|t| base_name(object_name(high, t), high_suffix)).collect();
    // Original (display) names per base name, for the report.
    let mut low_names: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (t, b) in low_base.iter().enumerate() {
        let names = low_names.entry(b.as_str()).or_default();
        let n = object_name(low, t).to_string();
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let mut high_names: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (t, b) in high_base.iter().enumerate() {
        let names = high_names.entry(b.as_str()).or_default();
        let n = object_name(high, t).to_string();
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let mut ids: BTreeMap<&str, u32> = BTreeMap::new();
    let mut report = MatchReport::default();
    for (base, names) in &low_names {
        if high_names.contains_key(base) {
            ids.insert(base, ids.len() as u32);
            report.matched.push((*base).to_string());
        } else {
            report.unmatched_low.extend(names.iter().cloned());
        }
    }
    for (base, names) in &high_names {
        if !ids.contains_key(base) {
            report.unmatched_high.extend(names.iter().cloned());
        }
    }
    let group = |b: &String| ids.get(b.as_str()).copied().unwrap_or(UNMATCHED);
    NameGroups { low: low_base.iter().map(group).collect(), high: high_base.iter().map(group).collect(), report }
}

/// Stable hue in [0, 1) for a name (equal names get equal ID colors).
pub fn name_hue(name: &str) -> f32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Base name of the object owning triangle `tri` (used for per-object ID colors).
pub fn triangle_base_name(mesh: &Mesh, tri: usize, suffix: &str) -> String {
    base_name(object_name(mesh, tri), suffix)
}
