//! Mesh-map baking (CPU, BVH ray casting, rayon).
//!
//! For every covered texel of a texture set the low-poly surface point is found by UV
//! rasterization. With a high-poly mesh, that point is projected onto the high poly along the
//! low-poly normal (cage: `front` above, `rear` below) and detail maps are taken there; without
//! one, the low poly is its own detail ("low on low": flat tangent normals).
//!
//! Maps: tangent-space normal (OpenGL / MikkTSpace frame of the low poly), world-space normal,
//! position (normalized per axis to the low-poly bounds), ID (color per mesh element), ambient
//! occlusion (cosine-weighted hemisphere rays), curvature (3D neighbourhood convexity, 0.5 flat),
//! thickness (rays into the surface; bright = thin), height (signed distance to the high poly
//! inside the cage, 0.5 flat) and bent normal (average open direction, tangent space). Gutters are padded from the nearest
//! island texel up to the dilation width and diffused beyond it. Optional: supersampling
//! ([`Antialiasing`]), averaged cage normals, skipping back faces, a custom cage mesh.
//!
//! With [`subcraft_core::BakeMatchMode::ByName`], every ray against the high poly (projection,
//! occlusion, bent normal, thickness) only accepts high triangles whose part matches the low
//! part by name ([`name_match`]); the shared high-poly BVH is traversed with a triangle filter.

pub mod curvature;
pub mod name_match;
pub mod normal_curvature;
pub mod paint_maps;
pub mod seam_check;
pub mod skew;
pub mod surface;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::MeshMapKind;
use subcraft_core::color::hsv_to_rgb;
use subcraft_image::TiledImage;
use subcraft_mesh::ray_batch::{BatchControl, CurvatureSurface, CurvatureTexel, NO_GROUP, ProjectionRay, RayBatchBackend, RayBatchError, RayMode, RayTexel, ray_backends};
use subcraft_mesh::{Bvh, Mesh, topology};

use name_match::PartMatch;
use surface::{Texel, cone_directions, cosine_directions, hash01, texels};

/// Where the ID map takes its colours from (issue #117). The colours come from the mesh the bake
/// reads its detail from: the high poly when there is one, else the low poly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IdSource {
    /// The file's vertex colours (polypaint), interpolated over each triangle.
    VertexColor,
    /// The file's colour of each material (diffuse / base colour).
    MaterialColor,
    /// One generated colour per connected mesh element (polygroup-like): the default.
    #[default]
    MeshId,
    /// One generated colour per named object of the file (per material when it names none).
    Submesh,
    /// One generated colour per high-poly file, when several are baked together.
    FileId,
}

impl IdSource {
    pub const ALL: [IdSource; 5] = [IdSource::VertexColor, IdSource::MaterialColor, IdSource::MeshId, IdSource::Submesh, IdSource::FileId];
}

/// How occlusion rays are spread over their cone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RayDistribution {
    /// More rays near the normal (the physically weighted choice).
    #[default]
    Cosine,
    /// The same number of rays in every direction of the cone.
    Uniform,
}

impl RayDistribution {
    pub const ALL: [RayDistribution; 2] = [RayDistribution::Cosine, RayDistribution::Uniform];
    pub fn label(self) -> &'static str {
        match self {
            RayDistribution::Cosine => "Cosine",
            RayDistribution::Uniform => "Uniform",
        }
    }
}

/// How much an occluder counts with its distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AoAttenuation {
    /// Any hit within the distance occludes fully.
    #[default]
    None,
    /// Occlusion fades smoothly to nothing at the maximum distance.
    Smooth,
    /// Occlusion fades linearly to nothing at the maximum distance.
    Linear,
}

impl AoAttenuation {
    pub const ALL: [AoAttenuation; 3] = [AoAttenuation::None, AoAttenuation::Smooth, AoAttenuation::Linear];
    pub fn label(self) -> &'static str {
        match self {
            AoAttenuation::None => "None",
            AoAttenuation::Smooth => "Smooth",
            AoAttenuation::Linear => "Linear",
        }
    }
    /// Weight of a hit at `t` of `max`.
    fn weight(self, t: f32, max: f32) -> f32 {
        let x = (1.0 - t / max.max(1e-9)).clamp(0.0, 1.0);
        match self {
            AoAttenuation::None => 1.0,
            AoAttenuation::Linear => x,
            AoAttenuation::Smooth => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// Tonemapping of the curvature map (Painter's Curvature > Auto tonemapping / Min / Max).
///
/// Curvature leaves the baker with 0.5 on flat surfaces, but how far edges reach towards 0 or 1
/// depends on the mesh and the radius, so the map is often a narrow band of greys. Auto stretches
/// it around 0.5 until the strongest edges of this texture set (or UDIM tile) span the whole range;
/// otherwise `min` and `max` pick the raw values that become black and white. The default (auto
/// off, 0 to 1) leaves the map unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvatureTonemap {
    pub auto: bool,
    /// Raw curvature that becomes 0 when `auto` is off.
    pub min: f32,
    /// Raw curvature that becomes 1 when `auto` is off.
    pub max: f32,
}

impl Default for CurvatureTonemap {
    fn default() -> Self {
        Self { auto: false, min: 0.0, max: 1.0 }
    }
}

impl CurvatureTonemap {
    /// Share of the strongest curvature values allowed to clip with `auto` (outliers such as
    /// single spikes on tiny triangles must not decide the stretch).
    pub const AUTO_CLIP: f32 = 0.005;
    /// Largest stretch `auto` applies, so a nearly flat tile does not turn its noise into edges.
    pub const MAX_GAIN: f32 = 16.0;

    /// True when the map passes through unchanged.
    pub fn is_identity(&self) -> bool {
        !self.auto && self.min == 0.0 && self.max == 1.0
    }

    /// The linear remap `value * scale + offset` these settings give for `values` (raw curvature).
    pub fn remap(&self, values: &[f32]) -> (f32, f32) {
        if self.auto {
            let mut dev: Vec<f32> = values.iter().filter(|v| v.is_finite()).map(|v| (v - 0.5).abs()).collect();
            if dev.is_empty() {
                return (1.0, 0.0);
            }
            let k = ((dev.len() as f32 * (1.0 - Self::AUTO_CLIP)) as usize).min(dev.len() - 1);
            let (_, reach, _) = dev.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
            let gain = (0.5 / reach.max(1e-6)).clamp(1.0, Self::MAX_GAIN);
            (gain, 0.5 - 0.5 * gain)
        } else {
            let span = self.max - self.min;
            if span.abs() < 1e-6 {
                return (1.0, 0.0);
            }
            (1.0 / span, -self.min / span)
        }
    }

    /// Tonemap `values` in place (clamped to 0..1).
    pub fn apply(&self, values: &mut [f32]) {
        if self.is_identity() {
            return;
        }
        let (scale, offset) = self.remap(values);
        for v in values.iter_mut() {
            *v = (*v * scale + offset).clamp(0.0, 1.0);
        }
    }
}

/// Options of the occlusion rays (ambient occlusion; the ray cone and distances also shape the
/// bent normals). The default is the plain cosine hemisphere, which may run on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AoOptions {
    /// Occluders nearer than this are ignored, as a fraction of the diagonal.
    pub min_distance: f32,
    /// Full opening angle of the ray cone in degrees (180 = the whole hemisphere).
    pub spread_angle: f32,
    pub distribution: RayDistribution,
    pub attenuation: AoAttenuation,
    /// A floor under the model occludes too (Y up).
    pub ground_plane: bool,
    /// How far the floor lies under the model's lowest point, as a fraction of the diagonal.
    pub ground_offset: f32,
}

impl Default for AoOptions {
    fn default() -> Self {
        Self { min_distance: 0.0, spread_angle: 180.0, distribution: RayDistribution::Cosine, attenuation: AoAttenuation::None, ground_plane: false, ground_offset: 0.0 }
    }
}

/// The thickness baker's ray options (Painter's Thickness settings), beside its ray count and
/// maximum distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThicknessOptions {
    /// Surfaces nearer than this are skipped, as a fraction of the diagonal.
    pub min_distance: f32,
    /// Full opening angle of the ray cone in degrees (180 = the whole hemisphere).
    pub spread_angle: f32,
    pub distribution: RayDistribution,
}

impl Default for ThicknessOptions {
    fn default() -> Self {
        Self { min_distance: 0.0, spread_angle: 180.0, distribution: RayDistribution::Cosine }
    }
}

impl ThicknessOptions {
    /// The plain cosine hemisphere from the surface (what the GPU kernels compute).
    pub fn is_plain(&self) -> bool {
        self.min_distance <= 0.0 && self.spread_angle >= 180.0 && self.distribution == RayDistribution::Cosine
    }
}

impl AoOptions {
    /// The plain cosine hemisphere (what the GPU kernels and the fast CPU path compute).
    pub fn is_plain(&self) -> bool {
        self.min_distance <= 0.0 && self.spread_angle >= 180.0 && self.distribution == RayDistribution::Cosine && self.attenuation == AoAttenuation::None && !self.ground_plane
    }
}

/// What the position map is normalized to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PositionNormalization {
    /// Each axis over the model's bounding box.
    #[default]
    BoundingBox,
    /// All axes over the sphere around the bounding box (proportions are kept).
    BoundingSphere,
    /// No normalization: the world position times [`PositionOptions::scale`], with the world
    /// origin at 0.5 (values past 0..1 are clipped).
    None,
}

impl PositionNormalization {
    pub const ALL: [PositionNormalization; 3] = [PositionNormalization::BoundingBox, PositionNormalization::BoundingSphere, PositionNormalization::None];
    pub fn label(self) -> &'static str {
        match self {
            PositionNormalization::BoundingBox => "Bounding box",
            PositionNormalization::BoundingSphere => "Bounding sphere",
            PositionNormalization::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionOptions {
    /// `Some(0 | 1 | 2)`: a grayscale map of X, Y or Z alone; `None`: X, Y, Z in red, green, blue.
    pub single_axis: Option<u8>,
    pub normalization: PositionNormalization,
    /// Map value per world unit with [`PositionNormalization::None`].
    pub scale: f32,
}

impl Default for PositionOptions {
    fn default() -> Self {
        Self { single_axis: None, normalization: PositionNormalization::default(), scale: 1.0 }
    }
}

/// What the height map's full range stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum HeightNormalization {
    /// White at the cage's front reach, black at its rear reach.
    #[default]
    Cage,
    /// White / black at this distance above / below the low poly, as a fraction of the diagonal.
    Manual { distance: f32 },
}

/// How generated ID colours (one per mesh element or object) are picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IdGenerator {
    /// Unrelated colours.
    Random,
    /// Saturated colours stepping around the hue wheel.
    #[default]
    HueShift,
    /// Gray levels.
    Grayscale,
}

impl IdGenerator {
    pub const ALL: [IdGenerator; 3] = [IdGenerator::Random, IdGenerator::HueShift, IdGenerator::Grayscale];
    pub fn label(self) -> &'static str {
        match self {
            IdGenerator::Random => "Random",
            IdGenerator::HueShift => "Hue shift",
            IdGenerator::Grayscale => "Grayscale",
        }
    }
    /// The colour of element / object `i`. Never black, the ID map's "nothing here".
    pub fn color(self, i: u32) -> [f32; 3] {
        match self {
            IdGenerator::Random => [1u32, 2, 3].map(|c| 0.15 + 0.85 * hash01(i, 100 + c)),
            IdGenerator::HueShift => hsv_to_rgb([(i as f32 * 0.618_034).fract(), 0.65, 0.9]),
            IdGenerator::Grayscale => [0.1 + 0.9 * (i as f32 * 0.618_034).fract(); 3],
        }
    }
}

/// Supersampling of the whole bake: each output texel averages this many texels per side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Antialiasing {
    #[default]
    None,
    X2,
    X4,
    X8,
}

impl Antialiasing {
    pub const ALL: [Antialiasing; 4] = [Antialiasing::None, Antialiasing::X2, Antialiasing::X4, Antialiasing::X8];
    /// Texels baked per output texel, per side.
    pub fn factor(self) -> u32 {
        match self {
            Antialiasing::None => 1,
            Antialiasing::X2 => 2,
            Antialiasing::X4 => 4,
            Antialiasing::X8 => 8,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Antialiasing::None => "None",
            Antialiasing::X2 => "Subsampling 2x2",
            Antialiasing::X4 => "Subsampling 4x4",
            Antialiasing::X8 => "Subsampling 8x8",
        }
    }
}

/// The largest size a supersampled bake works at; a higher antialiasing is reduced to fit.
pub const MAX_SUPERSAMPLED_SIZE: u32 = 8192;

/// A custom cage: the low poly pushed outwards by the artist, with the low poly's UVs. Each
/// projection ray starts on the cage and runs through the low-poly point with the same UV.
/// Two cages are equal when they are the same loaded mesh.
#[derive(Clone)]
pub struct CageMesh(pub std::sync::Arc<Mesh>);

impl PartialEq for CageMesh {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for CageMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CageMesh({:?}, {} triangles)", self.0.name, self.0.triangle_count())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakeSettings {
    /// Output size (square).
    pub size: u32,
    pub maps: Vec<MeshMapKind>,
    pub ao_samples: u32,
    /// Occlusion ray length as a fraction of the mesh bounding-box diagonal.
    pub ao_distance: f32,
    pub thickness_samples: u32,
    pub thickness_distance: f32,
    /// Curvature neighbourhood radius as a fraction of the diagonal.
    pub curvature_radius: f32,
    /// High-poly projection cage above / below the low poly, fractions of the diagonal.
    pub cage_front: f32,
    pub cage_rear: f32,
    /// Cast AO and thickness rays on the GPU when a ray backend is registered (falls back to the
    /// CPU on any error). The CPU result stays the reference.
    pub use_gpu: bool,
    /// Which high-poly parts rays from each low-poly part may hit (see [`name_match`]).
    pub name_match: subcraft_core::BakeMatch,
    /// Transform put on the loaded high poly before baking (identity = as imported).
    pub placement: subcraft_core::BakePlacement,
    /// Derive curvature from the tangent-space normal map of this bake (measured along the
    /// surface, see [`normal_curvature`]) instead of from the detail geometry.
    pub curvature_from_normal: bool,
    pub normal_curvature: normal_curvature::NormalCurvatureSettings,
    /// Contrast stretch of the finished curvature map (Painter's tonemapping).
    pub curvature_tonemap: CurvatureTonemap,
    /// Where the ID map's colours come from.
    pub id_source: IdSource,
    /// Padding: gutter texels up to this many texels from an island copy the nearest island texel.
    pub dilation: u32,
    /// Fill the gutters beyond the dilation with a smooth blend of the padded islands
    /// (off: they keep the map's neutral value).
    pub diffusion: bool,
    pub antialiasing: Antialiasing,
    /// Cast the projection rays along normals averaged over every face at a position, so hard
    /// edges and UV seams of the low poly leave no gap between neighbouring faces' rays. The
    /// baked tangent frame stays the low poly's own.
    pub average_normals: bool,
    /// Straighten the averaged rays away from the hard edges (needs `average_normals` and the
    /// distance-based cage), see [`skew`].
    pub skew: skew::SkewCorrection,
    /// Painted mask scaling the skew correction per texel (white: full correction). Per texture
    /// set: the caller sets the map of the set it bakes.
    pub skew_map: Option<paint_maps::PaintMap>,
    /// Painted mask pushing the distance-based cage out per texel (white: by `cage_push`).
    pub cage_push_map: Option<paint_maps::PaintMap>,
    /// How far white on the cage push map moves the cage: the frontal and rear distances grow by
    /// this many times themselves (1: doubled).
    pub cage_push: f32,
    /// Projection rays skip high-poly faces that face away from them.
    pub ignore_backface: bool,
    /// A custom cage instead of the distance-based one (texels outside its UVs use the distances).
    pub cage: Option<CageMesh>,
    pub ao: AoOptions,
    pub thickness: ThicknessOptions,
    pub position: PositionOptions,
    pub height: HeightNormalization,
    pub id_generator: IdGenerator,
    /// The vertex colour set the ID map reads with [`IdSource::VertexColor`] (0 = the first).
    /// With several high-poly files this is the set of the main (first) file.
    pub vertex_color_set: u32,
    /// The vertex colour set of each additional high-poly file, in file order (entry 0 = the
    /// second file). `None` or a missing entry: the main file's [`Self::vertex_color_set`].
    pub extra_vertex_color_sets: Vec<Option<u32>>,
    /// Additional ID maps (issue #733). [`bake_streamed`] ignores them: the caller bakes each
    /// one as an ID-only bake with [`ExtraIdBake::settings`] and stores it as its own map.
    pub extra_ids: Vec<ExtraIdBake>,
    /// ID map texels whose rays miss the high poly take the nearest baked ID (issue #466). Off
    /// for an additional ID map baked from a file with only some parts: the rest stays black.
    pub id_fill_misses: bool,
}

/// One additional ID map (issue #733): baked like the ID map, from its own source mesh with its
/// own colour options, and stored as [`MeshMapKind::Id2`]..[`MeshMapKind::Id9`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExtraIdBake {
    /// The map it is stored as: one of [`MeshMapKind::EXTRA_IDS`].
    pub map: MeshMapKind,
    /// Baked with the other maps (an unticked entry keeps its settings and its baked map).
    pub enabled: bool,
    /// High-poly file the colours come from. Empty: the bake's high poly (all its files), or the
    /// low poly when there is none. It is placed like the main high poly; parts it lacks read black.
    pub source: String,
    pub id_source: IdSource,
    pub id_generator: IdGenerator,
    /// Vertex colour set read with [`IdSource::VertexColor`].
    pub vertex_color_set: u32,
    pub antialiasing: Antialiasing,
}

impl ExtraIdBake {
    /// A new entry for `map` with the ID baker's defaults.
    pub fn new(map: MeshMapKind) -> Self {
        let d = BakeSettings::default();
        Self { map, enabled: true, source: String::new(), id_source: d.id_source, id_generator: d.id_generator, vertex_color_set: 0, antialiasing: d.antialiasing }
    }

    /// The first slot of [`MeshMapKind::EXTRA_IDS`] no entry uses (`None`: all are taken).
    pub fn free_slot(entries: &[ExtraIdBake]) -> Option<MeshMapKind> {
        MeshMapKind::EXTRA_IDS.into_iter().find(|k| entries.iter().all(|e| e.map != *k))
    }

    /// The settings of its ID-only bake: `base` (size, cage, matching, placement, dilation...)
    /// with this entry's colour options, baking only [`MeshMapKind::Id`].
    pub fn settings(&self, base: &BakeSettings) -> BakeSettings {
        let mut s = base.clone();
        s.maps = vec![MeshMapKind::Id];
        s.id_source = self.id_source;
        s.id_generator = self.id_generator;
        s.vertex_color_set = self.vertex_color_set;
        s.antialiasing = self.antialiasing;
        // Its own file has its own colour sets; with the main high poly every file reads this one.
        s.extra_vertex_color_sets = Vec::new();
        s.extra_ids = Vec::new();
        // A file of only some parts: what it lacks reads black (no ID), not a neighbour's ID.
        s.id_fill_misses = self.source.is_empty();
        s
    }
}

impl BakeSettings {
    /// The vertex colour set the ID map reads on triangles from high-poly file `file`
    /// (0 = the main file; see [`subcraft_mesh::Mesh::file_of`]).
    pub fn vertex_color_set_of(&self, file: u32) -> u32 {
        match file.checked_sub(1) {
            None => self.vertex_color_set,
            Some(i) => self.extra_vertex_color_sets.get(i as usize).copied().flatten().unwrap_or(self.vertex_color_set),
        }
    }
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            size: 1024,
            maps: MeshMapKind::default_bake(),
            ao_samples: 32,
            ao_distance: 0.25,
            thickness_samples: 16,
            thickness_distance: 0.5,
            curvature_radius: 0.02,
            cage_front: 0.01,
            cage_rear: 0.01,
            use_gpu: true,
            name_match: subcraft_core::BakeMatch::default(),
            placement: subcraft_core::BakePlacement::default(),
            curvature_from_normal: false,
            normal_curvature: normal_curvature::NormalCurvatureSettings::default(),
            curvature_tonemap: CurvatureTonemap::default(),
            id_source: IdSource::default(),
            dilation: 16,
            diffusion: true,
            antialiasing: Antialiasing::None,
            average_normals: false,
            skew: skew::SkewCorrection::default(),
            skew_map: None,
            cage_push_map: None,
            cage_push: 1.0,
            ignore_backface: false,
            cage: None,
            ao: AoOptions::default(),
            thickness: ThicknessOptions::default(),
            position: PositionOptions::default(),
            height: HeightNormalization::default(),
            id_generator: IdGenerator::default(),
            vertex_color_set: 0,
            extra_vertex_color_sets: Vec::new(),
            extra_ids: Vec::new(),
            id_fill_misses: true,
        }
    }
}

/// The high poly as the baker and the overlay see it: `placement` applied to a copy (the
/// imported mesh is untouched). The identity returns the same allocation.
pub fn place_high(low: &Mesh, high: &std::sync::Arc<Mesh>, placement: &subcraft_core::BakePlacement) -> std::sync::Arc<Mesh> {
    if placement.is_identity() { return high.clone(); }
    let m = placement.matrix((low.bounds.min, low.bounds.max), (high.bounds.min, high.bounds.max));
    std::sync::Arc::new(high.transformed(&m))
}

/// A mesh with its BVH.
#[derive(Clone, Copy)]
pub struct Geometry<'a> {
    pub mesh: &'a Mesh,
    pub bvh: &'a Bvh,
}

pub struct BakeInput<'a> {
    pub low: Geometry<'a>,
    pub high: Option<Geometry<'a>>,
    pub material_slot: u32,
}

/// Shared progress / cancellation for a bake running on another thread.
#[derive(Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
    current_map: AtomicUsize,
    reports: Mutex<Vec<MapReport>>,
    warnings: Mutex<Vec<String>>,
}

/// How one map was produced.
#[derive(Clone, Debug, PartialEq)]
pub struct MapReport {
    pub kind: MeshMapKind,
    /// `CPU`, `GPU (adapter)`, `CPU (GPU failed: reason)`, or `CPU, projection on GPU (adapter)` when only the
    /// high-poly projection ran on the GPU.
    pub backend: String,
    pub seconds: f32,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 { 0.0 } else { self.done.load(Ordering::Relaxed) as f32 / t as f32 }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    /// Maps finished by the current / last bake, in order.
    pub fn reports(&self) -> Vec<MapReport> {
        self.reports.lock().map(|r| r.clone()).unwrap_or_default()
    }
    /// Things the current / last bake did not do as asked (cage misses, CPU fallbacks), in order.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().map(|w| w.clone()).unwrap_or_default()
    }
    fn warn(&self, text: String) {
        if let Ok(mut w) = self.warnings.lock() {
            w.push(text);
        }
    }
    pub fn current_map(&self) -> Option<MeshMapKind> {
        self.current_map.load(Ordering::Relaxed).checked_sub(1).and_then(|i| MeshMapKind::ALL.get(i).copied())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BakeError {
    Cancelled,
    NoTexels,
    NoProjection,
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakeError::Cancelled => f.write_str("bake cancelled"),
            BakeError::NoTexels => f.write_str("texture set has no UV-covered texels"),
            BakeError::NoProjection => f.write_str("no high-poly surface was reached: align the low/high models in the same coordinates and increase Cage front / rear"),
        }
    }
}

impl std::error::Error for BakeError {}

/// Where detail maps are sampled for one texel.
#[derive(Clone, Copy)]
struct Detail {
    position: Vec3,
    normal: Vec3,
    /// Triangle in the detail mesh.
    tri: Option<u32>,
    /// Offset direction for ray origins (geometric normal).
    offset: Vec3,
    /// Signed distance from the low-poly point to the detail surface along the cage normal
    /// (positive = the detail stands above the low poly).
    height: f32,
    /// Name-match group of the low part ([`NO_GROUP`]: rays hit every high part).
    group: u32,
}

const CHUNK: usize = 2048;

/// One projection ray: from the cage through the low-poly point and on to the rear reach.
struct CageRay {
    origin: Vec3,
    dir: Vec3,
    /// Distance from the origin to the low-poly point.
    front: f32,
    /// Length of the ray.
    reach: f32,
}

/// The projection rays of one texture set's bake.
struct Cage {
    front: f32,
    rear: f32,
    /// Averaged normal per low-poly vertex (see [`averaged_normals`]).
    smooth: Option<Vec<Vec3>>,
    /// Custom cage: its surface point per texel of the bake (`None` outside its UVs).
    custom: Option<Vec<Option<Vec3>>>,
    /// Skew correction of the averaged rays.
    skew: Option<skew::SkewField>,
    /// Painted skew amount per texel (scales the correction).
    skew_map: Option<paint_maps::PaintMap>,
    /// Painted cage push per texel, and how far white pushes.
    push: Option<(paint_maps::PaintMap, f32)>,
    size: u32,
}

impl Cage {
    fn new(low: &Mesh, material_slot: u32, size: u32, settings: &BakeSettings, diag: f32) -> Self {
        let custom = settings.cage.as_ref().map(|cage| {
            // The cage's slot with the low poly's material name, else the same slot number.
            let slot = low.materials.get(material_slot as usize)
                .and_then(|m| cage.0.materials.iter().position(|c| c.name == m.name))
                .map_or(material_slot, |i| i as u32);
            let mut at = vec![None; (size * size) as usize];
            for t in texels(&cage.0, slot, size) {
                at[t.index as usize] = Some(t.position);
            }
            at
        });
        let skew = (settings.average_normals && settings.skew.enabled).then(|| skew::SkewField::new(low, settings.skew, diag));
        let push = settings.cage_push_map.clone().filter(|_| settings.cage_push > 0.0).map(|m| (m, settings.cage_push));
        let skew_map = settings.skew_map.clone().filter(|_| skew.is_some());
        Self { front: settings.cage_front * diag, rear: settings.cage_rear * diag, smooth: settings.average_normals.then(|| averaged_normals(low)), custom, skew, skew_map, push, size }
    }

    fn ray(&self, low: &Mesh, t: &Texel) -> CageRay {
        if let Some(Some(start)) = self.custom.as_ref().map(|c| c[t.index as usize]) {
            let to_low = t.position - start;
            let front = to_low.length();
            if front > 1e-9 {
                return CageRay { origin: start, dir: to_low / front, front, reach: front + self.rear };
            }
        }
        let mut n = t.normal.normalize_or(t.geometric_normal);
        if let Some(smooth) = &self.smooth {
            let [a, b, c] = low.tri(t.tri as usize).map(|i| i as usize);
            let l = normal_curvature::barycentric(t.position, low.positions[a], low.positions[b], low.positions[c]);
            let own = n;
            n = (smooth[a] * l[0] + smooth[b] * l[1] + smooth[c] * l[2]).normalize_or(n);
            if let Some(skew) = &self.skew {
                let painted = self.skew_map.as_ref().map_or(1.0, |m| m.at_texel(t.index, self.size));
                n = skew.direction(t.position, n, own, t.geometric_normal, painted);
            }
        }
        let grow = self.push.as_ref().map_or(1.0, |(m, push)| 1.0 + push * m.at_texel(t.index, self.size));
        let (front, rear) = (self.front * grow, self.rear * grow);
        CageRay { origin: t.position + n * front, dir: -n, front, reach: front + rear }
    }
}

/// One normal per vertex, averaged over every triangle that touches the vertex's position,
/// whatever its smoothing or UV splits. Each triangle counts by its corner angle there, so the
/// result does not depend on how faces are triangulated. These are the directions the baker's
/// rays follow with [`BakeSettings::average_normals`]; the bake overlay draws its cage along them too.
pub fn averaged_normals(mesh: &Mesh) -> Vec<Vec3> {
    // `+ 0.0` folds -0.0 into 0.0 so both zeros share a key.
    let key = |p: Vec3| (p + Vec3::ZERO).to_array().map(f32::to_bits);
    let mut sums: std::collections::HashMap<[u32; 3], Vec3> = std::collections::HashMap::new();
    for t in 0..mesh.triangle_count() {
        let p = mesh.tri_positions(t);
        let Some(mut n) = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize() else { continue };
        // Follow the shading normals where the winding disagrees with them.
        let [ia, ib, ic] = mesh.tri(t).map(|i| i as usize);
        if n.dot(mesh.normals[ia] + mesh.normals[ib] + mesh.normals[ic]) < 0.0 {
            n = -n;
        }
        for i in 0..3 {
            let (a, b) = ((p[(i + 1) % 3] - p[i]).normalize_or_zero(), (p[(i + 2) % 3] - p[i]).normalize_or_zero());
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            *sums.entry(key(p[i])).or_default() += n * angle;
        }
    }
    mesh.positions.iter().zip(&mesh.normals).map(|(p, n)| sums.get(&key(*p)).and_then(|s| s.try_normalize()).unwrap_or(*n)).collect()
}

/// Give the texels whose projection missed the high poly (`tri` is `None`) the value of the
/// nearest texel that hit it. Nothing changes when every texel missed or none did.
fn fill_id_misses(size: usize, comps: usize, dense: &mut [f32], samples: &[Texel], details: &[Detail]) {
    if details.iter().all(|d| d.tri.is_some()) || details.iter().all(|d| d.tri.is_none()) {
        return;
    }
    let mut hit = vec![false; size * size];
    for (t, d) in samples.iter().zip(details) {
        if d.tri.is_some() { hit[t.index as usize] = true; }
    }
    let nearest = subcraft_image::dilate::nearest_covered(size, size, &hit);
    for (t, d) in samples.iter().zip(details) {
        let i = t.index as usize;
        let from = nearest[i];
        if d.tri.is_none() && from != u32::MAX {
            let from = from as usize;
            dense.copy_within(from * comps..(from + 1) * comps, i * comps);
        }
    }
}

/// Pad a map's gutters: texels within the dilation width copy the nearest island texel, the
/// rest is diffused from the padded islands or left at the neutral value already in `dense`.
fn pad_gutters(size: usize, comps: usize, dense: &mut [f32], covered: &[bool], nearest: &[u32], settings: &BakeSettings) {
    let reach = settings.dilation as f32;
    let known: Vec<bool> = (0..size * size).into_par_iter().map(|i| {
        if covered[i] { return true; }
        let s = nearest[i];
        if s == u32::MAX { return false; }
        let s = s as usize;
        let (dx, dy) = ((i % size) as f32 - (s % size) as f32, (i / size) as f32 - (s / size) as f32);
        dx * dx + dy * dy <= reach * reach
    }).collect();
    let src = dense.to_vec();
    dense.par_chunks_mut(comps).enumerate().for_each(|(i, dst)| {
        if known[i] && !covered[i] {
            let s = nearest[i] as usize;
            dst.copy_from_slice(&src[s * comps..(s + 1) * comps]);
        }
    });
    if settings.diffusion {
        subcraft_image::dilate::diffuse_fill(size, size, comps, dense, &known);
    }
}

/// Average `factor` x `factor` texels into one (supersampled bakes). Direction maps are
/// renormalized after averaging.
fn downsample(kind: MeshMapKind, image: &TiledImage, factor: u32) -> TiledImage {
    let comps = image.comps();
    let (w, h) = ((image.width() / factor) as usize, (image.height() / factor) as usize);
    let (big, f) = (image.width() as usize, factor as usize);
    let src = image.to_dense();
    let unit = comps >= 3 && matches!(kind, MeshMapKind::Normal | MeshMapKind::WorldSpaceNormal | MeshMapKind::BentNormal);
    let mut dense = vec![0.0f32; w * h * comps];
    dense.par_chunks_mut(w * comps).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for sy in 0..f {
                for sx in 0..f {
                    let i = ((y * f + sy) * big + x * f + sx) * comps;
                    for c in 0..comps {
                        acc[c] += src[i + c];
                    }
                }
            }
            for v in acc.iter_mut() {
                *v /= (f * f) as f32;
            }
            if unit {
                let v = (Vec3::new(acc[0], acc[1], acc[2]) * 2.0 - Vec3::ONE).normalize_or(Vec3::Z) * 0.5 + Vec3::splat(0.5);
                acc[..3].copy_from_slice(&v.to_array());
            }
            row[x * comps..(x + 1) * comps].copy_from_slice(&acc[..comps]);
        }
    });
    TiledImage::from_dense(w as u32, h as u32, comps, image.fill(), &dense)
}

/// Bake the requested maps for one texture set.
pub fn bake(input: &BakeInput, settings: &BakeSettings, progress: &Progress) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    bake_streamed(input, settings, progress, |_, _| {})
}

/// Publish each complete, padded map as soon as it is ready. The callback runs on the calling
/// thread; synchronous callers keep the same exact output through [`bake`]. No partial map is
/// published, including on cancellation. Cloning an image here only shares its immutable tiles.
pub fn bake_streamed(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress,
    completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let wants_rays = settings.maps.iter().any(|k| matches!(k, MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal | MeshMapKind::Thickness | MeshMapKind::Curvature));
    let backends = if settings.use_gpu && wants_rays { ray_backends() } else { Vec::new() };
    let gpus: Vec<&dyn RayBatchBackend> = backends.iter().map(|b| b.as_ref()).collect();
    bake_streamed_on(input, settings, progress, &gpus, completed)
}

/// [`bake_streamed`] with an explicit ray backend (`None` = CPU only).
pub fn bake_streamed_with(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress, gpu_backend: Option<&dyn RayBatchBackend>,
    completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    bake_streamed_on(input, settings, progress, gpu_backend.as_slice(), completed)
}

/// [`bake_streamed`] on explicit ray backends (empty = CPU only). The first one also projects
/// onto the high poly; with several, the ray maps (AO, bent normal, thickness, curvature) are
/// spread over them, one map per free GPU, while the CPU bakes the other maps (issue #731).
pub fn bake_streamed_on(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress, gpus: &[&dyn RayBatchBackend],
    mut completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let asked = settings.antialiasing.factor();
    let factor = supersampling(settings);
    if factor <= 1 {
        let maps = bake_core(input, settings, progress, gpus, &mut completed)?;
        if asked > 1 {
            progress.warn(format!("Antialiasing was skipped: a {} px bake cannot be supersampled within {MAX_SUPERSAMPLED_SIZE} px.", settings.size));
        }
        return Ok(maps);
    }
    // Bake everything `factor` times larger (distances given in texels scale with it), then average.
    let inner = BakeSettings {
        size: settings.size * factor,
        antialiasing: Antialiasing::None,
        dilation: settings.dilation * factor,
        normal_curvature: normal_curvature::NormalCurvatureSettings { radius_texels: settings.normal_curvature.radius_texels * factor as f32, ..settings.normal_curvature },
        ..settings.clone()
    };
    let mut out = BTreeMap::new();
    bake_core(input, &inner, progress, gpus, &mut |kind, image: &TiledImage| {
        let small = downsample(kind, image, factor);
        completed(kind, &small);
        out.insert(kind, small);
    })?;
    if factor < asked {
        progress.warn(format!("Antialiasing was reduced to {factor}x{factor}: a {} px bake cannot be supersampled further within {MAX_SUPERSAMPLED_SIZE} px.", settings.size));
    }
    Ok(out)
}

/// The supersampling factor a bake really uses: the asked one, halved until the larger bake fits
/// in [`MAX_SUPERSAMPLED_SIZE`].
pub fn supersampling(settings: &BakeSettings) -> u32 {
    let mut factor = settings.antialiasing.factor();
    while factor > 1 && settings.size.saturating_mul(factor) > MAX_SUPERSAMPLED_SIZE {
        factor /= 2;
    }
    factor
}

/// One bake at exactly `settings.size`, without supersampling.
fn bake_core(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress, gpus: &[&dyn RayBatchBackend],
    completed: &mut dyn FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let gpu_backend = gpus.first().copied();
    let size = settings.size;
    progress.current_map.store(0, Ordering::Relaxed);
    if let Ok(mut r) = progress.reports.lock() {
        r.clear();
    }
    if let Ok(mut w) = progress.warnings.lock() {
        w.clear();
    }
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let detail_geo = input.high.unwrap_or(input.low);
    let diag = low.bounds.diagonal().max(detail_geo.mesh.bounds.diagonal()).max(1e-6);
    let bias = diag * 2e-4;
    // By mesh name: rays from a low part only hit high triangles of its group.
    let matching = input.high.and_then(|high| PartMatch::new(low, high.mesh, &settings.name_match));
    let accepts = |group: u32, tri: u32| matching.as_ref().is_none_or(|m| m.accepts(group, tri));

    // Project onto the detail surface.
    let cage = input.high.map(|_| Cage::new(low, input.material_slot, size, settings, diag));
    // The texel's own surface: low on low, or a cage ray that reached no high-poly surface.
    let own = |t: &Texel, group: u32| Detail { position: t.position, normal: t.normal, tri: input.high.is_none().then_some(t.tri), offset: t.geometric_normal, height: 0.0, group };
    // The detail at a cage ray's closest high-poly hit (`tri`, distance, barycentrics u, v).
    let detail_at = |t: &Texel, group: u32, ray: &CageRay, hit: Option<(u32, f32, f32, f32)>| -> Detail {
        let (Some(high), Some((tri, dist, u, v))) = (input.high, hit) else { return own(t, group) };
        // Outwards, against the ray.
        let n = -ray.dir;
        let sp = high.mesh.interpolate(tri as usize, u, v);
        let ng = sp.geometric_normal;
        let normal = if sp.normal.dot(n) < 0.0 { -sp.normal } else { sp.normal };
        Detail { position: sp.position, normal, tri: Some(tri), offset: if ng.dot(n) < 0.0 { -ng } else { ng }, height: ray.front - dist, group }
    };
    let project = |t: &Texel| -> Detail {
        let group = matching.as_ref().map_or(NO_GROUP, |m| m.low_group(t.tri));
        let (Some(high), Some(cage)) = (input.high, &cage) else { return own(t, group) };
        let ray = cage.ray(low, t);
        let accept = |tri: u32| accepts(group, tri) && (!settings.ignore_backface || high.mesh.geometric_normal(tri as usize).dot(ray.dir) < 0.0);
        let hit = high.bvh.intersect_filtered(ray.origin, ray.dir, 0.0, ray.reach, accept).map(|h| (h.tri, h.t, h.u, h.v));
        detail_at(t, group, &ray, hit)
    };

    let wanted = |k: MeshMapKind| settings.maps.contains(&k);
    // Curvature from the normal map needs the normal map first (baked here even when it was not
    // asked for; it is then kept internal).
    let from_normal = settings.curvature_from_normal && wanted(MeshMapKind::Curvature);
    let mut order = settings.maps.clone();
    if from_normal {
        order.retain(|k| *k != MeshMapKind::Normal);
        let at = order.iter().position(|k| *k == MeshMapKind::Curvature).expect("curvature wanted");
        order.insert(at, MeshMapKind::Normal);
    }
    let work_items = order.len().max(1) * samples.len();
    progress.total.store(work_items, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);

    // Low on low: the detail is the texel itself, so it is derived on the fly instead of holding
    // a second 48-byte record per texel (800 MB at 4096) for the whole bake.
    // The projection runs on the GPU when it can (with name matching too: the kernel filters hits by part).
    let tri_group = matching.as_ref().map(|m| m.high_groups());
    let mut projected_on: Option<String> = None;
    let details: Option<Vec<Detail>> = match (input.high, &cage) {
        (Some(high), Some(cage)) => {
            let mut fast = None;
            if let Some(gpu) = gpu_backend.filter(|_| settings.use_gpu) {
                let group = |t: &Texel| matching.as_ref().map_or(NO_GROUP, |m| m.low_group(t.tri));
                let ray = |i: usize| {
                    let t = &samples[i];
                    let r = cage.ray(low, t);
                    ProjectionRay { origin: r.origin.to_array(), dir: r.dir.to_array(), reach: r.reach, group: group(t) }
                };
                let control = GpuControl::new(progress, 0);
                match gpu.project(high.bvh, tri_group, samples.len(), &ray, settings.ignore_backface, &control) {
                    Ok(hits) => {
                        projected_on = Some(gpu.description());
                        fast = Some(
                            samples
                                .par_iter()
                                .zip(hits.par_iter())
                                .map(|(t, h)| detail_at(t, group(t), &cage.ray(low, t), h.map(|h| (h.tri, h.t, h.u, h.v))))
                                .collect(),
                        );
                    }
                    Err(RayBatchError::Cancelled) => return Err(BakeError::Cancelled),
                    Err(RayBatchError::Unsupported(why)) => log::warn!("GPU high-poly projection failed, using the CPU: {why}"),
                }
            }
            Some(fast.unwrap_or_else(|| samples.par_iter().map(project).collect()))
        }
        _ => None,
    };
    if details.as_ref().is_some_and(|details| details.iter().all(|d| d.tri.is_none())) {
        return Err(BakeError::NoProjection);
    }
    if let Some(details) = &details {
        let missed = details.iter().filter(|d| d.tri.is_none()).count();
        if missed > 0 {
            progress.warn(format!(
                "Cage: {missed} of {} texels ({:.1}%) reached no high-poly surface and keep the low poly's own surface. Increase Cage front / rear or align the meshes.",
                details.len(), 100.0 * missed as f32 / details.len() as f32));
        }
    }
    let elements = if wanted(MeshMapKind::Id) || (wanted(MeshMapKind::Curvature) && !from_normal) { Some(topology::mesh_elements(detail_geo.mesh)) } else { None };
    // The ID source the mesh can actually serve: a file without the asked colours falls back to
    // generated ones, and says so.
    let id_source = match settings.id_source {
        // Every file reads its own set (issue #366); one file reads `vertex_color_set` as before.
        IdSource::VertexColor if wanted(MeshMapKind::Id) && (0..detail_geo.mesh.file_count()).all(|f| detail_geo.mesh.color_set(settings.vertex_color_set_of(f) as usize).is_none()) => {
            let mut asked: Vec<u32> = (0..detail_geo.mesh.file_count()).map(|f| settings.vertex_color_set_of(f) + 1).collect();
            asked.sort_unstable();
            asked.dedup();
            let asked = asked.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
            match detail_geo.mesh.color_set_count() {
                0 => progress.warn("ID map: the model has no vertex colours, so one colour per mesh element was used instead".into()),
                n => progress.warn(format!("ID map: the model has {n} vertex colour set(s), not set {asked}, so one colour per mesh element was used instead")),
            }
            IdSource::MeshId
        }
        IdSource::VertexColor if wanted(MeshMapKind::Id) && detail_geo.mesh.file_count() > 1 => {
            for f in 0..detail_geo.mesh.file_count() {
                let set = settings.vertex_color_set_of(f);
                if detail_geo.mesh.color_set(set as usize).is_none() {
                    progress.warn(format!("ID map: high-poly file {} has no vertex colour set {}, so its parts got one colour per mesh element instead", f + 1, set + 1));
                }
            }
            IdSource::VertexColor
        }
        IdSource::MaterialColor if wanted(MeshMapKind::Id) && detail_geo.mesh.material_colors.iter().all(Option::is_none) => {
            progress.warn("ID map: the model's materials have no colours, so one colour per object / material was used instead".into());
            IdSource::Submesh
        }
        IdSource::FileId if wanted(MeshMapKind::Id) && detail_geo.mesh.file_count() < 2 => {
            progress.warn("ID map: File ID needs several high-poly files, so the one file's colour was used everywhere".into());
            IdSource::FileId
        }
        source => source,
    };
    let bounds = low.bounds;
    let extent = bounds.extent().max(Vec3::splat(1e-6));

    // Every map shares the covered texels, so the nearest-texel search is done once.
    let n = (size * size) as usize;
    let mut covered = vec![false; n];
    for t in &samples {
        covered[t.index as usize] = true;
    }
    let nearest = subcraft_image::dilate::nearest_covered(size as usize, size as usize, &covered);

    // The nearest occluder of an occlusion ray between `min` and `max`: the detail mesh or, with
    // the ground plane on, the floor under the model.
    let ground = settings.ao.ground_plane.then(|| bounds.min.y.min(detail_geo.mesh.bounds.min.y) - settings.ao.ground_offset.max(0.0) * diag);
    let occluder = |origin: Vec3, dir: Vec3, min: f32, max: f32, group: u32| -> Option<f32> {
        let mut nearest = detail_geo.bvh.intersect_filtered(origin, dir, min, max, |tri| accepts(group, tri)).map(|h| h.t);
        if let Some(y) = ground && dir.y < -1e-6 {
            let t = (y - origin.y) / dir.y;
            if t > min && t < max {
                nearest = Some(nearest.map_or(t, |n| n.min(t)));
            }
        }
        nearest
    };

    // The GPU ray kernels filter hits by part (name matching) but do not know the occlusion or
    // thickness options (cone, distances, attenuation, ground plane).
    let gpu_suits = |kind: MeshMapKind| {
        let plain = match kind {
            MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal => settings.ao.is_plain(),
            MeshMapKind::Thickness => settings.thickness.is_plain(),
            _ => true,
        };
        settings.use_gpu && plain
    };
    // One map on one GPU: the values, or `None` when the GPU cannot do it (`backend` says why).
    let gpu_map = |gpu: &dyn RayBatchBackend, kind: MeshMapKind, gpu_details: &[Detail], backend: &mut String| -> Result<Option<Vec<[f32; 3]>>, BakeError> {
        if kind == MeshMapKind::Curvature {
            let radius = settings.curvature_radius * diag;
            Ok(gpu_curvature_map(gpu, detail_geo, elements.as_deref(), gpu_details, radius, progress, backend)?.map(|v| v.into_iter().map(|a| [a; 3]).collect()))
        } else {
            gpu_ray_map(gpu, kind, settings, detail_geo.bvh, tri_group, &samples, gpu_details, bias, diag, progress, backend)
        }
    };
    // Several GPUs (issue #731): the ray maps start at once, one map per free GPU (a faster card
    // simply takes the next one), while the loop below bakes the CPU maps; each GPU map is picked
    // up in map order. One GPU keeps the plain sequence.
    let ray_map = |k: MeshMapKind| matches!(k, MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal | MeshMapKind::Thickness) || (k == MeshMapKind::Curvature && !from_normal);
    let parallel: Vec<MeshMapKind> = if gpus.len() > 1 { order.iter().copied().filter(|&k| ray_map(k) && gpu_suits(k)).collect() } else { Vec::new() };
    let shared_details: Option<Vec<Detail>> = (!parallel.is_empty() && details.is_none()).then(|| samples.par_iter().map(project).collect());
    let jobs = GpuJobs::default();
    std::thread::scope(|scope| {
        for &gpu in gpus.iter().take(parallel.len()) {
            let (jobs, parallel, gpu_map) = (&jobs, &parallel, &gpu_map);
            let gpu_details = details.as_deref().or(shared_details.as_deref()).unwrap_or(&[]);
            scope.spawn(move || {
                while let Some(kind) = jobs.next(parallel) {
                    let started = Instant::now();
                    let mut label = "CPU".to_string();
                    // A panicking GPU job must not leave the bake waiting for its map: the CPU redoes it.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gpu_map(gpu, kind, gpu_details, &mut label))).unwrap_or_else(|_| {
                        label = "CPU (GPU failed)".to_string();
                        Ok(None)
                    });
                    jobs.done(kind, result, label, started.elapsed());
                }
            });
        }
        let result = (|| -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
            let mut out = BTreeMap::new();
            let mut internal_normal: Option<TiledImage> = None;
            for &kind in &order {
                progress.current_map.store(MeshMapKind::ALL.iter().position(|k| *k == kind).expect("known map") + 1, Ordering::Relaxed);
                if from_normal && kind == MeshMapKind::Curvature {
                    let started = Instant::now();
                    let normal = out.get(&MeshMapKind::Normal).or(internal_normal.as_ref()).expect("normal baked before curvature");
                    let mut image = normal_curvature::curvature_from_normal_map(low, input.material_slot, normal, size, &settings.normal_curvature, Some(progress))?;
                    if !settings.curvature_tonemap.is_identity() {
                        // Gutters copy island texels, so the stretch measured over the whole map
                        // follows the islands; the neutral fill beyond the islands stays flat.
                        let mut dense = image.to_dense();
                        settings.curvature_tonemap.apply(&mut dense);
                        image = TiledImage::from_dense(size, size, 1, [0.5, 0.0, 0.0, 0.0], &dense);
                    }
                    if let Ok(mut r) = progress.reports.lock() {
                        r.push(MapReport { kind, backend: "CPU (from the normal map)".into(), seconds: started.elapsed().as_secs_f32() });
                    }
                    completed(kind, &image);
                    out.insert(kind, image);
                    continue;
                }
                let comps = kind.components();
                // Per-texel evaluation, chunked for cancellation and progress.
                let eval = |i: usize| -> [f32; 3] {
                    let t = &samples[i];
                    let d = &details.as_ref().map_or_else(|| project(t), |details| details[i]);
                    match kind {
                        MeshMapKind::Normal => {
                            let n = t.normal.normalize_or(t.geometric_normal);
                            let tan = (t.tangent.truncate() - n * n.dot(t.tangent.truncate())).normalize_or(n.any_orthonormal_vector());
                            let bit = n.cross(tan) * if t.tangent.w < 0.0 { -1.0 } else { 1.0 };
                            let h = d.normal;
                            let ts = Vec3::new(h.dot(tan), h.dot(bit), h.dot(n)).normalize_or(Vec3::Z);
                            [ts.x * 0.5 + 0.5, ts.y * 0.5 + 0.5, ts.z * 0.5 + 0.5]
                        }
                        MeshMapKind::WorldSpaceNormal => {
                            let n = d.normal;
                            [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]
                        }
                        MeshMapKind::Position => {
                            let p = match settings.position.normalization {
                                PositionNormalization::BoundingBox => (t.position - bounds.min) / extent,
                                // The sphere around the box: one scale for every axis, 0.5 at the centre.
                                PositionNormalization::BoundingSphere => (t.position - (bounds.min + bounds.max) * 0.5) / extent.length().max(1e-6) + Vec3::splat(0.5),
                                PositionNormalization::None => t.position * settings.position.scale + Vec3::splat(0.5),
                            }
                            .clamp(Vec3::ZERO, Vec3::ONE);
                            match settings.position.single_axis {
                                Some(axis) => [p[axis.min(2) as usize]; 3],
                                None => [p.x, p.y, p.z],
                            }
                        }
                        MeshMapKind::Id => {
                            let Some(tri) = d.tri else { return kind.neutral_value()[..3].try_into().expect("RGB ID"); };
                            let mesh = detail_geo.mesh;
                            let tri = tri as usize;
                            let generated = |i: u32| settings.id_generator.color(i);
                            let slot = mesh.tri_material.get(tri).copied().unwrap_or(0);
                            match id_source {
                                IdSource::VertexColor => {
                                    let [a, b, c] = mesh.tri_positions(tri);
                                    let l = normal_curvature::barycentric(d.position, a, b, c);
                                    match mesh.corner_color_in(settings.vertex_color_set_of(mesh.file_of(tri)) as usize, tri, l[1], l[2]) {
                                        Some(c) => [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)],
                                        None => generated(elements.as_ref().map_or(0, |e| e[tri])),
                                    }
                                }
                                IdSource::MaterialColor => mesh.material_color(tri).unwrap_or_else(|| generated(slot)),
                                IdSource::MeshId => generated(elements.as_ref().map_or(0, |e| e[tri])),
                                // Objects are numbered after the material slots so the two never share a colour.
                                IdSource::Submesh => match mesh.tri_object.get(tri).copied().flatten() {
                                    Some(object) => generated(mesh.materials.len() as u32 + object),
                                    None => generated(slot),
                                },
                                IdSource::FileId => generated(mesh.file_of(tri)),
                            }
                        }
                        // Additional ID maps are baked as `Id` with their own settings (the caller
                        // renames the result); asked for directly they stay neutral.
                        MeshMapKind::Id2 | MeshMapKind::Id3 | MeshMapKind::Id4 | MeshMapKind::Id5
                        | MeshMapKind::Id6 | MeshMapKind::Id7 | MeshMapKind::Id8 | MeshMapKind::Id9 => kind.neutral_value(),
                        MeshMapKind::AmbientOcclusion if !settings.ao.is_plain() => {
                            let o = &settings.ao;
                            let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                            cone_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 1), o.spread_angle, o.distribution == RayDistribution::Uniform, &mut dirs);
                            let origin = d.position + d.offset * bias;
                            let (min, max) = (o.min_distance.max(0.0) * diag, settings.ao_distance * diag);
                            let (mut occluded, mut counted) = (0.0f32, 0u32);
                            for dir in &dirs {
                                if dir.dot(d.offset) <= 0.0 {
                                    continue;
                                }
                                counted += 1;
                                if let Some(hit) = occluder(origin, *dir, min, max, d.group) {
                                    occluded += o.attenuation.weight(hit, max);
                                }
                            }
                            let ao = if counted == 0 { 1.0 } else { 1.0 - occluded / counted as f32 };
                            [ao; 3]
                        }
                        MeshMapKind::AmbientOcclusion => {
                            let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                            cosine_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 1), &mut dirs);
                            let origin = d.position + d.offset * bias;
                            let dist = settings.ao_distance * diag;
                            let (mut occluded, mut counted) = (0u32, 0u32);
                            for dir in &dirs {
                                if dir.dot(d.offset) <= 0.0 {
                                    continue;
                                }
                                counted += 1;
                                if detail_geo.bvh.occluded_filtered(origin, *dir, 0.0, dist, |tri| accepts(d.group, tri)) {
                                    occluded += 1;
                                }
                            }
                            let ao = if counted == 0 { 1.0 } else { 1.0 - occluded as f32 / counted as f32 };
                            [ao; 3]
                        }
                        // Covered where the cage ray reached the high poly (low on low: everywhere).
                        MeshMapKind::Opacity => [if d.tri.is_some() { 1.0 } else { 0.0 }; 3],
                        MeshMapKind::Height => {
                            // Piecewise so the front and rear cage distances both span half the range.
                            let reach = match settings.height {
                                HeightNormalization::Cage => if d.height >= 0.0 { settings.cage_front } else { settings.cage_rear },
                                HeightNormalization::Manual { distance } => distance,
                            } * diag;
                            let h = 0.5 + 0.5 * (d.height / reach.max(1e-9)).clamp(-1.0, 1.0);
                            [h; 3]
                        }
                        MeshMapKind::BentNormal => {
                            let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                            let origin = d.position + d.offset * bias;
                            let dist = settings.ao_distance * diag;
                            let mut sum = Vec3::ZERO;
                            if settings.ao.is_plain() {
                                cosine_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 3), &mut dirs);
                                for dir in &dirs {
                                    if dir.dot(d.offset) > 0.0 && !detail_geo.bvh.occluded_filtered(origin, *dir, 0.0, dist, |tri| accepts(d.group, tri)) {
                                        sum += *dir;
                                    }
                                }
                            } else {
                                let o = &settings.ao;
                                cone_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 3), o.spread_angle, o.distribution == RayDistribution::Uniform, &mut dirs);
                                for dir in &dirs {
                                    if dir.dot(d.offset) > 0.0 && occluder(origin, *dir, o.min_distance.max(0.0) * diag, dist, d.group).is_none() {
                                        sum += *dir;
                                    }
                                }
                            }
                            bent_normal_texel(sum, t, d)
                        }
                        MeshMapKind::Thickness => {
                            let o = &settings.thickness;
                            let mut dirs = Vec::with_capacity(settings.thickness_samples as usize);
                            if o.is_plain() {
                                cosine_directions(-d.normal, settings.thickness_samples.max(1), hash01(t.index, 2), &mut dirs);
                            } else {
                                cone_directions(-d.normal, settings.thickness_samples.max(1), hash01(t.index, 2), o.spread_angle, o.distribution == RayDistribution::Uniform, &mut dirs);
                            }
                            let origin = d.position - d.offset * bias;
                            // Never zero: the average below divides by it (a zero distance gave a NaN map).
                            let max = (settings.thickness_distance * diag).max(1e-9);
                            // Surfaces nearer than the minimum distance are skipped.
                            let near = (o.min_distance.max(0.0) * diag).min(max);
                            let total: f32 = dirs.iter().map(|dir| detail_geo.bvh.intersect_filtered(origin, *dir, near, max, |tri| accepts(d.group, tri)).map_or(max, |h| h.t)).sum();
                            let avg = total / dirs.len() as f32 / max;
                            [1.0 - avg; 3]
                        }
                        MeshMapKind::Curvature => {
                            [curvature::curvature_on_surface(detail_geo.mesh, detail_geo.bvh, d.position, d.normal,
                                settings.curvature_radius * diag, d.tri.zip(elements.as_deref())); 3]
                        }
                    }
                };
                let mut started = Instant::now();
                let mut backend = match &projected_on {
                    Some(gpu) => format!("CPU, projection on {gpu}"),
                    None => "CPU".to_string(),
                };
                let backend_gpu = if gpu_suits(kind) { gpu_backend } else { None };
                let gpu = if parallel.contains(&kind) {
                    // Baked on one of the GPUs since the start: wait for it.
                    let (result, label, seconds) = jobs.wait(kind);
                    backend = label;
                    started = Instant::now().checked_sub(seconds).unwrap_or(started);
                    result?
                } else {
                    // The GPU kernels need every texel's projection; low-on-low bakes derive it only now.
                    let derived: Option<Vec<Detail>> = (backend_gpu.is_some() && details.is_none()).then(|| samples.par_iter().map(project).collect());
                    let gpu_details = details.as_deref().or(derived.as_deref()).unwrap_or(&[]);
                    match backend_gpu {
                        Some(gpu) => gpu_map(gpu, kind, gpu_details, &mut backend)?,
                        None => None,
                    }
                };
                let mut values: Vec<[f32; 3]> = match gpu {
                    Some(v) => v,
                    None => {
                        let mut values: Vec<[f32; 3]> = Vec::with_capacity(samples.len());
                        for start in (0..samples.len()).step_by(CHUNK) {
                            if progress.is_cancelled() {
                                return Err(BakeError::Cancelled);
                            }
                            let end = (start + CHUNK).min(samples.len());
                            values.par_extend((start..end).into_par_iter().map(eval));
                            progress.done.fetch_add(end - start, Ordering::Relaxed);
                        }
                        values
                    }
                };
                if kind == MeshMapKind::Curvature && !settings.curvature_tonemap.is_identity() {
                    // Measured over this bake's covered texels only (one texture set or UDIM tile).
                    let mut raw: Vec<f32> = values.iter().map(|v| v[0]).collect();
                    settings.curvature_tonemap.apply(&mut raw);
                    for (v, r) in values.iter_mut().zip(raw) {
                        *v = [r; 3];
                    }
                }
                let neutral = kind.neutral_value();
                let mut dense: Vec<f32> = neutral[..comps].iter().copied().cycle().take(n * comps).collect();
                for (t, v) in samples.iter().zip(&values) {
                    let i = t.index as usize;
                    dense[i * comps..(i + 1) * comps].copy_from_slice(&v[..comps]);
                }
                if kind == MeshMapKind::Id && settings.id_fill_misses && let Some(details) = &details {
                    // A texel whose projection ray reached no high-poly surface has no ID (issue #466: the
                    // low poly's rim texels next to open edges of the high poly). Writing the empty colour
                    // there put black texels inside islands, which dilation then spread into lines: they
                    // take the ID of the nearest texel that has one instead.
                    fill_id_misses(size as usize, comps, &mut dense, &samples, details);
                }
                if matches!(kind, MeshMapKind::AmbientOcclusion | MeshMapKind::Thickness) {
                    // Ray maps are a finite-sample estimate with a per-texel rotation: remove the
                    // grain and the stair-steps of the few discrete levels.
                    smooth_covered(size as usize, comps, &mut dense, &covered);
                }
                pad_gutters(size as usize, comps, &mut dense, &covered, &nearest, settings);
                let mut fill = [0.0f32; 4];
                fill[..comps].copy_from_slice(&neutral[..comps]);
                let image = TiledImage::from_dense(size, size, comps, fill, &dense);
                if progress.is_cancelled() { return Err(BakeError::Cancelled); }
                if !wanted(kind) {
                    internal_normal = Some(image);
                    continue;
                }
                if let Ok(mut r) = progress.reports.lock() {
                    r.push(MapReport { kind, backend, seconds: started.elapsed().as_secs_f32() });
                }
                completed(kind, &image);
                out.insert(kind, image);
            }
            progress.current_map.store(0, Ordering::Relaxed);
            Ok(out)
        })();
        // On an error the GPUs start no further map.
        jobs.stop();
        result
    })
}

type GpuResult = (Result<Option<Vec<[f32; 3]>>, BakeError>, String, Duration);

/// The ray maps shared out between several GPUs: a queue of maps and their results.
#[derive(Default)]
struct GpuJobs {
    next: AtomicUsize,
    stopped: AtomicBool,
    results: Mutex<BTreeMap<MeshMapKind, GpuResult>>,
    ready: std::sync::Condvar,
}

impl GpuJobs {
    /// The next map for a free GPU, or `None` when all are taken or the bake stopped.
    fn next(&self, maps: &[MeshMapKind]) -> Option<MeshMapKind> {
        if self.stopped.load(Ordering::Relaxed) {
            return None;
        }
        maps.get(self.next.fetch_add(1, Ordering::Relaxed)).copied()
    }

    fn done(&self, kind: MeshMapKind, result: Result<Option<Vec<[f32; 3]>>, BakeError>, label: String, seconds: Duration) {
        self.results.lock().expect("bake results").insert(kind, (result, label, seconds));
        self.ready.notify_all();
    }

    /// The result of `kind`, waiting for its GPU to finish.
    fn wait(&self, kind: MeshMapKind) -> GpuResult {
        let mut results = self.results.lock().expect("bake results");
        loop {
            if let Some(r) = results.remove(&kind) {
                return r;
            }
            results = self.ready.wait(results).expect("bake results");
        }
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Where the high-poly projection fails, per texel of one texture set: what the Bake window's
/// ray-miss heatmap shows before baking.
#[derive(Clone, Debug, PartialEq)]
pub struct CageMisses {
    /// One component: 1 = the ray from the cage reached no high-poly surface, 0 = it did (or the
    /// texel is not covered). Gutters copy the nearest covered texel, so filtering stays clean.
    pub image: TiledImage,
    pub covered: usize,
    pub missed: usize,
}

/// Cast exactly the projection rays the baker casts (same cage, diagonal, normals, backface
/// rule and name matching) at `size`² and record which miss the high poly.
pub fn cage_misses(input: &BakeInput, settings: &BakeSettings, size: u32) -> Result<CageMisses, BakeError> {
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let n = (size * size) as usize;
    let mut covered = vec![false; n];
    for t in &samples {
        covered[t.index as usize] = true;
    }
    let mut dense = vec![0.0f32; n];
    let mut missed = 0;
    if let Some(high) = input.high {
        let diag = low.bounds.diagonal().max(high.mesh.bounds.diagonal()).max(1e-6);
        let cage = Cage::new(low, input.material_slot, size, settings, diag);
        let matching = PartMatch::new(low, high.mesh, &settings.name_match);
        let miss: Vec<bool> = samples.par_iter().map(|t| {
            let group = matching.as_ref().map_or(NO_GROUP, |m| m.low_group(t.tri));
            let ray = cage.ray(low, t);
            high.bvh.intersect_filtered(ray.origin, ray.dir, 0.0, ray.reach, |tri| {
                matching.as_ref().is_none_or(|m| m.accepts(group, tri)) && (!settings.ignore_backface || high.mesh.geometric_normal(tri as usize).dot(ray.dir) < 0.0)
            }).is_none()
        }).collect();
        for (t, m) in samples.iter().zip(miss) {
            if m {
                dense[t.index as usize] = 1.0;
                missed += 1;
            }
        }
    }
    let nearest = subcraft_image::dilate::nearest_covered(size as usize, size as usize, &covered);
    subcraft_image::dilate::apply_nearest(1, &mut dense, &covered, &nearest);
    Ok(CageMisses { image: TiledImage::from_dense(size, size, 1, [0.0; 4], &dense), covered: samples.len(), missed })
}

/// 3 x 3 binomial blur over covered texels only (uncovered neighbours do not contribute).
pub(crate) fn smooth_covered(size: usize, comps: usize, dense: &mut [f32], covered: &[bool]) {
    let src = dense.to_vec();
    const W: [f32; 3] = [1.0, 2.0, 1.0];
    for y in 0..size {
        for x in 0..size {
            if !covered[y * size + x] { continue; }
            let mut acc = [0.0f32; 4];
            let mut wsum = 0.0;
            for (dy, wy) in W.iter().enumerate() {
                let yy = y as isize + dy as isize - 1;
                if yy < 0 || yy >= size as isize { continue; }
                for (dx, wx) in W.iter().enumerate() {
                    let xx = x as isize + dx as isize - 1;
                    if xx < 0 || xx >= size as isize { continue; }
                    let j = yy as usize * size + xx as usize;
                    if !covered[j] { continue; }
                    let w = wy * wx;
                    wsum += w;
                    for c in 0..comps { acc[c] += src[j * comps + c] * w; }
                }
            }
            for c in 0..comps { dense[(y * size + x) * comps + c] = acc[c] / wsum; }
        }
    }
}

struct GpuControl<'a> {
    progress: &'a Progress,
    count: usize,
    /// Work units this batch has added to `progress.done` so far. Batches on several GPUs run at
    /// the same time, so each adds and takes back only its own share.
    reported: AtomicUsize,
}

impl<'a> GpuControl<'a> {
    fn new(progress: &'a Progress, count: usize) -> Self {
        Self { progress, count, reported: AtomicUsize::new(0) }
    }

    fn report(&self, units: usize) {
        let before = self.reported.swap(units, Ordering::Relaxed);
        if units >= before {
            self.progress.done.fetch_add(units - before, Ordering::Relaxed);
        } else {
            self.progress.done.fetch_sub(before - units, Ordering::Relaxed);
        }
    }

    /// The whole batch is done.
    fn finish(&self) {
        self.report(self.count);
    }

    /// The batch failed: its share goes back (the CPU redoes it).
    fn undo(&self) {
        self.report(0);
    }
}

impl BatchControl for GpuControl<'_> {
    fn is_cancelled(&self) -> bool {
        self.progress.is_cancelled()
    }
    fn progress(&self, fraction: f32) {
        self.report((fraction.clamp(0.0, 1.0) * self.count as f32) as usize);
    }
}

/// Curvature per sample from the backend, or `None` for the CPU.
fn gpu_curvature_map(
    gpu: &dyn RayBatchBackend, geo: Geometry, elements: Option<&[u32]>, details: &[Detail], radius: f32, progress: &Progress, backend: &mut String,
) -> Result<Option<Vec<f32>>, BakeError> {
    let Some(elements) = elements else { return Ok(None) };
    let mesh = geo.mesh;
    // Triangles of different connected elements or objects are "foreign" to each other.
    let mut ids: std::collections::HashMap<(u32, Option<u32>), u32> = std::collections::HashMap::new();
    let tri_group: Vec<u32> = (0..mesh.triangle_count())
        .map(|t| {
            let next = ids.len() as u32;
            *ids.entry((elements[t], mesh.tri_object.get(t).copied().flatten())).or_insert(next)
        })
        .collect();
    let tri_normals: Vec<[f32; 12]> = (0..mesh.triangle_count())
        .map(|t| {
            let [a, b, c] = mesh.tri(t).map(|i| mesh.normals[i as usize]);
            [a.x, a.y, a.z, 0.0, b.x, b.y, b.z, 0.0, c.x, c.y, c.z, 0.0]
        })
        .collect();
    let texel = |i: usize| {
        let d = &details[i];
        CurvatureTexel { position: d.position.to_array(), normal: d.normal.to_array(), group: d.tri.map_or(NO_GROUP, |t| tri_group[t as usize]) }
    };
    let control = GpuControl::new(progress, details.len());
    let surface = CurvatureSurface { tri_normals: &tri_normals, tri_group: &tri_group };
    match gpu.curvature(geo.bvh, surface, details.len(), &texel, radius, &control) {
        Ok(values) => {
            control.finish();
            *backend = gpu.description();
            Ok(Some(values))
        }
        Err(RayBatchError::Cancelled) => Err(BakeError::Cancelled),
        Err(RayBatchError::Unsupported(why)) => {
            log::warn!("GPU curvature bake failed, using the CPU: {why}");
            control.undo();
            *backend = format!("CPU (GPU failed: {why})");
            Ok(None)
        }
    }
}

/// AO, thickness and bent normal values per sample from the registered ray backend, or `None` to
/// use the CPU (not a ray map, no backend, or the backend refused: `backend` then says why).
#[allow(clippy::too_many_arguments)]
fn gpu_ray_map(
    gpu: &dyn RayBatchBackend, kind: MeshMapKind, settings: &BakeSettings, bvh: &Bvh, tri_group: Option<&[u32]>, samples: &[Texel], details: &[Detail], bias: f32, diag: f32,
    progress: &Progress, backend: &mut String,
) -> Result<Option<Vec<[f32; 3]>>, BakeError> {
    let (rays, mode, salt, sign) = match kind {
        MeshMapKind::AmbientOcclusion => (settings.ao_samples.max(1), RayMode::Occlusion { distance: settings.ao_distance * diag }, 1, 1.0),
        // The same fan as AO with its own seed, as the CPU `eval` casts it.
        MeshMapKind::BentNormal => (settings.ao_samples.max(1), RayMode::BentNormal { distance: settings.ao_distance * diag }, 3, 1.0),
        MeshMapKind::Thickness => (settings.thickness_samples.max(1), RayMode::MeanDistance { max: (settings.thickness_distance * diag).max(1e-9) }, 2, -1.0),
        _ => return Ok(None),
    };
    let texel = |i: usize| {
        let (t, d) = (&samples[i], &details[i]);
        // Same origins, axes and seeds as the CPU `eval`: occlusion starts above the surface,
        // thickness just below it, looking inward.
        let origin = d.position + d.offset * (bias * sign);
        RayTexel { origin: origin.to_array(), rotation: hash01(t.index, salt), axis: (d.normal * sign).to_array(), offset: d.offset.to_array(), group: d.group }
    };
    let control = GpuControl::new(progress, samples.len());
    match gpu.run(bvh, tri_group, samples.len(), &texel, rays, mode, &control) {
        Ok(values) => {
            control.finish();
            *backend = gpu.description();
            Ok(Some(match mode {
                RayMode::BentNormal { .. } => values
                    .chunks_exact(3)
                    .zip(samples.iter().zip(details))
                    .map(|(sum, (t, d))| bent_normal_texel(Vec3::new(sum[0], sum[1], sum[2]), t, d))
                    .collect(),
                _ => values.into_iter().map(|a| [a; 3]).collect(),
            }))
        }
        Err(RayBatchError::Cancelled) => Err(BakeError::Cancelled),
        Err(RayBatchError::Unsupported(why)) => {
            log::warn!("GPU {} bake failed, using the CPU: {why}", kind.display_name());
            control.undo();
            *backend = format!("CPU (GPU failed: {why})");
            Ok(None)
        }
    }
}

/// A bent normal texel from the sum of its open ray directions, in the texel's tangent space
/// (encoded 0..1). Fully occluded texels keep the surface normal.
fn bent_normal_texel(sum: Vec3, t: &Texel, d: &Detail) -> [f32; 3] {
    let n = t.normal.normalize_or(t.geometric_normal);
    let tan = (t.tangent.truncate() - n * n.dot(t.tangent.truncate())).normalize_or(n.any_orthonormal_vector());
    let bit = n.cross(tan) * if t.tangent.w < 0.0 { -1.0 } else { 1.0 };
    let bent = sum.try_normalize().unwrap_or(d.normal);
    let ts = Vec3::new(bent.dot(tan), bent.dot(bit), bent.dot(n)).normalize_or(Vec3::Z);
    [ts.x * 0.5 + 0.5, ts.y * 0.5 + 0.5, ts.z * 0.5 + 0.5]
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod match_tests;
#[cfg(test)]
mod placement_tests;
#[cfg(test)]
mod skew_tests;
#[cfg(test)]
mod opacity_tests;
#[cfg(test)]
mod paint_map_tests;
