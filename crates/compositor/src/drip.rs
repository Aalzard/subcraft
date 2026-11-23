//! Painter's Dripping Rust generator: rust on the edges and in the cavities of the model, and
//! the streaks it leaves down the surface.
//!
//! The rust itself is local to a texel: the mesh's edges (and the baked Curvature and Ambient
//! Occlusion, when there are any) against the Rust Spreading threshold, broken up by the
//! generator's noise. The drips are not: a streak under a rusty edge depends on texels far
//! above it. They are computed once for the whole texture set ([`subcraft_materials::drip`]),
//! at 1024 texels across at most, kept with the set's geometry, and read by every tile.

use std::sync::{Arc, OnceLock};

use rayon::prelude::*;
use subcraft_core::MeshMapKind;
use subcraft_doc::Generator;
use subcraft_image::{TILE_TEXELS, TiledImage};
use subcraft_materials::{MeshMaps, TextureSetGeometry, TileGeometry};

use crate::TileContext;

/// The drips are computed on a grid of at most this many texels across.
const FIELD_MAX: u32 = 1024;
/// How far from a mesh edge the rust starts on its own, in units of the model's longest side.
const EDGE_REACH: f32 = 0.02;
/// The streak noise is this many times finer than the rust's breakup noise.
const STREAK_SCALE: f32 = 8.0;

/// The drips of one generator over a whole texture set.
struct Drips {
    /// Everything the drips depend on: the generator's settings and the baked maps it read.
    settings: String,
    curvature: Option<TiledImage>,
    occlusion: Option<TiledImage>,
    field: OnceLock<Field>,
}

struct Field {
    size: usize,
    stride: u32,
    values: Vec<f32>,
    covered: Vec<bool>,
}

fn same_map(a: &Option<TiledImage>, b: Option<&TiledImage>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same_layout(b) && a.same_storage(b),
        _ => false,
    }
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn unit(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { fallback }
}

/// The settings with the Invert taken out: the drips are the same either way.
fn settings_key(g: &Generator) -> String {
    format!("{:?}", Generator { invert: false, ..*g })
}

/// Per texel of a tile: the rust, and the part of it that drips (thin streak sources).
fn rust_tile(g: &Generator, set: &TextureSetGeometry, maps: &MeshMaps, geo: &TileGeometry) -> (Vec<f32>, Vec<f32>) {
    let drip = g.drip_settings();
    let (level, softness, contrast) = (unit(g.level, 0.5), unit(g.softness, 0.3), unit(drip.contrast, 0.5));
    let amount = unit(g.noise_amount, 0.0);
    let offset = if g.offset.iter().all(|v| v.is_finite()) { g.offset } else { [0.0; 3] };
    let read = |kind: MeshMapKind| {
        maps.get(kind).map(|_| {
            let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
            maps.read_tile(kind, geo, &mut map);
            map
        })
    };
    let (curvature, occlusion) = (read(MeshMapKind::Curvature), read(MeshMapKind::AmbientOcclusion));
    let (sharp, rounded) = (set.edge_distance_tile(geo), set.rounded_edge_distance_tile(geo));
    let noise = subcraft_materials::sample::organic_tile_at(&g.noise, offset, geo);
    let mut fine = g.noise;
    fine.scale = (fine.scale * STREAK_SCALE).clamp(0.01, 2048.0);
    fine.seed = fine.seed.wrapping_add(101);
    let streaks = subcraft_materials::sample::organic_tile_at(&fine, offset, geo);
    // Spreading 0: no rust at all; 0.5: the edges and cavities; 1: nearly everywhere.
    let threshold = 1.2 - 1.4 * level;
    let half = (softness * 0.5 * (1.0 - 0.9 * contrast)).max(0.01);
    let mut rust = vec![0.0f32; TILE_TEXELS];
    let mut sources = vec![0.0f32; TILE_TEXELS];
    for i in 0..TILE_TEXELS {
        if geo.coverage(i) <= 0.0 {
            continue;
        }
        let edge = (1.0 - sharp[i].min(rounded[i]) / EDGE_REACH).clamp(0.0, 1.0);
        let convex = curvature.as_ref().map_or(0.0, |c| ((c[i][0] - 0.5) * 2.0).clamp(0.0, 1.0));
        let cavity = occlusion.as_ref().map_or(0.0, |o| (1.0 - o[i][0]).clamp(0.0, 1.0));
        let raw = edge.max(convex).max(cavity);
        // The grunge eats into the rust as well as adding to it: no solid outline along every edge.
        let v = smoothstep(threshold - half, threshold + half, raw * (1.0 - 0.5 * amount) + (noise[i] - 0.5) * amount * 2.0);
        if v.is_finite() {
            rust[i] = v;
            sources[i] = v * smoothstep(0.5, 0.75, streaks[i]);
        }
    }
    (rust, sources)
}

fn build_field(g: &Generator, set: &TextureSetGeometry, maps: &MeshMaps) -> Field {
    let res = set.resolution.max(1);
    let stride = (res / FIELD_MAX).max(1);
    let size = (res / stride) as usize;
    let drip = g.drip_settings();
    let axis = g.axis.index();
    // One sample per stride x stride block, at its middle texel.
    let pick = stride / 2;
    let samples: Vec<(usize, [f32; 3], f32)> = (0..set.tile_count())
        .into_par_iter()
        .flat_map_iter(|index| {
            let geo = set.tile(index);
            let mut found = Vec::new();
            if !geo.is_empty() {
                let (_, sources) = rust_tile(g, set, maps, &geo);
                for (i, source) in sources.iter().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x % stride == pick && y % stride == pick && geo.coverage(i) > 0.0 {
                        let at = (y / stride) as usize * size + (x / stride) as usize;
                        if at < size * size {
                            found.push((at, geo.position(i), *source));
                        }
                    }
                }
            }
            found
        })
        .collect();
    let mut position = vec![[0.0f32; 3]; size * size];
    let mut covered = vec![false; size * size];
    let mut source = vec![0.0f32; size * size];
    for (at, p, s) in samples {
        position[at] = p;
        covered[at] = true;
        source[at] = s;
    }
    // Positions are in units of the model's longest side; the length is a share of its size
    // along the axis.
    let length = unit(drip.length, 0.25) * (set.bounds.extent()[axis] / set.max_extent).max(1e-6);
    let mut values = subcraft_materials::drip::flow(size, &position, &covered, axis, &source, length, unit(drip.smoothness, 0.3));
    // Keep only what ran down to a texel: a source is rust already, and a level surface, where
    // nothing runs, gets no stain from its own sources.
    for (v, s) in values.iter_mut().zip(&source) {
        if *v <= *s {
            *v = 0.0;
        }
    }
    Field { size, stride, values, covered }
}

impl Field {
    /// The drips at texel (x, y) of the full-resolution set: bilinear over the covered samples.
    fn at(&self, x: u32, y: u32) -> f32 {
        let s = self.stride as f32;
        let (fx, fy) = ((x as f32 + 0.5) / s - 0.5, (y as f32 + 0.5) / s - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        for (dx, dy, w) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
            let (cx, cy) = (x0 as i64 + dx, y0 as i64 + dy);
            if w > 0.0 && cx >= 0 && cy >= 0 && (cx as usize) < self.size && (cy as usize) < self.size {
                let i = cy as usize * self.size + cx as usize;
                if self.covered[i] {
                    sum += self.values[i] * w;
                    weight += w;
                }
            }
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    }
}

/// Coverage of a Dripping Rust generator for one tile.
pub(crate) fn eval(g: &Generator, tc: &TileContext, out: &mut [[f32; 4]]) {
    let drip = g.drip_settings();
    let empty = if g.invert { 1.0 } else { 0.0 };
    if !drip.valid() || tc.geo.is_empty() {
        // A damaged file, or a tile without surface: nothing rather than NaN.
        out.fill([empty, 1.0, 0.0, 0.0]);
        return;
    }
    let (set, maps) = (tc.set_geometry, tc.sample.mesh_maps);
    let (rust, _) = rust_tile(g, set, maps, tc.geo);
    let field = (drip.intensity > 0.0 && drip.length > 0.0).then(|| {
        let key = settings_key(g);
        let (curvature, occlusion) = (maps.get(MeshMapKind::Curvature), maps.get(MeshMapKind::AmbientOcclusion));
        let drips: Arc<Drips> = set.shared(
            |d: &Drips| d.settings == key && same_map(&d.curvature, curvature) && same_map(&d.occlusion, occlusion),
            || Drips { settings: key.clone(), curvature: curvature.cloned(), occlusion: occlusion.cloned(), field: OnceLock::new() },
        );
        drips.field.get_or_init(|| build_field(g, set, maps));
        drips
    });
    // A drip shows from a tenth of its source's strength; a high Rust Contrast ends it sharply.
    let ramp = 0.05 + (1.0 - drip.contrast) * 0.6;
    for (i, o) in out.iter_mut().enumerate() {
        let mut v = rust[i];
        if let Some(field) = field.as_ref().and_then(|d| d.field.get())
            && tc.geo.coverage(i) > 0.0
        {
            let (x, y) = tc.geo.texel_xy(i);
            let d = field.at(x, y) * drip.intensity * 1.5;
            v = v.max(smoothstep(0.1, 0.1 + ramp, d));
        }
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}
