//! Graph materials: a fill layer whose channel images are rendered by an external material
//! graph (a Substance archive `.sbsar`, or a MaterialCraft / `.sbs` package), with the graph's
//! exposed parameters as controls.
//!
//! Like procedural materials (ADR-0047) the layer stays an ordinary fill layer: each channel the
//! graph fills is a [`FillSource::Bitmap`] pointing at a rendered image resource, so the
//! compositor, the exporter and every tool see plain bitmaps. [`GraphMaterial`] adds what is
//! needed to render again: the package (an embedded resource that keeps the original file
//! bytes), which graph inside it, the parameter values and the render size. Changing a value
//! renders on a worker thread and then swaps the channel images in one undo step.
//!
//! Parameter *descriptions* ([`GraphParam`]) are not saved: they are read from the package by
//! its engine whenever they are needed. This module is engine-neutral; the engines (crates
//! `subcraft-sbsar`, `subcraft-materialcraft`) convert their own descriptions into it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, ResourceId};

use crate::{FillSource, Layer, LayerContent};

/// Most parameter values one layer keeps (files claiming more are cut).
pub const MAX_VALUES: usize = 512;
/// Render sizes as log2 of the side: 16 to 4096 pixels.
pub const MIN_SIZE_LOG2: u32 = 4;
pub const MAX_SIZE_LOG2: u32 = 12;

/// Which engine renders a package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEngine {
    /// A compiled Substance archive (`.sbsar`), rendered by open-sbsar.
    Sbsar,
    /// A MaterialCraft material (`.mcmat`, `.mcraft`) or a Substance source graph (`.sbs`); the
    /// package resource's name keeps the original file name, which tells them apart.
    MaterialCraft,
}

impl GraphEngine {
    pub fn label(self) -> &'static str {
        match self {
            GraphEngine::Sbsar => "Substance archive",
            GraphEngine::MaterialCraft => "MaterialCraft",
        }
    }
}

/// What drives a graph layer. Saved with the layer.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphMaterial {
    pub engine: GraphEngine,
    /// The package: a resource of kind [`crate::ResourceKind::GraphPackage`].
    pub package: ResourceId,
    /// The graph inside the package (its path or name); empty for the first one.
    pub graph: String,
    /// Values the user changed, by parameter id. A parameter not listed uses its default.
    pub values: BTreeMap<String, Vec<f64>>,
    /// Render size, log2 of the side.
    pub size_log2: u32,
}

impl GraphMaterial {
    pub fn new(engine: GraphEngine, package: ResourceId, graph: impl Into<String>, size_log2: u32) -> Self {
        GraphMaterial { engine, package, graph: graph.into(), values: BTreeMap::new(), size_log2: size_log2.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2) }
    }

    /// The same material with sanitized fields (a loaded file may hold anything).
    pub fn sanitized(mut self) -> Self {
        self.size_log2 = self.size_log2.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2);
        while self.values.len() > MAX_VALUES {
            self.values.pop_last();
        }
        for v in self.values.values_mut() {
            v.truncate(4);
            v.retain(|x| x.is_finite());
        }
        self.values.retain(|_, v| !v.is_empty());
        self
    }

    /// The value of `param`: the saved one clamped to the control, else its default.
    pub fn value_of(&self, param: &GraphParam) -> Vec<f64> {
        self.values.get(&param.id).map(|v| param.clamp(v)).unwrap_or_else(|| param.default.clone())
    }

    /// Store `v` for `param` (clamped); a value equal to the default is dropped so files stay
    /// small and a later change of default in the package is followed.
    pub fn set_value(&mut self, param: &GraphParam, v: &[f64]) {
        let v = param.clamp(v);
        if v == param.default {
            self.values.remove(&param.id);
        } else {
            self.values.insert(param.id.clone(), v);
        }
    }

    /// Side of the rendered images in pixels.
    pub fn size_px(&self) -> u32 {
        1 << self.size_log2.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2)
    }
}

/// How a parameter is shown and what values it takes.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphParamKind {
    /// 1 to 4 sliders. `step` 0 = continuous.
    Float { components: usize, min: Vec<f64>, max: Vec<f64>, step: f64 },
    /// 1 to 4 whole-number sliders.
    Int { components: usize, min: Vec<f64>, max: Vec<f64> },
    /// A checkbox (0 or 1).
    Bool,
    /// A colour (3 or 4 numbers in 0..1). `linear` says whether the engine reads it as linear
    /// (MaterialCraft) or sRGB-encoded (Substance); the picker shows the colour either way.
    Color { alpha: bool, linear: bool },
    /// A drop-down of (value, label).
    Enum(Vec<(i32, String)>),
    /// A random seed: a whole number with a Random button.
    Seed,
}

/// One parameter a graph exposes.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphParam {
    pub id: String,
    pub label: String,
    /// The group the graph puts it in ("" for none).
    pub group: String,
    pub kind: GraphParamKind,
    pub default: Vec<f64>,
}

impl GraphParam {
    /// Number of values the parameter takes.
    pub fn components(&self) -> usize {
        match &self.kind {
            GraphParamKind::Float { components, .. } | GraphParamKind::Int { components, .. } => (*components).clamp(1, 4),
            GraphParamKind::Color { alpha, .. } => {
                if *alpha {
                    4
                } else {
                    3
                }
            }
            GraphParamKind::Bool | GraphParamKind::Enum(_) | GraphParamKind::Seed => 1,
        }
    }

    /// Slider range of component `i`.
    pub fn range(&self, i: usize) -> (f64, f64) {
        let pick = |v: &Vec<f64>, d: f64| v.get(i).or(v.first()).copied().filter(|x| x.is_finite()).unwrap_or(d);
        match &self.kind {
            GraphParamKind::Float { min, max, .. } | GraphParamKind::Int { min, max, .. } => {
                let (a, b) = (pick(min, 0.0), pick(max, 1.0));
                if a <= b { (a, b) } else { (b, a) }
            }
            GraphParamKind::Seed => (0.0, 99_999.0),
            _ => (0.0, 1.0),
        }
    }

    /// `v` made acceptable: the right number of components (missing ones from the default),
    /// finite, whole where the control is whole, an existing entry for a drop-down. Slider
    /// values may go past the range (Substance allows typing them) but not without bound.
    pub fn clamp(&self, v: &[f64]) -> Vec<f64> {
        let n = self.components();
        let def = |i: usize| self.default.get(i).or(self.default.last()).copied().unwrap_or(0.0);
        let mut out: Vec<f64> = (0..n).map(|i| v.get(i).copied().filter(|x| x.is_finite()).unwrap_or_else(|| def(i))).collect();
        match &self.kind {
            GraphParamKind::Float { step, .. } => {
                for (i, x) in out.iter_mut().enumerate() {
                    let (lo, hi) = self.range(i);
                    let span = (hi - lo).max(1.0);
                    *x = x.clamp(lo - 100.0 * span, hi + 100.0 * span);
                    if *step > 0.0 {
                        *x = (*x / step).round() * step;
                    }
                }
            }
            GraphParamKind::Int { .. } => {
                for (i, x) in out.iter_mut().enumerate() {
                    let (lo, hi) = self.range(i);
                    let span = (hi - lo).max(1.0);
                    *x = x.clamp(lo - 100.0 * span, hi + 100.0 * span).round();
                }
            }
            GraphParamKind::Color { .. } => out.iter_mut().for_each(|x| *x = x.clamp(0.0, 1.0)),
            GraphParamKind::Bool => out[0] = if out[0] >= 0.5 { 1.0 } else { 0.0 },
            GraphParamKind::Enum(opts) => {
                let x = out[0].round() as i32;
                out[0] = if opts.iter().any(|(o, _)| *o == x) { x as f64 } else { def(0) };
            }
            GraphParamKind::Seed => out[0] = out[0].round().clamp(0.0, i32::MAX as f64),
        }
        out
    }
}

/// The channel an output with this usage fills, and its rank: 0 for a direct match, higher for
/// a fallback that is only used when no direct match exists (Diffuse for Base Color,
/// Glossiness, inverted, for Roughness). Usages follow Substance's names (`baseColor`,
/// `ambientOcclusion`, ...); case does not matter.
pub fn channel_for_usage(usage: &str) -> Option<(ChannelKind, u8)> {
    Some(match usage.to_ascii_lowercase().as_str() {
        "basecolor" => (ChannelKind::BaseColor, 0),
        "diffuse" => (ChannelKind::BaseColor, 1),
        "normal" => (ChannelKind::Normal, 0),
        "roughness" => (ChannelKind::Roughness, 0),
        "glossiness" => (ChannelKind::Roughness, 1),
        "metallic" => (ChannelKind::Metallic, 0),
        "height" => (ChannelKind::Height, 0),
        "displacement" => (ChannelKind::Height, 1),
        "ambientocclusion" => (ChannelKind::AmbientOcclusion, 0),
        "emissive" => (ChannelKind::Emissive, 0),
        "opacity" => (ChannelKind::Opacity, 0),
        "specularlevel" => (ChannelKind::Specular, 0),
        "anisotropylevel" => (ChannelKind::AnisotropyLevel, 0),
        "anisotropyangle" => (ChannelKind::AnisotropyAngle, 0),
        "transmissive" => (ChannelKind::Transmissive, 0),
        "scattering" => (ChannelKind::Scattering, 0),
        "scatteringcolor" => (ChannelKind::ScatteringColor, 0),
        "sheencolor" => (ChannelKind::SheenColor, 0),
        "sheenopacity" => (ChannelKind::SheenOpacity, 0),
        "sheenroughness" => (ChannelKind::SheenRoughness, 0),
        "coatcolor" => (ChannelKind::CoatColor, 0),
        "coatopacity" => (ChannelKind::CoatOpacity, 0),
        "coatroughness" => (ChannelKind::CoatRoughness, 0),
        "coatnormal" => (ChannelKind::CoatNormal, 0),
        "coatspecularlevel" => (ChannelKind::CoatSpecularLevel, 0),
        "ior" => (ChannelKind::Ior, 0),
        _ => return None,
    })
}

/// True when a fallback of `rank` for `channel` must be inverted (Glossiness for Roughness).
pub fn usage_inverts(channel: ChannelKind, rank: u8) -> bool {
    channel == ChannelKind::Roughness && rank > 0
}

/// The images a graph layer shows now, by channel (its enabled bitmap fill channels).
pub fn current_images(layer: &Layer) -> BTreeMap<ChannelKind, ResourceId> {
    let mut out = BTreeMap::new();
    if let LayerContent::Fill(f) = &layer.content {
        for (k, c) in &f.channels {
            if let FillSource::Bitmap { resource } = c.source
                && c.enabled
            {
                out.insert(*k, resource);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider() -> GraphParam {
        GraphParam { id: "a".into(), label: "A".into(), group: String::new(), kind: GraphParamKind::Int { components: 2, min: vec![0.0], max: vec![10.0] }, default: vec![1.0, 2.0] }
    }

    #[test]
    fn clamping_keeps_values_a_control_accepts() {
        let p = slider();
        assert_eq!(p.clamp(&[3.4]), vec![3.0, 2.0], "a missing component takes the default");
        assert_eq!(p.clamp(&[f64::NAN, 1e12]), vec![1.0, 1010.0]);
        let c = GraphParam { kind: GraphParamKind::Color { alpha: false, linear: false }, default: vec![0.5; 3], ..p.clone() };
        assert_eq!(c.clamp(&[2.0, -1.0, 0.25, 9.0]), vec![1.0, 0.0, 0.25]);
        let t = GraphParam { kind: GraphParamKind::Bool, default: vec![0.0], ..p.clone() };
        assert_eq!(t.clamp(&[0.7]), vec![1.0]);
        let e = GraphParam { kind: GraphParamKind::Enum(vec![(1, "L".into()), (3, "S".into())]), default: vec![1.0], ..p.clone() };
        assert_eq!(e.clamp(&[3.0]), vec![3.0]);
        assert_eq!(e.clamp(&[2.0]), vec![1.0], "not an entry: the default");
        let f = GraphParam { kind: GraphParamKind::Float { components: 1, min: vec![0.0], max: vec![1.0], step: 0.25 }, default: vec![0.5], ..p };
        assert_eq!(f.clamp(&[0.3]), vec![0.25]);
    }

    #[test]
    fn a_value_equal_to_the_default_is_not_stored() {
        let p = slider();
        let mut m = GraphMaterial::new(GraphEngine::Sbsar, ResourceId::new(), "", 9);
        m.set_value(&p, &[5.0, 2.0]);
        assert_eq!(m.values.get("a"), Some(&vec![5.0, 2.0]));
        assert_eq!(m.value_of(&p), vec![5.0, 2.0]);
        m.set_value(&p, &[1.0, 2.0]);
        assert!(m.values.is_empty());
        assert_eq!(m.value_of(&p), vec![1.0, 2.0]);
    }

    #[test]
    fn sanitizing_bounds_a_loaded_material() {
        let mut m = GraphMaterial::new(GraphEngine::MaterialCraft, ResourceId::new(), "g", 9);
        m.size_log2 = 40;
        m.values.insert("x".into(), vec![f64::NAN]);
        m.values.insert("y".into(), vec![1.0; 9]);
        let m = m.sanitized();
        assert_eq!(m.size_log2, MAX_SIZE_LOG2);
        assert!(!m.values.contains_key("x"));
        assert_eq!(m.values["y"].len(), 4);
    }

    #[test]
    fn usages_map_to_channels_with_fallbacks_ranked_lower() {
        assert_eq!(channel_for_usage("baseColor"), Some((ChannelKind::BaseColor, 0)));
        assert_eq!(channel_for_usage("diffuse"), Some((ChannelKind::BaseColor, 1)));
        assert_eq!(channel_for_usage("glossiness"), Some((ChannelKind::Roughness, 1)));
        assert!(usage_inverts(ChannelKind::Roughness, 1) && !usage_inverts(ChannelKind::Roughness, 0));
        assert_eq!(channel_for_usage("ambientOcclusion"), Some((ChannelKind::AmbientOcclusion, 0)));
        assert_eq!(channel_for_usage("specular"), None, "a specular colour map has no channel here");
    }
}
