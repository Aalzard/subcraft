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

    pub fn get(&self, kind: MeshMapKind) -> Option<&TiledImage> {
        self.maps.get(&kind)
    }

    /// Values of `kind` for every texel of `geo`'s tile (gray maps replicated to 3 components).
    /// A map at a different resolution is point-sampled at texel centers.
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
            for (i, o) in out.iter_mut().enumerate() {
                let [u, v] = geo.uv(i);
                let x = ((u * img.width() as f32) as u32).min(img.width() - 1);
                let y = ((v * img.height() as f32) as u32).min(img.height() - 1);
                *o = expand(img.get(x, y));
            }
        }
    }
}
