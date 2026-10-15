//! Bounded-work path rasterization. Coverage is the closest distance to the entire curve,
//! so overlapping segments/caps do not compound opacity. Only touched tiles are allocated.
use std::collections::BTreeMap;
use glam::Vec2;
use subcraft_core::ChannelKind;
use subcraft_doc::{PenPath, empty_paint_image};
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
}

impl PenRasterizer {
    pub fn new(path: &PenPath, resolution: u32) -> Result<Self, &'static str> {
        if resolution == 0 || resolution > 8192 { return Err("invalid pen resolution"); }
        let mut points = path.polyline(0.35 / resolution as f32)?;
        for p in &mut points { *p *= resolution as f32; }
        if points.len() == 1 { points.push(points[0]); }
        let size = resolution.div_ceil(TILE_SIZE);
        let mut bins = vec![Vec::new(); (size * size) as usize];
        let radius = path.radius * resolution as f32 + 1.0;
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
        let tiles = bins.iter().enumerate().filter(|(_, b)| !b.is_empty()).map(|(i, _)| i).collect();
        let images = path.values.keys().map(|&k| (k, empty_paint_image(k, resolution))).collect();
        Ok(Self { path: path.clone(), points, bins, tiles, tile: 0, pixel: 0, segment: 0, distance: f32::INFINITY, coverage: vec![0.0; TILE_TEXELS], images, resolution })
    }

    pub fn progress(&self) -> f32 { if self.tiles.is_empty() { 1.0 } else { self.tile as f32 / self.tiles.len() as f32 } }
    pub fn is_done(&self) -> bool { self.tile == self.tiles.len() }
    /// At most `budget` segment-distance evaluations, plus completed 64² tile writes.
    pub fn step(&mut self, mut budget: usize) {
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

    pub fn finish(self) -> Option<BTreeMap<ChannelKind, TiledImage>> { self.is_done().then_some(self.images) }
}
