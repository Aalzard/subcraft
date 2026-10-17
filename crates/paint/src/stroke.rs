//! A paint stroke: path → stamps → coverage → texel writes into the live layer.
//!
//! Straight lines, lazy mouse and symmetry all feed the same pipeline: screen samples are
//! resampled at the brush spacing, jittered, ray-cast through the view camera (and through each
//! symmetry-mirrored camera), and the resulting stamps are applied by the tile kernel.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;
use rayon::prelude::*;
use subcraft_core::LayerId;
use subcraft_doc::{Projection, ProjectionMode, ResourceStore, TextureSet};
use subcraft_image::{TILE_TEXELS, TILE_SIZE, Wrap};
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
    resources: ResourceStore,
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
            resources: ResourceStore::default(),
        })
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
        self.write(ts, ctx.geometry, &tiles)
    }

    /// Recompute `tiles` of every target image from snapshot + coverage and store them.
    fn write(&self, ts: &mut TextureSet, geometry: &TextureSetGeometry, tiles: &[usize]) -> Result<Vec<usize>, PaintError> {
        let tiles: Vec<usize> =
            tiles.iter().copied().filter(|t| self.coverage.get(t).is_some_and(|c| c.iter().any(|&v| v > 0.0))).collect();
        let opacity = self.settings.stroke_opacity.clamp(0.0, 1.0);
        let writes: Vec<(ImageKey, usize, Vec<f32>)> = tiles
            .par_iter()
            .flat_map_iter(|&t| {
                let cov = &self.coverage[&t];
                self.images.iter().map(move |img| {
                    let source = img.texture.filter(|_| self.mode == StrokeMode::Paint).map(|id| {
                        let mut values = vec![[0.0; 4]; TILE_TEXELS];
                        if let Some(bitmap) = self.resources.bitmap(id) {
                            // UV sampling needs no surface cache. Object projections reuse the
                            // exact normalized positions, normals and sampler used by fill layers.
                            let geo = (self.projection.mode != ProjectionMode::Uv).then(|| geometry.tile(t));
                            let linearize = matches!(img.key, ImageKey::Channel(k) if k.is_color());
                            let res = geometry.resolution as f32;
                            let x0 = (t as u32 % geometry.tiles_x) * TILE_SIZE;
                            let y0 = (t as u32 / geometry.tiles_x) * TILE_SIZE;
                            for (i, out) in values.iter_mut().enumerate() {
                                if cov[i] <= 0.0 { continue; }
                                let uv = [(x0 + i as u32 % TILE_SIZE) as f32 / res + 0.5 / res,
                                    (y0 + i as u32 / TILE_SIZE) as f32 / res + 0.5 / res];
                                *out = subcraft_materials::sample::sample_projected(&self.projection, uv,
                                    geo.as_ref().map_or([0.0; 3], |g| g.position(i)),
                                    geo.as_ref().map_or([0.0; 3], |g| g.normal(i)), |p| {
                                        if !self.texture_tiling && p.iter().any(|&v| !(0.0..=1.0).contains(&v)) {
                                            [0.0; 4]
                                        } else { bitmap.sample(p[0], p[1], if self.texture_tiling { Wrap::Repeat } else { Wrap::Clamp }, linearize) }
                                    });
                            }
                        }
                        values
                    });
                    (img.key, t, img.compute_tile(t, cov, opacity, self.mode, source.as_deref()))
                })
            })
            .collect();
        store(ts, self.layer, &writes)?;
        let mut tiles = tiles;
        tiles.sort_unstable();
        Ok(tiles)
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
        self.write(ts, ctx.geometry, &tiles)
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
