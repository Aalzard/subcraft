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
}

impl ProceduralKind {
    pub const ALL: [ProceduralKind; 9] = [
        ProceduralKind::Clouds,
        ProceduralKind::Cells,
        ProceduralKind::Grunge,
        ProceduralKind::Scratches,
        ProceduralKind::WoodRings,
        ProceduralKind::Brushed,
        ProceduralKind::Checker,
        ProceduralKind::GradientU,
        ProceduralKind::GradientV,
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
}

impl ProceduralParams {
    pub fn new(kind: ProceduralKind) -> Self {
        Self { kind, scale: 4.0, seed: 1, balance: 0.5, contrast: 0.0, invert: false }
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
