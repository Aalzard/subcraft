//! Copy-on-write tiled texel storage (ADR-0003).
//!
//! A [`TiledImage`] is a grid of `TILE_SIZE`² tiles. Unallocated tiles implicitly hold the image's
//! `fill` value, so sparse paint layers cost almost nothing. Both the tile table and each tile are
//! reference counted: cloning an image is O(1) and mutation copies only the tiles it touches.
//! History snapshots, compositor caches and the live document therefore share all untouched
//! texel data, and "what changed" is answered by pointer comparison ([`TiledImage::tile_changed`]).
//!
//! Texels are stored as interleaved `f16` components. All math happens in `f32`.

use std::sync::Arc;

use half::f16;
use half::slice::HalfFloatSliceExt;
use subcraft_core::IRect;

pub const TILE_SIZE: u32 = 64;
pub const TILE_TEXELS: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// One tile of interleaved half-float texels.
#[derive(Clone, PartialEq)]
pub struct Tile {
    pub data: Box<[f16]>,
}

impl Tile {
    pub fn filled(comps: usize, fill: &[f32; 4]) -> Self {
        let mut data = vec![f16::ZERO; TILE_TEXELS * comps].into_boxed_slice();
        let px: Vec<f16> = (0..comps).map(|c| f16::from_f32(fill[c])).collect();
        for t in data.chunks_exact_mut(comps) {
            t.copy_from_slice(&px);
        }
        Self { data }
    }
}

pub type TileSlot = Option<Arc<Tile>>;

#[derive(Clone)]
pub struct TiledImage {
    width: u32,
    height: u32,
    comps: u8,
    tiles_x: u32,
    tiles_y: u32,
    tiles: Arc<Vec<TileSlot>>,
    fill: [f32; 4],
}

impl std::fmt::Debug for TiledImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TiledImage")
            .field("size", &(self.width, self.height))
            .field("comps", &self.comps)
            .field("allocated", &self.allocated_tiles())
            .finish()
    }
}

/// Value equality: identical layout, fill and texels. Shared tiles compare by pointer first,
/// so comparing an image with its own snapshot is cheap.
impl PartialEq for TiledImage {
    fn eq(&self, other: &Self) -> bool {
        if !self.same_layout(other) || self.fill != other.fill {
            return false;
        }
        if Arc::ptr_eq(&self.tiles, &other.tiles) {
            return true;
        }
        let comps = self.comps();
        let fill: Vec<f16> = (0..comps).map(|c| f16::from_f32(self.fill[c])).collect();
        let is_fill = |t: &Tile| t.data.chunks_exact(comps).all(|px| px == fill.as_slice());
        self.tiles.iter().zip(other.tiles.iter()).all(|(a, b)| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.data == b.data,
            (Some(t), None) | (None, Some(t)) => is_fill(t),
        })
    }
}

impl TiledImage {
    pub fn new(width: u32, height: u32, comps: usize, fill: [f32; 4]) -> Self {
        assert!((1..=4).contains(&comps), "1..=4 components supported");
        assert!(width > 0 && height > 0);
        let tiles_x = width.div_ceil(TILE_SIZE);
        let tiles_y = height.div_ceil(TILE_SIZE);
        Self {
            width,
            height,
            comps: comps as u8,
            tiles_x,
            tiles_y,
            tiles: Arc::new(vec![None; (tiles_x * tiles_y) as usize]),
            fill,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn comps(&self) -> usize {
        self.comps as usize
    }
    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }
    pub fn tiles_y(&self) -> u32 {
        self.tiles_y
    }
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
    pub fn fill(&self) -> [f32; 4] {
        self.fill
    }

    #[inline]
    pub fn tile_index(&self, tx: u32, ty: u32) -> usize {
        (ty * self.tiles_x + tx) as usize
    }

    #[inline]
    pub fn tile_coords(&self, index: usize) -> (u32, u32) {
        (index as u32 % self.tiles_x, index as u32 / self.tiles_x)
    }

    /// Texel rectangle covered by a tile (clipped to the image).
    pub fn tile_rect(&self, index: usize) -> IRect {
        let (tx, ty) = self.tile_coords(index);
        let x0 = (tx * TILE_SIZE) as i32;
        let y0 = (ty * TILE_SIZE) as i32;
        IRect::new(
            x0,
            y0,
            (x0 + TILE_SIZE as i32).min(self.width as i32),
            (y0 + TILE_SIZE as i32).min(self.height as i32),
        )
    }

    /// Indices of tiles overlapping a texel rectangle.
    pub fn tiles_in_rect(&self, r: &IRect) -> Vec<usize> {
        let r = r.intersect(&IRect::from_size(self.width, self.height));
        if r.is_empty() {
            return Vec::new();
        }
        let ts = TILE_SIZE as i32;
        let (tx0, ty0) = (r.x0 / ts, r.y0 / ts);
        let (tx1, ty1) = ((r.x1 - 1) / ts, (r.y1 - 1) / ts);
        let mut out = Vec::with_capacity(((tx1 - tx0 + 1) * (ty1 - ty0 + 1)) as usize);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                out.push(self.tile_index(tx as u32, ty as u32));
            }
        }
        out
    }

    pub fn tile(&self, index: usize) -> Option<&Arc<Tile>> {
        self.tiles[index].as_ref()
    }

    pub fn is_allocated(&self, index: usize) -> bool {
        self.tiles[index].is_some()
    }

    pub fn allocated_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    /// Bytes of texel data owned (shared tiles are counted fully).
    pub fn memory_bytes(&self) -> usize {
        self.allocated_tiles() * TILE_TEXELS * self.comps() * 2
    }

    /// True if both images share the same tile table (no change at all).
    pub fn same_storage(&self, other: &TiledImage) -> bool {
        Arc::ptr_eq(&self.tiles, &other.tiles) && self.fill == other.fill
    }

    /// True if tile `index` differs in identity between two images of identical layout.
    pub fn tile_changed(&self, other: &TiledImage, index: usize) -> bool {
        match (&self.tiles[index], &other.tiles[index]) {
            (None, None) => self.fill != other.fill,
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            _ => true,
        }
    }

    pub fn same_layout(&self, other: &TiledImage) -> bool {
        self.width == other.width && self.height == other.height && self.comps == other.comps
    }

    /// Indices of tiles that differ between two images of identical layout.
    pub fn changed_tiles(&self, other: &TiledImage) -> Vec<usize> {
        if self.same_storage(other) {
            return Vec::new();
        }
        (0..self.tile_count()).filter(|&i| self.tile_changed(other, i)).collect()
    }

    /// Decode tile `index` into `out` (`TILE_TEXELS * comps` f32 values).
    pub fn read_tile(&self, index: usize, out: &mut [f32]) {
        let comps = self.comps();
        debug_assert_eq!(out.len(), TILE_TEXELS * comps);
        match &self.tiles[index] {
            Some(t) => t.data.convert_to_f32_slice(out),
            None => {
                for px in out.chunks_exact_mut(comps) {
                    px.copy_from_slice(&self.fill[..comps]);
                }
            }
        }
    }

    /// Encode `data` into tile `index`.
    pub fn write_tile(&mut self, index: usize, data: &[f32]) {
        let comps = self.comps();
        debug_assert_eq!(data.len(), TILE_TEXELS * comps);
        let slot = &mut Arc::make_mut(&mut self.tiles)[index];
        match slot {
            Some(t) => Arc::make_mut(t).data.convert_from_f32_slice(data),
            None => {
                let mut buf = vec![f16::ZERO; TILE_TEXELS * comps].into_boxed_slice();
                buf.convert_from_f32_slice(data);
                *slot = Some(Arc::new(Tile { data: buf }));
            }
        }
    }

    /// Mutable access to the tile table for parallel per-tile writes.
    /// Use [`slot_data_mut`] on individual slots.
    pub fn slots_mut(&mut self) -> TileSlots<'_> {
        TileSlots {
            comps: self.comps as usize,
            fill: self.fill,
            slots: Arc::make_mut(&mut self.tiles).as_mut_slice(),
        }
    }

    /// Replace a tile slot wholesale (used by undo-free loaders and tests).
    pub fn set_slot(&mut self, index: usize, slot: TileSlot) {
        Arc::make_mut(&mut self.tiles)[index] = slot;
    }

    /// Drop tile allocations equal to the fill value (compaction after erase/clear).
    pub fn compact(&mut self) {
        let comps = self.comps();
        let fill: Vec<f16> = (0..comps).map(|c| f16::from_f32(self.fill[c])).collect();
        let needs = self.tiles.iter().any(|t| {
            t.as_ref()
                .is_some_and(|t| t.data.chunks_exact(comps).all(|px| px == fill.as_slice()))
        });
        if !needs {
            return;
        }
        for slot in Arc::make_mut(&mut self.tiles).iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|t| t.data.chunks_exact(comps).all(|px| px == fill.as_slice()))
            {
                *slot = None;
            }
        }
    }

    /// Slow single-texel read (tests, pickers).
    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        let mut out = [0.0; 4];
        let comps = self.comps();
        let idx = self.tile_index(x / TILE_SIZE, y / TILE_SIZE);
        match &self.tiles[idx] {
            Some(t) => {
                let o = (((y % TILE_SIZE) * TILE_SIZE + (x % TILE_SIZE)) as usize) * comps;
                for c in 0..comps {
                    out[c] = t.data[o + c].to_f32();
                }
            }
            None => out[..comps].copy_from_slice(&self.fill[..comps]),
        }
        out
    }

    /// Slow single-texel write (tests, tools operating on few texels).
    pub fn set(&mut self, x: u32, y: u32, v: [f32; 4]) {
        let comps = self.comps();
        let idx = self.tile_index(x / TILE_SIZE, y / TILE_SIZE);
        let fill = self.fill;
        let slot = &mut Arc::make_mut(&mut self.tiles)[idx];
        let data = slot_data_mut(slot, comps, &fill);
        let o = (((y % TILE_SIZE) * TILE_SIZE + (x % TILE_SIZE)) as usize) * comps;
        for c in 0..comps {
            data[o + c] = f16::from_f32(v[c]);
        }
    }

    /// Dense row-major f32 copy (`width * height * comps`).
    pub fn to_dense(&self) -> Vec<f32> {
        let comps = self.comps();
        let w = self.width as usize;
        let mut out = vec![0.0f32; w * self.height as usize * comps];
        let mut buf = vec![0.0f32; TILE_TEXELS * comps];
        for i in 0..self.tile_count() {
            self.read_tile(i, &mut buf);
            let r = self.tile_rect(i);
            for y in r.y0..r.y1 {
                let ly = (y - r.y0) as usize;
                let src = &buf[ly * TILE_SIZE as usize * comps..][..r.width() as usize * comps];
                let dst = &mut out[(y as usize * w + r.x0 as usize) * comps..][..src.len()];
                dst.copy_from_slice(src);
            }
        }
        out
    }

    /// Build from dense row-major f32 data.
    pub fn from_dense(width: u32, height: u32, comps: usize, fill: [f32; 4], data: &[f32]) -> Self {
        assert_eq!(data.len(), (width * height) as usize * comps);
        let mut img = TiledImage::new(width, height, comps, fill);
        let mut buf = vec![0.0f32; TILE_TEXELS * comps];
        let w = width as usize;
        for i in 0..img.tile_count() {
            let r = img.tile_rect(i);
            buf.iter_mut().enumerate().for_each(|(k, v)| *v = fill[k % comps]);
            for y in r.y0..r.y1 {
                let ly = (y - r.y0) as usize;
                let src = &data[(y as usize * w + r.x0 as usize) * comps..][..r.width() as usize * comps];
                buf[ly * TILE_SIZE as usize * comps..][..src.len()].copy_from_slice(src);
            }
            img.write_tile(i, &buf);
        }
        img.compact();
        img
    }
}

impl TiledImage {
    /// Bilinear resample to a new size (texel centers aligned). An image with no allocated tiles
    /// stays sparse.
    pub fn resampled(&self, width: u32, height: u32) -> TiledImage {
        if width == self.width && height == self.height {
            return self.clone();
        }
        let comps = self.comps();
        if self.allocated_tiles() == 0 {
            return TiledImage::new(width, height, comps, self.fill);
        }
        let src = self.to_dense();
        let (sw, sh) = (self.width as usize, self.height as usize);
        let mut out = vec![0.0f32; (width * height) as usize * comps];
        for y in 0..height as usize {
            let fy = ((y as f32 + 0.5) * sh as f32 / height as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(sh - 1);
            for x in 0..width as usize {
                let fx = ((x as f32 + 0.5) * sw as f32 / width as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
                let (x0, tx) = (fx.floor() as usize, fx.fract());
                let x1 = (x0 + 1).min(sw - 1);
                for c in 0..comps {
                    let at = |xx: usize, yy: usize| src[(yy * sw + xx) * comps + c];
                    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                    let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                    out[(y * width as usize + x) * comps + c] = top + (bot - top) * ty;
                }
            }
        }
        TiledImage::from_dense(width, height, comps, self.fill, &out)
    }
}

/// Mutable view over a tile table; slots can be split across threads (`par_iter_mut`).
pub struct TileSlots<'a> {
    pub comps: usize,
    pub fill: [f32; 4],
    pub slots: &'a mut [TileSlot],
}

/// Copy-on-write access to a slot's texels, allocating from `fill` if empty.
pub fn slot_data_mut<'a>(slot: &'a mut TileSlot, comps: usize, fill: &[f32; 4]) -> &'a mut [f16] {
    if slot.is_none() {
        *slot = Some(Arc::new(Tile::filled(comps, fill)));
    }
    &mut Arc::make_mut(slot.as_mut().expect("allocated above")).data
}

/// Read a slot (None = fill) into f32.
pub fn slot_read(slot: &TileSlot, comps: usize, fill: &[f32; 4], out: &mut [f32]) {
    match slot {
        Some(t) => t.data.convert_to_f32_slice(out),
        None => {
            for px in out.chunks_exact_mut(comps) {
                px.copy_from_slice(&fill[..comps]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_until_written() {
        let mut img = TiledImage::new(256, 256, 2, [0.25, 0.0, 0.0, 0.0]);
        assert_eq!(img.tile_count(), 16);
        assert_eq!(img.allocated_tiles(), 0);
        assert_eq!(img.get(10, 10)[0], 0.25);
        img.set(70, 3, [1.0, 0.5, 0.0, 0.0]);
        assert_eq!(img.allocated_tiles(), 1);
        assert_eq!(img.get(70, 3)[..2], [1.0, 0.5]);
        assert_eq!(img.get(71, 3)[0], 0.25);
    }

    #[test]
    fn clone_is_copy_on_write() {
        let mut a = TiledImage::new(128, 128, 1, [0.0; 4]);
        a.set(0, 0, [1.0; 4]);
        let snapshot = a.clone();
        assert!(a.same_storage(&snapshot));
        a.set(100, 100, [0.5; 4]);
        assert!(!a.same_storage(&snapshot));
        // Untouched tile shared, touched tile copied.
        assert!(!a.tile_changed(&snapshot, a.tile_index(0, 0)));
        assert!(a.tile_changed(&snapshot, a.tile_index(1, 1)));
        assert_eq!(snapshot.get(100, 100)[0], 0.0, "snapshot must be untouched");
        assert_eq!(a.changed_tiles(&snapshot), vec![a.tile_index(1, 1)]);
    }

    #[test]
    fn dense_roundtrip_non_multiple_size() {
        let (w, h) = (100, 70);
        let data: Vec<f32> = (0..w * h * 3).map(|i| (i % 97) as f32 / 97.0).collect();
        let img = TiledImage::from_dense(w as u32, h as u32, 3, [0.0; 4], &data);
        let back = img.to_dense();
        for (a, b) in data.iter().zip(&back) {
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn resample_preserves_values_and_sparsity() {
        let empty = TiledImage::new(128, 128, 2, [0.0; 4]);
        let r = empty.resampled(256, 256);
        assert_eq!((r.width(), r.height(), r.allocated_tiles()), (256, 256, 0));

        // Left half 1, right half 0: halves survive up- and down-sampling.
        let mut img = TiledImage::new(128, 128, 1, [0.0; 4]);
        for y in 0..128 {
            for x in 0..64 {
                img.set(x, y, [1.0; 4]);
            }
        }
        for size in [64u32, 256] {
            let r = img.resampled(size, size);
            assert_eq!(r.get(size / 8, size / 2)[0], 1.0, "{size}");
            assert_eq!(r.get(size - size / 8, size / 2)[0], 0.0, "{size}");
        }
    }

    #[test]
    fn compact_drops_fill_tiles() {
        let mut img = TiledImage::new(128, 64, 1, [0.0; 4]);
        img.set(1, 1, [0.0; 4]);
        img.set(70, 1, [1.0; 4]);
        assert_eq!(img.allocated_tiles(), 2);
        img.compact();
        assert_eq!(img.allocated_tiles(), 1);
    }

    #[test]
    fn tiles_in_rect_clips() {
        let img = TiledImage::new(200, 200, 1, [0.0; 4]);
        assert_eq!(img.tiles_in_rect(&IRect::new(-10, -10, 5, 5)), vec![0]);
        assert_eq!(img.tiles_in_rect(&IRect::new(60, 0, 70, 1)).len(), 2);
        assert!(img.tiles_in_rect(&IRect::new(300, 300, 400, 400)).is_empty());
    }
}
