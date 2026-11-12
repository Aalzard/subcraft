//! A paint stroke: path → stamps → coverage → texel writes into the live layer.
//!
//! Straight lines, lazy mouse and symmetry all feed the same pipeline: screen samples are
//! resampled at the brush spacing, jittered, ray-cast through the view camera (and through each
//! symmetry-mirrored camera), and the resulting stamps are applied by the tile kernel.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use rayon::prelude::*;
use subcraft_core::LayerId;
use subcraft_doc::{Projection, ProjectionMode, ResourceStore, TextureSet};
use subcraft_image::{TILE_TEXELS, TILE_SIZE, TiledImage, Wrap};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, Mesh};

use crate::camera::Camera;
use crate::depth::{DepthCache, DepthMap};
use crate::kernel::{KernelInput, TexelSample, TileWork};
use crate::path::{PathSample, ScreenPath, rand01};
use crate::settings::{Alignment, BrushMaterial, BrushSettings, SizeContext, SizeSpace, ImageStamp, StampKind, PaintTarget, PolygonFillMode, StrokeMode, StrokeOptions};
use crate::stamp::{Dab, Stamp, resolve_overlap};
use crate::target::{ImageKey, PaintError, TargetImage, begin_targets, store};

/// Scene inputs a stroke reads.
#[derive(Clone, Copy)]
pub struct PaintContext<'a> {
    pub mesh: &'a Mesh,
    pub bvh: &'a Bvh,
    /// Geometry of the texture set being painted (slot + resolution).
    pub geometry: &'a TextureSetGeometry,
    pub camera: &'a Camera,
    pub depth: &'a DepthCache,
}

pub struct Stroke {
    layer: LayerId,
    mode: StrokeMode,
    settings: BrushSettings,
    options: StrokeOptions,
    images: Vec<TargetImage>,
    coverage: HashMap<usize, Box<[f32]>>,
    path: ScreenPath,
    stamp_count: u32,
    /// Pen pressure for the stamps placed next (1 for a mouse).
    pressure: f32,
    last_depth: Option<f32>,
    alpha: Option<Arc<subcraft_image::Bitmap>>,
    stencil: Option<Arc<crate::stencil::Stencil>>,
    parts: Option<subcraft_doc::PartMask>,
    projection: Projection,
    texture_tiling: bool,
    albedo_filter: subcraft_image::TextureFilter,
    resources: ResourceStore,
    camera_projection: bool,
    /// The view the stroke started in: projects images and anchors the clone offset.
    view_camera: Option<Camera>,
    clone: Option<CloneSource>,
    /// Smudge: live copies of the target images (the snapshots stay pre-stroke) and the paint
    /// carried along (premultiplied value, alpha) per stroke copy and image.
    work: Vec<TiledImage>,
    carried: Vec<Option<[f32; 4]>>,
    /// Average UV units per object unit of the painted texture set (2D-view strokes).
    uv_per_unit: Option<f32>,
    raster: RasterCache,
    preview: LivePreview,
    /// Normal-map / height stamp brush and its image.
    stamp: Option<(ImageStamp, Arc<subcraft_image::Bitmap>)>,
    /// Per tile and texel `[sum weight*v0, sum weight*v1, sum weight]` of the stamps laid so far.
    stamp_acc: HashMap<usize, Box<[[f32; 3]]>>,
    /// Tiled stamp mode: UV origin and the UV size of one image repeat, fixed by the first stamp.
    tile_ref: Option<(Vec2, f32)>,
}

/// Live preview of expensive strokes: while the brush touches more than `tiles` tiles per stamp,
/// only every `stride`-th stamp is applied to the layer as the pen moves; the others wait in
/// `deferred` and are applied in one batched pass when the stroke ends. Coverage accumulates as
/// `1 - prod(1 - a_i)`, which does not depend on stamp order, so the final texels equal the exact
/// stroke up to float rounding.
#[derive(Clone)]
struct LivePreview {
    /// Tiles per stamp above which stamps are thinned out; 0 disables the preview.
    tiles: usize,
    /// Tiles the last applied batch touched (the cost estimate).
    last_tiles: usize,
    deferred: Vec<Stamp>,
}

/// At most this many stamps are skipped between two applied ones.
const PREVIEW_MAX_STRIDE: usize = 3;

/// The visible texel samples of every tile the stroke touched, per camera: later stamps on the
/// same tile skip rasterizing and the visibility test (a pen stroke stamps the same tiles again
/// and again). Dropped when the cameras, mesh or visibility settings change, and when it grows
/// past [`RASTER_CACHE_SAMPLES`].
#[derive(Clone)]
struct RasterCache {
    enabled: bool,
    key: Option<RasterKey>,
    tiles: HashMap<(usize, usize), Arc<[TexelSample]>>,
    samples: usize,
}

type RasterKey = (Vec<Camera>, Alignment, Option<u32>, u32, u32, usize);

/// About 120 MB of cached samples.
const RASTER_CACHE_SAMPLES: usize = 3_000_000;

/// Where a clone stroke copies from: an object-space surface point, and the screen offset from
/// the stroke's own position to it (fixed by the first dab, then kept so later strokes align).
#[derive(Clone, Copy, Debug, PartialEq)]
struct CloneSource {
    point: Vec3,
    offset: Option<Vec2>,
}

/// Per-tile texel remapping for the image sources (clone / camera projection).
enum Remap {
    None,
    /// Clone: the UV of the surface point each covered texel copies from.
    Clone(Vec<Option<[f32; 2]>>),
    /// Camera projection: image coordinates of each covered texel as seen from the stroke's view.
    Camera(Vec<Option<[f32; 2]>>),
}

impl Stroke {
    /// Start a stroke on `layer`, snapshotting the target images (O(1) copy-on-write clones).
    pub fn begin(
        ts: &TextureSet,
        layer: LayerId,
        target: PaintTarget,
        mode: StrokeMode,
        settings: BrushSettings,
        options: StrokeOptions,
        material: &BrushMaterial,
    ) -> Result<Self, PaintError> {
        let images = begin_targets(ts, layer, target, material)?;
        Ok(Self {
            layer,
            mode,
            settings,
            options,
            images,
            coverage: HashMap::new(),
            path: ScreenPath::new(),
            stamp_count: 0,
            pressure: 1.0,
            last_depth: None,
            alpha: None,
            stencil: None,
            parts: ts.find(layer).and_then(|l| l.parts.clone()),
            projection: material.projection,
            texture_tiling: material.texture_tiling,
            albedo_filter: ts.albedo_filter,
            resources: ResourceStore::default(),
            camera_projection: material.camera_projection,
            view_camera: None,
            clone: None,
            work: Vec::new(),
            carried: Vec::new(),
            uv_per_unit: None,
            raster: RasterCache { enabled: std::env::var_os("SUBCRAFT_NO_RASTER_CACHE").is_none(), key: None, tiles: HashMap::new(), samples: 0 },
            preview: LivePreview { tiles: if std::env::var_os("SUBCRAFT_NO_STROKE_PREVIEW").is_some() { 0 } else { 32 }, last_tiles: 0, deferred: Vec::new() },
            stamp: None,
            stamp_acc: HashMap::new(),
            tile_ref: None,
        })
    }

    /// The original stamping path: rasterize and visibility-test every touched tile again for each
    /// stamp and rewrite all of them (used to
    /// check that the per-stroke sample cache and the unchanged-tile skip change nothing).
    pub fn without_raster_cache(mut self) -> Self {
        self.raster.enabled = false;
        self
    }

    /// Thin out the stamps applied while drawing once a stamp touches more than `tiles` tiles
    /// (`0`: never); the skipped ones are applied by [`Stroke::flush_deferred`].
    pub fn with_live_preview(mut self, tiles: usize) -> Self {
        self.preview.tiles = tiles;
        self
    }

    /// Apply the stamps the live preview skipped (call at the end of the stroke, after `finish`).
    pub fn flush_deferred(&mut self, ts: &mut TextureSet, ctx: &PaintContext) -> Result<Vec<usize>, PaintError> {
        if self.preview.deferred.is_empty() {
            return Ok(Vec::new());
        }
        self.check(ts, ctx)?;
        let stamps = std::mem::take(&mut self.preview.deferred);
        let cameras = self.cameras(ctx);
        let alignment = self.settings.alignment;
        self.apply_stamps(ts, ctx, &cameras, &stamps, alignment, true)
    }

    /// Clone from `point` (object space). `offset` is the screen offset kept from an earlier
    /// stroke; `None` lets the first dab define it.
    pub fn with_clone(mut self, point: Vec3, offset: Option<Vec2>) -> Self {
        self.clone = Some(CloneSource { point, offset });
        self
    }

    /// The clone offset in screen pixels, once the first dab has fixed it.
    pub fn clone_offset(&self) -> Option<Vec2> {
        self.clone.and_then(|c| c.offset)
    }

    /// Use a brush alpha (tip image: white paints) instead of the round procedural tip.
    pub fn with_alpha(mut self, alpha: Option<Arc<subcraft_image::Bitmap>>) -> Self {
        self.alpha = alpha;
        self
    }

    /// Paint with an image stamp (normal map / height): only paint strokes on the stamp's channel
    /// use it. Needs the decoded image; `None` leaves an ordinary brush.
    pub fn with_stamp(mut self, stamp: Option<ImageStamp>, image: Option<Arc<subcraft_image::Bitmap>>) -> Self {
        if let (Some(stamp), Some(image)) = (stamp, image)
            && self.mode == StrokeMode::Paint
        {
            let channel = stamp.kind.channel();
            let mut any = false;
            for img in self.images.iter_mut().filter(|i| i.key.channel() == Some(channel)) {
                img.stamp = Some(stamp.kind);
                any = true;
            }
            if any { self.stamp = Some((stamp, image)); }
        }
        self
    }

    /// Capture immutable image sources at stroke start. A missing source paints nothing.
    pub fn with_resources(mut self, resources: &ResourceStore) -> Self {
        for img in &self.images {
            if let Some(resource) = img.texture.and_then(|id| resources.get(id)) {
                self.resources.insert_shared(resource.clone());
            }
        }
        self
    }

    /// Mask the stroke with a screen-space stencil (white paints, black blocks).
    pub fn with_stencil(mut self, stencil: Option<Arc<crate::stencil::Stencil>>) -> Self {
        self.stencil = stencil;
        self
    }

    /// Stamps emitted so far (per view; symmetry mirrors share the index).
    pub fn stamp_count(&self) -> u32 {
        self.stamp_count
    }

    /// Stroke coverage of a tile (0..1 per texel), if the stroke touched it.
    pub fn coverage(&self, tile: usize) -> Option<&[f32]> {
        self.coverage.get(&tile).map(|c| &c[..])
    }

    /// Pen pressure (0..1) applied to the stamps of the following points.
    pub fn set_pressure(&mut self, pressure: f32) {
        self.pressure = pressure.clamp(0.0, 1.0);
    }

    /// Feed a cursor position; returns the tiles rewritten in `ts`.
    pub fn add_point(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cursor: Vec2) -> Result<Vec<usize>, PaintError> {
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        self.path.push(cursor, spacing, self.options.lazy_distance_px, self.options.smoothing, &mut samples);
        self.paint_samples(ts, ctx, &samples)
    }

    /// Feed many cursor positions at once (a recorded stroke), painted in one pass instead of one
    /// pass per position. With an orthographic camera the stamps are those of calling
    /// [`Stroke::add_point`] for each position; with a perspective one the stamp spacing uses the
    /// depth known at the start of the batch.
    pub fn add_points(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cursors: &[Vec2]) -> Result<Vec<usize>, PaintError> {
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        for &cursor in cursors {
            self.path.push(cursor, spacing, self.options.lazy_distance_px, self.options.smoothing, &mut samples);
        }
        self.paint_samples(ts, ctx, &samples)
    }

    /// End of stroke: with `catch_up` the brush glides to the last cursor position.
    pub fn finish(&mut self, ts: &mut TextureSet, ctx: &PaintContext) -> Result<Vec<usize>, PaintError> {
        if !self.options.catch_up {
            return Ok(Vec::new());
        }
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        self.path.finish(spacing, &mut samples);
        self.paint_samples(ts, ctx, &samples)
    }

    /// Straight segment from the brush position to `to` (the first call just places the brush).
    pub fn line_to(&mut self, ts: &mut TextureSet, ctx: &PaintContext, to: Vec2) -> Result<Vec<usize>, PaintError> {
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        self.path.line_to(to, spacing, &mut samples);
        self.paint_samples(ts, ctx, &samples)
    }

    /// Feed a 2D-view cursor position in texture UV (0..1, origin top-left; `line`: straight
    /// segment). Stamps are UV discs ([`Alignment::Uv`]) with no 3D visibility test, so
    /// hidden and back faces are reachable. Symmetry and stencils belong to the 3D view and do
    /// not apply. `texels_per_px` converts the lazy-mouse distance from view pixels to texels.
    pub fn add_uv_point(&mut self, ts: &mut TextureSet, ctx: &PaintContext, uv: Vec2, line: bool, texels_per_px: f32) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        let res = ctx.geometry.resolution as f32;
        let radius_uv = self.uv_radius(ctx);
        let spacing = (self.settings.spacing * 2.0 * radius_uv * res).max(0.5);
        let mut samples = Vec::new();
        if line {
            self.path.line_to(uv * res, spacing, &mut samples);
        } else {
            self.path.push(uv * res, spacing, self.options.lazy_distance_px * texels_per_px.max(0.0), self.options.smoothing, &mut samples);
        }
        self.paint_uv_samples(ts, ctx, &samples, radius_uv)
    }

    /// Brush radius in UV units: the 3D brush size at the texture set's average texel density,
    /// so a stroke covers the same texels in both views.
    /// Texture size space is exact here; Viewport has no meaning in the 2D view (its zoom is
    /// not a camera) and uses the object size.
    fn uv_radius(&mut self, ctx: &PaintContext) -> f32 {
        let upu = *self.uv_per_unit.get_or_insert_with(|| uv_per_unit(ctx.mesh, ctx.geometry.material_slot));
        let c = SizeContext { diagonal: ctx.mesh.bounds.diagonal(), pixels_per_unit: 0.0, viewport_height: 0.0, uv_per_unit: upu };
        self.settings.radius_in(&c) * upu
    }

    /// World radius of the brush in its size space, under the current view.
    fn base_radius(&mut self, ctx: &PaintContext) -> f32 {
        let s = self.settings;
        let diagonal = ctx.mesh.bounds.diagonal();
        let c = match s.size_space {
            SizeSpace::Object => return s.radius(diagonal),
            SizeSpace::Viewport => SizeContext { diagonal, pixels_per_unit: ctx.camera.pixels_per_unit(self.depth_hint(ctx)), viewport_height: ctx.camera.viewport[1] as f32, uv_per_unit: 0.0 },
            SizeSpace::Texture => SizeContext { diagonal, pixels_per_unit: 0.0, viewport_height: 0.0,
                uv_per_unit: *self.uv_per_unit.get_or_insert_with(|| uv_per_unit(ctx.mesh, ctx.geometry.material_slot)) },
        };
        s.radius_in(&c)
    }

    fn paint_uv_samples(&mut self, ts: &mut TextureSet, ctx: &PaintContext, samples: &[PathSample], radius_uv: f32) -> Result<Vec<usize>, PaintError> {
        if samples.is_empty() || radius_uv <= 0.0 || !radius_uv.is_finite() {
            return Ok(Vec::new());
        }
        let s = self.settings;
        let res = ctx.geometry.resolution as f32;
        // The world radius that gives `radius_uv` at the set's average texel density.
        let upu = self.uv_per_unit.unwrap_or(0.0);
        let radius = if upu > 0.0 { radius_uv / upu } else { s.radius(ctx.mesh.bounds.diagonal()) };
        let [size_jitter, flow_jitter, angle_jitter, position_jitter] = s.jitters(self.pressure);
        let mut stamps = Vec::with_capacity(samples.len());
        for sample in samples {
            let k = self.stamp_count;
            self.stamp_count += 1;
            let scale = 1.0 - size_jitter * rand01(s.seed, k, 0);
            let flow = s.flow.clamp(0.0, 1.0) * (1.0 - flow_jitter * rand01(s.seed, k, 1));
            let mut angle = (s.angle_deg + angle_jitter * (2.0 * rand01(s.seed, k, 2) - 1.0)).to_radians();
            if s.follow_path {
                angle += sample.dir.y.atan2(sample.dir.x);
            }
            let mut pos = sample.pos;
            if position_jitter > 0.0 {
                let (a, d) = (rand01(s.seed, k, 3) * std::f32::consts::TAU, rand01(s.seed, k, 4).sqrt());
                pos += Vec2::new(a.cos(), a.sin()) * d * position_jitter * 2.0 * radius_uv * scale * res;
            }
            stamps.push(Stamp {
                camera: 0,
                screen: pos,
                center: Vec3::ZERO,
                normal: Vec3::Z,
                uv: pos / res,
                tri: 0,
                radius: radius * scale,
                radius_px: 0.0,
                radius_uv: radius_uv * scale,
                flow,
                angle,
                roundness: s.roundness,
                hardness: s.hardness,
                tangent: Vec3::X,
                bitangent: Vec3::Y,
            });
        }
        self.apply_stamps(ts, ctx, std::slice::from_ref(ctx.camera), &stamps, Alignment::Uv, false)
    }

    fn check(&self, ts: &TextureSet, ctx: &PaintContext) -> Result<(), PaintError> {
        if ctx.geometry.resolution != ts.resolution || ts.material_slot != ctx.geometry.material_slot {
            return Err(PaintError::ResolutionMismatch);
        }
        Ok(())
    }

    /// View camera, its radial copies, then every symmetry mirror of those (all combinations of
    /// enabled planes). Without radial symmetry the list is exactly the mirror list (ADR 0043).
    fn cameras(&self, ctx: &PaintContext) -> Vec<Camera> {
        let center = ctx.mesh.bounds.center();
        let mut cams = vec![*ctx.camera];
        let mirrors = self.options.symmetry.iter().filter(|&&m| m).count();
        let radial = &self.options.radial;
        let n = radial.effective_count(mirrors);
        if n > 1 {
            let pivot = center + Vec3::from_array(radial.center_offset);
            for k in 1..n {
                let angle = std::f32::consts::TAU * k as f32 / n as f32;
                cams.push(ctx.camera.rotated(radial.axis_dir(), pivot, angle));
            }
        }
        for axis in 0..3 {
            if self.options.symmetry[axis] {
                let mirrored: Vec<Camera> = cams.iter().map(|c| c.mirrored(axis, center[axis])).collect();
                cams.extend(mirrored);
            }
        }
        cams
    }

    fn depth_hint(&self, ctx: &PaintContext) -> f32 {
        self.last_depth.unwrap_or_else(|| ctx.camera.view_depth(ctx.mesh.bounds.center()).max(1e-3))
    }

    fn spacing_px(&mut self, ctx: &PaintContext) -> f32 {
        // The spacing follows the size the pen pressure gives the dabs: with the nominal size, a
        // light contact (a few percent of the diameter) left a dotted line. A mouse is factor 1.
        let r = self.base_radius(ctx) * self.settings.pressure_size_factor(self.pressure);
        let diameter_px = 2.0 * r * ctx.camera.pixels_per_unit(self.depth_hint(ctx));
        (self.settings.spacing * diameter_px).max(0.5)
    }

    fn paint_samples(&mut self, ts: &mut TextureSet, ctx: &PaintContext, samples: &[PathSample]) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        self.view_camera.get_or_insert(*ctx.camera);
        if self.mode == StrokeMode::Clone {
            let source = self.clone.as_mut().ok_or(PaintError::NoCloneSource)?;
            if source.offset.is_none() {
                let at = ctx.camera.project(source.point).ok_or(PaintError::NoCloneSource)?;
                source.offset = Some(at.truncate() - samples[0].pos);
            }
        }
        let cameras = self.cameras(ctx);
        let base_radius = self.base_radius(ctx);
        let s = &self.settings;
        let [size_jitter, flow_jitter, angle_jitter, position_jitter] = s.jitters(self.pressure);
        let mut stamps = Vec::new();
        let mut stamp_k: Vec<u32> = Vec::new();
        for sample in samples {
            let k = self.stamp_count;
            self.stamp_count += 1;
            let radius = base_radius * s.pressure_size_factor(self.pressure) * (1.0 - size_jitter * rand01(s.seed, k, 0));
            let flow = s.flow.clamp(0.0, 1.0) * s.pressure_flow_factor(self.pressure) * (1.0 - flow_jitter * rand01(s.seed, k, 1));
            let mut angle = (s.angle_deg + angle_jitter * (2.0 * rand01(s.seed, k, 2) - 1.0)).to_radians();
            if s.follow_path {
                angle += sample.dir.y.atan2(sample.dir.x);
            }
            let mut screen = sample.pos;
            if position_jitter > 0.0 {
                let r_px = radius * ctx.camera.pixels_per_unit(self.depth_hint(ctx));
                let (a, d) = (rand01(s.seed, k, 3) * std::f32::consts::TAU, rand01(s.seed, k, 4).sqrt());
                screen += Vec2::new(a.cos(), a.sin()) * d * position_jitter * 2.0 * r_px;
            }
            let mut dabs = vec![Dab { screen, radius, flow, angle, roundness: s.roundness, hardness: s.hardness }];
            if s.scatter_count > 0 {
                // Particles around the path point; each draws from its own random streams.
                let diameter_px = 2.0 * radius * ctx.camera.pixels_per_unit(self.depth_hint(ctx));
                dabs.clear();
                for i in 0..s.scatter_count {
                    let st = 16 + i * 8;
                    let (a, d) = (rand01(s.seed, k, st) * std::f32::consts::TAU, rand01(s.seed, k, st + 1).sqrt());
                    let mut at = screen + Vec2::new(a.cos(), a.sin()) * d * s.scatter_radius.max(0.0) * diameter_px;
                    at.y += s.scatter_fall.max(0.0) * diameter_px * rand01(s.seed, k, st + 2);
                    dabs.push(Dab {
                        screen: at,
                        radius: radius * (1.0 - s.scatter_size_random.clamp(0.0, 1.0) * rand01(s.seed, k, st + 3)),
                        flow: flow * (1.0 - s.scatter_opacity_random.clamp(0.0, 1.0) * rand01(s.seed, k, st + 4)),
                        angle: angle + (s.scatter_rotation_deg * (2.0 * rand01(s.seed, k, st + 5) - 1.0)).to_radians(),
                        roundness: s.roundness,
                        hardness: s.hardness,
                    });
                }
            }
            for dab in &dabs {
                for (ci, cam) in cameras.iter().enumerate() {
                    let parts = self.parts.as_ref().filter(|p| p.enabled);
                    let accept = |t: u32| ctx.mesh.tri_material[t as usize] == ctx.geometry.material_slot && parts.is_none_or(|p|
                        ctx.geometry.triangle_region(t as usize, p.kind).is_some_and(|id| p.parts.contains(&id)));
                    if let Some(st) = resolve_overlap(dab, ci, cam, ctx.mesh, ctx.bvh, s.alignment, self.alpha.is_some(), accept) {
                        if ci == 0 {
                            self.last_depth = Some(cam.view_depth(st.center).max(1e-3));
                        }
                        stamps.push(st);
                        stamp_k.push(k);
                    }
                }
            }
        }
        if self.mode == StrokeMode::Smudge {
            return self.smudge_stamps(ts, ctx, &cameras, &stamps);
        }
        if self.preview.tiles > 0 && matches!(self.mode, StrokeMode::Paint | StrokeMode::Erase) {
            let stride = (self.preview.last_tiles / self.preview.tiles).clamp(1, PREVIEW_MAX_STRIDE) as u32;
            if stride > 1 {
                let mut live = Vec::with_capacity(stamps.len() / stride as usize + 1);
                for (st, k) in stamps.into_iter().zip(stamp_k) {
                    if k % stride == 0 { live.push(st) } else { self.preview.deferred.push(st) }
                }
                stamps = live;
            }
        }
        self.apply_stamps(ts, ctx, &cameras, &stamps, self.settings.alignment, true)
    }

    fn apply_stamps(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cameras: &[Camera], stamps: &[Stamp], alignment: Alignment, stencil: bool) -> Result<Vec<usize>, PaintError> {
        if stamps.is_empty() {
            return Ok(Vec::new());
        }
        let depth: Vec<Arc<DepthMap>> =
            if alignment == Alignment::Uv { Vec::new() } else { ctx.depth.get_all(ctx.mesh, cameras) };
        let backface_cos = self.settings.backface_culling.then(|| self.settings.backface_angle_deg.to_radians().cos());
        let input = KernelInput {
            mesh: ctx.mesh,
            bvh: ctx.bvh,
            geometry: ctx.geometry,
            alignment,
            cameras,
            depth: &depth,
            backface_cos,
            alpha: self.alpha.as_deref(),
            stencil: self.stencil.as_deref().filter(|_| stencil),
        };
        if let Some((st, _)) = &self.stamp
            && st.tiled
            && self.tile_ref.is_none()
            && let Some(first) = stamps.first()
        {
            self.tile_ref = Some((first.uv, (2.0 * first.radius_uv).max(1e-6)));
        }
        let t_bin = std::time::Instant::now();
        let bins = input.bin(stamps);
        profile::add(profile::BIN, t_bin);
        if alignment != Alignment::Uv {
            // Cost estimate for the live preview: the tiles a batch touches per stamp.
            let n = stamps.len().max(1);
            self.preview.last_tiles = bins.len() / ((n as f32).sqrt().ceil() as usize);
        }
        let cached = self.raster.enabled;
        if cached {
            let key: RasterKey = (cameras.to_vec(), alignment, backface_cos.map(f32::to_bits), ctx.geometry.resolution, ctx.geometry.material_slot, ctx.mesh as *const Mesh as usize);
            if self.raster.key.as_ref() != Some(&key) || self.raster.samples > RASTER_CACHE_SAMPLES {
                self.raster.tiles.clear();
                self.raster.samples = 0;
            }
            self.raster.key = Some(key);
            let t_ras = std::time::Instant::now();
            let mut missing = std::collections::BTreeSet::new();
            for (&t, w) in &bins {
                for &si in &w.stamps {
                    let c = stamps[si as usize].camera;
                    if !self.raster.tiles.contains_key(&(t, c)) {
                        missing.insert((t, c));
                    }
                }
            }
            let built: Vec<((usize, usize), Arc<[TexelSample]>)> =
                missing.into_par_iter().map(|(t, c)| ((t, c), Arc::from(input.tile_samples(t, c)))).collect();
            for (k, samples) in built {
                self.raster.samples += samples.len();
                self.raster.tiles.insert(k, samples);
            }
            profile::add(profile::RASTER, t_ras);
        }
        let raster = &self.raster.tiles;
        let single_stamp = self.stamp.as_ref().filter(|(s, _)| !s.tiled).map(|(s, b)| (s.clone(), b.clone()));
        let work: Vec<(usize, TileWork, Box<[f32]>, Option<Box<[[f32; 3]]>>)> = bins
            .into_iter()
            .map(|(t, w)| {
                let cov = self.coverage.remove(&t).unwrap_or_else(|| vec![0.0; TILE_TEXELS].into_boxed_slice());
                let acc = single_stamp.as_ref().map(|_| self.stamp_acc.remove(&t).unwrap_or_else(|| vec![[0.0; 3]; TILE_TEXELS].into_boxed_slice()));
                (t, w, cov, acc)
            })
            .collect();
        let t_acc = std::time::Instant::now();
        let done: Vec<(usize, Box<[f32]>, Option<Box<[[f32; 3]]>>, bool)> = work
            .into_par_iter()
            .map(|(t, w, mut cov, mut acc)| {
                // A stamp's bounding region covers whole triangles, so many touched tiles gain
                // nothing; those need no rewrite, no re-composite and no GPU upload.
                let before = cached.then(|| (cov.clone(), acc.clone()));
                if cached {
                    let per_camera: Vec<Option<&[TexelSample]>> = (0..cameras.len()).map(|c| raster.get(&(t, c)).map(|s| &s[..])).collect();
                    input.accumulate_visible(&per_camera, stamps, &w.stamps, &mut cov);
                    if let (Some((cfg, bitmap)), Some(acc)) = (&single_stamp, acc.as_mut()) {
                        for &si in &w.stamps {
                            let st = &stamps[si as usize];
                            let Some(samples) = per_camera[st.camera] else { continue };
                            lay_stamp(&input, samples, st, bitmap, cfg, acc);
                        }
                    }
                } else {
                    let samples = input.rasterize_tile(t, w.tris.iter().copied());
                    input.accumulate(&samples, stamps, &w.stamps, &mut cov);
                    if let (Some((cfg, bitmap)), Some(acc)) = (&single_stamp, acc.as_mut()) {
                        for &si in &w.stamps {
                            lay_stamp(&input, &samples, &stamps[si as usize], bitmap, cfg, acc);
                        }
                    }
                }
                self.restrict_coverage(ctx.geometry, t, &mut cov);
                let changed = before.is_none_or(|(c, a)| c != cov || a != acc);
                (t, cov, acc, changed)
            })
            .collect();
        profile::add(profile::ACCUM, t_acc);
        let tiles: Vec<usize> = done.iter().filter(|d| d.3).map(|d| d.0).collect();
        for (t, cov, acc, _) in done {
            self.coverage.insert(t, cov);
            if let Some(acc) = acc { self.stamp_acc.insert(t, acc); }
        }
        let t_wr = std::time::Instant::now();
        let r = self.write(ts, ctx, &tiles);
        profile::add(profile::WRITE, t_wr);
        r
    }

    /// Recompute `tiles` of every target image from snapshot + coverage and store them.
    fn write(&self, ts: &mut TextureSet, ctx: &PaintContext, tiles: &[usize]) -> Result<Vec<usize>, PaintError> {
        let geometry = ctx.geometry;
        let tiles: Vec<usize> =
            tiles.iter().copied().filter(|t| self.coverage.get(t).is_some_and(|c| c.iter().any(|&v| v > 0.0))).collect();
        let opacity = self.settings.stroke_opacity.clamp(0.0, 1.0);
        let writes: Vec<(ImageKey, usize, Box<[half::f16]>)> = tiles
            .par_iter()
            .flat_map_iter(|&t| {
                let cov = &self.coverage[&t];
                let remap = self.remap(ctx, t, cov);
                self.images.iter().map(move |img| {
                    let source = if img.stamp.is_some() { self.stamp_source(ctx, t, cov) } else { match (&remap, self.mode) {
                        (Remap::Clone(uvs), StrokeMode::Clone) => Some(clone_source_tile(img, uvs)),
                        _ => img.texture.filter(|_| self.mode == StrokeMode::Paint).map(|id| {
                            let mut values = vec![[0.0; 4]; TILE_TEXELS];
                            if let Some(bitmap) = self.resources.bitmap(id) {
                                // UV sampling needs no surface cache. Object projections reuse the
                                // exact normalized positions, normals and sampler used by fill layers.
                                let geo = (self.projection.mode != ProjectionMode::Uv).then(|| geometry.tile(t));
                                let linearize = img.key.channel().is_some_and(|k| k.is_color());
                                let res = geometry.resolution as f32;
                                let x0 = (t as u32 % geometry.tiles_x) * TILE_SIZE;
                                let y0 = (t as u32 / geometry.tiles_x) * TILE_SIZE;
                                let filter = if img.key.channel() == Some(subcraft_core::ChannelKind::BaseColor) { self.albedo_filter } else { subcraft_image::TextureFilter::Bilinear };
                                let sample = |p: [f32; 2]| if !self.texture_tiling && p.iter().any(|&v| !(0.0..=1.0).contains(&v)) {
                                    [0.0; 4]
                                } else {
                                    bitmap.sample_filtered(p[0], p[1], if self.texture_tiling { Wrap::Repeat } else { Wrap::Clamp }, linearize, filter)
                                };
                                let sc = self.projection.rotation_sin_cos();
                                for (i, out) in values.iter_mut().enumerate() {
                                    if cov[i] <= 0.0 { continue; }
                                    if let Remap::Camera(coords) = &remap {
                                        *out = coords[i].map_or([0.0; 4], |p| sample(self.projection.transform_2d_sc(sc, p)));
                                        continue;
                                    }
                                    let uv = [(x0 + i as u32 % TILE_SIZE) as f32 / res + 0.5 / res,
                                        (y0 + i as u32 / TILE_SIZE) as f32 / res + 0.5 / res];
                                    *out = subcraft_materials::sample::sample_projected_sc(&self.projection, sc, uv,
                                        geo.as_ref().map_or([0.0; 3], |g| g.position(i)),
                                        geo.as_ref().map_or([0.0; 3], |g| g.normal(i)), sample);
                                }
                            }
                            values
                        }),
                    } };
                    (img.key, t, subcraft_image::TiledImage::encode_tile(&img.compute_tile(t, cov, opacity, self.mode, source.as_deref())))
                })
            })
            .collect();
        crate::target::store_encoded(ts, self.layer, writes)?;
        let mut tiles = tiles;
        tiles.sort_unstable();
        Ok(tiles)
    }

    /// Per-texel `[v0, v1, 0, 1]` source of a stamp stroke on tile `t`: the stamps' summed
    /// tangent-space slope (height stamps: height), averaged where stamps overlap beyond weight 1.
    fn stamp_source(&self, ctx: &PaintContext, t: usize, cov: &[f32]) -> Option<Vec<[f32; 4]>> {
        let (cfg, bitmap) = self.stamp.as_ref()?;
        let mut values = vec![[0.0; 4]; TILE_TEXELS];
        if !cfg.tiled {
            let acc = self.stamp_acc.get(&t)?;
            for ((out, a), &c) in values.iter_mut().zip(acc.iter()).zip(cov) {
                if c <= 0.0 || a[2] <= 0.0 { continue; }
                let n = a[2].max(1.0);
                *out = [a[0] / n, a[1] / n, 0.0, 1.0];
            }
            return Some(values);
        }
        let (origin, size) = self.tile_ref?;
        let g = ctx.geometry;
        let res = g.resolution as f32;
        let x0 = (t as u32 % g.tiles_x) * TILE_SIZE;
        let y0 = (t as u32 / g.tiles_x) * TILE_SIZE;
        let angle = self.settings.angle_deg.to_radians();
        for (i, out) in values.iter_mut().enumerate() {
            if cov[i] <= 0.0 { continue; }
            let uv = Vec2::new((x0 + i as u32 % TILE_SIZE) as f32 / res + 0.5 / res, (y0 + i as u32 / TILE_SIZE) as f32 / res + 0.5 / res);
            let p = crate::image_stamp::rotate(uv - origin, -angle) / size + Vec2::splat(0.5);
            *out = match cfg.kind {
                StampKind::Height => [crate::image_stamp::sample_height(bitmap, cfg, p, Wrap::Repeat), 0.0, 0.0, 1.0],
                StampKind::Normal => {
                    let v = crate::image_stamp::to_local(crate::image_stamp::sample_slope(bitmap, cfg, p, Wrap::Repeat), angle);
                    [v.x, -v.y, 0.0, 1.0] // UV space: V down -> tangent space V up
                }
            };
        }
        Some(values)
    }

    /// Object-space position of covered texel `i` of a tile.
    fn texel_position(&self, ctx: &PaintContext, geo: &subcraft_materials::TileGeometry, i: usize) -> Vec3 {
        let g = ctx.geometry;
        g.bounds.min + Vec3::from(geo.position(i)) * g.max_extent
    }

    fn remap(&self, ctx: &PaintContext, tile: usize, cov: &[f32]) -> Remap {
        let Some(cam) = self.view_camera.as_ref() else { return Remap::None };
        if self.mode == StrokeMode::Clone {
            let Some(offset) = self.clone.and_then(|c| c.offset) else { return Remap::None };
            let slot = ctx.geometry.material_slot;
            let geo = ctx.geometry.tile(tile);
            let uvs = (0..TILE_TEXELS).map(|i| {
                if cov[i] <= 0.0 { return None; }
                let at = cam.project(self.texel_position(ctx, &geo, i))?;
                let (o, d) = cam.ray(at.truncate() + offset);
                let hit = ctx.bvh.intersect_filtered(o, d, 0.0, f32::INFINITY, |t| ctx.mesh.tri_material[t as usize] == slot)?;
                let uv = ctx.mesh.interpolate(hit.tri as usize, hit.u, hit.v).uv;
                Some([uv.x, uv.y])
            }).collect();
            return Remap::Clone(uvs);
        }
        if self.camera_projection && self.images.iter().any(|i| i.texture.is_some()) {
            let geo = ctx.geometry.tile(tile);
            let [w, h] = [cam.viewport[0] as f32, cam.viewport[1] as f32];
            let side = w.min(h).max(1.0);
            let coords = (0..TILE_TEXELS).map(|i| {
                if cov[i] <= 0.0 { return None; }
                let at = cam.project(self.texel_position(ctx, &geo, i))?;
                Some([0.5 + (at.x - w * 0.5) / side, 0.5 + (at.y - h * 0.5) / side])
            }).collect();
            return Remap::Camera(coords);
        }
        Remap::None
    }

    /// Smudge: stamp by stamp, pick up what is under the brush and drag it along. Works on live
    /// copies of the target images, so a stamp sees what earlier stamps left.
    fn smudge_stamps(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cameras: &[Camera], stamps: &[Stamp]) -> Result<Vec<usize>, PaintError> {
        if stamps.is_empty() {
            return Ok(Vec::new());
        }
        if self.work.is_empty() {
            self.work = self.images.iter().map(|i| i.snapshot.clone()).collect();
        }
        // One carried colour per stroke copy (symmetry) and image: shared, the paint picked up on
        // one side of the mirror was deposited on the other.
        let images = self.images.len();
        if self.carried.len() != images * cameras.len().max(1) {
            self.carried = vec![None; images * cameras.len().max(1)];
        }
        let alignment = self.settings.alignment;
        let depth: Vec<Arc<DepthMap>> =
            if alignment == Alignment::Uv { Vec::new() } else { ctx.depth.get_all(ctx.mesh, cameras) };
        let backface_cos = self.settings.backface_culling.then(|| self.settings.backface_angle_deg.to_radians().cos());
        let input = KernelInput {
            mesh: ctx.mesh, bvh: ctx.bvh, geometry: ctx.geometry, alignment, cameras, depth: &depth, backface_cos,
            alpha: self.alpha.as_deref(), stencil: None,
        };
        let mut changed = std::collections::BTreeSet::new();
        let (carry, strength, material, falloff) = (self.settings.smudge_carry.clamp(0.0, 1.0), self.settings.smudge_strength.clamp(0.0, 1.0),
            self.settings.smudge_material.clamp(0.0, 1.0), self.settings.smudge_falloff.clamp(0.25, 8.0));
        for st in stamps {
            let footprints: Vec<(usize, Vec<f32>)> = input
                .bin(std::slice::from_ref(st))
                .into_iter()
                .map(|(t, w)| {
                    let samples = input.rasterize_tile(t, w.tris.iter().copied());
                    let mut fp = input.footprints(&samples, st);
                    self.restrict_coverage(ctx.geometry, t, &mut fp);
                    if falloff != 1.0 {
                        fp.iter_mut().for_each(|v| *v = v.powf(falloff));
                    }
                    (t, fp)
                })
                .filter(|(_, fp)| fp.iter().any(|&v| v > 0.0))
                .collect();
            if footprints.is_empty() {
                continue;
            }
            // Pick up: footprint-weighted mean of the live paint, premultiplied by alpha.
            let mut pickup = vec![[0.0f32; 4]; self.images.len()];
            let mut weight = 0.0f32;
            for (t, fp) in &footprints {
                for (k, img) in self.images.iter().enumerate() {
                    let comps = img.snapshot.comps();
                    let mut buf = vec![0.0f32; TILE_TEXELS * comps];
                    self.work[k].read_tile(*t, &mut buf);
                    for (px, &w) in buf.chunks_exact(comps).zip(fp) {
                        if w <= 0.0 { continue; }
                        let a = if img.has_alpha { px[img.value_comps] } else { 1.0 };
                        for j in 0..img.value_comps { pickup[k][j] += px[j] * a * w; }
                        pickup[k][3] += a * w;
                    }
                }
                weight += fp.iter().sum::<f32>();
            }
            if weight <= 1e-6 { continue; }
            for (k, p) in pickup.iter_mut().enumerate() {
                for v in p.iter_mut() { *v /= weight; }
                let slot = st.camera * images + k;
                self.carried[slot] = Some(match self.carried[slot] {
                    None => *p,
                    Some(c) => std::array::from_fn(|j| c[j] * carry + p[j] * (1.0 - carry)),
                });
                if material > 0.0 {
                    // Smudge with material: the brush paint (opaque) mixes into what is carried.
                    let c = self.carried[slot].as_mut().expect("set above");
                    let v = self.images[k].value;
                    for j in 0..3 { c[j] += (v[j] - c[j]) * material; }
                    c[3] += (1.0 - c[3]) * material;
                }
            }
            // Deposit the carried paint, weighted by the footprint.
            let mut writes = Vec::new();
            for (t, fp) in &footprints {
                for (k, img) in self.images.iter().enumerate() {
                    let c = self.carried[st.camera * images + k].expect("set above");
                    let comps = img.snapshot.comps();
                    let n = img.value_comps;
                    let mut buf = vec![0.0f32; TILE_TEXELS * comps];
                    self.work[k].read_tile(*t, &mut buf);
                    for (px, &w) in buf.chunks_exact_mut(comps).zip(fp) {
                        let s = (w * st.flow * strength).clamp(0.0, 1.0);
                        if s <= 0.0 { continue; }
                        if img.has_alpha {
                            let a0 = px[n];
                            let a = a0 + (c[3] - a0) * s;
                            if a > 1e-6 {
                                for j in 0..n { px[j] = (px[j] * a0 + (c[j] - px[j] * a0) * s) / a; }
                            }
                            px[n] = a;
                            if img.key.channel() == Some(subcraft_core::ChannelKind::Normal) && a > 1e-6 {
                                let unit = subcraft_core::normal::encode(subcraft_core::normal::decode([px[0], px[1], px[2]]));
                                px[..3].copy_from_slice(&unit);
                            }
                        } else {
                            px[0] += (c[0] - px[0]) * s;
                        }
                    }
                    self.work[k].write_tile(*t, &buf);
                    writes.push((img.key, *t, buf));
                }
                changed.insert(*t);
            }
            store(ts, self.layer, &writes)?;
        }
        Ok(changed.into_iter().collect())
    }

    /// Fill the triangle / mesh element / UV island under `screen` at full coverage (scaled by
    /// stroke opacity). Only triangles of the painted texture set are filled.
    pub fn polygon_fill(&mut self, ts: &mut TextureSet, ctx: &PaintContext, screen: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        let (o, d) = ctx.camera.ray(screen);
        let Some(hit) = ctx.bvh.intersect(o, d, 0.0, f32::INFINITY) else { return Ok(Vec::new()) };
        self.fill_from(ts, ctx, hit.tri, mode)
    }

    /// [`Stroke::polygon_fill`] from the 2D view: the triangle whose UV footprint contains `uv`.
    pub fn polygon_fill_uv(&mut self, ts: &mut TextureSet, ctx: &PaintContext, uv: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        match triangle_at_uv(ctx.mesh, ctx.geometry, uv) {
            Some(tri) => self.fill_from(ts, ctx, tri, mode),
            None => Ok(Vec::new()),
        }
    }

    /// Fill everything a screen box `[a, b]` (pixels) touches, grown to `mode`'s elements.
    /// `visible_only` skips back faces and occluded triangles (see
    /// [`crate::select::triangles_in_screen_box`]).
    pub fn polygon_fill_box(&mut self, ts: &mut TextureSet, ctx: &PaintContext, a: Vec2, b: Vec2, mode: PolygonFillMode, visible_only: bool) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        let seeds = crate::select::triangles_in_screen_box(ctx.mesh, ctx.bvh, ctx.camera, ctx.geometry.material_slot, a, b, visible_only);
        let tris = crate::select::grow(ctx.mesh, ctx.geometry, &seeds, mode);
        self.fill_triangles(ts, ctx, &tris)
    }

    /// [`Stroke::polygon_fill_box`] from the 2D view: a box in UV space.
    pub fn polygon_fill_uv_box(&mut self, ts: &mut TextureSet, ctx: &PaintContext, a: Vec2, b: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.check(ts, ctx)?;
        let seeds = crate::select::triangles_in_uv_box(ctx.mesh, ctx.geometry.material_slot, a, b);
        let tris = crate::select::grow(ctx.mesh, ctx.geometry, &seeds, mode);
        self.fill_triangles(ts, ctx, &tris)
    }

    fn fill_from(&mut self, ts: &mut TextureSet, ctx: &PaintContext, hit_tri: u32, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        if ctx.mesh.tri_material[hit_tri as usize] != ctx.geometry.material_slot {
            return Ok(Vec::new());
        }
        let tris = crate::select::grow(ctx.mesh, ctx.geometry, &[hit_tri], mode);
        self.fill_triangles(ts, ctx, &tris)
    }

    /// Fill `tris` (of the painted texture set) at full coverage, scaled by stroke opacity.
    fn fill_triangles(&mut self, ts: &mut TextureSet, ctx: &PaintContext, tris: &[u32]) -> Result<Vec<usize>, PaintError> {
        if tris.is_empty() {
            return Ok(Vec::new());
        }
        let mesh = ctx.mesh;
        let input = KernelInput {
            mesh,
            bvh: ctx.bvh,
            geometry: ctx.geometry,
            alignment: self.settings.alignment,
            cameras: &[],
            depth: &[],
            backface_cos: None,
            alpha: None,
            stencil: None,
        };
        let mut bins: std::collections::BTreeMap<usize, Vec<u32>> = std::collections::BTreeMap::new();
        for &t in tris {
            for tile in input.triangle_tiles(t) {
                bins.entry(tile).or_default().push(t);
            }
        }
        let work: Vec<(usize, Vec<u32>, Box<[f32]>)> = bins
            .into_iter()
            .map(|(t, tr)| (t, tr, self.coverage.remove(&t).unwrap_or_else(|| vec![0.0; TILE_TEXELS].into_boxed_slice())))
            .collect();
        let done: Vec<(usize, Box<[f32]>)> = work
            .into_par_iter()
            .map(|(t, tr, mut cov)| {
                for s in input.rasterize_tile(t, tr) {
                    cov[s.idx as usize] = 1.0;
                }
                self.restrict_coverage(ctx.geometry, t, &mut cov);
                (t, cov)
            })
            .collect();
        let tiles: Vec<usize> = done.iter().map(|(t, _)| *t).collect();
        self.coverage.extend(done);
        self.write(ts, ctx, &tiles)
    }

    fn restrict_coverage(&self, geometry: &TextureSetGeometry, tile: usize, cov: &mut [f32]) {
        if let Some(parts) = self.parts.as_ref().filter(|p| p.enabled) {
            let geo = geometry.tile(tile);
            for (i, c) in cov.iter_mut().enumerate() {
                *c *= parts.factor(geo.region(i, parts.kind));
            }
        }
    }
}

/// Add one stamp's image contribution to a tile's accumulator.
fn lay_stamp(input: &KernelInput, samples: &[TexelSample], st: &Stamp, image: &subcraft_image::Bitmap, cfg: &ImageStamp, acc: &mut [[f32; 3]]) {
    for (slot, v) in acc.iter_mut().zip(input.stamp_values(samples, st, image, cfg)) {
        let w = v[0] * st.flow;
        if w <= 0.0 { continue; }
        slot[0] += w * v[1];
        slot[1] += w * v[2];
        slot[2] += w;
    }
}

/// Bilinear sample of a paint image at `uv` as straight `[value..., alpha]` (alpha 1 without one).
fn sample_image(img: &TargetImage, uv: [f32; 2]) -> [f32; 4] {
    let (w, h) = (img.snapshot.width(), img.snapshot.height());
    let x = (uv[0] * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
    let y = (uv[1] * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let n = img.value_comps;
    let mut pm = [0.0f32; 4];
    for (xx, yy, wt) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)), (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] {
        let px = img.snapshot.get(xx, yy);
        let a = if img.has_alpha { px[n] } else { 1.0 };
        for j in 0..n { pm[j] += px[j] * a * wt; }
        pm[3] += a * wt;
    }
    let a = pm[3];
    let mut out = [0.0, 0.0, 0.0, a];
    if a > 1e-6 { for j in 0..n { out[j] = pm[j] / a; } }
    out
}

/// Source colors for a clone stroke: the pre-stroke paint at each texel's source point.
fn clone_source_tile(img: &TargetImage, uvs: &[Option<[f32; 2]>]) -> Vec<[f32; 4]> {
    uvs.iter().map(|uv| uv.map_or([0.0; 4], |uv| sample_image(img, uv))).collect()
}

/// Average UV units per object unit over a material slot's triangles (square root of the UV to
/// surface area ratio). Falls back to one texture width per bounding-box diagonal.
pub fn uv_per_unit(mesh: &Mesh, slot: u32) -> f32 {
    let (mut world, mut uv) = (0.0f64, 0.0f64);
    for t in mesh.material_triangles(slot) {
        let p = mesh.tri_positions(t);
        let q = mesh.tri_uvs(t);
        world += (p[1] - p[0]).cross(p[2] - p[0]).length() as f64;
        uv += (q[1] - q[0]).perp_dot(q[2] - q[0]).abs() as f64;
    }
    if world > 1e-20 && uv > 1e-20 {
        (uv / world).sqrt() as f32
    } else {
        1.0 / mesh.bounds.diagonal().max(1e-6)
    }
}

/// Triangle of the geometry's material slot whose (tile-wrapped) UV triangle contains `uv`.
pub fn triangle_at_uv(mesh: &Mesh, geometry: &TextureSetGeometry, uv: Vec2) -> Option<u32> {
    if !uv.is_finite() || uv.min_element() < 0.0 || uv.max_element() >= 1.0 {
        return None;
    }
    let texel = (uv * geometry.resolution as f32).as_uvec2();
    let tile = (texel.y / TILE_SIZE * geometry.tiles_x + texel.x / TILE_SIZE) as usize;
    let mut best: Option<(f32, u32)> = None;
    for &t in geometry.tile_triangles(tile) {
        if mesh.tri_material[t as usize] != geometry.material_slot {
            continue;
        }
        let (q, _) = subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t as usize));
        let (e0, e1, w) = (q[1] - q[0], q[2] - q[0], uv - q[0]);
        let d = e0.perp_dot(e1);
        if d.abs() < 1e-14 {
            continue;
        }
        let (b1, b2) = (w.perp_dot(e1) / d, e0.perp_dot(w) / d);
        // Distance outside the triangle in barycentric units (0 = inside); the nearest wins so
        // clicks on a shared edge still pick a triangle.
        let outside = (-b1).max(0.0) + (-b2).max(0.0) + (b1 + b2 - 1.0).max(0.0);
        if outside < 1e-4 && best.is_none_or(|b| outside < b.0) {
            best = Some((outside, t));
        }
    }
    best.map(|b| b.1)
}

/// Wall-clock nanoseconds spent in each stage of the stamp pipeline since the last
/// [`profile::take`]; read by the benchmarks. Costs one `Instant::now` per stage per call.
pub mod profile {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;
    pub const BIN: usize = 0;
    pub const RASTER: usize = 1;
    pub const ACCUM: usize = 2;
    pub const WRITE: usize = 3;
    pub const NAMES: [&str; 4] = ["bin", "raster", "accumulate", "write"];
    static NS: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
    pub(super) fn add(stage: usize, since: Instant) {
        NS[stage].fetch_add(since.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
    /// Milliseconds per stage, resetting the counters.
    pub fn take() -> [f64; 4] {
        std::array::from_fn(|i| NS[i].swap(0, Ordering::Relaxed) as f64 / 1e6)
    }
}
