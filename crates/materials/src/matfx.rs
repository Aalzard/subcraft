//! Kernels of the material filters ([`subcraft_doc::MaterialFilter`]: MatFinish / MatFX and PBR
//! Validate).
//!
//! A filter is evaluated in two steps so the expensive part is shared by every channel:
//!
//! 1. [`pattern`]: up to four noise values per texel that depend on the object-space position
//!    and the pattern parameters only (scale, seed, ...). [`pattern_tile`] caches them per tile
//!    like every other procedural field.
//! 2. [`respond`]: turns the pattern plus the surface inputs (edge wear mask, occlusion) into a
//!    [`Response`], the change of Base Color, Roughness, Metallic and Height at that texel. It is
//!    cheap arithmetic, so dragging a colour or an amount never recomputes noise.
//!
//! Everything is sampled in object space (units of the model's longest side): no pattern follows
//! a UV seam.
use glam::Vec3;
use subcraft_doc::{MaterialFilter, ProceduralKind, ProceduralParams};
use subcraft_image::TILE_TEXELS;

use crate::sample::{TileGeometry, field};

/// Pattern values stored per texel.
pub const PATTERN: usize = 4;

/// Height written at full strength, in height units (the viewport bump scale is 0.02 per unit
/// of slope, so these read as a clear but physical relief on a 2K map).
const DIMPLE_HEIGHT: f32 = 0.30;
const FILM_HEIGHT: f32 = 0.04;
const PEEL_HEIGHT: f32 = 0.03;
const GRAIN_HEIGHT: f32 = 0.012;

#[inline]
fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    crate::noise::hash2(crate::noise::hash2(x, y, seed) as i32, z, seed ^ 0x51ed_270b)
}

#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[inline]
fn floor3(p: Vec3) -> Vec3 {
    Vec3::new(crate::noise::floor(p.x), crate::noise::floor(p.y), crate::noise::floor(p.z))
}

#[inline]
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Gradient (Perlin) noise, 0..1 centred on 0.5. Value noise showed its lattice as soft blocks
/// once thresholded into rust patches; gradient noise has no such axis-aligned plateaus.
fn vnoise(p: Vec3, seed: u32) -> f32 {
    let c = floor3(p);
    let f = p - c;
    let u = f * f * f * (f * (f * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));
    let (x, y, z) = (c.x as i32, c.y as i32, c.z as i32);
    let g = |dx: i32, dy: i32, dz: i32| {
        let h = hash3(x + dx, y + dy, z + dz, seed);
        // One of the 12 cube-edge directions, as in improved Perlin noise.
        let d = f - Vec3::new(dx as f32, dy as f32, dz as f32);
        match h % 12 {
            0 => d.x + d.y,
            1 => -d.x + d.y,
            2 => d.x - d.y,
            3 => -d.x - d.y,
            4 => d.x + d.z,
            5 => -d.x + d.z,
            6 => d.x - d.z,
            7 => -d.x - d.z,
            8 => d.y + d.z,
            9 => -d.y + d.z,
            10 => d.y - d.z,
            _ => -d.y - d.z,
        }
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(g(0, 0, 0), g(1, 0, 0), u.x);
    let x10 = lerp(g(0, 1, 0), g(1, 1, 0), u.x);
    let x01 = lerp(g(0, 0, 1), g(1, 0, 1), u.x);
    let x11 = lerp(g(0, 1, 1), g(1, 1, 1), u.x);
    (lerp(lerp(x00, x10, u.y), lerp(x01, x11, u.y), u.z) * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Fractal noise, 0..1 (centred on 0.5).
fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut norm, mut amp, mut freq) = (0.0, 0.0, 0.5, 1.0);
    for o in 0..octaves {
        sum += vnoise(p * freq + Vec3::splat(o as f32 * 19.19), seed.wrapping_add(o.wrapping_mul(1013))) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

/// Three decorrelated noise values around 0 (a domain-warp vector), each in -0.5..0.5.
fn warp(p: Vec3, seed: u32) -> Vec3 {
    Vec3::new(vnoise(p, seed ^ 0x1b3), vnoise(p + Vec3::new(31.7, 11.3, 5.9), seed ^ 0x9e7), vnoise(p + Vec3::new(3.1, 47.2, 23.8), seed ^ 0x4c5)) - Vec3::splat(0.5)
}

struct Cell {
    /// Distance to the nearest and second nearest feature point.
    f1: f32,
    f2: f32,
    id: u32,
    /// The nearest feature point.
    center: Vec3,
}

fn feature(c: Vec3, x: i32, y: i32, z: i32, seed: u32) -> (Vec3, u32) {
    let h = hash3(c.x as i32 + x, c.y as i32 + y, c.z as i32 + z, seed);
    let j = Vec3::new(unit(h), unit(h.rotate_left(11) ^ 0x68e3_1da4), unit(h.rotate_left(22) ^ 0xb529_7a4d));
    (c + Vec3::new(x as f32, y as f32, z as f32) + j * 0.86 + Vec3::splat(0.07), h)
}

fn voronoi(p: Vec3, seed: u32) -> Cell {
    let c = floor3(p);
    let mut out = Cell { f1: f32::MAX, f2: f32::MAX, id: 0, center: p };
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                let (q, h) = feature(c, x, y, z, seed);
                let d = (q - p).length_squared();
                if d < out.f1 {
                    out.f2 = out.f1;
                    out.f1 = d;
                    out.id = h;
                    out.center = q;
                } else if d < out.f2 {
                    out.f2 = d;
                }
            }
        }
    }
    out.f1 = out.f1.sqrt();
    out.f2 = out.f2.sqrt();
    out
}

/// A unit vector and two more that complete an orthonormal frame, from a hash.
fn frame(h: u32) -> [Vec3; 3] {
    let z = unit(h) * 2.0 - 1.0;
    let a = unit(h.rotate_left(13) ^ 0x7f4a_7c15) * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let d = Vec3::new(r * a.cos(), r * a.sin(), z);
    let e1 = d.any_orthonormal_vector();
    [d, e1, d.cross(e1)]
}

/// The noise values of `filter` at `position` (object space, units of the model's longest side).
/// What each slot means is private to the filter's branch of [`respond`].
pub fn pattern(filter: &MaterialFilter, position: [f32; 3]) -> [f32; PATTERN] {
    pattern_on(filter, position, [0.0; 3])
}

/// [`pattern`] on a surface with normal `normal` (object space; zero when unknown). Only filters
/// with a direction read it: their lines must lie in the surface, whichever way it faces.
pub fn pattern_on(filter: &MaterialFilter, position: [f32; 3], normal: [f32; 3]) -> [f32; PATTERN] {
    let p = Vec3::from_array(position);
    match *filter {
        MaterialFilter::Galvanized { scale, seed, .. } => {
            let q = p * scale;
            let q = q + warp(q * 0.6, seed) * 0.5;
            let cell = voronoi(q, seed);
            let [d, e1, e2] = frame(cell.id);
            // Feathery crystal growth: long streaks along the spangle's own direction.
            let local = q - cell.center;
            let streak = fbm(Vec3::new(local.dot(d) * 1.4, local.dot(e1) * 9.0, local.dot(e2) * 9.0) + Vec3::splat(unit(cell.id) * 64.0), 3, seed ^ 0x6a1);
            [unit(cell.id.wrapping_mul(0x2545_f491) ^ 0x9e37_79b9), streak, cell.f2 - cell.f1, 0.0]
        }
        MaterialFilter::Hammered { scale, irregularity, seed, .. } => {
            let q = p * scale;
            let q = q + warp(q * 0.45, seed) * irregularity * 0.9;
            // A smooth minimum of paraboloid dishes: every dimple is a shallow bowl and the
            // ridges where two bowls meet are slightly rounded, as hammered sheet is.
            let c = floor3(q);
            let (mut sum, mut nearest, mut id) = (0.0f32, f32::MAX, 0u32);
            const K: f32 = 16.0;
            for z in -1..=1 {
                for y in -1..=1 {
                    for x in -1..=1 {
                        let (f, h) = feature(c, x, y, z, seed);
                        let (r1, r2) = (unit(h.rotate_left(7) ^ 0x3c6e_f372), unit(h.rotate_left(19) ^ 0xa54f_f53a));
                        let w = (f - q).length_squared() * (0.85 + 0.5 * r1 * irregularity) + 0.14 * r2 * irregularity;
                        sum += (-K * w).exp();
                        if w < nearest {
                            nearest = w;
                            id = h;
                        }
                    }
                }
            }
            let h = -sum.max(1e-30).ln() / K;
            [(h / 0.62).clamp(0.0, 1.0), unit(id.wrapping_mul(0x2545_f491)), vnoise(q * 7.0, seed ^ 0x77), 0.0]
        }
        MaterialFilter::PowderCoated { scale, seed, .. } => {
            let q = p * scale;
            [fbm(q, 2, seed), vnoise(q * 6.0, seed ^ 0x51), 0.0, 0.0]
        }
        MaterialFilter::EdgeDamages { scale, seed, .. } => {
            let q = p * scale;
            [fbm(q * 0.5, 3, seed ^ 0x2a), vnoise(q * 4.0, seed ^ 0x93), 0.0, 0.0]
        }
        MaterialFilter::RustWeathering { scale, seed, .. } => {
            let q = p * scale;
            let big = fbm(q + warp(q * 0.5, seed) * 1.2, 4, seed);
            let fine = fbm(q * 12.0, 4, seed ^ 0x35);
            // Isolated pits: only some cells carry one, small and round.
            let cell = voronoi(q * 6.0, seed ^ 0x818);
            let pit = if unit(cell.id) > 0.55 { 1.0 - smoothstep(0.0, 0.22 + 0.2 * unit(cell.id.rotate_left(9)), cell.f1) } else { 0.0 };
            // Runs: noise stretched along the model's vertical axis.
            let run = fbm(Vec3::new(q.x * 6.0, q.y * 0.45, q.z * 6.0), 2, seed ^ 0xd21);
            [big, fine, pit, run]
        }
        MaterialFilter::PeelingPaint { scale, seed, .. } => {
            let params = ProceduralParams { scale: scale / 3.0, seed, ..ProceduralParams::new(ProceduralKind::Clouds) };
            let flake = crate::chips::eval(&params, position);
            let q = p * scale * 3.2;
            let cell = voronoi(q + warp(q * 0.7, seed ^ 0x44) * 0.8, seed ^ 0x1f3);
            [flake, cell.f2 - cell.f1, fbm(p * scale * 7.0, 4, seed ^ 0x62), vnoise(p * scale * 30.0, seed ^ 0x29)]
        }
        MaterialFilter::PbrValidate { .. } => [0.0; PATTERN],
        MaterialFilter::Grinded { scale, seed, .. } => {
            let q = p * scale;
            let q = q + warp(q * 0.5, seed) * 0.35;
            // One grinder pass per cell: scratches are circles around the disc's pivot, which
            // sits well outside the cell, so each pass is a fan of near-parallel arcs.
            let cell = voronoi(q, seed);
            let [d, e1, e2] = frame(cell.id);
            let (a, b) = (unit(cell.id.rotate_left(5) ^ 0x1f83_d9ab) - 0.5, unit(cell.id.rotate_left(17) ^ 0x5be0_cd19) - 0.5);
            let l = q - (cell.center + (e1 * a + e2 * b).normalize_or_zero() * 2.2);
            let along = l.dot(d);
            let radius = (l - d * along).length();
            let offset = Vec3::splat(unit(cell.id) * 64.0);
            let scratch = fbm(Vec3::new(radius * 26.0, along * 1.2, 0.0) + offset, 3, seed ^ 0x3d);
            let band = fbm(Vec3::new(radius * 4.0, along * 0.6, 7.0) + offset, 2, seed ^ 0x8f);
            [scratch, unit(cell.id.wrapping_mul(0x2545_f491) ^ 0x9e37_79b9), band, 0.0]
        }
        MaterialFilter::Rough { scale, grunge_scale, warp: wander, seed, .. } => {
            let q = p * scale;
            let q = q + warp(q * 0.12, seed) * wander * 2.5;
            // Brush lines run along the model's X axis: noise stretched 50 times that way. A face
            // looking down X would cut the lines into dots, so there they run along Y instead.
            let turn = smoothstep(0.6, 0.85, normal[0].abs());
            let along_x = if turn < 1.0 { fbm(Vec3::new(q.x * 0.02, q.y, q.z), 3, seed ^ 0x5b) } else { 0.5 };
            let along_y = if turn > 0.0 { fbm(Vec3::new(q.x, q.y * 0.02, q.z), 3, seed ^ 0x5b) } else { 0.5 };
            let brush = along_x + (along_y - along_x) * turn;
            let g = p * grunge_scale;
            let grunge = fbm(g + warp(g * 0.5, seed ^ 0xa7) * 0.9, 4, seed ^ 0xa7);
            [brush, grunge, vnoise(q * 3.0, seed ^ 0x17), 0.0]
        }
        MaterialFilter::Raw { scale, seed, .. } => {
            let q = p * scale;
            let big = fbm(q + warp(q * 0.5, seed) * 1.1, 4, seed);
            let mid = fbm(q * 7.0, 3, seed ^ 0x2d);
            let cell = voronoi(q * 9.0, seed ^ 0x6c1);
            let pit = if unit(cell.id) > 0.7 { 1.0 - smoothstep(0.0, 0.16 + 0.14 * unit(cell.id.rotate_left(9)), cell.f1) } else { 0.0 };
            [big, mid, pit, vnoise(q * 40.0, seed ^ 0x99)]
        }
        MaterialFilter::DetailEdgeWear { grunge_scale, seed, .. } => {
            let q = p * grunge_scale;
            [fbm(q + warp(q * 0.6, seed) * 0.7, 4, seed ^ 0x4e), fbm(q * 5.0, 3, seed ^ 0xb3), vnoise(q * 18.0, seed ^ 0x61), 0.0]
        }
    }
}

/// The parameter bits [`pattern`] reads (and nothing else), as a field key.
fn pattern_key(filter: &MaterialFilter) -> field::FieldKey {
    let bits = match *filter {
        MaterialFilter::Galvanized { scale, seed, .. } => [1, scale.to_bits(), seed, 0, 0],
        MaterialFilter::Hammered { scale, irregularity, seed, .. } => [2, scale.to_bits(), seed, irregularity.to_bits(), 0],
        MaterialFilter::PowderCoated { scale, seed, .. } => [3, scale.to_bits(), seed, 0, 0],
        MaterialFilter::EdgeDamages { scale, seed, .. } => [4, scale.to_bits(), seed, 0, 0],
        MaterialFilter::RustWeathering { scale, seed, .. } => [5, scale.to_bits(), seed, 0, 0],
        MaterialFilter::PeelingPaint { scale, seed, .. } => [6, scale.to_bits(), seed, 0, 0],
        MaterialFilter::PbrValidate { .. } => [7, 0, 0, 0, 0],
        MaterialFilter::Grinded { scale, seed, .. } => [8, scale.to_bits(), seed, 0, 0],
        MaterialFilter::Rough { scale, grunge_scale, warp, seed, .. } => [9, scale.to_bits(), seed, grunge_scale.to_bits(), warp.to_bits()],
        MaterialFilter::Raw { scale, seed, .. } => [10, scale.to_bits(), seed, 0, 0],
        MaterialFilter::DetailEdgeWear { grunge_scale, seed, .. } => [11, grunge_scale.to_bits(), seed, 0, 0],
    };
    field::FieldKey::matfx(bits)
}

/// [`pattern`] of every texel of a tile ([`PATTERN`] values per texel), cached on the tile.
pub fn pattern_tile(filter: &MaterialFilter, geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(pattern_key(filter), || {
        let directional = matches!(filter, MaterialFilter::Rough { .. });
        let mut out = Vec::with_capacity(TILE_TEXELS * PATTERN);
        for i in 0..TILE_TEXELS {
            let normal = if directional { geo.normal(i) } else { [0.0; 3] };
            out.extend_from_slice(&pattern_on(filter, geo.position(i), normal));
        }
        out
    })
}

/// Surface inputs of a texel that do not come from the pattern.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Surface {
    /// Edge wear mask, 0..1 (1 on a worn edge).
    pub edge: f32,
    /// 0 = open, 1 = fully occluded (1 - baked ambient occlusion).
    pub occlusion: f32,
}

/// What a filter does to one texel of the material below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Base Color: the colour below is multiplied by `color_gain`, then `color` is laid over it
    /// with `color_alpha`.
    pub color: [f32; 3],
    pub color_alpha: f32,
    pub color_gain: f32,
    /// Roughness: `roughness_add` is added to the value below, then `roughness` is laid over it.
    pub roughness: f32,
    pub roughness_alpha: f32,
    pub roughness_add: f32,
    pub metallic: f32,
    pub metallic_alpha: f32,
    /// Added to the Height below.
    pub height: f32,
}

impl Response {
    pub const NONE: Self = Self { color: [0.0; 3], color_alpha: 0.0, color_gain: 1.0, roughness: 0.0, roughness_alpha: 0.0, roughness_add: 0.0, metallic: 0.0, metallic_alpha: 0.0, height: 0.0 };

    pub fn base_color(&self, below: [f32; 3]) -> [f32; 3] {
        mix3(below.map(|c| c * self.color_gain), self.color, self.color_alpha)
    }
    pub fn rough(&self, below: f32) -> f32 {
        let b = (below + self.roughness_add).clamp(0.0, 1.0);
        b + (self.roughness - b) * self.roughness_alpha
    }
    pub fn metal(&self, below: f32) -> f32 {
        below + (self.metallic - below) * self.metallic_alpha
    }
}

const RUST_BRIGHT: [f32; 3] = [0.37, 0.155, 0.048];
const RUST_MID: [f32; 3] = [0.20, 0.075, 0.028];
const RUST_DARK: [f32; 3] = [0.062, 0.030, 0.019];
const RUST_STAIN: [f32; 3] = [0.30, 0.15, 0.07];
/// Standard deviation of [`fbm`] around 0.5 (measured: 0.08 to 0.10 for 2 to 4 octaves), to turn
/// a noise value into a z-score so thresholds mean a known coverage.
const FBM_SD: f32 = 0.088;

#[inline]
fn z(v: f32) -> f32 {
    (v - 0.5) / FBM_SD
}

/// [`z`] cut at 2.5 standard deviations, for values used as a signed strength.
#[inline]
fn zc(v: f32) -> f32 {
    z(v).clamp(-2.5, 2.5)
}
const ZINC: [f32; 3] = [0.78, 0.80, 0.83];
const MILL_SCALE: [f32; 3] = [0.085, 0.095, 0.115];

/// Rust colour for a thickness 0..1 (thin rust is orange, thick rust dark brown) and a grain 0..1.
fn rust_color(thickness: f32, grain: f32) -> [f32; 3] {
    let g = (0.5 + z(grain) * 0.22).clamp(0.0, 1.0);
    let c = mix3(RUST_BRIGHT, RUST_MID, smoothstep(0.0, 0.6, thickness * 0.8 + g * 0.3));
    let c = mix3(c, RUST_DARK, smoothstep(0.5, 1.1, thickness * 0.6 + g * 0.6));
    c.map(|v| v * (0.7 + 0.6 * g))
}

/// The change `filter` makes at a texel with pattern values `pat` (from [`pattern`]) and
/// surface inputs `s`. PBR Validate has no response (see [`validate`]).
pub fn respond(filter: &MaterialFilter, pat: &[f32], s: Surface) -> Response {
    let mut r = Response::NONE;
    match *filter {
        MaterialFilter::Galvanized { spangle, tint, roughness, roughness_variation, relief, .. } => {
            let (cell, streak, border) = (pat[0], pat[1], pat[2]);
            let b = (cell - 0.5) * 0.55 + z(streak) * 0.2;
            r.color_gain = (1.0 + b * 0.75 * spangle).max(0.0);
            r.color = ZINC.map(|c| (c * r.color_gain).clamp(0.0, 1.0));
            r.color_alpha = tint;
            r.roughness = (roughness + ((cell - 0.5) * 0.6 + z(streak) * 0.12) * roughness_variation * 0.7).clamp(0.03, 1.0);
            r.roughness_alpha = 1.0;
            r.metallic = 1.0;
            r.metallic_alpha = 1.0;
            let groove = 1.0 - smoothstep(0.0, 0.08, border);
            r.height = (-groove * 0.6 + z(streak) * 0.08) * relief * GRAIN_HEIGHT;
        }
        MaterialFilter::Hammered { depth, roughness_variation, .. } => {
            let (h, cell, fine) = (pat[0], pat[1], pat[2]);
            r.height = ((h - 0.5) + (fine - 0.5) * 0.04) * depth * DIMPLE_HEIGHT;
            // Ridges are burnished by handling; the bowls keep the hammer's matte face.
            r.roughness_add = (-(h - 0.5) * 0.4 + (cell - 0.5) * 0.12 + (fine - 0.5) * 0.06) * roughness_variation;
            r.color_gain = 1.0 + ((h - 0.5) * 0.14 + (cell - 0.5) * 0.06) * roughness_variation;
        }
        MaterialFilter::PowderCoated { color, color_amount, roughness, orange_peel, grain, .. } => {
            let (peel, g) = (pat[0], pat[1]);
            r.color_gain = 1.0 + (g - 0.5) * 0.08 * grain;
            r.color = color.map(|c| (c * r.color_gain).clamp(0.0, 1.0));
            r.color_alpha = color_amount;
            r.roughness = (roughness + (peel - 0.5) * 0.10 + (g - 0.5) * 0.14 * grain).clamp(0.03, 1.0);
            r.roughness_alpha = 1.0;
            r.metallic = 0.0;
            r.metallic_alpha = 1.0;
            r.height = (peel - 0.5) * orange_peel * FILM_HEIGHT * 0.45 + (g - 0.5) * grain * GRAIN_HEIGHT * 0.15;
        }
        MaterialFilter::EdgeDamages { depth, color, roughness, metallic, .. } => {
            let (var, fine) = (pat[0], pat[1]);
            let m = s.edge.clamp(0.0, 1.0);
            r.color = color.map(|c| (c * (1.0 + z(var) * 0.07)).clamp(0.0, 1.0));
            r.color_alpha = m;
            r.roughness = (roughness + (fine - 0.5) * 0.25 + z(var) * 0.03).clamp(0.03, 1.0);
            r.roughness_alpha = m;
            r.metallic = metallic;
            r.metallic_alpha = m;
            r.height = -m * depth * FILM_HEIGHT * (0.75 + 0.5 * fine);
        }
        MaterialFilter::RustWeathering { amount, edges, cavities, drips, relief, roughness, .. } => {
            if amount <= 0.0 {
                return r;
            }
            let (big, fine, pit, run) = (pat[0], pat[1], pat[2], pat[3]);
            // In z-scores of the patch noise, so Amount is a known coverage on an open surface:
            // about 3% at 0.25, 20% at 0.45, 60% at 0.7, everything at 1.
            let threshold = 3.2 - amount * 5.2;
            let d = z(big) + edges * s.edge.clamp(0.0, 1.0) * 2.2 + cavities * s.occlusion.clamp(0.0, 1.0) * 4.0 + drips * z(run) * 0.8 * smoothstep(-0.5, 1.0, z(big)) - threshold;
            let d = d + z(fine) * 0.4;
            let pits = smoothstep(0.3, 0.6, pit) * smoothstep(0.15, 0.6, amount + z(big) * 0.08);
            let m = smoothstep(0.0, 0.55, d).max(pits);
            // A thin orange stain reaches further than the rust itself.
            let stain = smoothstep(-1.8, 0.1, d) * 0.3;
            // Pits are small but deep: dark, old rust.
            let thickness = smoothstep(0.2, 2.6, d).max(pits);
            r.color = mix3(RUST_STAIN, rust_color(thickness, fine), m);
            r.color_alpha = m.max(stain);
            r.roughness = (roughness + z(fine) * 0.03).clamp(0.03, 1.0);
            r.roughness_alpha = m.max(stain * 0.6);
            r.metallic = 0.0;
            r.metallic_alpha = m.max(stain * 0.35);
            let grit = (0.5 + z(fine) * 0.25).clamp(0.0, 1.0);
            r.height = m * relief * PEEL_HEIGHT * (0.2 + 0.8 * grit + 0.5 * thickness) - pits * relief * PEEL_HEIGHT * 0.6;
        }
        MaterialFilter::PeelingPaint { amount, edge_bias, thickness, curl, color, roughness, metallic, rust, .. } => {
            if amount <= 0.0 {
                return r;
            }
            let (flake, crack, under, fine) = (pat[0], pat[1], pat[2], pat[3]);
            let drive = flake + edge_bias * s.edge.clamp(0.0, 1.0) * 0.35 + z(under) * 0.012;
            let t = 1.04 - amount * 1.2;
            let w = 0.012;
            let peel = smoothstep(t - w, t + w, drive);
            let lip = smoothstep(t - 0.10, t - w, drive) * (1.0 - peel);
            let crack_zone = smoothstep(t - 0.30, t - 0.05, drive) * (1.0 - peel);
            let crack = (1.0 - smoothstep(0.0, 0.05, crack)) * crack_zone;
            let rusty = if rust > 0.0 {
                let t = 2.4 - rust * 4.8;
                smoothstep(t - 0.7, t + 0.7, z(under) + (fine - 0.5) * 1.5)
            } else {
                0.0
            };
            r.color = mix3(color.map(|c| c * (0.88 + 0.24 * fine)), rust_color(rusty * (0.4 + 0.6 * fine), under), rusty).map(|c| c.clamp(0.0, 1.0));
            r.color_alpha = peel;
            r.color_gain = 1.0 - crack * 0.4 + lip * 0.06;
            r.roughness = roughness + (0.88 - roughness) * rusty + (fine - 0.5) * 0.12;
            r.roughness = r.roughness.clamp(0.03, 1.0);
            r.roughness_alpha = peel;
            r.roughness_add = crack * 0.12;
            r.metallic = metallic * (1.0 - rusty);
            r.metallic_alpha = peel;
            r.height = (-peel + lip * lip * curl * 0.9 - crack * 0.5) * thickness * PEEL_HEIGHT + peel * rusty * (fine - 0.3) * thickness * PEEL_HEIGHT * 0.4;
        }
        MaterialFilter::PbrValidate { .. } => {}
        MaterialFilter::Grinded { intensity, .. } => {
            let (scratch, pass, band) = (zc(pat[0]), pat[1] - 0.5, zc(pat[2]));
            // Each pass catches the light differently; inside it the scratches are fine grooves.
            r.height = (scratch * 0.22 + band * 0.08) * intensity * GRAIN_HEIGHT;
            r.roughness_add = (scratch * 0.045 + band * 0.03 + pass * 0.12) * intensity;
            r.color_gain = 1.0 + (scratch * 0.03 + band * 0.015 + pass * 0.10) * intensity;
        }
        MaterialFilter::Rough { brushing, grunge, .. } => {
            let (brush, patch, fine) = (zc(pat[0]), smoothstep(-0.4, 1.4, z(pat[1])), pat[2] - 0.5);
            r.height = (brush * 0.4 + fine * 0.1) * brushing * GRAIN_HEIGHT;
            r.roughness_add = (0.06 + brush * 0.08 + fine * 0.03) * brushing + (patch - 0.25) * 0.24 * grunge;
            r.color_gain = (1.0 + (brush * 0.035 + fine * 0.03) * brushing - patch * 0.22 * grunge).max(0.0);
        }
        MaterialFilter::Raw { intensity, .. } => {
            let (big, mid, pit, fine) = (z(pat[0]), zc(pat[1]), pat[2], pat[3] - 0.5);
            // Mill scale: a dark blue-gray oxide skin, thicker in patches, flaked off in others.
            let skin = smoothstep(-1.2, 1.0, big + mid * 0.45);
            r.color = MILL_SCALE.map(|c| (c * (1.0 + mid * 0.08 + fine * 0.1)).clamp(0.0, 1.0));
            r.color_alpha = (skin * 0.7 + pit * 0.3).min(1.0) * intensity;
            r.color_gain = 1.0 + (mid * 0.04 + fine * 0.05) * intensity;
            r.roughness_add = (0.10 + skin * 0.28 + mid * 0.04 + fine * 0.08 + pit * 0.15) * intensity;
            r.height = (skin * 0.6 + mid * 0.12 + fine * 0.2 - pit * 1.2) * intensity * GRAIN_HEIGHT;
        }
        MaterialFilter::DetailEdgeWear { contrast, grunge, color, roughness, metallic, .. } => {
            let (big, fine, grain) = (z(pat[0]), zc(pat[1]), pat[2] - 0.5);
            let broken = s.edge.clamp(0.0, 1.0) * (1.0 - grunge * smoothstep(-0.8, 0.9, big + fine * 0.5));
            let w = 0.5 - 0.47 * contrast;
            let m = smoothstep(0.5 - w, 0.5 + w, broken);
            r.color = color.map(|c| (c * (1.0 + grain * 0.12)).clamp(0.0, 1.0));
            r.color_alpha = m;
            r.roughness = (roughness + grain * 0.16 + fine * 0.02).clamp(0.03, 1.0);
            r.roughness_alpha = m;
            r.metallic = metallic;
            r.metallic_alpha = m;
            r.height = -m * FILM_HEIGHT * 0.2;
        }
    }
    r
}

/// Verdict of [`validate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    TooDark,
    TooBright,
    PartialMetal,
}

impl Verdict {
    /// Flag colour (linear RGB).
    pub fn color(self) -> [f32; 3] {
        match self {
            Self::Ok => [0.05, 0.55, 0.08],
            Self::TooDark => [0.80, 0.03, 0.03],
            Self::TooBright => [0.05, 0.12, 0.85],
            Self::PartialMetal => [0.85, 0.65, 0.02],
        }
    }
}

/// sRGB-encoded 0..255 luminance of a linear colour.
pub fn srgb_luminance_8bit(c: [f32; 3]) -> f32 {
    let l = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).clamp(0.0, 1.0);
    let e = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    e * 255.0
}

/// PBR plausibility of a linear Base Color with its Metallic. Dielectrics: 30 sRGB (50 when
/// `strict`) to 240 sRGB. Metals: at least 180 sRGB (70% reflectance). Metallic values between
/// 0.1 and 0.9 are flagged when `flag_partial_metal` is on; otherwise they are judged by the
/// nearer of the two rules.
pub fn validate(base: [f32; 3], metallic: f32, strict: bool, flag_partial_metal: bool) -> Verdict {
    let l = srgb_luminance_8bit(base);
    if flag_partial_metal && metallic > 0.1 && metallic < 0.9 {
        return Verdict::PartialMetal;
    }
    if metallic >= 0.5 {
        if l < 180.0 { Verdict::TooDark } else { Verdict::Ok }
    } else if l < if strict { 50.0 } else { 30.0 } {
        Verdict::TooDark
    } else if l > 240.0 {
        Verdict::TooBright
    } else {
        Verdict::Ok
    }
}

/// The colour PBR Validate shows: the verdict's flag over a dim gray copy of the original, so
/// the texture's detail still reads.
pub fn validate_color(base: [f32; 3], verdict: Verdict) -> [f32; 3] {
    let l = (0.2126 * base[0] + 0.7152 * base[1] + 0.0722 * base[2]).clamp(0.0, 1.0);
    let flag = verdict.color();
    std::array::from_fn(|i| flag[i] * (0.55 + 0.45 * l.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points() -> impl Iterator<Item = [f32; 3]> {
        (0..4000).map(|k| [(k % 61) as f32 / 61.0, (k % 37) as f32 / 37.0 + k as f32 * 1e-4, (k % 17) as f32 / 17.0])
    }

    #[test]
    fn patterns_are_finite_bounded_and_deterministic() {
        for f in MaterialFilter::ALL {
            for p in points() {
                let a = pattern(&f, p);
                assert_eq!(a, pattern(&f, p), "{}", f.label());
                for v in a {
                    assert!(v.is_finite() && (-0.001..=1.75).contains(&v), "{} {v} at {p:?}", f.label());
                }
            }
        }
    }

    #[test]
    fn patterns_are_continuous_in_object_space() {
        // No seams: a tiny step never jumps (cell ids do, so only the smooth slots are checked).
        let smooth: [(MaterialFilter, &[usize]); 4] = [
            (MaterialFilter::ALL[1], &[0]),
            (MaterialFilter::ALL[2], &[0, 1]),
            (MaterialFilter::ALL[3], &[0, 1]),
            (MaterialFilter::ALL[4], &[0, 1, 3]),
        ];
        for (f, slots) in smooth {
            let mut worst = 0.0f32;
            for p in points() {
                let (a, b) = (pattern(&f, p), pattern(&f, [p[0] + 4e-5, p[1] + 2e-5, p[2] - 2e-5]));
                for &s in slots {
                    worst = worst.max((a[s] - b[s]).abs());
                }
            }
            assert!(worst < 0.05, "{} jumps by {worst}", f.label());
        }
    }

    #[test]
    fn seed_and_scale_change_the_pattern() {
        let a = MaterialFilter::Hammered { scale: 16.0, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
        let b = MaterialFilter::Hammered { scale: 16.0, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 2 };
        let c = MaterialFilter::Hammered { scale: 24.0, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
        let diff = |x: &MaterialFilter, y: &MaterialFilter| points().map(|p| (pattern(x, p)[0] - pattern(y, p)[0]).abs()).sum::<f32>() / 4000.0;
        assert!(diff(&a, &b) > 0.05 && diff(&a, &c) > 0.05);
        assert_ne!(pattern_key(&a), pattern_key(&b));
        assert_ne!(pattern_key(&a), pattern_key(&c));
        // Depth does not change the pattern: the cached field is shared while it is dragged.
        let d = MaterialFilter::Hammered { scale: 16.0, depth: 0.9, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
        assert_eq!(pattern_key(&a), pattern_key(&d));
    }

    #[test]
    fn hammered_dimples_cover_the_height_range_and_only_move_height_and_roughness() {
        let f = MaterialFilter::ALL[1];
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for p in points() {
            let r = respond(&f, &pattern(&f, p), Surface::default());
            lo = lo.min(r.height);
            hi = hi.max(r.height);
            assert_eq!((r.color_alpha, r.metallic_alpha, r.roughness_alpha), (0.0, 0.0, 0.0));
            assert!((r.color_gain - 1.0).abs() < 0.1);
        }
        assert!(lo < -0.03 && hi > 0.01, "dimples span {lo}..{hi}");
        // Zero depth: flat.
        let flat = MaterialFilter::Hammered { scale: 16.0, depth: 0.0, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
        assert!(points().all(|p| respond(&flat, &pattern(&flat, p), Surface::default()).height == 0.0));
    }

    #[test]
    fn galvanized_is_a_full_metal_with_varied_spangles() {
        let f = MaterialFilter::ALL[0];
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for p in points() {
            let r = respond(&f, &pattern(&f, p), Surface::default());
            assert_eq!((r.metallic, r.metallic_alpha, r.roughness_alpha), (1.0, 1.0, 1.0));
            lo = lo.min(r.roughness);
            hi = hi.max(r.roughness);
            // Zinc stays a plausible metal albedo.
            assert_eq!(validate(r.base_color([0.8; 3]), 1.0, false, true), Verdict::Ok, "{:?}", r.base_color([0.8; 3]));
        }
        assert!(hi - lo > 0.15, "spangles differ in roughness: {lo}..{hi}");
    }

    #[test]
    fn powder_coat_is_a_dielectric_and_keeps_the_colour_below_by_default() {
        let f = MaterialFilter::ALL[2];
        for p in points().take(500) {
            let r = respond(&f, &pattern(&f, p), Surface::default());
            assert_eq!((r.metallic, r.metallic_alpha, r.color_alpha), (0.0, 1.0, 0.0));
            let c = r.base_color([0.6, 0.1, 0.1]);
            assert!((c[0] - 0.6).abs() < 0.03 && (c[1] - 0.1).abs() < 0.01);
            assert!((r.rough(0.1) - 0.48).abs() < 0.13);
        }
    }

    #[test]
    fn edge_damages_follow_the_edge_mask_exactly() {
        let f = MaterialFilter::ALL[3];
        for p in points().take(500) {
            let pat = pattern(&f, p);
            let off = respond(&f, &pat, Surface { edge: 0.0, occlusion: 0.0 });
            assert_eq!((off.color_alpha, off.roughness_alpha, off.metallic_alpha, off.height), (0.0, 0.0, 0.0, 0.0));
            assert_eq!(off.base_color([0.2, 0.3, 0.4]), [0.2, 0.3, 0.4]);
            let on = respond(&f, &pat, Surface { edge: 1.0, occlusion: 0.0 });
            assert_eq!((on.color_alpha, on.metallic_alpha, on.metal(0.0)), (1.0, 1.0, 1.0));
            assert!(on.height < 0.0, "chips are carved");
        }
    }

    #[test]
    fn finishes_keep_the_metal_and_only_texture_it() {
        // Grinded, Rough and Raw at their defaults: Metallic untouched, Roughness and Height vary.
        for f in [MaterialFilter::ALL[7], MaterialFilter::ALL[8], MaterialFilter::ALL[9]] {
            let (mut rough, mut height) = ((f32::MAX, f32::MIN), (f32::MAX, f32::MIN));
            for p in points() {
                let r = respond(&f, &pattern(&f, p), Surface::default());
                assert_eq!((r.metallic_alpha, r.roughness_alpha), (0.0, 0.0), "{}", f.label());
                assert!(r.color_gain > 0.5 && r.color_gain < 1.5 && r.height.abs() < 0.05, "{}", f.label());
                let v = r.rough(0.3);
                rough = (rough.0.min(v), rough.1.max(v));
                height = (height.0.min(r.height), height.1.max(r.height));
            }
            assert!(rough.1 - rough.0 > 0.08, "{} roughness {rough:?}", f.label());
            assert!(height.1 - height.0 > 0.002, "{} height {height:?}", f.label());
        }
        // Zero strength changes nothing.
        let off = [
            MaterialFilter::Grinded { scale: 9.0, intensity: 0.0, seed: 1 },
            MaterialFilter::Rough { scale: 60.0, grunge_scale: 5.0, brushing: 0.0, warp: 0.3, grunge: 0.0, seed: 1 },
            MaterialFilter::Raw { scale: 5.0, intensity: 0.0, seed: 1 },
        ];
        for f in off {
            for p in points().take(500) {
                let r = respond(&f, &pattern(&f, p), Surface::default());
                assert_eq!((r.base_color([0.5; 3]), r.rough(0.3), r.height), ([0.5; 3], 0.3, 0.0), "{}", f.label());
            }
        }
        // Raw darkens toward mill scale; Grinded and Rough never lay a colour over the metal.
        let raw = MaterialFilter::ALL[9];
        let mean = points().map(|p| respond(&raw, &pattern(&raw, p), Surface::default()).base_color([0.56; 3])[0]).sum::<f32>() / 4000.0;
        assert!(mean < 0.5 && mean > 0.2, "{mean}");
        for f in [MaterialFilter::ALL[7], MaterialFilter::ALL[8]] {
            assert!(points().all(|p| respond(&f, &pattern(&f, p), Surface::default()).color_alpha == 0.0));
        }
    }

    #[test]
    fn grinder_scratches_run_along_arcs() {
        // Inside one pass the pattern changes much faster across the scratches (radially) than
        // along them: compare the steepest and the shallowest of many small steps.
        let f = MaterialFilter::ALL[7];
        let (mut aniso, mut n) = (0.0f32, 0);
        for p in points().take(400) {
            let base = pattern(&f, p);
            let mut steps = Vec::new();
            for k in 0..24 {
                let d = frame(hash3(k, 3, 9, 77))[0] * 6e-4;
                let q = pattern(&f, [p[0] + d.x, p[1] + d.y, p[2] + d.z]);
                if q[1] == base[1] {
                    steps.push((q[0] - base[0]).abs());
                }
            }
            if steps.len() == 24 {
                let (lo, hi) = steps.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
                aniso += hi / (lo + 1e-4);
                n += 1;
            }
        }
        assert!(n > 100 && aniso / n as f32 > 6.0, "{} over {n}", aniso / n as f32);
    }

    #[test]
    fn rough_brush_lines_lie_in_the_surface_whichever_way_it_faces() {
        // The pattern barely changes along the brush direction and changes fast across it: along
        // X on a face looking up, along Y on a face looking down X.
        let f = MaterialFilter::Rough { scale: 60.0, grunge_scale: 9.0, brushing: 0.6, warp: 0.0, grunge: 0.5, seed: 1 };
        let step = |normal: [f32; 3], d: [f32; 3]| points().take(800).map(|p| (pattern_on(&f, p, normal)[0] - pattern_on(&f, [p[0] + d[0], p[1] + d[1], p[2] + d[2]], normal)[0]).abs()).sum::<f32>() / 800.0;
        let e = 4e-3;
        assert!(step([0.0, 1.0, 0.0], [e, 0.0, 0.0]) * 8.0 < step([0.0, 1.0, 0.0], [0.0, 0.0, e]));
        assert!(step([1.0, 0.0, 0.0], [0.0, e, 0.0]) * 8.0 < step([1.0, 0.0, 0.0], [0.0, 0.0, e]));
        // Unknown normal: the X lines.
        assert_eq!(pattern(&f, [0.3, 0.2, 0.1]), pattern_on(&f, [0.3, 0.2, 0.1], [0.0, 0.0, 1.0]));
    }

    #[test]
    fn rough_warp_and_grunge_scale_change_the_pattern_but_strengths_do_not() {
        let rough = |grunge_scale: f32, warp: f32, brushing: f32| MaterialFilter::Rough { scale: 60.0, grunge_scale, brushing, warp, grunge: 0.5, seed: 1 };
        assert_ne!(pattern_key(&rough(5.0, 0.3, 0.6)), pattern_key(&rough(6.0, 0.3, 0.6)));
        assert_ne!(pattern_key(&rough(5.0, 0.3, 0.6)), pattern_key(&rough(5.0, 0.4, 0.6)));
        assert_eq!(pattern_key(&rough(5.0, 0.3, 0.6)), pattern_key(&rough(5.0, 0.3, 0.9)));
        // Every filter kind has its own key.
        let keys: std::collections::BTreeSet<_> = MaterialFilter::ALL.iter().map(|f| format!("{:?}", pattern_key(f))).collect();
        assert_eq!(keys.len(), MaterialFilter::ALL.len());
    }

    #[test]
    fn detail_edge_wear_stays_on_the_edges_and_grunge_breaks_it_up() {
        let wear = |contrast: f32, grunge: f32| MaterialFilter::DetailEdgeWear { level: 0.5, contrast, grunge, grunge_scale: 24.0, color: [0.56, 0.57, 0.58], roughness: 0.3, metallic: 1.0, seed: 1 };
        let on = Surface { edge: 1.0, occlusion: 0.0 };
        for p in points().take(500) {
            let f = wear(0.6, 0.5);
            let off = respond(&f, &pattern(&f, p), Surface::default());
            assert_eq!((off.color_alpha, off.roughness_alpha, off.metallic_alpha, off.height), (0.0, 0.0, 0.0, 0.0));
            // Without grunge a full edge is fully worn, at any contrast.
            for c in [0.0, 0.6, 1.0] {
                let f = wear(c, 0.0);
                assert_eq!(respond(&f, &pattern(&f, p), on).color_alpha, 1.0);
            }
        }
        let (plain, broken) = (coverage(&wear(0.6, 0.0), on), coverage(&wear(0.6, 1.0), on));
        assert!(plain == 1.0 && broken > 0.15 && broken < 0.85, "{plain} {broken}");
        // Contrast hardens the border: fewer half-worn texels.
        let half = |f: MaterialFilter| points().filter(|&p| (0.1..0.9).contains(&respond(&f, &pattern(&f, p), Surface { edge: 0.6, occlusion: 0.0 }).color_alpha)).count();
        assert!(half(wear(1.0, 0.6)) * 2 < half(wear(0.0, 0.6)), "{} {}", half(wear(1.0, 0.6)), half(wear(0.0, 0.6)));
        // The wear is almost flat: far shallower than an Edge Damages chip.
        let f = wear(0.6, 0.0);
        assert!(points().take(200).all(|p| (-0.01..0.0).contains(&respond(&f, &pattern(&f, p), on).height)));
    }

    fn coverage(f: &MaterialFilter, s: Surface) -> f32 {
        points().map(|p| respond(f, &pattern(f, p), s).metallic_alpha).sum::<f32>() / 4000.0
    }

    #[test]
    fn rust_grows_with_amount_cavities_and_edges() {
        let rust = |amount: f32| MaterialFilter::RustWeathering { amount, edges: 0.5, cavities: 0.6, drips: 0.4, scale: 5.0, relief: 0.4, roughness: 0.88, seed: 1 };
        let open = Surface::default();
        assert_eq!(coverage(&rust(0.0), open), 0.0);
        let (a, b, c) = (coverage(&rust(0.2), open), coverage(&rust(0.5), open), coverage(&rust(1.0), open));
        assert!(a < b && b < c, "{a} {b} {c}");
        assert!(a < 0.2 && c > 0.8, "{a} {c}");
        assert!(coverage(&rust(0.4), Surface { edge: 0.0, occlusion: 1.0 }) > coverage(&rust(0.4), open) + 0.2);
        assert!(coverage(&rust(0.4), Surface { edge: 1.0, occlusion: 0.0 }) > coverage(&rust(0.4), open) + 0.1);
        // Rust is a rough dielectric with a brown-orange colour.
        let f = rust(1.0);
        for p in points().take(300) {
            let r = respond(&f, &pattern(&f, p), Surface { edge: 1.0, occlusion: 1.0 });
            assert_eq!(r.metallic, 0.0);
            assert!(r.roughness > 0.7);
            assert!(r.color[0] > r.color[1] && r.color[1] > r.color[2], "{:?}", r.color);
        }
    }

    #[test]
    fn peeling_paint_tracks_amount_and_steps_the_height_down() {
        let peel = |amount: f32| MaterialFilter::PeelingPaint { amount, scale: 5.0, edge_bias: 0.5, thickness: 0.5, curl: 0.5, color: [0.52, 0.53, 0.54], roughness: 0.45, metallic: 1.0, rust: 0.0, seed: 1 };
        let open = Surface::default();
        assert_eq!(coverage(&peel(0.0), open), 0.0);
        let (a, b, c) = (coverage(&peel(0.2), open), coverage(&peel(0.5), open), coverage(&peel(1.0), open));
        assert!(a < b && b < c && c > 0.9, "{a} {b} {c}");
        assert!(coverage(&peel(0.3), Surface { edge: 1.0, occlusion: 0.0 }) > coverage(&peel(0.3), open) + 0.1);
        let f = peel(0.5);
        let (mut peeled, mut lips) = (0, 0);
        for p in points() {
            let r = respond(&f, &pattern(&f, p), open);
            if r.color_alpha > 0.99 {
                assert!(r.height < -0.005, "peeled areas sit below the film");
                peeled += 1;
            } else if r.color_alpha == 0.0 && r.height > 0.0 {
                lips += 1;
            }
        }
        assert!(peeled > 400 && lips > 20, "{peeled} peeled, {lips} lip texels");
    }

    #[test]
    fn validate_follows_the_published_ranges() {
        let gray = |srgb: f32| {
            let e = srgb / 255.0;
            [if e <= 0.04045 { e / 12.92 } else { ((e + 0.055) / 1.055).powf(2.4) }; 3]
        };
        assert!((srgb_luminance_8bit(gray(128.0)) - 128.0).abs() < 0.01);
        assert_eq!(validate(gray(20.0), 0.0, false, true), Verdict::TooDark);
        assert_eq!(validate(gray(40.0), 0.0, false, true), Verdict::Ok);
        assert_eq!(validate(gray(40.0), 0.0, true, true), Verdict::TooDark);
        assert_eq!(validate(gray(128.0), 0.0, false, true), Verdict::Ok);
        assert_eq!(validate(gray(250.0), 0.0, false, true), Verdict::TooBright);
        assert_eq!(validate(gray(128.0), 1.0, false, true), Verdict::TooDark);
        assert_eq!(validate(gray(200.0), 1.0, false, true), Verdict::Ok);
        assert_eq!(validate(gray(250.0), 1.0, false, true), Verdict::Ok);
        assert_eq!(validate(gray(128.0), 0.5, false, true), Verdict::PartialMetal);
        assert_eq!(validate(gray(128.0), 0.4, false, false), Verdict::Ok);
        assert_eq!(validate(gray(128.0), 0.6, false, false), Verdict::TooDark);
        // The flag colours are told apart by their dominant channel.
        assert!(validate_color(gray(128.0), Verdict::Ok)[1] > validate_color(gray(128.0), Verdict::Ok)[0]);
        assert!(validate_color(gray(20.0), Verdict::TooDark)[0] > validate_color(gray(20.0), Verdict::TooDark)[1]);
    }
}
