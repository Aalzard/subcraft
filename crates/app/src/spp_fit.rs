//! Fitting uniform values for Painter layers whose content cannot be rebuilt.
//!
//! Procedural Substance materials and patterns have no SubCraft counterpart. Their layers are
//! rebuilt as ordinary uniform fills with the same mask, blending and place in the stack, and the
//! uniform values are chosen so that the whole rebuilt stack comes as close as possible to
//! Painter's flattened result: the composite is (nearly) linear in each fill value, so one
//! composite per fitted layer gives its influence on every texel, and a bounded least-squares
//! solve over all covered texels gives the values. The layers stay editable; what is lost is the
//! texture detail of the original material, which the remaining difference measures.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, TextureSet};
use subcraft_image::TiledImage;

/// A fitted layer and the channels whose uniform values are fitted.
pub type Fitted = (LayerId, Vec<ChannelKind>);

/// Set the uniform value of channel `kind` of fill layer `id` (searched in folders too).
pub fn set_uniform(layers: &mut [Arc<Layer>], id: LayerId, kind: ChannelKind, value: ChannelValue) -> bool {
    for l in layers.iter_mut() {
        if l.id == id {
            let l = Arc::make_mut(l);
            if let LayerContent::Fill(f) = &mut l.content {
                f.channels.insert(kind, FillChannel::new(FillSource::uniform(value)));
                return true;
            }
            return false;
        }
        if let LayerContent::Group(g) = &l.content {
            if g.children.iter().any(|c| contains(c, id)) {
                if let LayerContent::Group(g) = &mut Arc::make_mut(l).content {
                    return set_uniform(&mut g.children, id, kind, value);
                }
            }
        }
    }
    false
}

/// Replace channel `kind` of fill layer `id` by a bitmap source read through UV (the solved
/// content is a texture of the set); returns false when the layer is not a fill.
pub fn set_bitmap(layers: &mut [Arc<Layer>], id: LayerId, kind: ChannelKind, resource: subcraft_core::ResourceId) -> bool {
    for l in layers.iter_mut() {
        if l.id == id {
            let l = Arc::make_mut(l);
            if let LayerContent::Fill(f) = &mut l.content {
                f.channels.insert(kind, FillChannel::new(FillSource::Bitmap { resource }));
                f.projection = subcraft_doc::Projection::default();
                return true;
            }
            return false;
        }
        if let LayerContent::Group(g) = &l.content {
            if g.children.iter().any(|c| contains(c, id)) {
                if let LayerContent::Group(g) = &mut Arc::make_mut(l).content {
                    return set_bitmap(&mut g.children, id, kind, resource);
                }
            }
        }
    }
    false
}

/// Whether fill layer `id` has a uniform Normal channel (a material whose normal detail is to be
/// solved from Painter's result).
pub fn has_uniform_normal(layers: &[Arc<Layer>], id: LayerId) -> bool {
    fn find(l: &Layer, id: LayerId) -> Option<&Layer> {
        if l.id == id {
            return Some(l);
        }
        match &l.content {
            LayerContent::Group(g) => g.children.iter().find_map(|c| find(c, id)),
            _ => None,
        }
    }
    layers.iter().find_map(|l| find(l, id)).is_some_and(|l| matches!(&l.content, LayerContent::Fill(f) if f.channels.get(&ChannelKind::Normal).is_some_and(|c| matches!(c.source, FillSource::Uniform { .. }))))
}

/// Whether every source of fill layer `id` is uniform (so reading its solved bitmaps through UV
/// changes nothing else about it).
pub fn only_uniform(layers: &[Arc<Layer>], id: LayerId) -> bool {
    fn find(l: &Layer, id: LayerId) -> Option<&Layer> {
        if l.id == id {
            return Some(l);
        }
        match &l.content {
            LayerContent::Group(g) => g.children.iter().find_map(|c| find(c, id)),
            _ => None,
        }
    }
    layers.iter().find_map(|l| find(l, id)).is_some_and(|l| matches!(&l.content, LayerContent::Fill(f) if f.channels.values().all(|c| matches!(c.source, FillSource::Uniform { .. }))))
}

fn contains(l: &Layer, id: LayerId) -> bool {
    l.id == id || matches!(&l.content, LayerContent::Group(g) if g.children.iter().any(|c| contains(c, id)))
}

fn value_of(kind: ChannelKind, v: [f32; 3]) -> ChannelValue {
    if kind.components() == 3 { ChannelValue::rgb(v[0], v[1], v[2]) } else { ChannelValue::scalar(v[0]) }
}

/// Dense values of a channel at the texels listed in `at` (component-major per texel).
fn sample(img: &TiledImage, comps: usize, at: &[(u32, u32)]) -> Vec<f32> {
    let mut out = Vec::with_capacity(at.len() * comps);
    for &(x, y) in at {
        let p = img.get(x, y);
        out.extend_from_slice(&p[..comps]);
    }
    out
}

/// Bounded least squares `min |A x - b|^2, lo <= x <= hi` from the normal equations by projected
/// Gauss-Seidel (the problem is convex; a few hundred sweeps converge for these sizes).
fn solve_box(ata: &[f64], atb: &[f64], n: usize, lo: f64, hi: f64, x: &mut [f64]) {
    for _ in 0..400 {
        let mut change = 0.0f64;
        for i in 0..n {
            let d = ata[i * n + i];
            if d <= 1e-12 {
                continue;
            }
            let mut r = atb[i];
            for j in 0..n {
                if j != i {
                    r -= ata[i * n + j] * x[j];
                }
            }
            let v = (r / d).clamp(lo, hi);
            change = change.max((v - x[i]).abs());
            x[i] = v;
        }
        if change < 1e-6 {
            break;
        }
    }
}

/// Regions that let several layers be measured with one composite: `group[k]` is the region of
/// fitted layer `k` (for example the top-level folder holding it) and `owner[t]` the only region
/// that can affect sample `t` (`None` when several or none can). Layers of different regions
/// are perturbed together and each is read only where its region owns the texel.
pub struct Regions {
    pub group: Vec<usize>,
    pub owner: Vec<Option<usize>>,
}

/// What [`fit`] found: per fitted layer the uniform value of each channel and, when asked for,
/// the per-sample content that brings the stack to the target where the layer is visible.
pub struct FitResult {
    pub values: Vec<Vec<(ChannelKind, ChannelValue)>>,
    /// Per fitted layer and channel, `components` values per sample of `at`.
    pub detail: Vec<BTreeMap<ChannelKind, Vec<f32>>>,
}

/// Weight keeping solved detail near the fitted uniform value where a layer is barely visible.
const DETAIL_RIDGE: f32 = 0.05;

/// Fit the uniform values of `fitted` layers in `set` (whose layers hold the rebuilt stack) so its
/// composite matches `target` over the texels `at`. `composite` evaluates a set. Returns, per
/// fitted layer, the value of each of its channels, and leaves `set` holding them.
pub fn fit(
    set: &mut TextureSet,
    fitted: &[Fitted],
    target: &BTreeMap<ChannelKind, TiledImage>,
    at: &[(u32, u32)],
    regions: Option<&Regions>,
    composite: &dyn Fn(&TextureSet) -> BTreeMap<ChannelKind, TiledImage>,
) -> Vec<Vec<(ChannelKind, ChannelValue)>> {
    fit_with_detail(set, fitted, target, at, regions, false, composite).values
}

/// [`fit`], and with `detail` also each fitted layer's content per sample: the uniform value plus
/// its share of what is left between the (linearized) stack and the target, `J r / (sum J^2 + ridge)`
/// per texel (`J` the layer's influence there, `r` the remaining difference): the minimum-change
/// correction that closes the difference, split over the layers visible at the texel.
pub fn fit_with_detail(
    set: &mut TextureSet,
    fitted: &[Fitted],
    target: &BTreeMap<ChannelKind, TiledImage>,
    at: &[(u32, u32)],
    regions: Option<&Regions>,
    detail: bool,
    composite: &dyn Fn(&TextureSet) -> BTreeMap<ChannelKind, TiledImage>,
) -> FitResult {
    let n = fitted.len();
    let set_all = |set: &mut TextureSet, values: &dyn Fn(usize, ChannelKind) -> [f32; 3]| {
        for (k, (id, kinds)) in fitted.iter().enumerate() {
            for &kind in kinds {
                set_uniform(&mut set.layers, *id, kind, value_of(kind, values(k, kind)));
            }
        }
    };
    if n == 0 {
        return FitResult { values: Vec::new(), detail: Vec::new() };
    }
    // Influence of each fitted value: composite with that layer at 1 and all others at 0.
    set_all(set, &|_, _| [0.0; 3]);
    let base = composite(set);
    let channels: Vec<ChannelKind> = target.keys().copied().filter(|k| base.contains_key(k) && fitted.iter().any(|(_, ks)| ks.contains(k))).collect();
    let base_s: BTreeMap<ChannelKind, Vec<f32>> = channels.iter().map(|k| (*k, sample(&base[k], k.components(), at))).collect();
    // Rounds of layers measured together: one layer per region per round (one per round without regions).
    let rounds: Vec<Vec<usize>> = match regions {
        Some(r) => {
            let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
            let mut rounds: Vec<Vec<usize>> = Vec::new();
            // Layers with no fitted channel (listed only for the normal solve) need no composite.
            for k in (0..n).filter(|&k| !fitted[k].1.is_empty()) {
                let i = seen.entry(r.group[k]).or_default();
                if rounds.len() <= *i {
                    rounds.push(Vec::new());
                }
                rounds[*i].push(k);
                *i += 1;
            }
            rounds
        }
        None => (0..n).filter(|&k| !fitted[k].1.is_empty()).map(|k| vec![k]).collect(),
    };
    let mut influence: Vec<BTreeMap<ChannelKind, Vec<f32>>> = vec![BTreeMap::new(); n];
    for round in &rounds {
        set_all(set, &|j, _| if round.contains(&j) { [1.0; 3] } else { [0.0; 3] });
        let c = composite(set);
        for &k in round {
            for kind in channels.iter().filter(|kind| fitted[k].1.contains(kind)) {
                let comps = kind.components();
                let v = sample(&c[kind], comps, at);
                let mut d: Vec<f32> = v.iter().zip(&base_s[kind]).map(|(a, b)| a - b).collect();
                if let Some(r) = regions.filter(|_| round.len() > 1) {
                    for (t, o) in r.owner.iter().enumerate() {
                        if *o != Some(r.group[k]) {
                            d[t * comps..(t + 1) * comps].fill(0.0);
                        }
                    }
                }
                influence[k].insert(*kind, d);
            }
        }
    }
    // One bounded least-squares problem per channel component.
    let mut result: Vec<BTreeMap<ChannelKind, [f32; 3]>> = vec![BTreeMap::new(); n];
    for &kind in &channels {
        let comps = kind.components();
        let goal = sample(&target[&kind], comps, at);
        let users: Vec<usize> = (0..n).filter(|k| influence[*k].contains_key(&kind)).collect();
        let m = users.len();
        let (lo, hi) = if kind.is_signed() { (-1.0, 1.0) } else { (0.0, 1.0) };
        for c in 0..comps {
            let mut ata = vec![0.0f64; m * m];
            let mut atb = vec![0.0f64; m];
            let base_v = &base_s[&kind];
            for t in 0..at.len() {
                let i = t * comps + c;
                let r = (goal[i] - base_v[i]) as f64;
                for (a, &ka) in users.iter().enumerate() {
                    let ja = influence[ka][&kind][i] as f64;
                    if ja == 0.0 {
                        continue;
                    }
                    atb[a] += ja * r;
                    for (b, &kb) in users.iter().enumerate() {
                        ata[a * m + b] += ja * influence[kb][&kind][i] as f64;
                    }
                }
            }
            let mut x = vec![0.5f64.clamp(lo, hi); m];
            solve_box(&ata, &atb, m, lo, hi, &mut x);
            for (a, &k) in users.iter().enumerate() {
                result[k].entry(kind).or_insert([0.0; 3])[c] = x[a] as f32;
            }
        }
    }
    set_all(set, &|k, kind| result[k].get(&kind).copied().unwrap_or([0.5; 3]));
    let values = fitted.iter().enumerate().map(|(k, (_, kinds))| kinds.iter().map(|&kind| (kind, value_of(kind, result[k].get(&kind).copied().unwrap_or([0.5; 3])))).collect()).collect();
    let mut details: Vec<BTreeMap<ChannelKind, Vec<f32>>> = vec![BTreeMap::new(); if detail { n } else { 0 }];
    if detail {
        for &kind in &channels {
            let comps = kind.components();
            let goal = sample(&target[&kind], comps, at);
            let users: Vec<usize> = (0..n).filter(|k| influence[*k].contains_key(&kind)).collect();
            let (lo, hi) = if kind.is_signed() { (-1.0, 1.0) } else { (0.0, 1.0) };
            for &k in &users {
                let u = result[k].get(&kind).copied().unwrap_or([0.5; 3]);
                details[k].insert(kind, (0..at.len() * comps).map(|i| u[i % comps]).collect());
            }
            for i in 0..at.len() * comps {
                let c = i % comps;
                let mut predicted = base_s[&kind][i];
                let mut norm = DETAIL_RIDGE;
                for &k in &users {
                    let j = influence[k][&kind][i];
                    predicted += j * result[k].get(&kind).map_or(0.5, |v| v[c]);
                    norm += j * j;
                }
                let r = goal[i] - predicted;
                for &k in &users {
                    let j = influence[k][&kind][i];
                    if j != 0.0 {
                        let d = details[k].get_mut(&kind).unwrap();
                        d[i] = (d[i] + j * r / norm).clamp(lo, hi);
                    }
                }
            }
        }
    }
    FitResult { values, detail: details }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_solve_recovers_values_and_respects_bounds() {
        // Two unknowns observed through x0 + x1 and x1 alone.
        let a = [[1.0, 1.0], [0.0, 1.0], [1.0, 0.0]];
        let truth = [0.3, 0.6];
        let b: Vec<f64> = a.iter().map(|r| r[0] * truth[0] + r[1] * truth[1]).collect();
        let mut ata = vec![0.0; 4];
        let mut atb = vec![0.0; 2];
        for (r, bi) in a.iter().zip(&b) {
            for i in 0..2 {
                atb[i] += r[i] * bi;
                for j in 0..2 {
                    ata[i * 2 + j] += r[i] * r[j];
                }
            }
        }
        let mut x = vec![0.5; 2];
        solve_box(&ata, &atb, 2, 0.0, 1.0, &mut x);
        assert!((x[0] - 0.3).abs() < 1e-4 && (x[1] - 0.6).abs() < 1e-4, "{x:?}");
        let mut y = vec![0.5; 2];
        solve_box(&ata, &atb.iter().map(|v| v * 3.0).collect::<Vec<_>>(), 2, 0.0, 1.0, &mut y);
        assert!(y.iter().all(|v| (0.0..=1.0).contains(v)), "{y:?}");
    }
}
