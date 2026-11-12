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
    /// Input image (ADR-0045): an anchor point read instead of the baked mesh map (Edges, Dirt,
    /// Concave, Ambient Occlusion and Mesh Map kinds). Absent: the mesh map, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<crate::anchor::AnchorInput>,
    /// Settings of [`GeneratorKind::MaskEditor`]. Absent for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<MaskEditor>,
    /// Settings of the position, UV and conversion generators (3D Distance, 3D Linear Gradient,
    /// UV Random Color, Grayscale Conversion) and the Light's attenuation. Absent: their defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeSettings>,
}

/// Parameters of the position, UV and conversion generators. Positions are in the model's
/// bounding box, 0..=1 on each axis (Painter's Position map colours).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeSettings {
    /// 3D Distance: the source point.
    pub point: [f32; 3],
    /// 3D Distance: falloff length, in units of the model's longest side.
    pub radius: f32,
    /// 3D Distance: distance kept fully white before the falloff starts (same units).
    pub offset: f32,
    /// 3D Linear Gradient: where the gradient is 0 and where it is 1.
    pub start: [f32; 3],
    pub end: [f32; 3],
    /// Balance / Contrast of the result (see [`balance_contrast`]).
    pub balance: f32,
    pub contrast: f32,
    /// UV Random Color: the random sequence.
    pub seed: u32,
    /// Grayscale Conversion: the source image, how it becomes gray, its tiling and rotation
    /// (degrees; Safe Rotation snaps to 45 degree steps).
    pub texture: Option<subcraft_core::ResourceId>,
    pub grayscale: GrayscaleType,
    pub tile: f32,
    pub rotation: f32,
    pub safe_rotation: bool,
    /// Light: 0 lights the whole model alike; 1 fades it out completely toward the far side.
    pub attenuation: f32,
}

impl Default for ShapeSettings {
    fn default() -> Self {
        Self {
            point: [0.5; 3],
            radius: 0.5,
            offset: 0.0,
            start: [0.5, 0.0, 0.5],
            end: [0.5, 1.0, 0.5],
            balance: 0.5,
            contrast: 0.0,
            seed: 1,
            texture: None,
            grayscale: GrayscaleType::Luma,
            tile: 1.0,
            rotation: 0.0,
            safe_rotation: false,
            attenuation: 0.0,
        }
    }
}

impl ShapeSettings {
    pub fn valid(&self) -> bool {
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        self.point.iter().chain(&self.start).chain(&self.end).all(|v| v.is_finite() && (-1.0..=2.0).contains(v))
            && self.radius.is_finite() && (0.0..=4.0).contains(&self.radius)
            && self.offset.is_finite() && (0.0..=4.0).contains(&self.offset)
            && unit(self.balance) && unit(self.contrast) && unit(self.attenuation)
            && self.tile.is_finite() && (0.01..=256.0).contains(&self.tile)
            && self.rotation.is_finite() && (-360.0..=360.0).contains(&self.rotation)
    }

    /// The rotation in effect: snapped to 45 degree steps with Safe Rotation.
    pub fn effective_rotation(&self) -> f32 {
        if self.safe_rotation { (self.rotation / 45.0).round() * 45.0 } else { self.rotation }
    }

    /// 3D Distance before Balance / Contrast: 1 within `offset` of the point, falling linearly
    /// to 0 over `radius` (`d` in units of the model's longest side).
    pub fn distance_value(&self, d: f32) -> f32 {
        if self.radius <= 1e-6 {
            return if d <= self.offset { 1.0 } else { 0.0 };
        }
        (1.0 - (d - self.offset) / self.radius).clamp(0.0, 1.0)
    }

    /// 3D Linear Gradient before Balance / Contrast: the position projected on start -> end.
    pub fn gradient_value(&self, p: [f32; 3], start: [f32; 3], end: [f32; 3]) -> f32 {
        let d: [f32; 3] = std::array::from_fn(|a| end[a] - start[a]);
        let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if len2 <= 1e-12 {
            return 0.0;
        }
        (((p[0] - start[0]) * d[0] + (p[1] - start[1]) * d[1] + (p[2] - start[2]) * d[2]) / len2).clamp(0.0, 1.0)
    }

    /// UV Random Color: one gray value per UV island, from the island and the seed.
    pub fn island_value(&self, island: u32) -> f32 {
        let mut h = (island as u64) ^ ((self.seed as u64) << 32) ^ 0x9e37_79b9_7f4a_7c15;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        (h >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Light direction from Painter's angles (degrees): horizontal around the up axis (0 = toward
/// +Z, 90 = toward +X), vertical above the horizon (90 = straight up).
pub fn light_direction(horizontal: f32, vertical: f32) -> [f32; 3] {
    let (h, v) = (horizontal.to_radians(), vertical.to_radians());
    [v.cos() * h.sin(), v.sin(), v.cos() * h.cos()]
}

/// Painter's angles (degrees) of a light direction: the inverse of [`light_direction`].
pub fn light_angles(dir: [f32; 3]) -> (f32, f32) {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt().max(1e-12);
    let (x, y, z) = (dir[0] / len, dir[1] / len, dir[2] / len);
    let vertical = y.clamp(-1.0, 1.0).asin().to_degrees();
    let horizontal = if x.abs() < 1e-6 && z.abs() < 1e-6 { 0.0 } else { x.atan2(z).to_degrees() };
    (horizontal, vertical)
}

/// How a color texture becomes one gray value (the Grayscale Conversion options).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrayscaleType {
    /// Mean of the largest and smallest component (HSL lightness).
    Desaturation,
    /// Rec. 709 luminance.
    #[default]
    Luma,
    Average,
    Max,
    Min,
    Red,
    Green,
    Blue,
}

impl GrayscaleType {
    pub const ALL: [Self; 8] = [Self::Desaturation, Self::Luma, Self::Average, Self::Max, Self::Min, Self::Red, Self::Green, Self::Blue];
    pub fn label(self) -> &'static str {
        match self {
            Self::Desaturation => "Desaturation", Self::Luma => "Luma", Self::Average => "Average", Self::Max => "Max", Self::Min => "Min",
            Self::Red => "Red channel", Self::Green => "Green channel", Self::Blue => "Blue channel",
        }
    }
    pub fn convert(self, c: [f32; 3]) -> f32 {
        let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
        match self {
            Self::Desaturation => (max + min) * 0.5,
            Self::Luma => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2],
            Self::Average => (c[0] + c[1] + c[2]) / 3.0,
            Self::Max => max,
            Self::Min => min,
            Self::Red => c[0],
            Self::Green => c[1],
            Self::Blue => c[2],
        }
    }
}

/// What the Mask Editor's Curvature group extracts from the baked curvature (0.5 = flat).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurvatureMode {
    /// Convex edges: `(c - 0.5) * 2`, clamped.
    #[default]
    Edges,
    /// Concave creases: `(0.5 - c) * 2`, clamped.
    Cavities,
    /// Both: `|c - 0.5| * 2`.
    Dual,
    /// The map as baked.
    Unprocessed,
}

impl CurvatureMode {
    pub const ALL: [Self; 4] = [Self::Edges, Self::Cavities, Self::Dual, Self::Unprocessed];
    pub fn label(self) -> &'static str {
        match self { Self::Edges => "Edges", Self::Cavities => "Cavities", Self::Dual => "Dual", Self::Unprocessed => "Unprocessed" }
    }
    pub fn extract(self, c: f32) -> f32 {
        match self {
            Self::Edges => ((c - 0.5) * 2.0).clamp(0.0, 1.0),
            Self::Cavities => ((0.5 - c) * 2.0).clamp(0.0, 1.0),
            Self::Dual => ((c - 0.5).abs() * 2.0).clamp(0.0, 1.0),
            Self::Unprocessed => c,
        }
    }
}

/// Contrast then brightness, each -1..=1 with 0 neutral: positive contrast stretches around 0.5
/// by `1 / (1 - contrast)` (1 = a hard step), negative compresses by `1 + contrast`; brightness
/// is added. Clamped to 0..=1.
pub fn contrast_brightness(v: f32, contrast: f32, brightness: f32) -> f32 {
    let d = v - 0.5;
    let d = if contrast >= 1.0 { if d > 0.0 { 0.5 } else if d < 0.0 { -0.5 } else { 0.0 } } else if contrast >= 0.0 { d / (1.0 - contrast) } else { d * (1.0 + contrast) };
    (0.5 + d + brightness).clamp(0.0, 1.0)
}

/// Balance / Contrast remap, each 0..=1: a linear ramp `1 - contrast` wide centred on
/// `1 - balance` (Balance 0.5, Contrast 0 is the identity; Contrast 1 a hard threshold; a higher
/// Balance gives more white).
pub fn balance_contrast(v: f32, balance: f32, contrast: f32) -> f32 {
    let (centre, width) = (1.0 - balance, 1.0 - contrast);
    if width <= 1e-6 { return if v >= centre { 1.0 } else { 0.0 }; }
    ((v - (centre - width * 0.5)) / width).clamp(0.0, 1.0)
}

/// A texture contribution of the Mask Editor (its Texture and Texture 2 groups).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorTexture {
    /// The image; none contributes nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<subcraft_core::ResourceId>,
    pub opacity: f32,
    pub invert: bool,
    pub grayscale: GrayscaleType,
    pub blend: BlendMode,
    /// Repetitions across the UV square (UV) or the model's longest dimension (tri-planar).
    pub scale: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub triplanar: bool,
    /// Sharpness of the blend between the three projection planes, 0..=1.
    pub triplanar_contrast: f32,
    /// Keep the image's aspect ratio: a non-square image repeats `scale` times along its width
    /// and proportionally along its height.
    pub non_square: bool,
}

impl Default for EditorTexture {
    fn default() -> Self {
        Self { resource: None, opacity: 0.0, invert: false, grayscale: GrayscaleType::Luma, blend: BlendMode::Normal, scale: 1.0, contrast: 0.0, brightness: 0.0, triplanar: false, triplanar_contrast: 0.5, non_square: false }
    }
}

impl EditorTexture {
    /// Gray value of a sampled texel through the group's settings.
    pub fn shape(&self, rgb: [f32; 3]) -> f32 {
        let v = contrast_brightness(self.grayscale.convert(rgb).clamp(0.0, 1.0), self.contrast, self.brightness);
        if self.invert { 1.0 - v } else { v }
    }
    /// Texture repetitions along `[U, V]` for an image of `width` x `height`.
    pub fn tiling(&self, width: u32, height: u32) -> [f32; 2] {
        let aspect = if self.non_square && height > 0 { width as f32 / height as f32 } else { 1.0 };
        [self.scale, self.scale * aspect]
    }
    /// Tri-planar blend exponent of `triplanar_contrast` (0 = widest blend, 1 = a hard cut).
    pub fn triplanar_sharpness(&self) -> f32 { 1.0 / (1.0 - self.triplanar_contrast.clamp(0.0, 0.98)) }
}

/// The Mask Editor's Ambient Occlusion group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorOcclusion {
    pub opacity: f32,
    pub invert: bool,
    pub blend: BlendMode,
    pub balance: f32,
    pub contrast: f32,
}

impl Default for EditorOcclusion {
    fn default() -> Self { Self { opacity: 0.0, invert: false, blend: BlendMode::Normal, balance: 0.5, contrast: 0.0 } }
}

impl EditorOcclusion {
    pub fn shape(&self, ao: f32) -> f32 {
        let v = balance_contrast(ao, self.balance, self.contrast);
        if self.invert { 1.0 - v } else { v }
    }
}

/// The Mask Editor's Curvature group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorCurvature {
    pub opacity: f32,
    pub invert: bool,
    pub blend: BlendMode,
    pub mode: CurvatureMode,
    pub contrast: f32,
    pub brightness: f32,
}

impl Default for EditorCurvature {
    fn default() -> Self { Self { opacity: 1.0, invert: false, blend: BlendMode::Normal, mode: CurvatureMode::Edges, contrast: 0.0, brightness: 0.0 } }
}

impl EditorCurvature {
    pub fn shape(&self, curvature: f32) -> f32 {
        let v = contrast_brightness(self.mode.extract(curvature), self.contrast, self.brightness);
        if self.invert { 1.0 - v } else { v }
    }
}

/// One direction (Right to Left, Top To Bottom, Front to Back) of a directional group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorDirection {
    /// How much of this direction's gradient is blended in, 0..=1.
    pub amount: f32,
    pub invert: bool,
    pub blend: BlendMode,
}

impl Default for EditorDirection {
    fn default() -> Self { Self { amount: 0.0, invert: false, blend: BlendMode::Normal } }
}

/// The Mask Editor's World Space Normal and Position Gradient groups: three per-axis gradients
/// blended in order (X = Right to Left, Y = Top To Bottom, Z = Front to Back), then remapped.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorDirections {
    pub opacity: f32,
    pub invert: bool,
    pub blend: BlendMode,
    pub balance: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub axes: [EditorDirection; 3],
}

impl Default for EditorDirections {
    fn default() -> Self {
        // Top To Bottom on, the other two off: the group shows something as soon as its opacity is raised.
        let axis = |amount| EditorDirection { amount, ..EditorDirection::default() };
        Self { opacity: 0.0, invert: false, blend: BlendMode::Normal, balance: 0.5, contrast: 0.0, brightness: 0.0, axes: [axis(0.0), axis(1.0), axis(0.0)] }
    }
}

impl EditorDirections {
    pub const AXIS_LABELS: [&'static str; 3] = ["Right to Left", "Top To Bottom", "Front to Back"];

    /// The group's value from the three per-axis gradients (each 0..=1, 1 = right / top / front).
    /// `blend(mode, below, value)` is the grayscale blend function.
    pub fn shape(&self, gradients: [f32; 3], blend: impl Fn(BlendMode, f32, f32) -> f32) -> f32 {
        let mut v = 0.0;
        for (axis, g) in self.axes.iter().zip(gradients) {
            let amount = axis.amount.clamp(0.0, 1.0);
            if amount > 0.0 {
                let g = if axis.invert { 1.0 - g } else { g }.clamp(0.0, 1.0);
                v += (blend(axis.blend, v, g).clamp(0.0, 1.0) - v) * amount;
            }
        }
        let v = (balance_contrast(v, self.balance, self.contrast) + self.brightness).clamp(0.0, 1.0);
        if self.invert { 1.0 - v } else { v }
    }
}

/// The Mask Editor's Thickness group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorThickness {
    pub opacity: f32,
    pub invert: bool,
    pub contrast: f32,
    pub brightness: f32,
}

impl Default for EditorThickness {
    fn default() -> Self { Self { opacity: 0.0, invert: false, contrast: 0.0, brightness: 0.0 } }
}

impl EditorThickness {
    pub fn shape(&self, thickness: f32) -> f32 {
        let v = contrast_brightness(thickness, self.contrast, self.brightness);
        if self.invert { 1.0 - v } else { v }
    }
}

/// Settings of [`GeneratorKind::MaskEditor`]: seven contributions (two textures and five mesh-map
/// groups) blended bottom to top over black, each by its own opacity and blending mode, then the
/// global Blur, Balance, Contrast and Invert.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaskEditor {
    pub global_invert: bool,
    /// Gaussian radius in texture pixels applied to the blended contributions (0 = none).
    pub global_blur: f32,
    pub global_balance: f32,
    pub global_contrast: f32,
    pub texture: EditorTexture,
    pub texture2: EditorTexture,
    pub occlusion: EditorOcclusion,
    pub curvature: EditorCurvature,
    pub world_normal: EditorDirections,
    pub position: EditorDirections,
    pub thickness: EditorThickness,
}

impl Default for MaskEditor {
    fn default() -> Self {
        Self {
            global_invert: false, global_blur: 0.0, global_balance: 0.5, global_contrast: 0.0,
            texture: EditorTexture::default(), texture2: EditorTexture::default(), occlusion: EditorOcclusion::default(), curvature: EditorCurvature::default(),
            world_normal: EditorDirections::default(), position: EditorDirections::default(), thickness: EditorThickness::default(),
        }
    }
}

impl MaskEditor {
    /// Largest global blur radius, in pixels (the Blur filter's limit).
    pub const MAX_BLUR: f32 = 32.0;

    /// The global blur radius in effect (0 for anything out of range or non-finite).
    pub fn blur_radius(&self) -> f32 {
        if self.global_blur.is_finite() { self.global_blur.clamp(0.0, Self::MAX_BLUR) } else { 0.0 }
    }

    /// The final value from the blended (and blurred) contributions: Balance, Contrast, Invert.
    pub fn finish(&self, v: f32) -> f32 {
        let v = balance_contrast(v.clamp(0.0, 1.0), self.global_balance, self.global_contrast);
        if self.global_invert { 1.0 - v } else { v }
    }

    /// The image resources the two texture groups read.
    pub fn resources(&self) -> impl Iterator<Item = subcraft_core::ResourceId> {
        [self.texture.resource, self.texture2.resource].into_iter().flatten()
    }

    /// Whether every number is finite and inside the range its control offers.
    pub fn valid(&self) -> bool {
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        let signed = |v: f32| v.is_finite() && (-1.0..=1.0).contains(&v);
        let texture = |t: &EditorTexture| unit(t.opacity) && t.scale.is_finite() && (0.01..=256.0).contains(&t.scale) && signed(t.contrast) && signed(t.brightness) && unit(t.triplanar_contrast);
        let directions = |d: &EditorDirections| unit(d.opacity) && unit(d.balance) && unit(d.contrast) && signed(d.brightness) && d.axes.iter().all(|a| unit(a.amount));
        self.global_blur.is_finite() && (0.0..=Self::MAX_BLUR).contains(&self.global_blur) && unit(self.global_balance) && unit(self.global_contrast)
            && texture(&self.texture) && texture(&self.texture2)
            && unit(self.occlusion.opacity) && unit(self.occlusion.balance) && unit(self.occlusion.contrast)
            && unit(self.curvature.opacity) && signed(self.curvature.contrast) && signed(self.curvature.brightness)
            && directions(&self.world_normal) && directions(&self.position)
            && unit(self.thickness.opacity) && signed(self.thickness.contrast) && signed(self.thickness.brightness)
    }
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
    /// Break the mask up with the bundled grayscale grunge map (a photo-like dirt texture, as
    /// Painter's generators do) instead of the smooth organic volume noise. Absent in older
    /// projects: false, their original pattern. Not written when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grunge_map: bool,
    /// Painter's custom grunge (Use Custom Grunge): an image that replaces the breakup noise,
    /// projected tri-planar on the model at the pattern size. Absent: the noise or grunge map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_grunge: Option<subcraft_core::ResourceId>,
}

impl Default for PainterControls {
    fn default() -> Self { Self { edges_masking: 0.0, ao_masking: 0.0, curvature_weight: 1.0, smoothness: 0.0, grunge_scale: 1.0, grunge_floor: 0.0, grain: 0.0, grunge_map: false, custom_grunge: None } }
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
    /// Painter's Ambient Occlusion generator: occluded areas white (1 - baked AO), a smooth
    /// Balance / Contrast remap with no breakup by default.
    AmbientOcclusion,
    /// Painter's Mask Editor: textures and mesh maps blended by per-group opacity and blending
    /// mode, with global Blur / Balance / Contrast ([`Generator::editor`]).
    MaskEditor,
    /// Painter's 3D Distance: white at a point of the model, fading with the distance from it.
    #[serde(rename = "distance_3d")]
    Distance3D,
    /// Painter's 3D Linear Gradient: 0 at one point of the model, 1 at another.
    #[serde(rename = "linear_gradient_3d")]
    LinearGradient3D,
    /// Painter's UV Random Color (as a mask): one random gray per UV island.
    UvRandomColor,
    /// Painter's Grayscale Conversion: an image turned gray.
    GrayscaleConversion,
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
            GeneratorKind::AmbientOcclusion => "Ambient Occlusion".into(),
            GeneratorKind::MaskEditor => "Mask Editor".into(),
            GeneratorKind::Distance3D => "3D Distance".into(),
            GeneratorKind::LinearGradient3D => "3D Linear Gradient".into(),
            GeneratorKind::UvRandomColor => "UV Random Color".into(),
            GeneratorKind::GrayscaleConversion => "Grayscale Conversion".into(),
        }
    }

    /// Kinds whose settings live in [`Generator::shape`] (Light keeps Level / Softness and only
    /// reads its attenuation there).
    pub fn uses_shape(self) -> bool {
        matches!(self, GeneratorKind::Distance3D | GeneratorKind::LinearGradient3D | GeneratorKind::UvRandomColor | GeneratorKind::GrayscaleConversion)
    }

    /// Painter-style kinds whose Level / Softness the editor shows as Painter's own controls
    /// (Dirt Level / Dirt Contrast, Wear Level / Wear Contrast, Balance / Contrast).
    pub fn uses_painter_names(self) -> bool {
        matches!(self, GeneratorKind::Edges | GeneratorKind::Cavity | GeneratorKind::AmbientOcclusion | GeneratorKind::MeshMap(_))
    }
}

impl Generator {
    /// A generator as "add generator" creates it, with Substance Painter's defaults for the kinds
    /// Painter has: Dirt (Dirt Level 0.75, Dirt Contrast 0.39, Grunge Amount 0.52, Pattern size 16%,
    /// Edges Masking 0, Grunge Floor 0.73, grunge from the bundled grunge map, Grain 0), Metal Edge
    /// Wear (Wear Level 0.3 of the Spread reach, crisp contrast, grunge map breakup), Ambient
    /// Occlusion (Balance 0.53, Contrast 0, no breakup, Pattern size 12.5%) and raw mesh maps
    /// (Balance 0.5, Contrast 0: the map itself, smooth, no breakup). Seed 1, offsets 0.
    pub fn new(kind: GeneratorKind) -> Self {
        let mut noise = ProceduralParams::new(crate::fill::ProceduralKind::Clouds);
        // Pattern size 12.5%; Dirt 16%.
        noise.scale = if kind == GeneratorKind::Cavity { 100.0 / 16.0 } else { 8.0 };
        let (level, softness, noise_amount) = match kind {
            // Dirt Level 0.75 = threshold 0.25 on the occlusion; Dirt Contrast 0.39; Grunge Amount 0.52.
            GeneratorKind::Cavity => (0.25, 0.61, 0.52),
            // Wear Level 0.3; Wear Contrast 0.85; Grunge Amount 0.3.
            GeneratorKind::Edges => (0.3, 0.15, 0.3),
            GeneratorKind::Concave => (0.5, 0.15, 0.3),
            // Balance 0.53 (threshold 0.47), Contrast 0: smoothstep over the whole range, smooth.
            GeneratorKind::AmbientOcclusion => (0.47, 1.0, 0.0),
            // Balance 0.5, Contrast 0: the map itself.
            GeneratorKind::MeshMap(_) => (0.5, 1.0, 0.0),
            GeneratorKind::Directional | GeneratorKind::PositionGradient => (0.5, 0.15, 0.0),
            // Unused by the Mask Editor (its own settings are in `editor`) and the shape kinds
            // (theirs are in `shape`).
            GeneratorKind::MaskEditor | GeneratorKind::Distance3D | GeneratorKind::LinearGradient3D | GeneratorKind::UvRandomColor | GeneratorKind::GrayscaleConversion => (0.5, 1.0, 0.0),
        };
        Self {
            kind,
            level,
            softness,
            invert: false,
            noise_amount,
            noise,
            organic_breakup: matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity | GeneratorKind::Concave),
            edge_spread: (kind == GeneratorKind::Edges).then(EdgeSpread::default),
            direction: [0.0, 1.0, 0.0],
            axis: GradientAxis::Y,
            offset: [0.0; 3],
            // Dirt and Edge wear break up with the grunge map; its own detail replaces the fine grain.
            painter: matches!(kind, GeneratorKind::Edges | GeneratorKind::Cavity).then(|| PainterControls {
                edges_masking: 0.0,
                grunge_floor: if kind == GeneratorKind::Cavity { 0.73 } else { 0.0 },
                grunge_map: true,
                ..PainterControls::default()
            }),
            micro: None,
            input: None,
            editor: (kind == GeneratorKind::MaskEditor).then(MaskEditor::default),
            shape: kind.uses_shape().then(ShapeSettings::default),
        }
    }

    /// The position / UV / conversion settings in effect (defaults when absent).
    pub fn shape_settings(&self) -> ShapeSettings {
        self.shape.unwrap_or_default()
    }

    /// The Mask Editor settings in effect (the defaults when the kind is set but they are absent).
    pub fn editor_settings(&self) -> MaskEditor { self.editor.unwrap_or_default() }

    /// Global blur radius (pixels) of a Mask Editor; 0 for every other kind.
    pub fn editor_blur(&self) -> f32 {
        if self.kind == GeneratorKind::MaskEditor { self.editor_settings().blur_radius() } else { 0.0 }
    }

    /// Switch the generator to `kind`, creating the Mask Editor settings when it needs them.
    pub fn set_kind(&mut self, kind: GeneratorKind) {
        self.kind = kind;
        if kind == GeneratorKind::MaskEditor && self.editor.is_none() { self.editor = Some(MaskEditor::default()); }
        if kind.uses_shape() && self.shape.is_none() { self.shape = Some(ShapeSettings::default()); }
    }

    /// Image resources the generator reads (the Mask Editor's textures).
    pub fn resources(&self) -> Vec<subcraft_core::ResourceId> {
        let mut out: Vec<subcraft_core::ResourceId> = match self.kind {
            GeneratorKind::MaskEditor => self.editor_settings().resources().collect(),
            GeneratorKind::GrayscaleConversion => self.shape_settings().texture.into_iter().collect(),
            _ => Vec::new(),
        };
        out.extend(self.painter.and_then(|p| p.custom_grunge));
        out
    }

    /// Whether this kind reads a baked map that an anchor input can replace.
    pub fn accepts_input(&self) -> bool {
        !matches!(self.kind, GeneratorKind::Directional | GeneratorKind::PositionGradient | GeneratorKind::MaskEditor) && !self.kind.uses_shape()
    }

    /// Whether the micro effect is on and strong enough to change anything.
    /// The Mask Editor has no Micro controls: a micro effect kept from the kind it was switched
    /// from used to keep changing the layers below, with no way to turn it off.
    pub fn micro_active(&self) -> bool { self.kind != GeneratorKind::MaskEditor && self.micro.is_some_and(|m| m.height() != 0.0) }

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
    /// Anchor point (ADR-0045): captures the mask evaluated up to here. Changes nothing itself.
    Anchor,
    /// Grayscale read from an anchor point below (a channel of a content anchor, or a mask
    /// anchor), premultiplied by its coverage, through optional Levels (Painter's reference
    /// sources carry their own). Unresolved references contribute nothing.
    AnchorRef { input: crate::anchor::AnchorInput, levels: Option<Levels> },
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
            MaskEffectKind::Anchor => "Anchor Point".into(),
            MaskEffectKind::AnchorRef { .. } => "Anchor Reference".into(),
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

    /// QA 2026-11: switching a generator with a micro effect to Mask Editor (which has no Micro
    /// controls) kept the effect active on the layers below with no way to turn it off.
    #[test]
    fn a_mask_editor_never_applies_a_micro_effect_kept_from_another_kind() {
        let mut g = Generator::new(GeneratorKind::Cavity);
        g.set_micro_enabled(true);
        assert!(g.micro_active());
        g.set_kind(GeneratorKind::MaskEditor);
        assert!(!g.micro_active());
        g.set_kind(GeneratorKind::Cavity);
        assert!(g.micro_active(), "the tuned micro effect is back with the kind that has it");
    }

    #[test]
    fn position_and_uv_generators_have_their_math_and_settings() {
        let s = ShapeSettings::default();
        assert!(s.valid());
        // 3D Distance: white inside the offset, linear over the radius.
        let d = ShapeSettings { offset: 0.1, radius: 0.4, ..s };
        assert_eq!(d.distance_value(0.05), 1.0);
        assert!((d.distance_value(0.3) - 0.5).abs() < 1e-6);
        assert_eq!(d.distance_value(0.6), 0.0);
        // 3D Linear Gradient: the projection on start -> end, clamped.
        assert!((s.gradient_value([0.2, 0.25, 0.9], [0.5, 0.0, 0.5], [0.5, 1.0, 0.5]) - 0.25).abs() < 1e-6);
        assert_eq!(s.gradient_value([0.5, 1.5, 0.5], [0.5, 0.0, 0.5], [0.5, 1.0, 0.5]), 1.0);
        assert_eq!(s.gradient_value([0.5; 3], [0.5; 3], [0.5; 3]), 0.0, "a zero-length gradient");
        // UV Random Color: stable per island and seed, spread over 0..1.
        let v: Vec<f32> = (0..64).map(|i| s.island_value(i)).collect();
        assert_eq!(v[3], s.island_value(3));
        assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
        assert!(v.iter().cloned().fold(0.0, f32::max) > 0.8 && v.iter().cloned().fold(1.0, f32::min) < 0.2);
        assert_ne!(ShapeSettings { seed: 2, ..s }.island_value(3), v[3]);
        // Safe rotation snaps to 45 degree steps.
        assert_eq!(ShapeSettings { rotation: 50.0, safe_rotation: true, ..s }.effective_rotation(), 45.0);
        assert_eq!(ShapeSettings { rotation: 50.0, ..s }.effective_rotation(), 50.0);
        assert!(!ShapeSettings { tile: 0.0, ..s }.valid());
        // Light angles round-trip with the direction vector.
        let dir = light_direction(30.0, 45.0);
        let (h, vert) = light_angles(dir);
        assert!((h - 30.0).abs() < 1e-3 && (vert - 45.0).abs() < 1e-3, "{h} {vert}");
        assert!((light_direction(0.0, 90.0)[1] - 1.0).abs() < 1e-6);
        // New kinds carry their settings; switching kind creates them; Grayscale Conversion
        // reports its image as a resource.
        let g = Generator::new(GeneratorKind::Distance3D);
        assert!(g.shape.is_some() && !g.accepts_input() && g.kind.label() == "3D Distance");
        let mut g = Generator::new(GeneratorKind::Edges);
        assert!(g.shape.is_none());
        g.set_kind(GeneratorKind::GrayscaleConversion);
        let id = subcraft_core::ResourceId::new();
        g.shape = Some(ShapeSettings { texture: Some(id), ..g.shape_settings() });
        assert_eq!(g.resources(), vec![id]);
        // Older generators (no shape field) read and write as before.
        let old = Generator::new(GeneratorKind::Directional);
        assert!(!serde_json::to_string(&old).unwrap().contains("shape"));
        assert_eq!(serde_json::to_string(&GeneratorKind::Distance3D).unwrap(), "\"distance_3d\"");
        assert_eq!(serde_json::to_string(&GeneratorKind::LinearGradient3D).unwrap(), "\"linear_gradient_3d\"");
    }

    #[test]
    fn micro_effect_is_off_by_default_and_ticking_picks_the_kind_default() {
        for kind in [GeneratorKind::Edges, GeneratorKind::Cavity, GeneratorKind::Directional, GeneratorKind::Concave, GeneratorKind::PositionGradient, GeneratorKind::MeshMap(MeshMapKind::Curvature)] {
            let mut g = Generator::new(kind);
            assert!(g.micro.is_none() && !g.micro_active(), "{kind:?} defaults off");
            g.set_micro_enabled(true);
            let m = g.micro.unwrap();
            assert!(g.micro_active() && m.grain_follows);
            assert_eq!(m.direction, if kind == GeneratorKind::Edges { MicroDirection::Carve } else { MicroDirection::Raise });
            assert_eq!(m.height().signum(), m.direction.sign());
            // Unticking keeps the tuned values and turns the effect off.
            g.micro.as_mut().unwrap().amount = 0.9;
            g.set_micro_enabled(false);
            assert!(!g.micro_active() && g.micro.unwrap().amount == 0.9);
            g.set_micro_enabled(true);
            assert_eq!(g.micro.unwrap().amount, 0.9);
        }
        let mut g = Generator::new(GeneratorKind::Edges);
        g.set_micro_enabled(false);
        assert!(g.micro.is_none(), "unticking an untouched generator leaves it untouched");
    }

    #[test]
    fn painter_defaults_and_older_files() {
        let dirt = Generator::new(GeneratorKind::Cavity);
        // Dirt Level 0.75 (threshold 0.25), Dirt Contrast 0.39, Grunge Amount 0.52, Pattern size 16%,
        // Edges Masking 0, Grunge Floor 0.73, grunge map, Grain 0, Seed 1.
        assert_eq!((dirt.level, dirt.softness, dirt.noise_amount), (0.25, 0.61, 0.52));
        assert!((dirt.pattern_size() - 16.0).abs() < 1e-4 && dirt.noise.seed == 1 && dirt.offset == [0.0; 3]);
        assert_eq!(dirt.painter.unwrap().grunge_floor, 0.73);
        let pc = dirt.painter.unwrap();
        assert!(pc.grunge_map && pc.edges_masking == 0.0 && pc.grain == 0.0);
        let wear = Generator::new(GeneratorKind::Edges);
        assert_eq!(wear.level, 0.3);
        assert!(wear.painter.unwrap().grunge_map && wear.edge_spread.is_some());
        let ao = Generator::new(GeneratorKind::AmbientOcclusion);
        // Balance 0.53, Contrast 0, Breakup 0, Pattern size 12.5%, Seed 1, offsets 0.
        assert_eq!((ao.level, ao.softness, ao.noise_amount), (0.47, 1.0, 0.0));
        assert!((ao.pattern_size() - 12.5).abs() < 1e-4 && ao.noise.seed == 1 && ao.offset == [0.0; 3]);
        assert!(ao.painter.is_none() && ao.kind.uses_painter_names());
        assert_eq!(ao.kind.label(), "Ambient Occlusion");
        // Older files: no grunge_map field means the organic noise; false is not written.
        let old: PainterControls = serde_json::from_str(r#"{"edges_masking":0.5,"ao_masking":0,"curvature_weight":1,"smoothness":0,"grunge_scale":1}"#).unwrap();
        assert!(!old.grunge_map);
        assert!(!serde_json::to_string(&old).unwrap().contains("grunge_map"));
        let kind: GeneratorKind = serde_json::from_str("\"ambient_occlusion\"").unwrap();
        assert_eq!(kind, GeneratorKind::AmbientOcclusion);
    }


    #[test]
    fn mask_editor_remaps_match_hand_computed_values() {
        // Balance / Contrast: identity at 0.5 / 0, a ramp 1 - contrast wide centred on 1 - balance.
        for v in [0.0, 0.2, 0.5, 0.9, 1.0] { assert!((balance_contrast(v, 0.5, 0.0) - v).abs() < 1e-6); }
        assert!((balance_contrast(0.5, 0.5, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!((balance_contrast(0.25, 0.5, 0.5), balance_contrast(0.75, 0.5, 0.5)), (0.0, 1.0));
        assert!((balance_contrast(0.375, 0.5, 0.5) - 0.25).abs() < 1e-6);
        // More balance = more white: balance 0.75 centres the ramp on 0.25.
        assert!((balance_contrast(0.25, 0.75, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!((balance_contrast(0.399, 0.6, 1.0), balance_contrast(0.4, 0.6, 1.0)), (0.0, 1.0));
        // Contrast / Brightness: neutral at 0 / 0.
        for v in [0.0, 0.3, 1.0] { assert_eq!(contrast_brightness(v, 0.0, 0.0), v); }
        assert!((contrast_brightness(0.4, 0.5, 0.0) - 0.3).abs() < 1e-6);
        assert!((contrast_brightness(0.1, -0.5, 0.0) - 0.3).abs() < 1e-6);
        assert!((contrast_brightness(0.4, 0.5, 0.1) - 0.4).abs() < 1e-6);
        assert_eq!((contrast_brightness(0.49, 1.0, 0.0), contrast_brightness(0.51, 1.0, 0.0), contrast_brightness(0.2, -1.0, 0.0)), (0.0, 1.0, 0.5));
        assert_eq!(contrast_brightness(0.9, 0.0, 0.5), 1.0, "clamped");
        // Curvature modes around the flat 0.5.
        let modes = |c| CurvatureMode::ALL.map(|m| m.extract(c));
        assert_eq!(modes(0.5), [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(modes(0.75), [0.5, 0.0, 0.5, 0.75]);
        assert_eq!(modes(0.25), [0.0, 0.5, 0.5, 0.25]);
        assert_eq!(modes(1.0), [1.0, 0.0, 1.0, 1.0]);
        // Grayscale conversions of (0.8, 0.4, 0.2).
        let c = [0.8, 0.4, 0.2];
        let got = GrayscaleType::ALL.map(|g| g.convert(c));
        let want = [0.5, 0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.2, 1.4 / 3.0, 0.8, 0.2, 0.8, 0.4, 0.2];
        for (g, w) in got.iter().zip(want) { assert!((g - w).abs() < 1e-6, "{got:?}"); }
    }

    #[test]
    fn mask_editor_groups_shape_their_maps() {
        let ed = MaskEditor::default();
        assert!(ed.valid() && ed.blur_radius() == 0.0);
        // Defaults: only the curvature edges contribute; the global remap is the identity.
        assert_eq!((ed.curvature.opacity, ed.occlusion.opacity, ed.thickness.opacity, ed.texture.opacity, ed.world_normal.opacity, ed.position.opacity), (1.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        for v in [0.0, 0.3, 1.0] { assert!((ed.finish(v) - v).abs() < 1e-6); }
        assert!((MaskEditor { global_invert: true, ..ed }.finish(0.3) - 0.7).abs() < 1e-6);
        assert_eq!(MaskEditor { global_contrast: 1.0, global_balance: 0.7, ..ed }.finish(0.31), 1.0);
        assert_eq!(ed.curvature.shape(0.75), 0.5);
        assert_eq!(EditorCurvature { invert: true, mode: CurvatureMode::Cavities, ..ed.curvature }.shape(0.25), 0.5);
        assert!((EditorOcclusion { invert: true, ..ed.occlusion }.shape(0.2) - 0.8).abs() < 1e-6);
        assert!((EditorThickness { contrast: 0.5, brightness: 0.1, ..ed.thickness }.shape(0.4) - 0.4).abs() < 1e-6);
        // Textures: conversion, contrast, brightness, invert; tiling keeps the aspect on request.
        let t = EditorTexture { grayscale: GrayscaleType::Red, contrast: 0.5, invert: true, ..EditorTexture::default() };
        assert!((t.shape([0.4, 0.9, 0.9]) - 0.7).abs() < 1e-6);
        assert_eq!(EditorTexture { scale: 3.0, ..t }.tiling(512, 256), [3.0, 3.0]);
        assert_eq!(EditorTexture { scale: 3.0, non_square: true, ..t }.tiling(512, 256), [3.0, 6.0]);
        assert_eq!((EditorTexture { triplanar_contrast: 0.0, ..t }.triplanar_sharpness(), EditorTexture { triplanar_contrast: 0.5, ..t }.triplanar_sharpness()), (1.0, 2.0));
        assert!(EditorTexture { triplanar_contrast: 1.0, ..t }.triplanar_sharpness().is_finite());
        // Directions: the three gradients blend in order, each by its amount, with the plain
        // "normal" blend standing in for the compositor's.
        let normal = |_: BlendMode, _: f32, v: f32| v;
        let d = EditorDirections::default();
        assert_eq!(d.shape([0.1, 0.8, 0.3], normal), 0.8, "Top To Bottom only by default");
        let mix = EditorDirections { axes: [EditorDirection { amount: 1.0, ..Default::default() }, EditorDirection { amount: 0.5, invert: true, ..Default::default() }, EditorDirection::default()], ..d };
        // X fully: 0.1; then half of inverted Y (0.2): 0.15.
        assert!((mix.shape([0.1, 0.8, 0.3], normal) - 0.15).abs() < 1e-6);
        let multiply = |m: BlendMode, below: f32, v: f32| if m == BlendMode::Multiply { below * v } else { v };
        let both = EditorDirections { axes: [EditorDirection { amount: 1.0, ..Default::default() }, EditorDirection { amount: 1.0, blend: BlendMode::Multiply, ..Default::default() }, EditorDirection::default()], brightness: 0.1, invert: true, ..d };
        // 0.5 * 0.8 = 0.4, + 0.1 brightness, inverted.
        assert!((both.shape([0.5, 0.8, 0.0], multiply) - 0.5).abs() < 1e-6);
        // Out-of-range values are rejected; resources lists the texture images.
        assert!(!MaskEditor { global_blur: 40.0, ..ed }.valid() && !MaskEditor { thickness: EditorThickness { contrast: 2.0, ..ed.thickness }, ..ed }.valid());
        assert!(!MaskEditor { texture2: EditorTexture { scale: 0.0, ..ed.texture2 }, ..ed }.valid());
        assert_eq!(MaskEditor { global_blur: f32::NAN, ..ed }.blur_radius(), 0.0);
        let id = subcraft_core::ResourceId::new();
        let with = MaskEditor { texture2: EditorTexture { resource: Some(id), ..ed.texture2 }, ..ed };
        assert_eq!(with.resources().collect::<Vec<_>>(), vec![id]);
        assert_eq!(ed.resources().count(), 0);
    }

    #[test]
    fn mask_editor_generator_defaults_and_serialization() {
        let g = Generator::new(GeneratorKind::MaskEditor);
        assert_eq!(g.kind.label(), "Mask Editor");
        assert_eq!(g.editor, Some(MaskEditor::default()));
        assert!(!g.accepts_input() && g.noise_amount == 0.0 && g.editor_blur() == 0.0);
        assert_eq!(serde_json::from_str::<GeneratorKind>("\"mask_editor\"").unwrap(), GeneratorKind::MaskEditor);
        // Other kinds carry no editor and write none, so older projects serialize unchanged.
        let dirt = Generator::new(GeneratorKind::Cavity);
        assert!(dirt.editor.is_none() && !serde_json::to_string(&dirt).unwrap().contains("editor"));
        // Round trip, and a file written before a setting existed takes the default for it.
        let mut tuned = g;
        let ed = tuned.editor.as_mut().unwrap();
        ed.global_blur = 6.0;
        ed.occlusion = EditorOcclusion { opacity: 0.7, invert: true, blend: BlendMode::Multiply, balance: 0.6, contrast: 0.2 };
        ed.position.axes[2].amount = 0.4;
        ed.texture.resource = Some(subcraft_core::ResourceId::new());
        let json = serde_json::to_string(&tuned).unwrap();
        assert_eq!(serde_json::from_str::<Generator>(&json).unwrap(), tuned);
        assert_eq!(tuned.editor_blur(), 6.0);
        let sparse: MaskEditor = serde_json::from_str(r#"{"global_balance":0.7,"curvature":{"opacity":0.25}}"#).unwrap();
        assert_eq!(sparse, MaskEditor { global_balance: 0.7, curvature: EditorCurvature { opacity: 0.25, ..Default::default() }, ..Default::default() });
        // A Mask Editor that lost its settings (a hand-edited file) behaves as the default one.
        assert_eq!(Generator { editor: None, ..g }.editor_settings(), MaskEditor::default());
    }

    #[test]
    fn micro_height_is_bounded_and_ignores_bad_amounts() {
        let m = MicroEffect { enabled: true, amount: 5.0, direction: MicroDirection::Carve, grain_follows: true };
        assert_eq!(m.height(), -MicroEffect::MAX_HEIGHT);
        assert_eq!(MicroEffect { amount: f32::NAN, ..m }.height(), 0.0);
        assert_eq!(MicroEffect { enabled: false, ..m }.height(), 0.0);
    }

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
