//! Bitmaps seen through a fill projection: Filtering, UV Wrap and Shape crop.
//!
//! * **Bilinear | HQ** is trilinear: when the texture is minified (several source texels per
//!   output texel) it reads a box-filtered mip chain, so a strongly tiled image averages instead
//!   of aliasing. Magnified, it is exactly the plain bilinear sample.
//! * **Bilinear | Sharp** always reads the full-size image.
//! * **Nearest** takes the texel under the sample point.
//!
//! An axis that does not repeat ([`subcraft_doc::UvWrap`]) is clamped inside 0..=1 and
//! transparent outside. Mip levels are averaged in the space the texture is used in (linear for
//! color channels) and built once per bitmap, on first minified use.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Weak};

use parking_lot::Mutex;
use rayon::prelude::*;
use subcraft_core::color::linear_to_srgb;
use subcraft_doc::{FillFiltering, Projection, ProjectionMode};
use subcraft_image::{Bitmap, BitmapData, TILE_SIZE, TILE_TEXELS, TextureFilter, Wrap};

use super::geometry::TileGeometry;
use super::projection::{planar_coords, sample_projected_sc, triplanar_weights, unique_side};

/// Levels 1.. of a bitmap's mip chain (level 0 is the bitmap itself).
#[derive(Debug)]
pub struct MipChain {
    levels: Vec<Bitmap>,
}

impl MipChain {
    /// Box-filtered levels down to 1x1. Level `n` is `max(1, size >> n)` texels.
    pub fn build(bmp: &Bitmap, linearize: bool) -> Self {
        let mut levels = Vec::new();
        let (mut w, mut h) = (bmp.width, bmp.height);
        // Decoded RGBA of the previous level (none for level 0: read from the bitmap).
        let mut prev: Option<Vec<[f32; 4]>> = None;
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = vec![[0.0f32; 4]; (nw * nh) as usize];
            let row_of = |(y, row): (usize, &mut [[f32; 4]])| {
                let (y0, y1) = ((y as u32 * 2).min(h - 1), (y as u32 * 2 + 1).min(h - 1));
                for (x, o) in row.iter_mut().enumerate() {
                    let (x0, x1) = ((x as u32 * 2).min(w - 1), (x as u32 * 2 + 1).min(w - 1));
                    let at = |x: u32, y: u32| match &prev {
                        Some(p) => p[(y * w + x) as usize],
                        None => bmp.texel(x, y, linearize),
                    };
                    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                    *o = std::array::from_fn(|k| (a[k] + b[k] + c[k] + d[k]) * 0.25);
                }
            };
            // The chain is built lazily by the first tile that samples the bitmap, inside a
            // compositor stage cell's one-time init. A rayon join there lets this thread steal
            // another tile's job that waits on the same cell: a deadlock (QA fuzz UDIM seed 457).
            // Inside the pool the tiles already keep every core busy, so build it serially.
            if rayon::current_thread_index().is_some() {
                next.chunks_mut(nw as usize).enumerate().for_each(row_of);
            } else {
                next.par_chunks_mut(nw as usize).enumerate().for_each(row_of);
            }
            levels.push(encode_level(bmp, linearize, nw, nh, &next));
            prev = Some(next);
            (w, h) = (nw, nh);
        }
        Self { levels }
    }

    /// Number of levels below the full-size image.
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Level `n >= 1` (level 0 is the bitmap itself).
    pub fn level(&self, n: usize) -> &Bitmap {
        &self.levels[n - 1]
    }
}

/// Store decoded RGBA as a bitmap of the source layout: 8-bit sources stay 8-bit in their own
/// encoding (a third of the source memory for the whole chain), others become decoded floats.
fn encode_level(src: &Bitmap, linearize: bool, w: u32, h: u32, px: &[[f32; 4]]) -> Bitmap {
    let ch = src.channels as usize;
    let pick = |p: &[f32; 4], c: usize| if ch == 2 && c == 1 { p[3] } else { p[c] };
    let is_alpha = |c: usize| (ch == 2 && c == 1) || c == 3;
    if matches!(src.data, BitmapData::U8(_)) {
        let mut data = Vec::with_capacity(px.len() * ch);
        for p in px {
            for c in 0..ch {
                let v = pick(p, c);
                let v = if linearize && !is_alpha(c) { linear_to_srgb(v) } else { v };
                data.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        Bitmap::new_u8(w, h, src.channels, data)
    } else {
        let mut data = Vec::with_capacity(px.len() * ch);
        for p in px {
            for c in 0..ch {
                data.push(pick(p, c));
            }
        }
        Bitmap::new_f32(w, h, src.channels, data)
    }
}

type MipKey = (usize, bool);
static MIPS: LazyLock<Mutex<HashMap<MipKey, (Weak<Bitmap>, Arc<MipChain>)>>> = LazyLock::new(Default::default);

/// The cached mip chain of a shared bitmap (per color decoding). Chains of dropped bitmaps are
/// released the next time one is built.
pub fn mip_chain(bmp: &Arc<Bitmap>, linearize: bool) -> Arc<MipChain> {
    let key = (Arc::as_ptr(bmp) as usize, linearize);
    if let Some((owner, chain)) = MIPS.lock().get(&key)
        && owner.upgrade().is_some_and(|o| Arc::ptr_eq(&o, bmp))
    {
        return chain.clone();
    }
    let chain = Arc::new(MipChain::build(bmp, linearize));
    let mut map = MIPS.lock();
    map.retain(|_, (owner, _)| owner.strong_count() > 0);
    map.insert(key, (Arc::downgrade(bmp), chain.clone()));
    chain
}

/// A bitmap with the sampling options of a projection.
pub struct BitmapSampler<'a> {
    bmp: &'a Arc<Bitmap>,
    /// Built on the first minified sample.
    mips: std::cell::OnceCell<Arc<MipChain>>,
    hq: bool,
    linearize: bool,
    nearest: bool,
    repeat: [bool; 2],
}

impl<'a> BitmapSampler<'a> {
    /// `force_nearest` overrides the projection's Filtering (the texture set's pixel-art option).
    pub fn new(bmp: &'a Arc<Bitmap>, linearize: bool, projection: &Projection, force_nearest: bool) -> Self {
        let nearest = force_nearest || projection.filtering == FillFiltering::Nearest;
        let hq = !nearest && projection.filtering == FillFiltering::BilinearHq && bmp.width.max(bmp.height) > 1;
        Self { bmp, mips: Default::default(), hq, linearize, nearest, repeat: projection.effective_wrap().repeats() }
    }

    /// Whether [`BitmapSampler::sample`] uses its `lod` argument.
    #[inline]
    pub fn uses_lod(&self) -> bool {
        self.hq
    }

    /// Sample at texture coordinate `p`; `lod` is log2 of the source texels per output texel.
    #[inline]
    pub fn sample(&self, p: [f32; 2], lod: f32) -> [f32; 4] {
        if self.nearest {
            return nearest(self.bmp, p, self.repeat, self.linearize);
        }
        if !self.hq || lod <= 0.0 {
            return bilinear(self.bmp, p, self.repeat, self.linearize);
        }
        let mips = self.mips.get_or_init(|| mip_chain(self.bmp, self.linearize));
        let lod = lod.min(mips.levels.len() as f32);
        let lower = lod.floor() as usize;
        let level = |n: usize| -> &Bitmap { if n == 0 { self.bmp } else { &mips.levels[n - 1] } };
        let a = bilinear(level(lower), p, self.repeat, self.linearize);
        let t = lod - lower as f32;
        if t <= 0.0 {
            return a;
        }
        let b = bilinear(level(lower + 1), p, self.repeat, self.linearize);
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
    }
}

/// Source texels along the denser axis per unit of projected (pre-transform) coordinate.
#[inline]
fn texel_density(bmp: &Bitmap, projection: &Projection) -> f32 {
    (projection.scale[0].abs() * bmp.width as f32).max(projection.scale[1].abs() * bmp.height as f32)
}

#[inline]
fn fix(i: i64, n: i64, repeat: bool) -> u32 {
    if repeat { i.rem_euclid(n) as u32 } else { i.clamp(0, n - 1) as u32 }
}

#[inline]
fn nearest(bmp: &Bitmap, p: [f32; 2], repeat: [bool; 2], linearize: bool) -> [f32; 4] {
    if repeat == [true, true] {
        return bmp.sample_filtered(p[0], p[1], Wrap::Repeat, linearize, TextureFilter::Nearest);
    }
    let index = |v: f32, size: u32, repeat: bool| fix((v * size as f32).floor() as i64, size as i64, repeat);
    bmp.texel(index(p[0], bmp.width, repeat[0]), index(p[1], bmp.height, repeat[1]), linearize)
}

/// Bilinear sample with a wrap per axis (repeat, or clamp to the edge texel).
#[inline]
fn bilinear(bmp: &Bitmap, p: [f32; 2], repeat: [bool; 2], linearize: bool) -> [f32; 4] {
    if repeat == [true, true] {
        return bmp.sample(p[0], p[1], Wrap::Repeat, linearize);
    }
    let (w, h) = (bmp.width as i64, bmp.height as i64);
    let x = p[0] * w as f32 - 0.5;
    let y = p[1] * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (xa, xb) = (fix(x0 as i64, w, repeat[0]), fix(x0 as i64 + 1, w, repeat[0]));
    let (ya, yb) = (fix(y0 as i64, h, repeat[1]), fix(y0 as i64 + 1, h, repeat[1]));
    let (t00, t10, t01, t11) = (bmp.texel(xa, ya, linearize), bmp.texel(xb, ya, linearize), bmp.texel(xa, yb, linearize), bmp.texel(xb, yb, linearize));
    std::array::from_fn(|c| {
        let top = t00[c] + (t10[c] - t00[c]) * fx;
        let bot = t01[c] + (t11[c] - t01[c]) * fx;
        top + (bot - top) * fy
    })
}

/// Whether texel `(uv, pos)` lies inside the projected texture: inside the 0..=1 square on every
/// axis that does not repeat, and inside the projection box when Shape crop is on. The box is
/// the normalized bounding cube of the model.
#[inline]
pub fn projection_covers(projection: &Projection, sc: (f32, f32), uv: [f32; 2], pos: [f32; 3]) -> bool {
    projection_covers_at(projection, sc, uv, pos, [0.0; 3])
}

/// [`projection_covers`] with the texel's normal, which Wrap around edges needs to unfold the
/// position the same way the sampling does.
#[inline]
pub fn projection_covers_at(projection: &Projection, sc: (f32, f32), uv: [f32; 2], pos: [f32; 3], normal: [f32; 3]) -> bool {
    const EPS: f32 = 1e-4;
    // The position in the projection box's frame (symmetry, then the 3D transform).
    let local = projection.local(pos, normal).0;
    // The box is the bounding cube until it is moved, turned or scaled; then the crop follows
    // the box (and the symmetric copies with it).
    let shape = if projection.effective_transform3().is_identity() { pos } else { local };
    if projection.crops_to_shape() && !shape.iter().all(|c| (-EPS..=1.0 + EPS).contains(c)) {
        return false;
    }
    let pos = local;
    match projection.mode {
        ProjectionMode::Uv => projection.uv_wrap.covers(projection.transform_2d_sc(sc, uv)),
        ProjectionMode::Planar => {
            projection.uv_wrap.covers(projection.transform_2d_sc(sc, planar_coords(pos, projection.planar_axis)))
        }
        ProjectionMode::Spherical | ProjectionMode::Cylindrical => {
            projection.uv_wrap.covers(projection.transform_2d_sc(sc, projection.wrapped_coords(pos)))
        }
        ProjectionMode::Warp => {
            let (q, inside) = projection.warp_coords(pos);
            inside && projection.uv_wrap.covers(projection.transform_2d_sc(sc, q))
        }
        ProjectionMode::Triplanar => true,
    }
}

/// Coverage 0..=1 of texel `(uv, pos, normal)`: 0 outside the projected texture
/// ([`projection_covers`]), otherwise what backface culling leaves (1 without it).
#[inline]
pub fn projection_coverage(projection: &Projection, sc: (f32, f32), uv: [f32; 2], pos: [f32; 3], normal: [f32; 3]) -> f32 {
    if !projection_covers_at(projection, sc, uv, pos, normal) {
        return 0.0;
    }
    if projection.backface_culling().is_none() {
        return 1.0;
    }
    let (p, n) = projection.local(pos, normal);
    projection.facing_weight(p, n)
}

/// Per-texel coverage (1 inside, 0 outside, in between where backface culling fades) of a
/// projection on a tile; `None` when nothing is clipped ([`Projection::clips`]), which is every
/// projection at its defaults.
pub fn coverage_tile(projection: &Projection, geo: &TileGeometry) -> Option<Vec<f32>> {
    if !projection.clips() {
        return None;
    }
    let sc = projection.rotation_sin_cos();
    let depth = super::depth_cull::for_tile(projection, geo);
    Some((0..TILE_TEXELS).map(|i| {
        let c = projection_coverage(projection, sc, geo.uv(i), geo.position(i), geo.normal(i));
        match &depth { Some(d) if c > 0.0 => c * d.weight(projection, geo.position(i)), _ => c }
    }).collect())
}

/// Mip level of texel `i`: log2 of the source texels one output texel spans.
///
/// UV projection: the tiling times the image size over the texture-set resolution. 3D
/// projections: the object-space distance to the neighbouring texels (the shorter of the two
/// directions, so a UV seam next to the texel does not blur it).
#[inline]
fn texel_lod(projection: &Projection, density: f32, geo: &TileGeometry, i: usize) -> f32 {
    if projection.mode == ProjectionMode::Uv {
        return (density / geo.resolution as f32).log2();
    }
    let n = TILE_SIZE as usize;
    let (x, y) = (i % n, i / n);
    let p = geo.position(i);
    let step = |j: usize| -> Option<f32> {
        (geo.coverage(j) > 0.0).then(|| {
            let q = geo.position(j);
            ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt()
        })
    };
    let dx = step(if x + 1 < n { i + 1 } else { i - 1 });
    let dy = step(if y + 1 < n { i + n } else { i - n });
    let d = match (dx, dy) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return 0.0,
    };
    if d <= 0.0 { 0.0 } else { (d * density).log2() }
}

/// Evaluate a bitmap through `projection` for every texel of a tile: `[r, g, b, a]`, transparent
/// where the projection does not cover the texel.
pub fn bitmap_tile(bmp: &Arc<Bitmap>, linearize: bool, projection: &Projection, force_nearest: bool, geo: &TileGeometry, out: &mut [[f32; 4]]) {
    let sampler = BitmapSampler::new(bmp, linearize, projection, force_nearest);
    let sc = projection.rotation_sin_cos();
    let clips = projection.clips();
    let depth = super::depth_cull::for_tile(projection, geo);
    let density = texel_density(bmp, projection);
    // UV projection: one level for the whole tile.
    let uv_lod = (sampler.uses_lod() && projection.mode == ProjectionMode::Uv).then(|| texel_lod(projection, density, geo, 0));
    for (i, o) in out.iter_mut().enumerate() {
        let (uv, pos) = (geo.uv(i), geo.position(i));
        let mut cover = if clips { projection_coverage(projection, sc, uv, pos, geo.normal(i)) } else { 1.0 };
        if cover > 0.0 && let Some(d) = &depth {
            cover *= d.weight(projection, pos);
        }
        if cover <= 0.0 {
            *o = [0.0; 4];
            continue;
        }
        let lod = match uv_lod {
            Some(l) => l,
            None if sampler.uses_lod() => texel_lod(projection, density, geo, i),
            None => 0.0,
        };
        *o = sample_projected_sc(projection, sc, uv, pos, geo.normal(i), |p| sampler.sample(p, lod));
        o[3] *= cover;
    }
}

/// Whether a normal map read through `projection` has to be turned to the surface
/// ([`normal_map_tile`]): everything but an unrotated, unmirrored UV mapping, where the image's
/// axes already are the tangent frame's.
pub fn projection_turns_normals(projection: &Projection) -> bool {
    match projection.mode {
        ProjectionMode::Uv => projection.rotation_deg != 0.0 || projection.scale[0] < 0.0 || projection.scale[1] < 0.0,
        _ => true,
    }
}

/// Where an image's right and its top point at texel `i`, in the texel's UV tangent frame
/// (x toward +U, y toward V up): `(right, up)`, each a unit direction on the surface seen in that
/// frame. `image_at` gives the image coordinate a texel reads. The directions come from finite
/// differences inside the tile: how the image coordinate and the surface position change from
/// texel to texel. `None` where the tile has no covered neighbour or the mapping is degenerate.
fn image_frame(geo: &TileGeometry, i: usize, image_at: &dyn Fn(usize) -> [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
    let n = TILE_SIZE as usize;
    // Change of surface position and image coordinate per texel step along one tile axis, from
    // the covered neighbours (both sides when there are two).
    let step = |back: Option<usize>, fwd: Option<usize>| -> Option<([f32; 3], [f32; 2])> {
        let a = back.filter(|&j| geo.coverage(j) > 0.0).unwrap_or(i);
        let b = fwd.filter(|&j| geo.coverage(j) > 0.0).unwrap_or(i);
        if a == b {
            return None;
        }
        let (pa, pb, qa, qb) = (geo.position(a), geo.position(b), image_at(a), image_at(b));
        Some(([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]], [qb[0] - qa[0], qb[1] - qa[1]]))
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |v: [f32; 3]| -> Option<[f32; 3]> {
        let l = dot(v, v).sqrt();
        (l > 1e-20 && l.is_finite()).then(|| v.map(|c| c / l))
    };
    let (x, y) = (i % n, i / n);
    let (px, qx) = step((x > 0).then(|| i - 1), (x + 1 < n).then(|| i + 1))?;
    let (py, qy) = step((y > 0).then(|| i - n), (y + 1 < n).then(|| i + n))?;
    // The texel's tangent frame: T toward +U (texel x), B toward V up (against texel y).
    let t = unit(px)?;
    let b_raw = [-py[0], -py[1], -py[2]];
    let k = dot(b_raw, t);
    let b = unit([b_raw[0] - t[0] * k, b_raw[1] - t[1] * k, b_raw[2] - t[2] * k])?;
    // Texel steps that move one unit along an image direction `d`: the inverse of the image
    // coordinate's Jacobian. Then the same steps taken on the surface.
    let det = qx[0] * qy[1] - qy[0] * qx[1];
    if det.abs() < 1e-20 || !det.is_finite() {
        return None;
    }
    let on_surface = |d: [f32; 2]| -> Option<[f32; 2]> {
        let (sx, sy) = ((d[0] * qy[1] - d[1] * qy[0]) / det, (qx[0] * d[1] - qx[1] * d[0]) / det);
        let w = unit([px[0] * sx + py[0] * sy, px[1] * sx + py[1] * sy, px[2] * sx + py[2] * sy])?;
        Some([dot(w, t), dot(w, b)])
    };
    // Image right is +u; image up is against v.
    Some((on_surface([1.0, 0.0])?, on_surface([0.0, -1.0])?))
}

/// A decoded normal's tilt `(x, y)` turned from the image's frame into the tangent frame given by
/// [`image_frame`], keeping its length: the turn must not make the bump steeper or flatter.
#[inline]
fn turned(frame: ([f32; 2], [f32; 2]), nx: f32, ny: f32) -> (f32, f32) {
    let (right, up) = frame;
    let (tx, ty) = (right[0] * nx + up[0] * ny, right[1] * nx + up[1] * ny);
    let (before, after) = ((nx * nx + ny * ny).sqrt(), (tx * tx + ty * ty).sqrt());
    let s = if after > 1e-12 { before / after } else { 0.0 };
    (tx * s, ty * s)
}

/// Fix up a projected normal map after [`bitmap_tile`] wrote it to `out` (encoded
/// `[x, y, z, alpha]`): the tangent-space normals are turned from the image's frame into each
/// texel's UV tangent frame.
///
/// A normal map's x points to the image's right and its y to the image's top. Projected onto the
/// model, the image's right is some direction on the surface that has nothing to do with the
/// texel's +U: without this turn a decal's bumps are lit from the wrong side wherever the
/// projection and the UV layout disagree (a rotated decal, a UV island laid out sideways).
/// Tri-planar reads up to three images per texel, each with its own directions, so its texels
/// are sampled again here, turned side by side and then blended.
pub fn normal_map_tile(bmp: &Arc<Bitmap>, projection: &Projection, force_nearest: bool, geo: &TileGeometry, out: &mut [[f32; 4]]) {
    if !projection_turns_normals(projection) {
        return;
    }
    let sc = projection.rotation_sin_cos();
    if projection.mode == ProjectionMode::Triplanar {
        let sampler = BitmapSampler::new(bmp, false, projection, force_nearest);
        let density = texel_density(bmp, projection);
        for i in 0..TILE_TEXELS {
            if out[i][3] <= 0.0 || geo.coverage(i) <= 0.0 {
                continue;
            }
            let (pos, normal) = projection.local(geo.position(i), geo.normal(i));
            let w = triplanar_weights(normal, projection.triplanar_sharpness);
            let lod = if sampler.uses_lod() { texel_lod(projection, density, geo, i) } else { 0.0 };
            let (mut acc, mut ok) = ([0.0f32; 4], true);
            for axis in 0..3u8 {
                let wa = w[axis as usize];
                if wa <= 0.0 {
                    continue;
                }
                // The side this texel reads decides the side for its neighbours too, so the
                // differences never jump between the two sides of an axis.
                let negative = normal[axis as usize] < 0.0;
                let side = |p: [f32; 3]| {
                    let q = planar_coords(p, axis);
                    projection.transform_2d_sc(sc, if projection.triplanar_unique { unique_side(q, axis, negative) } else { q })
                };
                let Some(frame) = image_frame(geo, i, &|j| side(projection.local(geo.position(j), geo.normal(j)).0)) else {
                    ok = false;
                    break;
                };
                let s = sampler.sample(side(pos), lod);
                let (tx, ty) = turned(frame, s[0] * 2.0 - 1.0, s[1] * 2.0 - 1.0);
                acc[0] += tx * wa;
                acc[1] += ty * wa;
                acc[2] += (s[2] * 2.0 - 1.0) * wa;
                acc[3] += s[3] * wa;
            }
            if ok {
                out[i] = [acc[0] * 0.5 + 0.5, acc[1] * 0.5 + 0.5, acc[2] * 0.5 + 0.5, acc[3]];
            }
        }
        return;
    }
    let image_at = |i: usize| -> [f32; 2] {
        let mut q = [0.0f32; 2];
        sample_projected_sc(projection, sc, geo.uv(i), geo.position(i), geo.normal(i), |p| {
            q = p;
            [0.0; 4]
        });
        q
    };
    for i in 0..TILE_TEXELS {
        if out[i][3] <= 0.0 || geo.coverage(i) <= 0.0 {
            continue;
        }
        let Some(frame) = image_frame(geo, i, &image_at) else { continue };
        let o = &mut out[i];
        let (tx, ty) = turned(frame, o[0] * 2.0 - 1.0, o[1] * 2.0 - 1.0);
        o[0] = tx * 0.5 + 0.5;
        o[1] = ty * 0.5 + 0.5;
    }
}
