//! Tile read helpers.

use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_materials::TileGeometry;

/// Read the texels of `geo`'s tile from `img` as RGBA-padded f32 (`TILE_TEXELS` entries).
/// Images at another resolution are point-sampled at texel centers.
pub fn read_tile_rgba(img: &TiledImage, geo: &TileGeometry, out: &mut [[f32; 4]]) {
    debug_assert_eq!(out.len(), TILE_TEXELS);
    let comps = img.comps();
    if img.width() == geo.resolution && img.height() == geo.resolution {
        let mut buf = vec![0.0f32; TILE_TEXELS * comps];
        img.read_tile(img.tile_index(geo.x0 / TILE_SIZE, geo.y0 / TILE_SIZE), &mut buf);
        for (o, px) in out.iter_mut().zip(buf.chunks_exact(comps)) {
            let mut p = [0.0; 4];
            p[..comps].copy_from_slice(px);
            *o = p;
        }
    } else {
        for (i, o) in out.iter_mut().enumerate() {
            let [u, v] = geo.uv(i);
            let x = ((u * img.width() as f32) as u32).min(img.width() - 1);
            let y = ((v * img.height() as f32) as u32).min(img.height() - 1);
            *o = img.get(x, y);
        }
    }
}
