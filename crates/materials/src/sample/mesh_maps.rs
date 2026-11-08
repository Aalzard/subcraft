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
            let (w, h) = (img.width(), img.height());
            for (i, o) in out.iter_mut().enumerate() {
                let [u, v] = geo.uv(i);
                let fx = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
                let fy = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
                let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let (a, b, c, d) = (expand(img.get(x0, y0)), expand(img.get(x1, y0)), expand(img.get(x0, y1)), expand(img.get(x1, y1)));
                let mut px = [0.0f32; 3];
                for k in 0..3 {
                    px[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
                }
                *o = px;
            }
        }
    }
}
