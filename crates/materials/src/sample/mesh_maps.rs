//! Baked mesh maps supplied by the caller (the bake crate produces them).

use std::collections::BTreeMap;

use subcraft_core::MeshMapKind;
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};

use super::geometry::TileGeometry;

/// Mesh maps of one texture set. Missing maps read as [`MeshMapKind::neutral_value`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshMaps {
    maps: BTreeMap<MeshMapKind, TiledImage>,
}

impl MeshMaps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, kind: MeshMapKind, image: TiledImage) {
        self.maps.insert(kind, image);
    }

    pub fn remove(&mut self, kind: MeshMapKind) -> Option<TiledImage> {
        self.maps.remove(&kind)
    }

    pub fn get(&self, kind: MeshMapKind) -> Option<&TiledImage> {
        self.maps.get(&kind)
    }

    pub fn from_map(maps: BTreeMap<MeshMapKind, TiledImage>) -> Self {
        Self { maps }
    }

    pub fn iter(&self) -> impl Iterator<Item = (MeshMapKind, &TiledImage)> {
        self.maps.iter().map(|(k, v)| (*k, v))
    }

    pub fn as_map(&self) -> &BTreeMap<MeshMapKind, TiledImage> {
        &self.maps
    }

    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }

    /// Values of `kind` for every texel of `geo`'s tile (gray maps replicated to 3 components).
    /// A map at a different resolution is bilinearly sampled.
    pub fn read_tile(&self, kind: MeshMapKind, geo: &TileGeometry, out: &mut [[f32; 3]]) {
        debug_assert_eq!(out.len(), TILE_TEXELS);
        let Some(img) = self.maps.get(&kind) else {
            out.fill(kind.neutral_value());
            return;
        };
        let expand = |px: [f32; 4]| match img.comps() {
            1 | 2 => [px[0]; 3],
            _ => [px[0], px[1], px[2]],
        };
        if img.width() == geo.resolution && img.height() == geo.resolution {
            let comps = img.comps();
            let mut buf = vec![0.0f32; TILE_TEXELS * comps];
            img.read_tile(img.tile_index(geo.x0 / TILE_SIZE, geo.y0 / TILE_SIZE), &mut buf);
            for (o, px) in out.iter_mut().zip(buf.chunks_exact(comps)) {
                let mut p = [0.0; 4];
                p[..comps].copy_from_slice(px);
                *o = expand(p);
            }
        } else {
            // Bilinear, so a map baked below the set's resolution (1024 on a 2048 set) is smooth
            // instead of 2 x 2 blocks.
            resample(geo, img.width(), img.height(), |x, y| expand(img.get(x, y)), out);
        }
    }
}

/// Bilinear samples of a `w` x `h` map at the texel centres of `geo`'s tile. `texel` reads one
/// map texel (already expanded to 3 components).
///
/// A texel's U depends only on its column and its V only on its row, so the sample positions and
/// weights are worked out once per column and once per row. When the map texels the tile reads
/// form a small window (a map baked at or below the set's resolution), each of them is read once
/// into a buffer instead of four times per output texel, each read going through the tiled
/// image's tile lookup and half-float decode. The arithmetic is the same either way, so the
/// result is bit-identical to sampling every texel on its own.
fn resample(geo: &TileGeometry, w: u32, h: u32, texel: impl Fn(u32, u32) -> [f32; 3], out: &mut [[f32; 3]]) {
    let side = TILE_SIZE as usize;
    let axis = |t: f32, n: u32| {
        let f = (t * n as f32 - 0.5).clamp(0.0, (n - 1) as f32);
        let p0 = f.floor() as u32;
        (p0, (p0 + 1).min(n - 1), f - p0 as f32)
    };
    let cols: Vec<(u32, u32, f32)> = (0..side).map(|x| axis(geo.uv(x)[0], w)).collect();
    let rows: Vec<(u32, u32, f32)> = (0..side).map(|y| axis(geo.uv(y * side)[1], h)).collect();
    let mix = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], tx: f32, ty: f32| {
        let mut px = [0.0f32; 3];
        for k in 0..3 {
            px[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
        }
        px
    };
    // Positions grow with the column / row, so the window is spanned by the first and last.
    let (wx0, wx1) = (cols[0].0, cols[side - 1].1);
    let (wy0, wy1) = (rows[0].0, rows[side - 1].1);
    let (ww, wh) = ((wx1 - wx0 + 1) as usize, (wy1 - wy0 + 1) as usize);
    if ww * wh > 4 * TILE_TEXELS {
        // A map above the set's resolution: a window would read more texels than the samples do.
        for (i, o) in out.iter_mut().enumerate() {
            let ((x0, x1, tx), (y0, y1, ty)) = (cols[i % side], rows[i / side]);
            *o = mix(texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1), tx, ty);
        }
        return;
    }
    let mut window = Vec::with_capacity(ww * wh);
    for y in wy0..=wy1 {
        for x in wx0..=wx1 {
            window.push(texel(x, y));
        }
    }
    let at = |x: u32, y: u32| window[(y - wy0) as usize * ww + (x - wx0) as usize];
    for (i, o) in out.iter_mut().enumerate() {
        let ((x0, x1, tx), (y0, y1, ty)) = (cols[i % side], rows[i / side]);
        *o = mix(at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1), tx, ty);
    }
}
