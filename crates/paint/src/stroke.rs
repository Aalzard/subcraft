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
use crate::kernel::{KernelInput, TileWork};
use crate::path::{PathSample, ScreenPath, rand01};
use crate::settings::{Alignment, BrushMaterial, BrushSettings, PaintTarget, PolygonFillMode, StrokeMode, StrokeOptions};
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
    /// carried along (premultiplied value, alpha) per image.
    work: Vec<TiledImage>,
    carried: Vec<Option<[f32; 4]>>,
}

/// Where a clone stroke copies from: an object-space surface point, and the screen offset from
/// the stroke's own position to it (fixed by the first dab, then kept so later strokes align).
#[derive(Clone, Copy, Debug, PartialEq)]
struct CloneSource {
    point: Vec3,
    offset: Option<Vec2>,
}

/// How much of the carried paint survives each smudge stamp.
const SMUDGE_CARRY: f32 = 0.85;

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
        })
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

    /// Feed a cursor position; returns the tiles rewritten in `ts`.
    pub fn add_point(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cursor: Vec2) -> Result<Vec<usize>, PaintError> {
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        self.path.push(cursor, spacing, self.options.lazy_distance_px, &mut samples);
        self.paint_samples(ts, ctx, &samples)
    }

    /// Straight segment from the brush position to `to` (the first call just places the brush).
    pub fn line_to(&mut self, ts: &mut TextureSet, ctx: &PaintContext, to: Vec2) -> Result<Vec<usize>, PaintError> {
        let mut samples = Vec::new();
        let spacing = self.spacing_px(ctx);
        self.path.line_to(to, spacing, &mut samples);
        self.paint_samples(ts, ctx, &samples)
    }

    fn check(&self, ts: &TextureSet, ctx: &PaintContext) -> Result<(), PaintError> {
        if ctx.geometry.resolution != ts.resolution || ts.material_slot != ctx.geometry.material_slot {
            return Err(PaintError::ResolutionMismatch);
        }
        Ok(())
    }

    /// View camera followed by every symmetry mirror (all combinations of enabled planes).
    fn cameras(&self, ctx: &PaintContext) -> Vec<Camera> {
        let center = ctx.mesh.bounds.center();
        let mut cams = vec![*ctx.camera];
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

    fn spacing_px(&self, ctx: &PaintContext) -> f32 {
        let r = self.settings.radius(ctx.mesh.bounds.diagonal());
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
        let s = &self.settings;
        let base_radius = s.radius(ctx.mesh.bounds.diagonal());
        let mut stamps = Vec::new();
        for sample in samples {
            let k = self.stamp_count;
            self.stamp_count += 1;
            let radius = base_radius * (1.0 - s.size_jitter.clamp(0.0, 1.0) * rand01(s.seed, k, 0));
            let flow = s.flow.clamp(0.0, 1.0) * (1.0 - s.flow_jitter.clamp(0.0, 1.0) * rand01(s.seed, k, 1));
            let mut angle = (s.angle_deg + s.angle_jitter_deg * (2.0 * rand01(s.seed, k, 2) - 1.0)).to_radians();
            if s.follow_path {
                angle += sample.dir.y.atan2(sample.dir.x);
            }
            let mut screen = sample.pos;
            if s.position_jitter > 0.0 {
                let r_px = radius * ctx.camera.pixels_per_unit(self.depth_hint(ctx));
                let (a, d) = (rand01(s.seed, k, 3) * std::f32::consts::TAU, rand01(s.seed, k, 4).sqrt());
                screen += Vec2::new(a.cos(), a.sin()) * d * s.position_jitter * 2.0 * r_px;
            }
            let dab = Dab { screen, radius, flow, angle, roundness: s.roundness, hardness: s.hardness };
            for (ci, cam) in cameras.iter().enumerate() {
                let parts = self.parts.as_ref().filter(|p| p.enabled);
                let accept = |t: u32| ctx.mesh.tri_material[t as usize] == ctx.geometry.material_slot && parts.is_none_or(|p|
                    ctx.geometry.triangle_region(t as usize, p.kind).is_some_and(|id| p.parts.contains(&id)));
                if let Some(st) = resolve_overlap(&dab, ci, cam, ctx.mesh, ctx.bvh, s.alignment, self.alpha.is_some(), accept) {
                    if ci == 0 {
                        self.last_depth = Some(cam.view_depth(st.center).max(1e-3));
                    }
                    stamps.push(st);
                }
            }
        }
        if self.mode == StrokeMode::Smudge {
            return self.smudge_stamps(ts, ctx, &cameras, &stamps);
        }
        self.apply_stamps(ts, ctx, &cameras, &stamps)
    }

    fn apply_stamps(&mut self, ts: &mut TextureSet, ctx: &PaintContext, cameras: &[Camera], stamps: &[Stamp]) -> Result<Vec<usize>, PaintError> {
        if stamps.is_empty() {
            return Ok(Vec::new());
        }
        let alignment = self.settings.alignment;
        let depth: Vec<Arc<DepthMap>> =
            if alignment == Alignment::Uv { Vec::new() } else { cameras.iter().map(|c| ctx.depth.get(ctx.mesh, c)).collect() };
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
            stencil: self.stencil.as_deref(),
        };
        let bins = input.bin(stamps);
        let work: Vec<(usize, TileWork, Box<[f32]>)> = bins
            .into_iter()
            .map(|(t, w)| {
                let cov = self.coverage.remove(&t).unwrap_or_else(|| vec![0.0; TILE_TEXELS].into_boxed_slice());
                (t, w, cov)
            })
            .collect();
        let done: Vec<(usize, Box<[f32]>)> = work
            .into_par_iter()
            .map(|(t, w, mut cov)| {
                let samples = input.rasterize_tile(t, w.tris.iter().copied());
                input.accumulate(&samples, stamps, &w.stamps, &mut cov);
                self.restrict_coverage(ctx.geometry, t, &mut cov);
                (t, cov)
            })
            .collect();
        let tiles: Vec<usize> = done.iter().map(|(t, _)| *t).collect();
        self.coverage.extend(done);
        self.write(ts, ctx, &tiles)
    }

    /// Recompute `tiles` of every target image from snapshot + coverage and store them.
    fn write(&self, ts: &mut TextureSet, ctx: &PaintContext, tiles: &[usize]) -> Result<Vec<usize>, PaintError> {
        let geometry = ctx.geometry;
        let tiles: Vec<usize> =
            tiles.iter().copied().filter(|t| self.coverage.get(t).is_some_and(|c| c.iter().any(|&v| v > 0.0))).collect();
        let opacity = self.settings.stroke_opacity.clamp(0.0, 1.0);
        let writes: Vec<(ImageKey, usize, Vec<f32>)> = tiles
            .par_iter()
            .flat_map_iter(|&t| {
                let cov = &self.coverage[&t];
                let remap = self.remap(ctx, t, cov);
                self.images.iter().map(move |img| {
                    let source = match (&remap, self.mode) {
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
                                for (i, out) in values.iter_mut().enumerate() {
                                    if cov[i] <= 0.0 { continue; }
                                    if let Remap::Camera(coords) = &remap {
                                        *out = coords[i].map_or([0.0; 4], |p| sample(self.projection.transform_2d(p)));
                                        continue;
                                    }
                                    let uv = [(x0 + i as u32 % TILE_SIZE) as f32 / res + 0.5 / res,
                                        (y0 + i as u32 / TILE_SIZE) as f32 / res + 0.5 / res];
                                    *out = subcraft_materials::sample::sample_projected(&self.projection, uv,
                                        geo.as_ref().map_or([0.0; 3], |g| g.position(i)),
                                        geo.as_ref().map_or([0.0; 3], |g| g.normal(i)), sample);
                                }
                            }
                            values
                        }),
                    };
                    (img.key, t, img.compute_tile(t, cov, opacity, self.mode, source.as_deref()))
                })
            })
            .collect();
        store(ts, self.layer, &writes)?;
        let mut tiles = tiles;
        tiles.sort_unstable();
        Ok(tiles)
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
            self.carried = vec![None; self.images.len()];
        }
        let alignment = self.settings.alignment;
        let depth: Vec<Arc<DepthMap>> =
            if alignment == Alignment::Uv { Vec::new() } else { cameras.iter().map(|c| ctx.depth.get(ctx.mesh, c)).collect() };
        let backface_cos = self.settings.backface_culling.then(|| self.settings.backface_angle_deg.to_radians().cos());
        let input = KernelInput {
            mesh: ctx.mesh, bvh: ctx.bvh, geometry: ctx.geometry, alignment, cameras, depth: &depth, backface_cos,
            alpha: self.alpha.as_deref(), stencil: None,
        };
        let mut changed = std::collections::BTreeSet::new();
        for st in stamps {
            let footprints: Vec<(usize, Vec<f32>)> = input
                .bin(std::slice::from_ref(st))
                .into_iter()
                .map(|(t, w)| {
                    let samples = input.rasterize_tile(t, w.tris.iter().copied());
                    let mut fp = input.footprints(&samples, st);
                    self.restrict_coverage(ctx.geometry, t, &mut fp);
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
                self.carried[k] = Some(match self.carried[k] {
                    None => *p,
                    Some(c) => std::array::from_fn(|j| c[j] * SMUDGE_CARRY + p[j] * (1.0 - SMUDGE_CARRY)),
                });
            }
            // Deposit the carried paint, weighted by the footprint.
            let mut writes = Vec::new();
            for (t, fp) in &footprints {
                for (k, img) in self.images.iter().enumerate() {
                    let c = self.carried[k].expect("set above");
                    let comps = img.snapshot.comps();
                    let n = img.value_comps;
                    let mut buf = vec![0.0f32; TILE_TEXELS * comps];
                    self.work[k].read_tile(*t, &mut buf);
                    for (px, &w) in buf.chunks_exact_mut(comps).zip(fp) {
                        let s = (w * st.flow).clamp(0.0, 1.0);
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
        let mesh = ctx.mesh;
        let slot = ctx.geometry.material_slot;
        if mesh.tri_material[hit.tri as usize] != slot {
            return Ok(Vec::new());
        }
        let group = match mode {
            PolygonFillMode::Triangle => None,
            PolygonFillMode::MeshElement => Some(ctx.geometry.triangle_parts()),
            PolygonFillMode::UvIsland => Some(ctx.geometry.triangle_islands()),
        };
        let tris: Vec<u32> = match &group {
            None => vec![hit.tri],
            Some(g) => {
                let id = g[hit.tri as usize];
                mesh.material_triangles(slot).filter(|&t| g[t] == id).map(|t| t as u32).collect()
            }
        };
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
        for &t in &tris {
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
