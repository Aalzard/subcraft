//! Bake part matching by name.
//!
//! With [`BakeMatchMode::ByName`], every low- and high-poly triangle belongs to a part (its
//! object, or the whole mesh when the file has no objects). Part names are reduced to a *base
//! name*: lowercased, a trailing duplicate counter such as `.001` dropped, and then the low or
//! high suffix removed (`_low` / `_high` by default, `_lo` / `_hi` always accepted too). Parts
//! with equal base names form a group, and rays cast from a low part only hit high triangles of
//! its group. A low part without any high counterpart falls back to Always (any high triangle)
//! and is listed in [`PartMatch::unmatched_low`] so the caller can report it.

use std::collections::{BTreeMap, BTreeSet};

use subcraft_core::{BakeMatch, BakeMatchMode};
use subcraft_mesh::Mesh;

/// Group id of a low triangle without a matching high part (its rays hit everything).
pub const NO_GROUP: u32 = u32::MAX;

/// Short suffix forms accepted in addition to the configured ones.
pub const LOW_ALIAS: &str = "_lo";
pub const HIGH_ALIAS: &str = "_hi";

/// Base name of a part: lowercase, without a trailing `.123` duplicate counter, without the
/// first of `suffixes` (case-insensitive, longest first) it ends with. A name that *is* only a
/// suffix keeps it, so it never reduces to an empty base.
pub fn base_name(name: &str, suffixes: &[&str]) -> String {
    let mut s = name.trim().to_lowercase();
    if let Some(dot) = s.rfind('.') {
        let tail = &s[dot + 1..];
        if dot > 0 && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            s.truncate(dot);
        }
    }
    let mut suffixes: Vec<String> = suffixes.iter().map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect();
    suffixes.sort_by_key(|x| std::cmp::Reverse(x.len()));
    for suffix in &suffixes {
        if s.len() > suffix.len() && s.ends_with(suffix.as_str()) {
            s.truncate(s.len() - suffix.len());
            break;
        }
    }
    s
}

/// Base name of a low-poly part under `m`'s suffixes.
pub fn low_base(name: &str, m: &BakeMatch) -> String {
    base_name(name, &[&m.low_suffix, LOW_ALIAS])
}

/// Base name of a high-poly part under `m`'s suffixes.
pub fn high_base(name: &str, m: &BakeMatch) -> String {
    base_name(name, &[&m.high_suffix, HIGH_ALIAS])
}

/// Part (object) name of every triangle; the mesh name for triangles without an object.
fn tri_names(mesh: &Mesh) -> impl Iterator<Item = &str> + '_ {
    (0..mesh.triangle_count()).map(move |t| {
        mesh.tri_object.get(t).copied().flatten().and_then(|o| mesh.objects.get(o as usize)).map_or(mesh.name.as_str(), |o| o.name.as_str())
    })
}

/// Low → high part groups for one low / high mesh pair.
#[derive(Clone, Debug)]
pub struct PartMatch {
    low_tri: Vec<u32>,
    high_tri: Vec<u32>,
    /// Base names of the groups, indexed by group id.
    pub groups: Vec<String>,
    /// Low parts (display names) with no high counterpart: their rays hit any high part.
    pub unmatched_low: Vec<String>,
    /// High parts no low part matched: only unmatched low parts can reach them.
    pub unmatched_high: Vec<String>,
}

impl PartMatch {
    /// `None` for [`BakeMatchMode::Always`]: every ray may hit every high triangle.
    pub fn new(low: &Mesh, high: &Mesh, m: &BakeMatch) -> Option<Self> {
        if m.mode == BakeMatchMode::Always {
            return None;
        }
        // High base name → group id (BTreeMap keeps ids deterministic).
        let high_bases: BTreeSet<String> = tri_names(high).map(|n| high_base(n, m)).collect();
        let groups: Vec<String> = high_bases.into_iter().collect();
        let id: BTreeMap<&str, u32> = groups.iter().enumerate().map(|(i, g)| (g.as_str(), i as u32)).collect();
        let mut low_cache: BTreeMap<&str, u32> = BTreeMap::new();
        let mut unmatched_low = BTreeSet::new();
        let mut used = BTreeSet::new();
        let low_tri = tri_names(low).map(|n| *low_cache.entry(n).or_insert_with(|| {
            match id.get(low_base(n, m).as_str()) {
                Some(&g) => { used.insert(g); g }
                None => { unmatched_low.insert(n.to_string()); NO_GROUP }
            }
        })).collect();
        let high_tri = tri_names(high).map(|n| id[high_base(n, m).as_str()]).collect();
        let mut unmatched_high = BTreeSet::new();
        for n in tri_names(high) {
            if !used.contains(&id[high_base(n, m).as_str()]) { unmatched_high.insert(n.to_string()); }
        }
        Some(Self { low_tri, high_tri, groups, unmatched_low: unmatched_low.into_iter().collect(), unmatched_high: unmatched_high.into_iter().collect() })
    }

    /// Group of a low triangle ([`NO_GROUP`] if unmatched or out of range).
    pub fn low_group(&self, low_tri: u32) -> u32 {
        self.low_tri.get(low_tri as usize).copied().unwrap_or(NO_GROUP)
    }

    /// May a ray from a low part of `group` hit high triangle `high_tri`?
    pub fn accepts(&self, group: u32, high_tri: u32) -> bool {
        group == NO_GROUP || self.high_tri.get(high_tri as usize) == Some(&group)
    }

    /// One-line report of the low parts that fell back to Always, if any.
    pub fn report(&self) -> Option<String> {
        if self.unmatched_low.is_empty() {
            return None;
        }
        const SHOWN: usize = 6;
        let mut names = self.unmatched_low.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
        if self.unmatched_low.len() > SHOWN { names.push_str(&format!(" and {} more", self.unmatched_low.len() - SHOWN)); }
        Some(format!("Match by name: {} low-poly part(s) have no matching high-poly part and bake against all high-poly parts: {names}",
            self.unmatched_low.len()))
    }
}
