//! Masks: a paintable grayscale base plus an ordered effect stack.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use subcraft_core::{EffectId, MeshMapKind};
use subcraft_image::TiledImage;

use crate::blend::BlendMode;
use crate::fill::{ProceduralParams, Projection};

/// Grayscale levels remap.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub in_low: f32,
    pub in_high: f32,
    pub gamma: f32,
    pub out_low: f32,
    pub out_high: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self { in_low: 0.0, in_high: 1.0, gamma: 1.0, out_low: 0.0, out_high: 1.0 }
    }
}

impl Levels {
    pub fn apply(&self, v: f32) -> f32 {
        let t = ((v - self.in_low) / (self.in_high - self.in_low).max(1e-6)).clamp(0.0, 1.0);
        let t = t.powf(1.0 / self.gamma.max(1e-3));
        self.out_low + t * (self.out_high - self.out_low)
    }
}

/// Mask generator: derives coverage from baked mesh maps (the basis of smart masks).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    pub kind: GeneratorKind,
    /// Threshold / center of the transition.
    pub level: f32,
    /// Transition width.
    pub softness: f32,
    pub invert: bool,
    /// Procedural breakup applied to the threshold (0 = none).
    pub noise_amount: f32,
    pub noise: ProceduralParams,
    /// Seamless object-space volume noise instead of repeating axis projections. Absent in
    /// older projects/recipes: preserve their original pattern exactly.
    #[serde(default)]
    pub organic_breakup: bool,
    /// Geometry-aware wear. Missing in older recipes: retain their exact threshold behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_spread: Option<EdgeSpread>,
    /// World direction for `Directional` (normalized on use).
    pub direction: [f32; 3],
    /// World axis for `PositionGradient`. Absent in older projects: Y, their original behavior.
    #[serde(default, skip_serializing_if = "GradientAxis::is_y")]
    pub axis: GradientAxis,
    /// Moves the breakup noise through the model (object space, in units of the model's longest
    /// dimension; the editor shows percent), so a pattern can be slid without changing its seed.
    /// Absent in older projects: no offset, their original pattern.
    #[serde(default, skip_serializing_if = "is_zero_offset")]
    pub offset: [f32; 3],
    /// Substance Painter's extra generator controls (edges masking, wear's AO masking, curvature
    /// weight, smoothness). Absent in older projects and in generators made in SubCraft: they keep
    /// their exact behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub painter: Option<PainterControls>,
    /// "Micro effect": also writes this generator's pattern into the Normal / Height channels of the
    /// layers below. Absent (the default, and in every older project): colour channels only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro: Option<MicroEffect>,
}

/// Settings of a generator's micro effect (see [`Generator::micro`]). `Default` is the neutral
/// "off" state; [`MicroEffect::enabled_for`] gives the settings a freshly ticked checkbox starts with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicroEffect {
    /// Master switch. Stored (rather than removing the struct) so unticking keeps the tuned values.
    pub enabled: bool,
    /// Height change at full mask, 0..1 (1 = [`MicroEffect::MAX_HEIGHT`] height units).
    pub amount: f32,
    /// Raise (dirt fills, deposits) or carve (wear chips) the height.
    pub direction: MicroDirection,
    /// Modulate the height with the generator's own fine grain, so the micro surface is as
    /// speckled as the mask. Off: the height follows the smooth mask only.
    pub grain_follows: bool,
}

/// Which way a micro effect moves the height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicroDirection { Raise, Carve }

impl MicroDirection {
    pub fn sign(self) -> f32 { match self { Self::Raise => 1.0, Self::Carve => -1.0 } }
    pub fn label(self) -> &'static str { match self { Self::Raise => "Raise", Self::Carve => "Carve" } }
}

impl Default for MicroEffect {
    fn default() -> Self { Self { enabled: false, amount: 0.5, direction: MicroDirection::Raise, grain_follows: true } }
}

impl MicroEffect {
    /// Height units written by `amount` = 1 at full mask.
    pub const MAX_HEIGHT: f32 = 0.1;

    /// The micro settings a generator of `kind` starts with when the checkbox is ticked: edge wear
    /// chips (carves), everything else deposits (raises).
    pub fn enabled_for(kind: GeneratorKind) -> Self {
        Self {
            enabled: true,
            direction: if kind == GeneratorKind::Edges { MicroDirection::Carve } else { MicroDirection::Raise },
            ..Self::default()
        }
    }

    /// Signed height at full mask (0 when off or non-finite).
    pub fn height(&self) -> f32 {
        if !self.enabled || !self.amount.is_finite() { return 0.0; }
        self.direction.sign() * self.amount.clamp(0.0, 1.0) * Self::MAX_HEIGHT
    }
}

/// Controls of Painter's `Dirt` and `Metal Edge Wear` generators that a plain threshold generator
/// lacks. Every field is a strength 0..1 whose neutral value (`Default`) changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PainterControls {
    /// Dirt: fraction of convex edges removed from the dirt (Painter `edges_masking`; 0 = none).
    pub edges_masking: f32,
    /// Wear: fraction of the wear removed where the surface is occluded (Painter
    /// `Ambient_Occlusion_Masking`; 0 = none).
    pub ao_masking: f32,
    /// Wear: strength of the baked curvature detail next to the geometric edge band
    /// (Painter `Curvature_Weight`; 1 = as before).
    pub curvature_weight: f32,
    /// Wear: extra softening of the band edge (Painter `Edges_Smoothness`; 0 = none).
    pub smoothness: f32,
    /// Multiplier of the breakup noise's pattern scale (Painter `Grunge_Scale` relative to the
    /// default; 1 = as before).
    pub grunge_scale: f32,
    /// Painter's grunge also leaves a soft low-level tail where the threshold would give none (its
    /// dirt reads 0.05 to 0.15 over unoccluded surfaces at Grunge Amount 0.61): the breakup noise
    /// times this is added to the mask, which never exceeds 1 (0 = no tail).
    #[serde(default)]
    pub grunge_floor: f32,
    /// Fine grain (a noise six times finer than the breakup) that speckles the threshold, so wear
    /// edges chip and dirt falls off in grains instead of smooth bands (0 = none).
    #[serde(default)]
    pub grain: f32,
}

impl Default for PainterControls {
    fn default() -> Self { Self { edges_masking: 0.0, ao_masking: 0.0, curvature_weight: 1.0, smoothness: 0.0, grunge_scale: 1.0, grunge_floor: 0.0, grain: 0.0 } }
}

fn is_zero_offset(offset: &[f32; 3]) -> bool { *offset == [0.0; 3] }

/// World axis a position gradient runs along (0 at the mesh minimum, 1 at its maximum).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientAxis { X, #[default] Y, Z }

impl GradientAxis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub fn is_y(&self) -> bool { *self == Self::Y }
    pub fn index(self) -> usize { match self { Self::X => 0, Self::Y => 1, Self::Z => 2 } }
    pub fn label(self) -> &'static str { match self { Self::X => "X", Self::Y => "Y", Self::Z => "Z" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeSpread {
    /// Reach as percent of the mesh's longest dimension, independent of Level.
    pub distance: f32,
    /// Irregularity of the boundary, independent of holes from Breakup.
    pub variation: f32,
    /// Level is the amount of wear: it sets how far into the Spread reach the wear extends
    /// (0 = none, 1 = the whole reach). False keeps the original behavior, where Level only
    /// selected baked convex detail and Spread alone set the reach. Not written when false, so
    /// older projects round-trip unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub level_reach: bool,
}

impl EdgeSpread {
    /// Fraction of the reach that Level 0..1 uncovers: steep at first so low Levels still show
    /// wear beyond the edge itself, smooth and strictly increasing, 0 at Level 0.
    pub fn level_extent(level: f32) -> f32 { level.clamp(0.0, 1.0).powf(0.4) }
}

impl Default for EdgeSpread {
    fn default() -> Self { Self { distance: 2.5, variation: 0.8, level_reach: true } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorKind {
    /// Convex edges from the curvature map.
    Edges,
    /// Cavities from ambient occlusion.
    Cavity,
    /// Upward-facing surfaces (world normal · direction).
    Directional,
    /// Gradient along a world axis (`Generator::axis`, default Y) within the mesh bounds.
    PositionGradient,
    /// Concave creases and crevices from the curvature map (the counterpart of `Edges`).
    Concave,
    /// Raw mesh map value.
    MeshMap(MeshMapKind),
}

impl GeneratorKind {
    pub fn label(self) -> String {
        match self {
            GeneratorKind::Edges => "Edges".into(),
            GeneratorKind::Cavity => "Dirt".into(),
            GeneratorKind::Directional => "Directional".into(),
            GeneratorKind::PositionGradient => "Position Gradient".into(),
            GeneratorKind::Concave => "Concave Curvature".into(),
            GeneratorKind::MeshMap(m) => format!("Mesh Map: {}", m.display_name()),
        }
    }
}

impl Generator {
    pub fn new(kind: GeneratorKind) -> Self {
        let mut noise = ProceduralParams::new(crate::fill::ProceduralKind::Clouds);
        noise.scale = 8.0;
        Self {
            kind,
            level: 0.5,
            softness: 0.15,
            invert: false,
            noise_amount: if matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity | GeneratorKind::Concave) { 0.3 } else { 0.0 },
            noise,
            organic_breakup: matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity | GeneratorKind::Concave),
            edge_spread: (kind == GeneratorKind::Edges).then(EdgeSpread::default),
            direction: [0.0, 1.0, 0.0],
            axis: GradientAxis::Y,
            offset: [0.0; 3],
            // New Dirt/Edges generators get fine grain so they chip and speckle; older projects keep None.
            painter: matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity).then(|| PainterControls {
                grain: 0.7,
                edges_masking: if kind == GeneratorKind::Cavity { 0.5 } else { 0.0 },
                grunge_floor: if kind == GeneratorKind::Cavity { 0.1 } else { 0.0 },
                ..PainterControls::default()
            }),
            micro: None,
        }
    }

    /// Whether the micro effect is on and strong enough to change anything.
    pub fn micro_active(&self) -> bool { self.micro.is_some_and(|m| m.height() != 0.0) }

    /// Switch the micro effect on or off, keeping tuned values and starting from the per-kind defaults.
    pub fn set_micro_enabled(&mut self, on: bool) {
        match (&mut self.micro, on) {
            (Some(m), _) => m.enabled = on,
            (None, true) => self.micro = Some(MicroEffect::enabled_for(self.kind)),
            (None, false) => {}
        }
    }

    /// Approximate feature size as a percentage of the whole mesh's longest dimension.
    pub fn pattern_size(&self) -> f32 { 100.0 / self.noise.scale.max(0.01) }

    pub fn set_pattern_size(&mut self, percent: f32) {
        if percent.is_finite() { self.noise.scale = 100.0 / percent.clamp(1.0, 100.0); }
    }
}

/// ID Color Mask: selects the texels whose baked ID map color (`MeshMapKind::Id`) lies within
/// `tolerance` (Euclidean RGB distance) of any picked color. See ADR-0041.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdColorMask {
    /// Picked ID colors, as stored in the baked map (raw values, never color-transformed).
    pub colors: Vec<[f32; 3]>,
    /// Maximum RGB distance still counted as a full match.
    pub tolerance: f32,
    /// Width of the falloff beyond `tolerance` (0 = hard edge).
    pub softness: f32,
    pub invert: bool,
}

impl Default for IdColorMask {
    fn default() -> Self { Self { colors: Vec::new(), tolerance: Self::DEFAULT_TOLERANCE, softness: 0.0, invert: false } }
}

impl IdColorMask {
    pub const DEFAULT_TOLERANCE: f32 = 0.05;
    /// Upper bound on picked colors (keeps per-texel cost and file contents bounded).
    pub const MAX_COLORS: usize = 64;

    /// Parameters a project file may contain.
    pub fn valid(&self) -> bool {
        self.colors.len() <= Self::MAX_COLORS
            && self.colors.iter().flatten().all(|c| c.is_finite())
            && (0.0..=1.0).contains(&self.tolerance)
            && (0.0..=1.0).contains(&self.softness)
    }

    /// Adds a color unless an identical one is already picked. Returns whether it was added.
    pub fn add_color(&mut self, color: [f32; 3]) -> bool {
        if self.colors.len() >= Self::MAX_COLORS || self.colors.contains(&color) || color.iter().any(|c| !c.is_finite()) {
            return false;
        }
        self.colors.push(color);
        true
    }

    /// Mask value 0..1 for one ID map texel (before `invert`): 1 within `tolerance` of the
    /// nearest picked color, falling smoothly to 0 over `softness`. No colors selects nothing.
    pub fn match_value(&self, rgb: [f32; 3]) -> f32 {
        let d2 = self
            .colors
            .iter()
            .map(|c| (c[0] - rgb[0]).powi(2) + (c[1] - rgb[1]).powi(2) + (c[2] - rgb[2]).powi(2))
            .fold(f32::INFINITY, f32::min);
        if !d2.is_finite() {
            return 0.0;
        }
        let d = d2.sqrt();
        if d <= self.tolerance {
            1.0
        } else if self.softness > 0.0 {
            let t = ((d - self.tolerance) / self.softness).clamp(0.0, 1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        } else {
            0.0
        }
    }

    /// Final value including `invert`.
    pub fn value(&self, rgb: [f32; 3]) -> f32 {
        let v = self.match_value(rgb);
        if self.invert { 1.0 - v } else { v }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MaskEffectKind {
    /// Additional paint raster (2 comps: value, alpha).
    Paint { image: TiledImage },
    /// Uniform or procedural grayscale fill through a projection.
    Fill { value: f32, procedural: Option<ProceduralParams>, projection: Projection },
    Generator(Generator),
    Levels(Levels),
    Filter(crate::Filter),
    Invert,
    /// Grayscale from an imported image through a projection: the image's alpha if it has one
    /// (a logo on a transparent background), else its brightness.
    Image { resource: subcraft_core::ResourceId, projection: Projection, invert: bool },
    /// Selection by baked ID map color (ADR-0041). Without a baked ID map it is black.
    IdColor(IdColorMask),
}

impl MaskEffectKind {
    pub fn label(&self) -> String {
        match self {
            MaskEffectKind::Paint { .. } => "Paint".into(),
            MaskEffectKind::Fill { .. } => "Fill".into(),
            MaskEffectKind::Generator(g) => format!("Generator ({})", g.kind.label()),
            MaskEffectKind::Levels(_) => "Levels".into(),
            MaskEffectKind::Filter(filter) => filter.label().into(),
            MaskEffectKind::Invert => "Invert".into(),
            MaskEffectKind::Image { .. } => "Image".into(),
            MaskEffectKind::IdColor(_) => "ID Color Mask".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaskEffect {
    pub id: EffectId,
    pub name: String,
    pub visible: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub kind: MaskEffectKind,
}

impl MaskEffect {
    pub fn new(kind: MaskEffectKind) -> Self {
        Self { id: EffectId::new(), name: kind.label(), visible: true, blend: BlendMode::Normal, opacity: 1.0, kind }
    }

    /// Equality ignoring paint texels (dirty-region diffing: painted tiles are diffed instead).
    pub fn same_parameters(&self, other: &MaskEffect) -> bool {
        let kinds = match (&self.kind, &other.kind) {
            (MaskEffectKind::Paint { image: a }, MaskEffectKind::Paint { image: b }) => a.same_layout(b) && a.fill() == b.fill(),
            (a, b) => a == b,
        };
        kinds && self.id == other.id && self.visible == other.visible && self.blend == other.blend && self.opacity == other.opacity
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub enabled: bool,
    /// Paintable base raster (1 comp). Fill value 1 = white mask, 0 = black mask.
    pub base: TiledImage,
    pub effects: Vec<Arc<MaskEffect>>,
}

impl Mask {
    pub fn new(resolution: u32, white: bool) -> Self {
        let v = if white { 1.0 } else { 0.0 };
        Self { enabled: true, base: TiledImage::new(resolution, resolution, 1, [v; 4]), effects: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_color_match_respects_tolerance_softness_and_invert() {
        let mut m = IdColorMask::default();
        assert_eq!(m.value([0.5, 0.5, 0.5]), 0.0, "no picked colors selects nothing");
        assert!(m.add_color([0.9, 0.3, 0.3]));
        assert!(!m.add_color([0.9, 0.3, 0.3]), "duplicates are ignored");
        assert_eq!(m.value([0.9, 0.3, 0.3]), 1.0);
        assert_eq!(m.value([0.9, 0.34, 0.3]), 1.0, "0.04 is within the default 0.05 tolerance");
        assert_eq!(m.value([0.9, 0.36, 0.3]), 0.0, "0.06 is outside, hard edge");
        m.softness = 0.1;
        let half = m.value([0.9, 0.4, 0.3]); // d = 0.1: halfway through the falloff
        assert!((half - 0.5).abs() < 1e-4, "{half}");
        assert_eq!(m.value([0.9, 0.46, 0.3]), 0.0);
        m.invert = true;
        assert_eq!(m.value([0.9, 0.3, 0.3]), 0.0);
        assert!(m.valid());
        m.tolerance = 2.0;
        assert!(!m.valid());
    }
}
