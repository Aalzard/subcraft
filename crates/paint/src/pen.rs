//! Bounded-work path rasterization. Coverage is the closest distance to the entire curve,
//! so overlapping segments/caps do not compound opacity. Only touched tiles are allocated.
use std::collections::BTreeMap;
use glam::Vec2;
use subcraft_core::ChannelKind;
use subcraft_doc::{PathKind, PenPath, empty_paint_image};
use subcraft_image::{TiledImage, TILE_SIZE, TILE_TEXELS};

pub struct PenRasterizer {
    path: PenPath,
    points: Vec<Vec2>,
    bins: Vec<Vec<usize>>,
    tiles: Vec<usize>,
    tile: usize,
    pixel: usize,
    segment: usize,
    distance: f32,
    coverage: Vec<f32>,
    images: BTreeMap<ChannelKind, TiledImage>,
    resolution: u32,
    /// Stamped path: tip image, and the stamps (centre in texels, angle) of each segment.
    alpha: Option<std::sync::Arc<subcraft_image::Bitmap>>,
    stamps: Vec<(Vec2, f32)>,
    segment_stamps: Vec<Vec<u32>>,
}

/// Most stamps one path may place (beyond it the spacing is too small for the length).
const MAX_PATH_STAMPS: usize = 200_000;

/// Stamp centres along a polyline (texels), `step` apart from the first point, with the angle
/// of the segment each lies on; `segments[i]` lists the stamps on segment `i`.
fn place_stamps(points: &[Vec2], step: f32) -> Result<(Vec<(Vec2, f32)>, Vec<Vec<u32>>), &'static str> {
    let mut stamps = Vec::new();
    let mut segments = vec![Vec::new(); points.len().saturating_sub(1)];
    let mut next = 0.0f32; // arc length of the next stamp, measured from the current segment's start
    for (i, s) in points.windows(2).enumerate() {
        let d = s[1] - s[0];
        let len = d.length();
        let angle = if len > 1e-9 { d.y.atan2(d.x) } else { stamps.last().map_or(0.0, |l: &(Vec2, f32)| l.1) };
        if i == 0 || len > 1e-9 {
            while next <= len {
                let t = if len > 1e-9 { next / len } else { 0.0 };
                segments[i].push(stamps.len() as u32);
                stamps.push((s[0] + d * t, angle));
                if stamps.len() > MAX_PATH_STAMPS { return Err("too many stamps on this path: raise the spacing or the width"); }
                next += step;
                if len <= 1e-9 { break; }
            }
        }
        next -= len;
    }
    Ok((stamps, segments))
}

impl PenRasterizer {
    pub fn new(path: &PenPath, resolution: u32) -> Result<Self, &'static str> {
        if resolution == 0 || resolution > 8192 { return Err("invalid pen resolution"); }
        let mut points = path.polyline(0.35 / resolution as f32)?;
        for p in &mut points { *p *= resolution as f32; }
        if points.len() == 1 { points.push(points[0]); }
        let size = resolution.div_ceil(TILE_SIZE);
        let mut bins = vec![Vec::new(); (size * size) as usize];
        let filled = path.kind == PathKind::Filled;
        let stamped = path.kind == PathKind::Paint && path.stamp.is_some();
        // A filled path only needs the segments near a texel for its anti-aliased edge; a tip
        // image reaches to the corners of its square.
        let radius = if filled { 1.5 } else if stamped { path.radius * resolution as f32 * std::f32::consts::SQRT_2 + 1.0 } else { path.radius * resolution as f32 + 1.0 };
        let mut references = 0;
        for (i, segment) in points.windows(2).enumerate() {
            let lo = segment[0].min(segment[1]) - Vec2::splat(radius);
            let hi = segment[0].max(segment[1]) + Vec2::splat(radius);
            if hi.x < 0.0 || hi.y < 0.0 || lo.x >= resolution as f32 || lo.y >= resolution as f32 { continue; }
            let lo = (lo.max(Vec2::ZERO) / TILE_SIZE as f32).as_uvec2();
            let hi = (hi.min(Vec2::splat(resolution as f32 - 1.0)) / TILE_SIZE as f32).as_uvec2();
            for y in lo.y..=hi.y { for x in lo.x..=hi.x {
                references += 1;
                if references > 131072 { return Err("path is too complex at this width; reduce width or split the path"); }
                bins[(y * size + x) as usize].push(i);
            } }
        }
        let tiles = if filled {
            // Every tile of the path's bounding box: interior tiles have no segment nearby.
            let lo = points.iter().fold(Vec2::splat(f32::INFINITY), |m, p| m.min(*p)).max(Vec2::ZERO);
            let hi = points.iter().fold(Vec2::splat(f32::NEG_INFINITY), |m, p| m.max(*p)).min(Vec2::splat(resolution as f32 - 1.0));
            if hi.x < lo.x || hi.y < lo.y { Vec::new() } else {
                let (lo, hi) = ((lo / TILE_SIZE as f32).as_uvec2(), (hi / TILE_SIZE as f32).as_uvec2());
                (lo.y..=hi.y).flat_map(|y| (lo.x..=hi.x).map(move |x| (y * size + x) as usize)).collect()
            }
        } else {
            bins.iter().enumerate().filter(|(_, b)| !b.is_empty()).map(|(i, _)| i).collect()
        };
        let images = path.values.keys().map(|&k| (k, empty_paint_image(k, resolution))).collect();
        let (stamps, segment_stamps) = match path.stamp.filter(|_| stamped) {
            Some(st) => place_stamps(&points, (st.spacing * 2.0 * path.radius * resolution as f32).max(0.5))?,
            None => (Vec::new(), Vec::new()),
        };
        Ok(Self { path: path.clone(), points, bins, tiles, tile: 0, pixel: 0, segment: 0, distance: f32::INFINITY, coverage: vec![0.0; TILE_TEXELS], images, resolution, alpha: None, stamps, segment_stamps })
    }

    /// The image a path draws with ([`PenPath::image_resource`]): the stamped tip (round
    /// without it) or the ribbon image (a solid strip without it).
    pub fn with_alpha(mut self, alpha: Option<std::sync::Arc<subcraft_image::Bitmap>>) -> Self {
        self.alpha = alpha;
        self
    }

    pub fn progress(&self) -> f32 { if self.tiles.is_empty() { 1.0 } else { self.tile as f32 / self.tiles.len() as f32 } }
    pub fn is_done(&self) -> bool { self.tile == self.tiles.len() }
    /// At most `budget` segment-distance evaluations, plus completed 64² tile writes.
    pub fn step(&mut self, mut budget: usize) {
        if self.path.kind != PathKind::Paint || !self.stamps.is_empty() {
            // Filled interiors need whole rows of crossings, stamps a pass over their tiles:
            // both are done in one parallel pass.
            self.run();
            return;
        }
        let across = self.resolution.div_ceil(TILE_SIZE);
        while budget > 0 && !self.is_done() {
            let tile = self.tiles[self.tile];
            let x = (tile as u32 % across) * TILE_SIZE + self.pixel as u32 % TILE_SIZE;
            let y = (tile as u32 / across) * TILE_SIZE + self.pixel as u32 / TILE_SIZE;
            if x < self.resolution && y < self.resolution {
                let i = self.bins[tile][self.segment];
                let a = self.points[i]; let d = self.points[i + 1] - a;
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = if d.length_squared() > 1e-12 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                self.distance = self.distance.min(p.distance(a + d * t));
                self.segment += 1; budget -= 1;
                if self.segment < self.bins[tile].len() { continue; }
                let radius = self.path.radius * self.resolution as f32;
                // A one-texel edge band keeps thin strokes visible instead of aliasing away.
                let edge = (radius + 0.5 - self.distance).clamp(0.0, 1.0);
                self.coverage[self.pixel] = crate::falloff((self.distance - 0.5).max(0.0) / radius, self.path.hardness).min(edge) * self.path.opacity;
            }
            self.pixel += 1; self.segment = 0; self.distance = f32::INFINITY;
            if self.pixel == TILE_TEXELS {
                if self.coverage.iter().any(|&a| a > 0.0) {
                    for (&kind, image) in &mut self.images {
                        let n = kind.components(); let comps = n + 1; let value = self.path.values[&kind].0;
                        let mut data = vec![0.0; TILE_TEXELS * comps];
                        for (pixel, &alpha) in data.chunks_exact_mut(comps).zip(&self.coverage) {
                            pixel[..n].copy_from_slice(&value[..n]); pixel[n] = alpha;
                        }
                        image.write_tile(tile, &data);
                    }
                }
                self.coverage.fill(0.0); self.pixel = 0; self.tile += 1;
            }
        }
    }

    /// Rasterize every remaining tile now, in parallel (the same pixels as stepping). A path's
    /// whole raster takes milliseconds this way, where stepping by a small per-frame budget made
    /// a pen edit wait for hundreds of frames.
    pub fn run(&mut self) {
        use rayon::prelude::*;
        if self.path.kind == PathKind::Filled {
            self.run_filled();
            return;
        }
        if self.path.kind == PathKind::Ribbon {
            self.run_ribbon();
            return;
        }
        if !self.stamps.is_empty() {
            self.run_stamped();
            return;
        }
        if self.pixel != 0 || self.segment != 0 {
            // Finish the tile a step left half done, so the rest starts on a tile boundary.
            let current = self.tile;
            while self.tile == current && !self.is_done() { self.step(TILE_TEXELS * self.bins[self.tiles[current]].len().max(1)); }
        }
        let across = self.resolution.div_ceil(TILE_SIZE);
        let (radius, hardness, opacity, res) = (self.path.radius * self.resolution as f32, self.path.hardness, self.path.opacity, self.resolution);
        let (points, bins) = (&self.points, &self.bins);
        let done: Vec<(usize, Vec<f32>)> = self.tiles[self.tile..].par_iter().map(|&tile| {
            let mut coverage = vec![0.0f32; TILE_TEXELS];
            for (pixel, out) in coverage.iter_mut().enumerate() {
                let x = (tile as u32 % across) * TILE_SIZE + pixel as u32 % TILE_SIZE;
                let y = (tile as u32 / across) * TILE_SIZE + pixel as u32 / TILE_SIZE;
                if x >= res || y >= res { continue; }
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let mut distance = f32::INFINITY;
                for &i in &bins[tile] {
                    let a = points[i]; let d = points[i + 1] - a;
                    let t = if d.length_squared() > 1e-12 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                    distance = distance.min(p.distance(a + d * t));
                }
                let edge = (radius + 0.5 - distance).clamp(0.0, 1.0);
                *out = crate::falloff((distance - 0.5).max(0.0) / radius, hardness).min(edge) * opacity;
            }
            (tile, coverage)
        }).collect();
        for (tile, coverage) in done {
            if !coverage.iter().any(|&a| a > 0.0) { continue; }
            for (&kind, image) in &mut self.images {
                let n = kind.components(); let comps = n + 1; let value = self.path.values[&kind].0;
                let mut data = vec![0.0; TILE_TEXELS * comps];
                for (pixel, &alpha) in data.chunks_exact_mut(comps).zip(&coverage) {
                    pixel[..n].copy_from_slice(&value[..n]); pixel[n] = alpha;
                }
                image.write_tile(tile, &data);
            }
        }
        self.tile = self.tiles.len(); self.pixel = 0; self.segment = 0; self.distance = f32::INFINITY; self.coverage.fill(0.0);
    }

    /// Filled path: even-odd interior from the crossings of each texel row with the closed curve,
    /// anti-aliased within a texel of the curve.
    fn run_filled(&mut self) {
        use rayon::prelude::*;
        let (res, across, opacity) = (self.resolution, self.resolution.div_ceil(TILE_SIZE), self.path.opacity);
        let (points, bins) = (&self.points, &self.bins);
        let tiles = &self.tiles[self.tile..];
        let rows: std::collections::BTreeSet<u32> = tiles.iter().map(|&t| t as u32 / across).collect();
        // Crossing x positions of every texel row the tiles cover, sorted.
        let crossings: BTreeMap<u32, Vec<f32>> = rows
            .into_par_iter()
            .flat_map_iter(|ty| (ty * TILE_SIZE..((ty + 1) * TILE_SIZE).min(res)).collect::<Vec<_>>())
            .map(|y| {
                let yc = y as f32 + 0.5;
                let mut xs: Vec<f32> = points
                    .windows(2)
                    .filter_map(|s| {
                        let (a, b) = (s[0], s[1]);
                        ((a.y <= yc) != (b.y <= yc)).then(|| a.x + (yc - a.y) / (b.y - a.y) * (b.x - a.x))
                    })
                    .collect();
                xs.sort_by(|a, b| a.total_cmp(b));
                (y, xs)
            })
            .collect();
        let done: Vec<(usize, Vec<f32>)> = tiles.par_iter().map(|&tile| {
            let mut coverage = vec![0.0f32; TILE_TEXELS];
            for (pixel, out) in coverage.iter_mut().enumerate() {
                let x = (tile as u32 % across) * TILE_SIZE + pixel as u32 % TILE_SIZE;
                let y = (tile as u32 / across) * TILE_SIZE + pixel as u32 / TILE_SIZE;
                if x >= res || y >= res { continue; }
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let inside = crossings.get(&y).is_some_and(|xs| xs.partition_point(|&c| c < p.x) % 2 == 1);
                let mut distance = f32::INFINITY;
                for &i in &bins[tile] {
                    let a = points[i]; let d = points[i + 1] - a;
                    let t = if d.length_squared() > 1e-12 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                    distance = distance.min(p.distance(a + d * t));
                }
                let a = if inside { (distance + 0.5).min(1.0) } else { (0.5 - distance).max(0.0) };
                *out = a * opacity;
            }
            (tile, coverage)
        }).collect();
        for (tile, coverage) in done {
            if !coverage.iter().any(|&a| a > 0.0) { continue; }
            for (&kind, image) in &mut self.images {
                let n = kind.components(); let comps = n + 1; let value = self.path.values[&kind].0;
                let mut data = vec![0.0; TILE_TEXELS * comps];
                for (pixel, &alpha) in data.chunks_exact_mut(comps).zip(&coverage) {
                    pixel[..n].copy_from_slice(&value[..n]); pixel[n] = alpha;
                }
                image.write_tile(tile, &data);
            }
        }
        self.tile = self.tiles.len(); self.pixel = 0; self.segment = 0; self.distance = f32::INFINITY; self.coverage.fill(0.0);
    }

    /// Stamped path: each texel takes the strongest stamp of the tip over it (stamps do not
    /// build up, like the stroke's closest-distance coverage).
    fn run_stamped(&mut self) {
        use rayon::prelude::*;
        let (res, across, opacity, hardness) = (self.resolution, self.resolution.div_ceil(TILE_SIZE), self.path.opacity, self.path.hardness);
        let radius = (self.path.radius * res as f32).max(1e-3);
        let follow = self.path.stamp.is_some_and(|s| s.follow);
        let (stamps, segment_stamps, bins, alpha) = (&self.stamps, &self.segment_stamps, &self.bins, self.alpha.as_deref());
        let done: Vec<(usize, Vec<f32>)> = self.tiles[self.tile..].par_iter().map(|&tile| {
            let mut coverage = vec![0.0f32; TILE_TEXELS];
            let near: Vec<u32> = bins[tile].iter().flat_map(|&seg| segment_stamps[seg].iter().copied()).collect();
            for (pixel, out) in coverage.iter_mut().enumerate() {
                let x = (tile as u32 % across) * TILE_SIZE + pixel as u32 % TILE_SIZE;
                let y = (tile as u32 / across) * TILE_SIZE + pixel as u32 / TILE_SIZE;
                if x >= res || y >= res { continue; }
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let mut best = 0.0f32;
                for &k in &near {
                    let (c, angle) = stamps[k as usize];
                    let local = (p - c) / radius;
                    if local.length_squared() > 2.0 { continue; }
                    best = best.max(crate::stamp::tip_footprint(alpha, hardness, if follow { angle } else { 0.0 }, 1.0, local));
                }
                *out = best * opacity;
            }
            (tile, coverage)
        }).collect();
        for (tile, coverage) in done {
            if !coverage.iter().any(|&a| a > 0.0) { continue; }
            for (&kind, image) in &mut self.images {
                let n = kind.components(); let comps = n + 1; let value = self.path.values[&kind].0;
                let mut data = vec![0.0; TILE_TEXELS * comps];
                for (pixel, &alpha) in data.chunks_exact_mut(comps).zip(&coverage) {
                    pixel[..n].copy_from_slice(&value[..n]); pixel[n] = alpha;
                }
                image.write_tile(tile, &data);
            }
        }
        self.tile = self.tiles.len(); self.pixel = 0; self.segment = 0; self.distance = f32::INFINITY; self.coverage.fill(0.0);
    }

    /// Ribbon path: the image laid along the curve over the path's width. Across the path the
    /// image runs from one edge to the other; along it, it stretches over the length (`tiles`
    /// copies) or keeps its aspect ratio. The ends are cut square, not rounded.
    fn run_ribbon(&mut self) {
        use rayon::prelude::*;
        let settings = self.path.ribbon.unwrap_or_default();
        let (res, across, opacity) = (self.resolution, self.resolution.div_ceil(TILE_SIZE), self.path.opacity);
        let radius = (self.path.radius * res as f32).max(1e-3);
        let (points, bins, image) = (&self.points, &self.bins, self.alpha.as_deref());
        let last = points.len().saturating_sub(2);
        let open = !self.path.is_closed();
        let mut arc = Vec::with_capacity(points.len());
        let mut total = 0.0f32;
        for (i, p) in points.iter().enumerate() {
            if i > 0 { total += p.distance(points[i - 1]); }
            arc.push(total);
        }
        let total = total.max(1e-3);
        let aspect = image.map_or(1.0, |b| b.width as f32 / b.height.max(1) as f32);
        let copy_length = match settings.mode {
            subcraft_doc::RibbonMode::Stretch => total / settings.tiles.max(1e-3),
            subcraft_doc::RibbonMode::KeepAspect => 2.0 * radius * aspect * settings.tiles.max(1e-3),
        };
        let colored = settings.color_from_image && image.is_some();
        let done: Vec<(usize, Vec<f32>, Vec<[f32; 3]>)> = self.tiles[self.tile..].par_iter().map(|&tile| {
            let mut coverage = vec![0.0f32; TILE_TEXELS];
            let mut color = vec![[0.0f32; 3]; if colored { TILE_TEXELS } else { 0 }];
            for pixel in 0..TILE_TEXELS {
                let x = (tile as u32 % across) * TILE_SIZE + pixel as u32 % TILE_SIZE;
                let y = (tile as u32 / across) * TILE_SIZE + pixel as u32 / TILE_SIZE;
                if x >= res || y >= res { continue; }
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                // Closest point on the curve: segment, position along it, signed offset.
                let mut best: Option<(f32, usize, f32, f32)> = None;
                for &i in &bins[tile] {
                    let a = points[i]; let d = points[i + 1] - a;
                    let len2 = d.length_squared();
                    let raw_t = if len2 > 1e-12 { (p - a).dot(d) / len2 } else { 0.0 };
                    // Square ends: nothing before the first point or after the last one.
                    if open && ((i == 0 && raw_t < 0.0) || (i == last && raw_t > 1.0)) { continue; }
                    let t = raw_t.clamp(0.0, 1.0);
                    let dist = p.distance(a + d * t);
                    if best.is_none_or(|b| dist < b.0) {
                        let side = if len2 > 1e-12 { d.perp_dot(p - a) / len2.sqrt() } else { 0.0 };
                        best = Some((dist, i, t, side));
                    }
                }
                let Some((dist, i, t, side)) = best else { continue };
                let edge = (radius + 0.5 - dist).clamp(0.0, 1.0);
                if edge <= 0.0 { continue; }
                let along = (arc[i] + (arc[i + 1] - arc[i]) * t) / copy_length;
                let k = along.floor();
                let mut u = along - k;
                if settings.mirror && (k as i64).rem_euclid(2) == 1 { u = 1.0 - u; }
                let mut v = (0.5 + side / (2.0 * radius)).clamp(0.0, 1.0);
                if settings.flip { v = 1.0 - v; }
                let a = match image {
                    None => 1.0,
                    Some(b) if colored => {
                        let c = b.sample(u, v, subcraft_image::Wrap::Clamp, true);
                        color[pixel] = [c[0], c[1], c[2]];
                        c[3]
                    }
                    // The path's value through the image alpha: a ribbon image is RGBA, not a
                    // grayscale tip (QA 2026-11: red x alpha left its green and blue parts unpainted).
                    Some(b) => b.sample(u, v, subcraft_image::Wrap::Clamp, false)[3],
                };
                coverage[pixel] = a.clamp(0.0, 1.0) * edge * opacity;
            }
            (tile, coverage, color)
        }).collect();
        for (tile, coverage, color) in done {
            if !coverage.iter().any(|&a| a > 0.0) { continue; }
            for (&kind, img) in &mut self.images {
                let n = kind.components(); let comps = n + 1; let value = self.path.values[&kind].0;
                let from_image = colored && kind == ChannelKind::BaseColor;
                let mut data = vec![0.0; TILE_TEXELS * comps];
                for (pixel, (out, &alpha)) in data.chunks_exact_mut(comps).zip(&coverage).enumerate() {
                    if from_image { out[..3].copy_from_slice(&color[pixel]); } else { out[..n].copy_from_slice(&value[..n]); }
                    out[n] = alpha;
                }
                img.write_tile(tile, &data);
            }
        }
        self.tile = self.tiles.len(); self.pixel = 0; self.segment = 0; self.distance = f32::INFINITY; self.coverage.fill(0.0);
    }

    pub fn finish(self) -> Option<BTreeMap<ChannelKind, TiledImage>> { self.is_done().then_some(self.images) }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use subcraft_core::ChannelValue;
    use subcraft_doc::{PenAnchor, PenPath};

    fn path() -> PenPath {
        let anchors = [(0.1, 0.2), (0.5, 0.8), (0.9, 0.3), (0.6, 0.1)].into_iter().map(|(x, y)| PenAnchor::new(Vec2::new(x, y))).collect();
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.5, 0.0))].into_iter().collect();
        PenPath { anchors, closed: false, radius: 0.02, hardness: 0.5, opacity: 1.0, values, kind: Default::default(), stamp: None, ribbon: None }
    }

    #[test]
    fn running_in_parallel_gives_the_stepped_pixels_and_is_fast() {
        let mut stepped = PenRasterizer::new(&path(), 1024).unwrap();
        while !stepped.is_done() { stepped.step(16384); }
        let mut ran = PenRasterizer::new(&path(), 1024).unwrap();
        ran.step(5000); // half a tile first: run() must finish it
        let t = std::time::Instant::now();
        ran.run();
        let elapsed = t.elapsed();
        assert!(ran.is_done());
        let (a, b) = (stepped.finish().unwrap(), ran.finish().unwrap());
        assert_eq!(a[&ChannelKind::BaseColor], b[&ChannelKind::BaseColor], "identical raster");
        println!("1024² path raster in {elapsed:?}");
        assert!(elapsed.as_millis() < 1000, "a path raster takes well under a second ({elapsed:?})");
    }

    #[test]
    fn a_filled_path_covers_its_interior_with_an_anti_aliased_edge() {
        let square = [(0.25, 0.25), (0.75, 0.25), (0.75, 0.75), (0.25, 0.75)];
        let anchors = square.into_iter().map(|(x, y)| PenAnchor::new(Vec2::new(x, y))).collect();
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(0.2, 0.4, 0.6))].into_iter().collect();
        let filled = PenPath { anchors, closed: false, radius: 0.01, hardness: 1.0, opacity: 0.8, values, kind: subcraft_doc::PathKind::Filled, stamp: None, ribbon: None };
        let mut r = PenRasterizer::new(&filled, 128).unwrap();
        r.step(1);
        assert!(r.is_done(), "a filled path rasterizes in one pass");
        let img = &r.finish().unwrap()[&ChannelKind::BaseColor];
        let centre = img.get(64, 64);
        assert!((centre[3] - 0.8).abs() < 1e-3 && (centre[0] - 0.2).abs() < 1e-3, "{centre:?}");
        assert_eq!(img.get(10, 10)[3], 0.0, "outside stays empty");
        assert_eq!(img.get(64, 120)[3], 0.0);
        // The edge at x = 32 texels: half covered on the boundary texel row, full just inside.
        assert!((img.get(40, 64)[3] - 0.8).abs() < 1e-3);
        let edge = img.get(31, 64)[3];
        assert!(edge < 0.05, "{edge}");
        // The same anchors as a stroke paint only near the curve, not the middle.
        let mut stroke = filled.clone();
        stroke.kind = subcraft_doc::PathKind::Paint;
        let mut r = PenRasterizer::new(&stroke, 128).unwrap();
        r.run();
        assert_eq!(r.finish().unwrap()[&ChannelKind::BaseColor].get(64, 64)[3], 0.0);
    }

    #[test]
    fn a_stamped_path_places_tips_at_its_spacing() {
        let anchors = [(0.1, 0.5), (0.9, 0.5)].into_iter().map(|(x, y)| PenAnchor::new(Vec2::new(x, y))).collect();
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 1.0, 1.0))].into_iter().collect();
        let mut path = PenPath { anchors, closed: false, radius: 0.02, hardness: 1.0, opacity: 1.0, values, kind: subcraft_doc::PathKind::Paint, stamp: None, ribbon: None };
        let continuous = { let mut r = PenRasterizer::new(&path, 256).unwrap(); r.run(); r.finish().unwrap() };
        // Spacing 2: a stamp every two diameters (20.48 texels, radius 5.12) from x = 25.6.
        path.stamp = Some(subcraft_doc::PathStamp { alpha: None, spacing: 2.0, follow: true });
        let mut r = PenRasterizer::new(&path, 256).unwrap();
        r.step(1);
        assert!(r.is_done(), "a stamped path rasterizes in one pass");
        let stamped = r.finish().unwrap();
        let (a, b) = (&continuous[&ChannelKind::BaseColor], &stamped[&ChannelKind::BaseColor]);
        assert!(b.get(25, 127)[3] > 0.9 && b.get(46, 127)[3] > 0.9, "stamps at their centres");
        assert_eq!(b.get(35, 127)[3], 0.0, "a gap between two stamps");
        assert!(a.get(35, 127)[3] > 0.9, "the continuous stroke covers it");
        assert_eq!(b.get(128, 100)[3], 0.0, "nothing away from the path");
        // Spacing outside its range is refused by validation.
        path.stamp = Some(subcraft_doc::PathStamp { alpha: None, spacing: 0.0, follow: true });
        assert!(PenRasterizer::new(&path, 256).is_err());
    }

    #[test]
    fn a_ribbon_lays_its_image_along_the_path_with_square_ends() {
        use std::sync::Arc;
        use subcraft_doc::{PathKind, PathRibbon, RibbonMode};
        use subcraft_image::Bitmap;
        let anchors = [(0.1, 0.5), (0.9, 0.5)].into_iter().map(|(x, y)| PenAnchor::new(Vec2::new(x, y))).collect();
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 1.0, 0.0))].into_iter().collect();
        let mut path = PenPath { anchors, closed: false, radius: 0.05, hardness: 1.0, opacity: 1.0, values, kind: PathKind::Ribbon, stamp: None, ribbon: None };
        // Red on the left half of the image, blue on the right; top row and bottom row differ in green.
        let along = Arc::new(Bitmap::new_f32(2, 1, 4, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0]));
        let across = Arc::new(Bitmap::new_f32(1, 2, 4, vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]));
        let render = |path: &PenPath, image: &Arc<Bitmap>| {
            let mut r = PenRasterizer::new(path, 256).unwrap().with_alpha(Some(image.clone()));
            r.step(1);
            assert!(r.is_done());
            r.finish().unwrap().remove(&ChannelKind::BaseColor).unwrap()
        };
        path.ribbon = Some(PathRibbon { image: None, ..Default::default() });
        let img = render(&path, &along);
        // x from 25.6 to 230.4 texels, 12.8 texels either side of y = 128.
        let start = img.get(30, 128);
        let end = img.get(226, 128);
        assert!(start[3] > 0.99 && start[0] > 0.9 && start[2] < 0.1, "the image's left end at the path's start: {start:?}");
        assert!(end[3] > 0.99 && end[2] > 0.9 && end[0] < 0.1, "its right end at the path's end: {end:?}");
        assert_eq!(img.get(240, 128)[3], 0.0, "the end is cut square, not rounded");
        assert_eq!(img.get(128, 150)[3], 0.0, "nothing beyond the width");
        // Two copies, every second mirrored: the end shows the image's left end again.
        path.ribbon = Some(PathRibbon { tiles: 2.0, mirror: true, ..Default::default() });
        let mirrored = render(&path, &along).get(226, 128);
        assert!(mirrored[0] > 0.9 && mirrored[2] < 0.1, "{mirrored:?}");
        // Across: the image's top row on one side, the bottom row on the other; Flip swaps them.
        path.ribbon = Some(PathRibbon { mode: RibbonMode::KeepAspect, ..Default::default() });
        let img = render(&path, &across);
        let (above, below) = (img.get(128, 118), img.get(128, 138));
        assert!(above[0] > 0.9 && below[1] > 0.9, "{above:?} {below:?}");
        path.ribbon = Some(PathRibbon { mode: RibbonMode::KeepAspect, flip: true, ..Default::default() });
        let flipped = render(&path, &across).get(128, 118);
        assert!(flipped[1] > 0.9 && flipped[0] < 0.1, "{flipped:?}");
        // Without the image's colour, Base Color is the path's value through the image alpha.
        path.ribbon = Some(PathRibbon { color_from_image: false, ..Default::default() });
        let plain = render(&path, &along).get(30, 128);
        assert!(plain[1] > 0.99 && plain[0] < 0.01, "{plain:?}");
        // Also where the image is opaque but not red (its green half).
        path.ribbon = Some(PathRibbon { mode: RibbonMode::KeepAspect, color_from_image: false, ..Default::default() });
        let green_part = render(&path, &across).get(128, 138);
        assert!(green_part[3] > 0.9, "coverage is the image alpha: {green_part:?}");
    }
}
