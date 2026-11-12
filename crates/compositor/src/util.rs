//! Tile read helpers.

use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_materials::TileGeometry;

/// Read the texels of `geo`'s tile from `img` as RGBA-padded f32 (`TILE_TEXELS` entries).
/// Images at another resolution are point-sampled at texel centers.
pub fn read_tile_rgba(img: &TiledImage, geo: &TileGeometry, out: &mut [[f32; 4]]) {
    debug_assert_eq!(out.len(), TILE_TEXELS);
    let comps = img.comps();
    if img.width() == geo.resolution && img.height() == geo.resolution && comps == 4 {
        img.read_tile(img.tile_index(geo.x0 / TILE_SIZE, geo.y0 / TILE_SIZE), out.as_flattened_mut());
    } else if img.width() == geo.resolution && img.height() == geo.resolution {
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

/// Painter's Gradient Dynamic: the image's colour at the input value along one row (or column)
/// of it, at `position` across. `linearize` decodes a colour image for colour channels.
pub(crate) fn gradient_dynamic(bmp: &subcraft_image::Bitmap, t: f32, vertical: bool, position: f32, linearize: bool) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let (u, v) = if vertical { (position, t) } else { (t, position) };
    let s = bmp.sample(u.clamp(0.0, 1.0), v.clamp(0.0, 1.0), subcraft_image::Wrap::Clamp, linearize);
    [s[0], s[1], s[2]]
}

/// Whether any content effect of `layer` can change channel `kind`. A layer without content for
/// a channel only takes part in it (even in Replace mode) if such an effect exists; an effect on
/// another channel must not make it wipe this one.
pub(crate) fn effects_touch(layer: &subcraft_doc::Layer, kind: subcraft_core::ChannelKind) -> bool {
    use subcraft_doc::ContentEffectKind as K;
    layer.content_effects.iter().filter(|e| e.visible && e.blend_for(kind).is_active()).any(|e| match &e.kind {
        K::Paint { channels } => channels.contains_key(&kind),
        K::Fill(f) => f.channels.get(&kind).is_some_and(|c| c.enabled),
        K::Levels { channel, .. } => *channel == kind && kind != subcraft_core::ChannelKind::Normal,
        K::Filter { filter, .. } => e.applies_to(kind) && filter.valid(),
        K::Anchor => false,
    })
}
