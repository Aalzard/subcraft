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
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeSpread {
    /// Reach as percent of the mesh's longest dimension, independent of Level.
    pub distance: f32,
    /// Irregularity of the boundary, independent of holes from Breakup.
    pub variation: f32,
}

impl Default for EdgeSpread {
    fn default() -> Self { Self { distance: 1.5, variation: 0.7 } }
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
    /// Gradient along world Y within the mesh bounds.
    PositionGradient,
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
            noise_amount: if matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity) { 0.3 } else { 0.0 },
            noise,
            organic_breakup: matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity),
            edge_spread: (kind == GeneratorKind::Edges).then(EdgeSpread::default),
            direction: [0.0, 1.0, 0.0],
        }
    }

    /// Approximate feature size as a percentage of the whole mesh's longest dimension.
    pub fn pattern_size(&self) -> f32 { 100.0 / self.noise.scale.max(0.01) }

    pub fn set_pattern_size(&mut self, percent: f32) {
        if percent.is_finite() { self.noise.scale = 100.0 / percent.clamp(1.0, 100.0); }
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
    Invert,
    /// Grayscale from an imported image through a projection: the image's alpha if it has one
    /// (a logo on a transparent background), else its brightness.
    Image { resource: subcraft_core::ResourceId, projection: Projection, invert: bool },
}

impl MaskEffectKind {
    pub fn label(&self) -> String {
        match self {
            MaskEffectKind::Paint { .. } => "Paint".into(),
            MaskEffectKind::Fill { .. } => "Fill".into(),
            MaskEffectKind::Generator(g) => format!("Generator ({})", g.kind.label()),
            MaskEffectKind::Levels(_) => "Levels".into(),
            MaskEffectKind::Invert => "Invert".into(),
            MaskEffectKind::Image { .. } => "Image".into(),
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
