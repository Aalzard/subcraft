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

/// What the brush size is measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SizeSpace {
    /// Percent of the mesh bounding-box diagonal: the brush keeps its size on the model.
    #[default]
    Object,
    /// Percent of the viewport height: the brush keeps its size on screen, whatever the zoom.
    Viewport,
    /// Percent of the texture (UV 0..1): the brush covers the same texels everywhere.
    Texture,
}

impl SizeSpace {
    pub const ALL: [SizeSpace; 3] = [SizeSpace::Object, SizeSpace::Viewport, SizeSpace::Texture];

    pub fn label(self) -> &'static str {
        match self {
            SizeSpace::Object => "Object",
            SizeSpace::Viewport => "Viewport",
            SizeSpace::Texture => "Texture",
        }
    }
}

/// What a brush size is converted with (see [`BrushSettings::radius_in`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeContext {
    /// Mesh bounding-box diagonal (object units).
    pub diagonal: f32,
    /// Screen pixels per object unit under the brush (0 = unknown).
    pub pixels_per_unit: f32,
    /// Viewport height in pixels (0 = unknown).
    pub viewport_height: f32,
    /// Average UV units per object unit of the painted texture set (0 = unknown).
    pub uv_per_unit: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushSettings {
    /// Brush diameter in percent of what `size_space` names (the mesh bounding-box diagonal by default).
    pub size: f32,
    pub size_space: SizeSpace,
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
    /// Pen pressure scales the size jitter (a light touch jitters less); a mouse is unchanged.
    pub size_jitter_pressure: bool,
    pub flow_jitter_pressure: bool,
    pub angle_jitter_pressure: bool,
    pub position_jitter_pressure: bool,
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
            size_space: SizeSpace::Object,
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
            size_jitter_pressure: false,
            flow_jitter_pressure: false,
            angle_jitter_pressure: false,
            position_jitter_pressure: false,
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
    /// World (object-space) radius for a mesh with bounding-box diagonal `diagonal`, in
    /// [`SizeSpace::Object`] (the other spaces need [`BrushSettings::radius_in`]).
    pub fn radius(&self, diagonal: f32) -> f32 {
        self.size.max(0.0) * 0.01 * diagonal * 0.5
    }

    /// World (object-space) radius in the brush's size space. A space whose scale is unknown
    /// (0 or not finite) falls back to the object size, so the brush never vanishes.
    pub fn radius_in(&self, c: &SizeContext) -> f32 {
        let half = self.size.max(0.0) * 0.01 * 0.5;
        let known = |v: f32| v.is_finite() && v > 0.0;
        match self.size_space {
            SizeSpace::Viewport if known(c.pixels_per_unit) && known(c.viewport_height) => half * c.viewport_height / c.pixels_per_unit,
            SizeSpace::Texture if known(c.uv_per_unit) => half / c.uv_per_unit,
            _ => half * c.diagonal,
        }
    }

    /// Size, flow, angle (degrees) and position jitter for a pen pressure in 0..1 (1 for a
    /// mouse): a jitter whose pressure toggle is on is scaled by the pressure.
    pub fn jitters(&self, pressure: f32) -> [f32; 4] {
        let p = pressure.clamp(0.0, 1.0);
        let k = |on: bool| if on { p } else { 1.0 };
        [
            self.size_jitter.clamp(0.0, 1.0) * k(self.size_jitter_pressure),
            self.flow_jitter.clamp(0.0, 1.0) * k(self.flow_jitter_pressure),
            self.angle_jitter_deg * k(self.angle_jitter_pressure),
            self.position_jitter.max(0.0) * k(self.position_jitter_pressure),
        ]
    }

    /// Whether pen pressure drives the size / the flow (the toggle on the Size and Flow rows).
    pub fn size_pressure(&self) -> bool { self.pressure_size > 0.0 }
    pub fn flow_pressure(&self) -> bool { self.pressure_flow > 0.0 }

    /// Size at zero pressure as a fraction of the size ("Minimum Size"); 1 while pressure does
    /// not drive the size.
    pub fn min_size(&self) -> f32 { 1.0 - self.pressure_size.clamp(0.0, 1.0) }
    pub fn min_flow(&self) -> f32 { 1.0 - self.pressure_flow.clamp(0.0, 1.0) }

    /// Switch size pressure on (keeping `min` as the minimum size, below 1) or off.
    pub fn set_size_pressure(&mut self, on: bool, min: f32) {
        self.pressure_size = if on { 1.0 - min.clamp(0.0, MAX_PRESSURE_MIN) } else { 0.0 };
    }
    pub fn set_flow_pressure(&mut self, on: bool, min: f32) {
        self.pressure_flow = if on { 1.0 - min.clamp(0.0, MAX_PRESSURE_MIN) } else { 0.0 };
    }
}

/// Largest "Minimum Size / Flow": at 1 the pressure would do nothing, which is the toggle's off state.
pub const MAX_PRESSURE_MIN: f32 = 0.99;

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

/// How pen pressure is remapped before it scales size and flow (Painter's contextual toolbar
/// pressure curve): linear, ease-in (light pressure does less, `p²`) or ease-in-out
/// (smoothstep: soft at both ends).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureCurve {
    #[default]
    Linear,
    EaseIn,
    EaseInOut,
}

impl PressureCurve {
    pub const ALL: [PressureCurve; 3] = [PressureCurve::Linear, PressureCurve::EaseIn, PressureCurve::EaseInOut];

    pub fn label(self) -> &'static str {
        match self {
            PressureCurve::Linear => "Linear",
            PressureCurve::EaseIn => "Ease in",
            PressureCurve::EaseInOut => "Ease in-out",
        }
    }

    /// The remapped pressure (0..1); 0 and 1 stay put and the curve never decreases.
    pub fn apply(self, pressure: f32) -> f32 {
        let p = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 1.0 };
        match self {
            PressureCurve::Linear => p,
            PressureCurve::EaseIn => p * p,
            PressureCurve::EaseInOut => p * p * (3.0 - 2.0 * p),
        }
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
    /// Mirror plane positions: offset of each plane from the mesh bounds center along its own
    /// axis (object units; Painter's "Axis position"). Zero is the center.
    pub mirror_offset: [f32; 3],
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
    /// Flip copy: every other copy is also mirrored across the plane through the axis and the
    /// first copy's direction, so neighbouring copies face each other.
    pub flip: bool,
}

impl Default for RadialSymmetry {
    fn default() -> Self {
        Self { enabled: false, axis: 1, count: 4, center_offset: [0.0; 3], flip: false }
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

    /// Axis normal to the plane a flipped copy is mirrored across: the plane holds the rotation
    /// axis and the first copy's direction (the next axis after the rotation axis).
    pub fn flip_axis(&self) -> usize {
        ((self.axis as usize).min(2) + 2) % 3
    }
}

/// Reflection across the plane `p[axis] = at` (its own inverse).
fn reflection(axis: usize, at: f32) -> glam::Mat4 {
    let mut r = glam::Mat4::IDENTITY;
    r.col_mut(axis)[axis] = -1.0;
    r.w_axis[axis] = 2.0 * at;
    r
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

    /// Coordinate of the mirror plane along `axis` for a mesh whose bounds center is `center`.
    pub fn mirror_plane(&self, axis: usize, center: glam::Vec3) -> f32 {
        let offset = self.mirror_offset[axis];
        center[axis] + if offset.is_finite() { offset } else { 0.0 }
    }

    /// Radial pivot (the axis line passes through it) for a mesh whose bounds center is `center`.
    pub fn radial_pivot(&self, center: glam::Vec3) -> glam::Vec3 {
        center + glam::Vec3::from_array(self.radial.center_offset)
    }

    /// The view camera of every stroke copy, in paint order: the view, its radial copies (odd
    /// ones mirrored first when flipped), then every mirror of those (all combinations of the
    /// enabled planes). Without radial symmetry the list is exactly the mirror list (ADR 0043).
    pub fn copy_cameras(&self, camera: &crate::Camera, center: glam::Vec3) -> Vec<crate::Camera> {
        let mut cams = vec![*camera];
        let mirrors = self.symmetry.iter().filter(|&&m| m).count();
        let radial = &self.radial;
        let n = radial.effective_count(mirrors);
        if n > 1 {
            let pivot = self.radial_pivot(center);
            for k in 1..n {
                let angle = std::f32::consts::TAU * k as f32 / n as f32;
                let base = if radial.flip && k % 2 == 1 {
                    let v = radial.flip_axis();
                    camera.mirrored(v, pivot[v])
                } else {
                    *camera
                };
                cams.push(base.rotated(radial.axis_dir(), pivot, angle));
            }
        }
        for axis in 0..3 {
            if self.symmetry[axis] {
                let plane = self.mirror_plane(axis, center);
                let mirrored: Vec<crate::Camera> = cams.iter().map(|c| c.mirrored(axis, plane)).collect();
                cams.extend(mirrored);
            }
        }
        cams
    }

    /// Where the copies of the object-space point `p` land, `p` first, in the order of
    /// [`Self::copy_cameras`] (planes and pivot at their axis positions, flipped radial copies
    /// mirrored). As many points as [`StrokeOptions::copies`].
    pub fn symmetry_points(&self, center: glam::Vec3, p: glam::Vec3) -> Vec<glam::Vec3> {
        self.copy_transforms(center).iter().map(|t| t.transform_point3(p)).collect()
    }

    /// Object-space transform of every stroke copy, in the order of [`Self::copy_cameras`]
    /// (index 0 is the identity): a point painted at `p` is also painted at `copy_transforms[k] * p`.
    pub fn copy_transforms(&self, center: glam::Vec3) -> Vec<glam::Mat4> {
        use glam::Mat4;
        let mut list = vec![Mat4::IDENTITY];
        let mirrors = self.symmetry.iter().filter(|&&m| m).count();
        let radial = &self.radial;
        let n = radial.effective_count(mirrors);
        if n > 1 {
            let pivot = self.radial_pivot(center);
            for k in 1..n {
                let angle = std::f32::consts::TAU * k as f32 / n as f32;
                let rotate = Mat4::from_translation(pivot) * Mat4::from_axis_angle(radial.axis_dir(), angle) * Mat4::from_translation(-pivot);
                list.push(if radial.flip && k % 2 == 1 {
                    let v = radial.flip_axis();
                    rotate * reflection(v, pivot[v])
                } else {
                    rotate
                });
            }
        }
        for axis in 0..3 {
            if self.symmetry[axis] {
                let r = reflection(axis, self.mirror_plane(axis, center));
                let mirrored: Vec<Mat4> = list.iter().map(|t| r * *t).collect();
                list.extend(mirrored);
            }
        }
        list
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
    fn pressure_curves_keep_the_ends_rise_steadily_and_soften_light_pressure() {
        for c in PressureCurve::ALL {
            assert_eq!(c.apply(0.0), 0.0, "{c:?}");
            assert_eq!(c.apply(1.0), 1.0, "{c:?}");
            assert_eq!(c.apply(2.0), 1.0, "{c:?} clamps");
            assert_eq!(c.apply(f32::NAN), 1.0, "{c:?}: a broken reading paints at full pressure");
            let samples: Vec<f32> = (0..=20).map(|i| c.apply(i as f32 / 20.0)).collect();
            assert!(samples.windows(2).all(|w| w[1] >= w[0]), "{c:?} never decreases");
        }
        assert_eq!(PressureCurve::default(), PressureCurve::Linear);
        assert_eq!(PressureCurve::Linear.apply(0.3), 0.3);
        assert!((PressureCurve::EaseIn.apply(0.5) - 0.25).abs() < 1e-6, "ease in: half pressure does a quarter");
        assert!((PressureCurve::EaseInOut.apply(0.5) - 0.5).abs() < 1e-6, "ease in-out is symmetric");
        assert!(PressureCurve::EaseInOut.apply(0.2) < 0.2 && PressureCurve::EaseInOut.apply(0.8) > 0.8);
    }

    #[test]
    fn symmetry_points_mirror_and_turn_the_point_like_the_stroke_copies() {
        use glam::Vec3;
        let center = Vec3::new(1.0, 0.0, 0.0);
        let p = Vec3::new(3.0, 2.0, 0.5);
        let none = StrokeOptions::default();
        assert_eq!(none.symmetry_points(center, p), vec![p], "no symmetry: only the point");
        let x = StrokeOptions { symmetry: [true, false, false], ..Default::default() };
        assert_eq!(x.symmetry_points(center, p), vec![p, Vec3::new(-1.0, 2.0, 0.5)], "mirrored through x = 1");
        let radial = StrokeOptions { radial: RadialSymmetry { enabled: true, axis: 1, count: 4, center_offset: [0.0; 3], flip: false }, ..Default::default() };
        let pts = radial.symmetry_points(center, p);
        assert_eq!(pts.len(), 4);
        // A quarter turn about the vertical line through the centre.
        assert!((pts[1] - Vec3::new(1.5, 2.0, -2.0)).length() < 1e-5, "{:?}", pts[1]);
        for q in &pts {
            assert!(((Vec3::new(q.x, 0.0, q.z) - center).length() - (Vec3::new(p.x, 0.0, p.z) - center).length()).abs() < 1e-4, "same distance from the axis");
            assert!((q.y - p.y).abs() < 1e-5);
        }
        let both = StrokeOptions { symmetry: [false, false, true], ..radial };
        assert_eq!(both.symmetry_points(center, p).len() as u32, both.copies());
    }

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

    /// Painting through copy camera `k` at the screen point of `p` lands on `copy_transforms[k] * p`.
    fn assert_cameras_match_transforms(options: &StrokeOptions) {
        use glam::Vec3;
        let center = Vec3::new(0.3, -0.2, 0.1);
        let camera = crate::Camera::perspective(Vec3::new(1.0, 2.0, 6.0), center, Vec3::Y, 45.0, 0.1, 100.0, [640, 480]);
        let cams = options.copy_cameras(&camera, center);
        let transforms = options.copy_transforms(center);
        assert_eq!(cams.len(), transforms.len());
        assert_eq!(cams.len() as u32, options.copies());
        for p in [Vec3::new(0.5, 0.1, 0.4), Vec3::new(-0.2, 0.6, -0.3)] {
            let at = camera.project(p).unwrap();
            for (k, (cam, t)) in cams.iter().zip(&transforms).enumerate() {
                let copy = t.transform_point3(p);
                let seen = cam.project(copy).unwrap();
                assert!((seen - at).length() < 1e-2, "copy {k}: {seen} vs {at} ({options:?})");
            }
        }
    }

    #[test]
    fn copy_transforms_match_the_cameras_strokes_paint_through() {
        let radial = RadialSymmetry { enabled: true, axis: 1, count: 4, center_offset: [0.25, 0.0, -0.1], flip: false };
        for options in [
            StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() },
            StrokeOptions { symmetry: [true, false, true], mirror_offset: [0.4, 0.0, -0.3], ..StrokeOptions::default() },
            StrokeOptions { radial, ..StrokeOptions::default() },
            StrokeOptions { radial: RadialSymmetry { flip: true, ..radial }, ..StrokeOptions::default() },
            StrokeOptions { symmetry: [false, true, false], mirror_offset: [0.0, 0.2, 0.0], radial: RadialSymmetry { flip: true, count: 6, ..radial }, ..StrokeOptions::default() },
        ] {
            assert_cameras_match_transforms(&options);
        }
    }

    #[test]
    fn mirror_offset_moves_the_plane_and_flip_mirrors_every_other_copy() {
        use glam::Vec3;
        let center = Vec3::new(1.0, 2.0, 3.0);
        let options = StrokeOptions { symmetry: [true, false, false], mirror_offset: [0.5, 0.0, 0.0], ..StrokeOptions::default() };
        assert_eq!(options.mirror_plane(0, center), 1.5);
        let t = options.copy_transforms(center);
        assert!(t[1].transform_point3(Vec3::new(2.0, 0.0, 0.0)).distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-6, "x -> 2 * 1.5 - x");
        let bad = StrokeOptions { mirror_offset: [f32::NAN, 0.0, 0.0], ..options };
        assert_eq!(bad.mirror_plane(0, center), 1.0, "a non-finite offset falls back to the center");

        let radial = RadialSymmetry { enabled: true, axis: 1, count: 4, center_offset: [0.0; 3], flip: true };
        let t = StrokeOptions { radial, ..StrokeOptions::default() }.copy_transforms(Vec3::ZERO);
        let det: Vec<f32> = t.iter().map(|m| m.determinant()).collect();
        assert!(det.iter().zip([1.0, -1.0, 1.0, -1.0]).all(|(d, e)| (d - e).abs() < 1e-5), "odd copies are mirrored: {det:?}");
        // Axis Y: the flip plane holds Y and Z (spoke 0 points along Z), so copy 1 is x -> -x,
        // then a quarter turn around Y.
        let p = Vec3::new(1.0, 0.0, 0.0);
        let expected = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * Vec3::new(-1.0, 0.0, 0.0);
        assert!(t[1].transform_point3(p).distance(expected) < 1e-5);
        assert_eq!(radial.flip_axis(), 0);
        assert_eq!(RadialSymmetry { axis: 2, ..radial }.flip_axis(), 1);
    }
    #[test]
    fn size_space_converts_the_same_percentage_per_space() {
        let c = SizeContext { diagonal: 4.0, pixels_per_unit: 100.0, viewport_height: 800.0, uv_per_unit: 0.25 };
        let mut s = BrushSettings { size: 10.0, ..BrushSettings::default() };
        assert_eq!(s.radius_in(&c), s.radius(4.0), "Object is the default and matches radius()");
        assert!((s.radius_in(&c) - 0.2).abs() < 1e-6);
        s.size_space = SizeSpace::Viewport;
        // 10 % of 800 px = 80 px diameter = 40 px radius = 0.4 units at 100 px per unit.
        assert!((s.radius_in(&c) - 0.4).abs() < 1e-6);
        // Zooming in (more pixels per unit) shrinks the world radius: constant on screen.
        assert!((s.radius_in(&SizeContext { pixels_per_unit: 200.0, ..c }) * 200.0 - 40.0).abs() < 1e-4);
        s.size_space = SizeSpace::Texture;
        // 10 % of the texture = radius 0.05 UV = 0.2 units at 0.25 UV per unit.
        assert!((s.radius_in(&c) * c.uv_per_unit - 0.05).abs() < 1e-6);
        // Unknown scales fall back to the object size.
        let unknown = SizeContext { pixels_per_unit: 0.0, uv_per_unit: f32::NAN, ..c };
        assert_eq!(s.radius_in(&unknown), s.radius(4.0));
        s.size_space = SizeSpace::Viewport;
        assert_eq!(s.radius_in(&unknown), s.radius(4.0));
    }

    #[test]
    fn pressure_toggles_and_minimums() {
        let mut s = BrushSettings::default();
        assert!(s.size_pressure() && !s.flow_pressure(), "defaults: size follows pressure, flow does not");
        assert_eq!(s.min_size(), 0.0);
        s.set_size_pressure(true, 0.25);
        assert!((s.min_size() - 0.25).abs() < 1e-6);
        assert!((s.pressure_size_factor(0.0) - (0.25 + 0.75 * MIN_PRESSURE)).abs() < 1e-6);
        s.set_size_pressure(true, 1.0);
        assert!(s.size_pressure(), "a minimum of 1 would silently switch the toggle off");
        s.set_size_pressure(false, 0.25);
        assert!(!s.size_pressure());
        assert_eq!(s.pressure_size_factor(0.1), 1.0);
        s.set_flow_pressure(true, 0.5);
        assert!(s.flow_pressure() && (s.min_flow() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn jitter_pressure_toggles_scale_only_their_jitter() {
        let s = BrushSettings { size_jitter: 0.8, flow_jitter: 0.6, angle_jitter_deg: 90.0, position_jitter: 0.5,
            size_jitter_pressure: true, angle_jitter_pressure: true, ..BrushSettings::default() };
        assert_eq!(s.jitters(1.0), [0.8, 0.6, 90.0, 0.5], "a mouse (pressure 1) is unchanged");
        assert_eq!(s.jitters(0.5), [0.4, 0.6, 45.0, 0.5]);
        assert_eq!(s.jitters(0.0), [0.0, 0.6, 0.0, 0.5]);
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
