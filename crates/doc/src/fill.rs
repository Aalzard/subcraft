//! Fill sources and projections (shared by fill layers, mask fills and brush materials).

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelValue, MeshMapKind, ResourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionMode {
    /// Use mesh UVs.
    #[default]
    Uv,
    /// Blend three world-axis planar projections by surface normal.
    Triplanar,
    /// Planar projection along an object axis.
    Planar,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub mode: ProjectionMode,
    /// Tiling (UV mode) or world units per tile inverse (triplanar/planar).
    pub scale: [f32; 2],
    pub rotation_deg: f32,
    pub offset: [f32; 2],
    /// Triplanar blend sharpness (1 = soft, 32 = hard).
    pub triplanar_sharpness: f32,
    /// Planar projection axis (0 = X, 1 = Y, 2 = Z).
    pub planar_axis: u8,
}

impl Default for Projection {
    fn default() -> Self {
        Self {
            mode: ProjectionMode::Uv,
            scale: [1.0, 1.0],
            rotation_deg: 0.0,
            offset: [0.0, 0.0],
            triplanar_sharpness: 8.0,
            planar_axis: 2,
        }
    }
}

impl Projection {
    /// Apply scale/rotation/offset to a 2D coordinate.
    pub fn transform_2d(&self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        let x = p[0] * self.scale[0];
        let y = p[1] * self.scale[1];
        [x * c - y * s + self.offset[0], x * s + y * c + self.offset[1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_scales_x_and_y_separately_then_offsets() {
        let p = Projection { scale: [2.0, 3.0], offset: [0.25, -0.5], ..Projection::default() };
        assert_eq!(p.transform_2d([1.0, 1.0]), [2.25, 2.5]);
        let r = Projection { rotation_deg: 90.0, ..Projection::default() };
        let q = r.transform_2d([1.0, 0.0]);
        assert!(q[0].abs() < 1e-6 && (q[1] - 1.0).abs() < 1e-6, "{q:?}");
    }
}

/// Procedural pattern generators (original implementations in `subcraft-materials`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralKind {
    /// Fractal value/gradient noise.
    Clouds,
    /// Cellular (Worley) distance pattern.
    Cells,
    /// Sharp thresholded noise splotches.
    Grunge,
    /// Thin directional scratches.
    Scratches,
    /// Concentric rings with noise distortion.
    WoodRings,
    /// Anisotropic stretched noise.
    Brushed,
    Checker,
    /// Linear gradient along U.
    GradientU,
    /// Linear gradient along V.
    GradientV,
    /// Blotchy grime with fine specks and soft smears.
    Dirt,
    /// Thin cracks along cell borders (1 on a crack).
    Cracks,
    /// Vertical streaks, like rain or drips running down a surface.
    Streaks,
    /// Fine scattered dots.
    Speckles,
    /// Small pits: one dot per cell.
    Pores,
    /// Scratch generator: long hairlines and gouges with direction, length, width, depth,
    /// curvature, density, clustering and curvature-driven wear (`ProceduralParams::scratch`).
    Scratch,
}

impl ProceduralKind {
    pub const ALL: [ProceduralKind; 15] = [
        ProceduralKind::Clouds,
        ProceduralKind::Cells,
        ProceduralKind::Grunge,
        ProceduralKind::Scratches,
        ProceduralKind::WoodRings,
        ProceduralKind::Brushed,
        ProceduralKind::Checker,
        ProceduralKind::GradientU,
        ProceduralKind::GradientV,
        ProceduralKind::Dirt,
        ProceduralKind::Cracks,
        ProceduralKind::Streaks,
        ProceduralKind::Speckles,
        ProceduralKind::Pores,
        ProceduralKind::Scratch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProceduralKind::Clouds => "Clouds",
            ProceduralKind::Cells => "Cells",
            ProceduralKind::Grunge => "Grunge",
            ProceduralKind::Scratches => "Scratches",
            ProceduralKind::WoodRings => "Wood Rings",
            ProceduralKind::Brushed => "Brushed",
            ProceduralKind::Checker => "Checker",
            ProceduralKind::GradientU => "Gradient U",
            ProceduralKind::GradientV => "Gradient V",
            ProceduralKind::Dirt => "Dirt",
            ProceduralKind::Cracks => "Cracks",
            ProceduralKind::Streaks => "Streaks",
            ProceduralKind::Speckles => "Speckles",
            ProceduralKind::Pores => "Pores",
            ProceduralKind::Scratch => "Scratch Generator",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProceduralParams {
    pub kind: ProceduralKind,
    /// Feature frequency multiplier.
    pub scale: f32,
    pub seed: u32,
    /// Remap: output = smoothstep(balance - contrast, balance + contrast, raw).
    pub balance: f32,
    pub contrast: f32,
    pub invert: bool,
    /// Each color component is its own pattern (seeds `seed`, `seed + 1`, `seed + 2`), mapped
    /// `low`..`high` per component: colored noise. Not written when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colored: bool,
    /// Settings of [`ProceduralKind::Scratch`]; ignored by every other kind and not written
    /// while at its defaults, so older projects serialize unchanged.
    #[serde(default, skip_serializing_if = "ScratchParams::is_default")]
    pub scratch: ScratchParams,
}

impl ProceduralParams {
    pub fn new(kind: ProceduralKind) -> Self {
        Self { kind, scale: 4.0, seed: 1, balance: 0.5, contrast: 0.0, invert: false, colored: false, scratch: ScratchParams::default() }
    }
}

/// Scratch generator settings. Lengths and widths are in lattice cells (`ProceduralParams::scale`
/// cells per tile), so the pattern is the same at every resolution and tiles when `scale` is
/// an integer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScratchParams {
    /// Mean direction, degrees (0 = along U, 90 = along V).
    pub angle_deg: f32,
    /// Random deviation from the mean direction, +- degrees (90 = any direction).
    pub spread_deg: f32,
    /// Mean length in cells.
    pub length: f32,
    /// Random length variation 0..1.
    pub length_var: f32,
    /// Mean half-width in cells.
    pub width: f32,
    /// Random width variation 0..1.
    pub width_var: f32,
    /// Probability that a candidate scratch exists (up to 6 per cell), 0..1.
    pub density: f32,
    /// Sideways bend 0..1.
    pub curvature: f32,
    /// 0 = full depth along the whole scratch, 1 = pointed ends and shallow/deep variation.
    pub depth_falloff: f32,
    /// 0 = even distribution, 1 = scratches gather in patches.
    pub cluster: f32,
    /// 0 = ignore the baked curvature map, 1 = scratches only on edges (needs a bake).
    pub wear: f32,
}

impl Default for ScratchParams {
    fn default() -> Self {
        Self { angle_deg: 0.0, spread_deg: 90.0, length: 1.0, length_var: 0.5, width: 0.012, width_var: 0.5, density: 0.35, curvature: 0.1, depth_falloff: 0.6, cluster: 0.0, wear: 0.0 }
    }
}

impl ScratchParams {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Named starting points for the scratch generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScratchPreset {
    FineHairlines,
    HeavyGouges,
    BrushedMetal,
    ScuffedPlastic,
    Cracked,
}

impl ScratchPreset {
    pub const ALL: [ScratchPreset; 5] =
        [ScratchPreset::FineHairlines, ScratchPreset::HeavyGouges, ScratchPreset::BrushedMetal, ScratchPreset::ScuffedPlastic, ScratchPreset::Cracked];

    pub fn label(self) -> &'static str {
        match self {
            ScratchPreset::FineHairlines => "Fine hairlines",
            ScratchPreset::HeavyGouges => "Heavy gouges",
            ScratchPreset::BrushedMetal => "Brushed metal",
            ScratchPreset::ScuffedPlastic => "Scuffed plastic",
            ScratchPreset::Cracked => "Cracked",
        }
    }

    /// Scratch settings and cell scale of the preset.
    pub fn params(self) -> (f32, ScratchParams) {
        let d = ScratchParams::default();
        match self {
            ScratchPreset::FineHairlines => (6.0, ScratchParams { spread_deg: 90.0, length: 1.6, length_var: 0.6, width: 0.004, width_var: 0.4, density: 0.5, curvature: 0.15, depth_falloff: 0.8, cluster: 0.4, ..d }),
            ScratchPreset::HeavyGouges => (3.0, ScratchParams { angle_deg: 35.0, spread_deg: 40.0, length: 1.2, length_var: 0.7, width: 0.04, width_var: 0.6, density: 0.12, curvature: 0.05, depth_falloff: 0.5, cluster: 0.3, ..d }),
            ScratchPreset::BrushedMetal => (10.0, ScratchParams { angle_deg: 0.0, spread_deg: 3.0, length: 2.0, length_var: 0.6, width: 0.005, width_var: 0.5, density: 0.9, curvature: 0.0, depth_falloff: 0.3, cluster: 0.0, ..d }),
            ScratchPreset::ScuffedPlastic => (5.0, ScratchParams { angle_deg: 20.0, spread_deg: 60.0, length: 0.6, length_var: 0.8, width: 0.02, width_var: 0.8, density: 0.45, curvature: 0.5, depth_falloff: 1.0, cluster: 0.8, ..d }),
            ScratchPreset::Cracked => (3.0, ScratchParams { spread_deg: 90.0, length: 1.5, length_var: 0.5, width: 0.01, width_var: 0.3, density: 0.3, curvature: 0.7, depth_falloff: 0.9, cluster: 0.2, ..d }),
        }
    }

    /// Set the scale and scratch settings of `params` (kind becomes Scratch; seed is kept).
    pub fn apply(self, params: &mut ProceduralParams) {
        let (scale, scratch) = self.params();
        params.kind = ProceduralKind::Scratch;
        params.scale = scale;
        params.scratch = scratch;
    }
}

/// Where a channel's value comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FillSource {
    Uniform { value: ChannelValue },
    /// Imported bitmap resource (sampled through the projection).
    Bitmap { resource: ResourceId },
    /// Procedural pattern; grayscale result maps `low`..`high`.
    Procedural { params: ProceduralParams, low: ChannelValue, high: ChannelValue },
    /// Baked mesh map value.
    MeshMap { map: MeshMapKind },
}

impl FillSource {
    pub fn uniform(v: ChannelValue) -> Self {
        FillSource::Uniform { value: v }
    }
}
