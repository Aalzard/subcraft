//! Brush, material and stroke option types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, ChannelValue, ResourceId};
use subcraft_doc::{Projection, TextureSet};

/// How the brush footprint is laid onto the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    /// Tangent frame at the hit; footprint measured by 3D distance (wraps around curvature).
    #[default]
    TangentWrap,
    /// Tangent plane at the hit; footprint projected onto the plane (depth-limited by the radius).
    TangentPlanar,
    /// Screen-space disc projected from the camera.
    Camera,
    /// Disc in UV space around the hit UV (no 3D visibility test, like painting in the 2D view).
    Uv,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushSettings {
    /// Brush diameter in percent of the mesh bounding-box diagonal (object space).
    pub size: f32,
    /// Coverage each stamp adds (0..1).
    pub flow: f32,
    /// Maximum opacity a single stroke can reach, however many stamps overlap (0..1).
    pub stroke_opacity: f32,
    /// Stamp distance as a fraction of the brush diameter.
    pub spacing: f32,
    /// Fraction of the radius at full strength before the falloff starts (0..1).
    pub hardness: f32,
    pub angle_deg: f32,
    /// Footprint aspect (1 = round; < 1 squashes along the brush's local y axis).
    pub roundness: f32,
    /// Add the screen-space stroke direction to the angle.
    pub follow_path: bool,
    /// Random size reduction per stamp (0..1, fraction of the size).
    pub size_jitter: f32,
    /// Random flow reduction per stamp (0..1).
    pub flow_jitter: f32,
    /// Random angle offset per stamp, ± degrees.
    pub angle_jitter_deg: f32,
    /// Random screen offset per stamp, in brush diameters.
    pub position_jitter: f32,
    /// How much pen pressure shrinks the brush (0 = ignored, 1 = zero pressure gives a hairline).
    pub pressure_size: f32,
    /// How much pen pressure lowers the flow of each stamp (0 = ignored).
    pub pressure_flow: f32,
    /// Scatter mode: particles per path stamp (0 = off, a normal brush).
    pub scatter_count: u32,
    /// How far particles spread around the path, in brush diameters.
    pub scatter_radius: f32,
    /// Random size reduction per particle (0..1).
    pub scatter_size_random: f32,
    /// Random rotation per particle, ± degrees.
    pub scatter_rotation_deg: f32,
    /// Random opacity (flow) reduction per particle (0..1).
    pub scatter_opacity_random: f32,
    /// Particles fall down the screen by up to this many brush diameters (0 = no fall).
    pub scatter_fall: f32,
    /// Smudge: how much of the carried paint each stamp deposits (0..1).
    pub smudge_strength: f32,
    /// Smudge: how much of the carried paint survives each stamp (0 = blend only, near 1 = long smear).
    pub smudge_carry: f32,
    /// Smudge: footprint weight exponent (1 = brush falloff, higher = a tighter core).
    pub smudge_falloff: f32,
    /// Smudge: how much of the brush material mixes into the carried paint (0 = pure smudge).
    pub smudge_material: f32,
    pub alignment: Alignment,
    pub backface_culling: bool,
    /// Surfaces turned further than this from the projection direction are not painted.
    pub backface_angle_deg: f32,
    pub seed: u32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: 5.0,
            flow: 1.0,
            stroke_opacity: 1.0,
            spacing: 0.1,
            hardness: 0.5,
            angle_deg: 0.0,
            roundness: 1.0,
            follow_path: false,
            size_jitter: 0.0,
            flow_jitter: 0.0,
            angle_jitter_deg: 0.0,
            position_jitter: 0.0,
            pressure_size: 1.0,
            pressure_flow: 0.0,
            scatter_count: 0,
            scatter_radius: 1.0,
            scatter_size_random: 0.5,
            scatter_rotation_deg: 180.0,
            scatter_opacity_random: 0.3,
            scatter_fall: 0.0,
            smudge_strength: 1.0,
            smudge_carry: 0.85,
            smudge_falloff: 1.0,
            smudge_material: 0.0,
            alignment: Alignment::TangentWrap,
            backface_culling: true,
            backface_angle_deg: 90.0,
            seed: 1,
        }
    }
}

impl BrushSettings {
    /// World (object-space) radius for a mesh with bounding-box diagonal `diagonal`.
    pub fn radius(&self, diagonal: f32) -> f32 {
        self.size.max(0.0) * 0.01 * diagonal * 0.5
    }
}

/// Most particles one path stamp may scatter.
pub const MAX_SCATTER: u32 = 64;

/// Smallest pressure factor, so a feather-light touch still leaves a mark.
pub const MIN_PRESSURE: f32 = 0.02;

impl BrushSettings {
    /// Radius multiplier for a pen pressure in 0..1 (1 for a mouse).
    pub fn pressure_size_factor(&self, pressure: f32) -> f32 {
        1.0 - self.pressure_size.clamp(0.0, 1.0) * (1.0 - pressure.clamp(MIN_PRESSURE, 1.0))
    }

    /// Flow multiplier for a pen pressure in 0..1 (1 for a mouse).
    pub fn pressure_flow_factor(&self, pressure: f32) -> f32 {
        1.0 - self.pressure_flow.clamp(0.0, 1.0) * (1.0 - pressure.clamp(MIN_PRESSURE, 1.0))
    }
}

/// Brush strength at normalized distance `d` (0 = center, 1 = rim).
#[inline]
pub fn falloff(d: f32, hardness: f32) -> f32 {
    if d >= 1.0 {
        return 0.0;
    }
    let h = hardness.clamp(0.0, 1.0);
    if d <= h || h >= 1.0 {
        return 1.0;
    }
    let t = (d - h) / (1.0 - h);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushChannel {
    pub enabled: bool,
    /// Linear value for color channels.
    pub value: ChannelValue,
}

/// How a normal stroke interacts with normals already painted on this layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalPaintBlend {
    #[default]
    Replace,
    Detail,
}

/// What the stamp image of an [`ImageStamp`] brush encodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StampKind {
    /// Tangent-space normal map: painted into the layer's Normal channel (whiteout blend).
    #[default]
    Normal,
    /// Grayscale height: painted into the layer's Height channel.
    Height,
}

impl StampKind {
    /// The layer channel this kind of stamp writes.
    pub fn channel(self) -> ChannelKind {
        match self {
            StampKind::Normal => ChannelKind::Normal,
            StampKind::Height => ChannelKind::Height,
        }
    }
}

/// A "normal map stamp" (or height stamp) brush: the brush footprint is an image that is laid
/// onto the surface (rotated, sized and spaced like any tip) and writes its tangent-space
/// normals / heights instead of a flat value. Size, spacing, rotation, hardness (edge fade) and
/// jitter come from [`BrushSettings`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageStamp {
    pub kind: StampKind,
    /// The stamp image (a project resource; identity is never a path).
    pub resource: Option<ResourceId>,
    pub name: String,
    /// Scales the XY deviation of the normals (heights: gain). 1 = the map as authored.
    pub strength: f32,
    /// The map is DirectX (+Y down): flip its green channel.
    pub flip_green: bool,
    /// Repeat the image over the surface under the stroke (anchored at the stroke start) instead
    /// of one image per stamp.
    pub tiled: bool,
    /// Brush channels that were enabled before the stamp took over (restored by "Back to normal brush").
    pub restore_channels: Vec<ChannelKind>,
    /// Brush settings before the stamp changed spacing and edge fade.
    pub restore_settings: Option<BrushSettings>,
}

impl Default for ImageStamp {
    fn default() -> Self {
        Self { kind: StampKind::Normal, resource: None, name: String::new(), strength: 1.0, flip_green: false, tiled: false,
            restore_channels: Vec::new(), restore_settings: None }
    }
}

/// What a paint stroke deposits per channel; masks receive `mask_value`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushMaterial {
    pub channels: BTreeMap<ChannelKind, BrushChannel>,
    /// Optional image sources; channel enable switches still apply. Values remain available
    /// when an image is cleared. Resource identity never depends on a file path.
    pub textures: BTreeMap<ChannelKind, ResourceId>,
    pub projection: Projection,
    /// Repeat images; when false, coordinates outside the image are transparent.
    pub texture_tiling: bool,
    pub normal_blend: NormalPaintBlend,
    /// Project the channel images from the view the stroke started in (like a slide projector)
    /// instead of through `projection`'s UV / planar / tri-planar mapping. `projection`'s
    /// scale, rotation and offset still place the image on the screen.
    pub camera_projection: bool,
    /// Grayscale painted into mask rasters.
    pub mask_value: f32,
    /// Normal-map (or height) stamp brush; `None` = an ordinary brush.
    pub stamp: Option<ImageStamp>,
}

impl Default for BrushMaterial {
    fn default() -> Self {
        Self { channels: BTreeMap::new(), textures: BTreeMap::new(), projection: Projection::default(), texture_tiling: true,
            normal_blend: NormalPaintBlend::Replace, camera_projection: false, mask_value: 1.0, stamp: None }
    }
}

impl BrushMaterial {
    /// Every texture-set channel enabled at its default paint value; white for masks.
    pub fn new(ts: &TextureSet) -> Self {
        let channels = ts.channel_kinds().map(|k| (k, BrushChannel { enabled: true, value: k.default_paint_value() })).collect();
        Self { channels, ..Self::default() }
    }

    /// Only the given channels enabled, at the given values.
    pub fn with(values: &[(ChannelKind, ChannelValue)]) -> Self {
        let channels = values.iter().map(|&(k, value)| (k, BrushChannel { enabled: true, value })).collect();
        Self { channels, ..Self::default() }
    }

    pub fn set(&mut self, kind: ChannelKind, value: ChannelValue) {
        self.channels.insert(kind, BrushChannel { enabled: true, value });
    }

    pub fn disable(&mut self, kind: ChannelKind) {
        if let Some(c) = self.channels.get_mut(&kind) {
            c.enabled = false;
        }
    }

    /// Whether strokes write `kind`.
    pub fn is_enabled(&self, kind: ChannelKind) -> bool {
        self.channels.get(&kind).is_some_and(|c| c.enabled)
    }

    /// Switch painting of `kind` on or off, keeping its value.
    pub fn toggle(&mut self, kind: ChannelKind) {
        let on = !self.is_enabled(kind);
        self.set_enabled(kind, on);
    }

    /// Paint only `kind` among `channels`. Soloing the channel that is already the only one
    /// enabled turns every channel back on.
    pub fn solo(&mut self, kind: ChannelKind, channels: &[ChannelKind]) {
        let only = channels.iter().all(|&k| self.is_enabled(k) == (k == kind));
        for &k in channels {
            self.set_enabled(k, only || k == kind);
        }
    }

    fn set_enabled(&mut self, kind: ChannelKind, enabled: bool) {
        self.channels.entry(kind).or_insert(BrushChannel { enabled, value: kind.default_paint_value() }).enabled = enabled;
    }
}

/// Stroke-level options that shape the stamp path rather than the footprint.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StrokeOptions {
    /// Lazy mouse: the brush trails the cursor by this many pixels (0 = off).
    pub lazy_distance_px: f32,
    /// Weighted-average smoothing of the cursor path (0 = off, up to 0.95); runs before the rope.
    pub smoothing: f32,
    /// When the stroke ends, glide the brush the rest of the way to the last cursor position.
    pub catch_up: bool,
    /// Mirror planes (X, Y, Z) through the mesh bounds center.
    pub symmetry: [bool; 3],
    /// Copies rotated around an axis (combines with the mirror planes; ADR 0043).
    pub radial: RadialSymmetry,
}

/// Most stroke copies (view + mirrors + radial) one stroke paints; each needs a view-sized
/// depth map, so the radial count shrinks when mirror planes multiply it (ADR 0043).
pub const MAX_SYMMETRY_COPIES: u32 = 64;

/// Radial symmetry: the stroke repeated `count` times around an axis line, copy `k` rotated by
/// `k * 360 / count` degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RadialSymmetry {
    pub enabled: bool,
    /// Rotation axis: 0 = X, 1 = Y, 2 = Z.
    pub axis: u8,
    /// Total copies around the axis, including the original (2..=64 when enabled).
    pub count: u32,
    /// Offset of the axis line from the mesh bounds center (object units).
    pub center_offset: [f32; 3],
}

impl Default for RadialSymmetry {
    fn default() -> Self {
        Self { enabled: false, axis: 1, count: 4, center_offset: [0.0; 3] }
    }
}

impl RadialSymmetry {
    /// Copies actually painted around the axis (1 = off) next to `mirror_planes` enabled
    /// mirror planes: clamped to 1..=64 and so that `count * 2^mirror_planes` stays within
    /// [`MAX_SYMMETRY_COPIES`].
    pub fn effective_count(&self, mirror_planes: usize) -> u32 {
        if !self.enabled { return 1; }
        let limit = (MAX_SYMMETRY_COPIES >> mirror_planes.min(6)).max(1);
        self.count.clamp(1, MAX_SYMMETRY_COPIES).min(limit)
    }

    /// Unit vector of the rotation axis.
    pub fn axis_dir(&self) -> glam::Vec3 {
        let mut d = glam::Vec3::ZERO;
        d[(self.axis as usize).min(2)] = 1.0;
        d
    }
}

impl StrokeOptions {
    /// Number of copies (including the original) every stamp of the stroke produces.
    pub fn copies(&self) -> u32 {
        let mirrors = self.symmetry.iter().filter(|&&m| m).count();
        self.radial.effective_count(mirrors) << mirrors
    }

    /// Whether any symmetry (mirror or radial) is active.
    pub fn has_symmetry(&self) -> bool {
        self.copies() > 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrokeMode {
    #[default]
    Paint,
    /// Reduces paint alpha; on a mask raster, paints black (hides), like erasing a mask in other painters.
    Erase,
    /// Drags the paint under the brush along the stroke (enabled channels only).
    Smudge,
    /// Paints what lies at the clone source, offset from where the stroke starts.
    Clone,
}

/// What a paint stroke writes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintTarget {
    /// The channel images of a paint layer (enabled brush channels only).
    Content,
    /// The base raster of the layer's mask.
    MaskBase,
    /// The rasters of a Paint effect in the layer's content stack (works on fill layers too).
    ContentEffect(subcraft_core::EffectId),
    /// The raster of a Paint effect in the layer's mask stack.
    MaskEffect(subcraft_core::EffectId),
}

impl PaintTarget {
    /// Whether strokes change the mask (grayscale `mask_value`) rather than material channels.
    pub fn is_mask(self) -> bool {
        matches!(self, PaintTarget::MaskBase | PaintTarget::MaskEffect(_))
    }
}

/// Polygon-fill selection grown from the triangles a click or a box picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolygonFillMode {
    Triangle,
    MeshElement,
    UvIsland,
    /// The source polygon (quad / n-gon) the importer triangulated.
    Polygon,
    /// The whole mesh object (the texture set's triangles when the mesh has no objects).
    Object,
}

impl PolygonFillMode {
    /// In the order the tool options list them.
    pub const ALL: [Self; 5] = [Self::Triangle, Self::Polygon, Self::MeshElement, Self::UvIsland, Self::Object];

    pub fn label(self) -> &'static str {
        match self {
            Self::Triangle => "Triangle",
            Self::Polygon => "Polygon",
            Self::MeshElement => "Mesh",
            Self::UvIsland => "UV island",
            Self::Object => "Object",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_switches_toggle_and_solo() {
        use ChannelKind::*;
        let channels = [BaseColor, Roughness, Metallic];
        let mut m = BrushMaterial::with(&[(BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0)), (Roughness, ChannelValue::scalar(0.2))]);
        m.toggle(Metallic);
        assert!(m.is_enabled(Metallic), "a missing channel switches on at its default value");
        assert_eq!(m.channels[&Metallic].value, Metallic.default_paint_value());
        m.toggle(BaseColor);
        assert!(!m.is_enabled(BaseColor));
        m.solo(Roughness, &channels);
        assert_eq!(channels.map(|k| m.is_enabled(k)), [false, true, false]);
        assert_eq!(m.channels[&Roughness].value, ChannelValue::scalar(0.2), "values survive switching");
        m.solo(Roughness, &channels);
        assert_eq!(channels.map(|k| m.is_enabled(k)), [true, true, true], "solo again = all");
        assert_eq!(m.channels[&BaseColor].value, ChannelValue::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn radial_copies_are_clamped_by_mirror_planes() {
        let r = RadialSymmetry { enabled: true, count: 64, ..RadialSymmetry::default() };
        assert_eq!(r.effective_count(0), 64);
        assert_eq!(r.effective_count(1), 32);
        assert_eq!(r.effective_count(3), 8);
        assert_eq!(RadialSymmetry { count: 0, ..r }.effective_count(0), 1);
        assert_eq!(RadialSymmetry { count: 500, ..r }.effective_count(0), 64);
        assert_eq!(RadialSymmetry { enabled: false, ..r }.effective_count(0), 1);
        let all = StrokeOptions { symmetry: [true; 3], radial: r, ..StrokeOptions::default() };
        assert_eq!(all.copies(), MAX_SYMMETRY_COPIES);
        assert_eq!(StrokeOptions { radial: RadialSymmetry { count: 6, ..r }, ..StrokeOptions::default() }.copies(), 6);
        assert!(!StrokeOptions::default().has_symmetry());
        assert_eq!(RadialSymmetry { axis: 9, ..r }.axis_dir(), glam::Vec3::Z);
    }
    #[test]
    fn falloff_shape() {
        assert_eq!(falloff(0.0, 0.5), 1.0);
        assert_eq!(falloff(0.5, 0.5), 1.0);
        assert!((falloff(0.75, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(falloff(1.0, 0.5), 0.0);
        assert_eq!(falloff(0.99, 1.0), 1.0);
        let mut prev = 1.0;
        for i in 0..=100 {
            let v = falloff(i as f32 / 100.0, 0.2);
            assert!(v <= prev + 1e-6);
            prev = v;
        }
    }
}
