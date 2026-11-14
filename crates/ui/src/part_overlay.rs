//! Geometry mask and Polygon Fill feedback on the model in the 3D view: the geometry a geometry
//! mask leaves out is drawn grey with diagonal lines while the mask is edited, and the part (or
//! the element Polygon Fill would fill) under the cursor is tinted. The renderer lays a small
//! UV-space mask over the selected texture set; this module decides what is in it.

use glam::Vec2;
use subcraft_core::{LayerId, PartKind, TextureSetId};
use subcraft_doc::PartMask;
use subcraft_paint::PolygonFillMode;

use crate::state::UiState;

/// Side of the overlay mask in texels.
pub const SIZE: u32 = 1024;

type Excluded = (TextureSetId, LayerId, PartMask);

/// What is tinted: a part of the geometry mask's kind, or the Polygon Fill element grown from a
/// triangle in a fill mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    Part(PartKind, u32),
    Fill(PolygonFillMode, u32),
}

/// What the renderer was last given, and the greyed-out plane kept between cursor moves.
#[derive(Default)]
pub struct Overlay3d {
    key: Option<(u32, Option<Excluded>, Option<Hover>)>,
    grey_key: Option<Excluded>,
    grey: Vec<u8>,
}

/// What the 3D view shows this frame: `picking` while a geometry mask is edited (`hovered_part`
/// is the part under the cursor), otherwise `fill_triangle`, the triangle under the cursor while
/// the Polygon Fill tool hovers the model. Returns the overlay to hand the renderer when it
/// changed: `Some(None)` removes it, `Some(Some((material slot, texels)))` replaces it.
pub fn update(state: &mut UiState, picking: bool, hovered_part: Option<u32>, fill_triangle: Option<u32>) -> Option<Option<(u32, Vec<u8>)>> {
    let ts = state.session.selected_texture_set();
    let (ts_id, slot) = (ts.id, ts.material_slot);
    let excluded: Option<Excluded> = if picking {
        state.session.selected_layer().and_then(|l| l.parts.clone().filter(|p| p.enabled).map(|p| (ts_id, l.id, p)))
    } else {
        None
    };
    let hover = if picking {
        state.uv_parts.list_hover.or(hovered_part).map(|part| Hover::Part(state.session.mask_kind(), part))
    } else {
        fill_triangle.map(|t| Hover::Fill(state.fill_mode, t))
    };
    let key = (excluded.is_some() || hover.is_some()).then(|| (slot, excluded.clone(), hover));
    if key == state.part_overlay_3d.key {
        return None;
    }
    let change = key.is_some().then(|| {
        if excluded != state.part_overlay_3d.grey_key {
            let tris = if excluded.is_some() { state.session.excluded_uv_triangles() } else { Vec::new() };
            let mut grey = vec![0u8; (SIZE * SIZE) as usize];
            subcraft_app::parts::raster_uv_triangles(SIZE, &tris, &mut grey, 1, 0);
            state.part_overlay_3d.grey = grey;
            state.part_overlay_3d.grey_key = excluded.clone();
        }
        let tinted: Vec<[Vec2; 3]> = match hover {
            Some(Hover::Part(_, part)) => state.session.part_uv_triangles(part),
            Some(Hover::Fill(mode, triangle)) => state.session.polygon_fill_preview_uv(triangle, mode),
            None => Vec::new(),
        };
        let mut texels = vec![0u8; (SIZE * SIZE) as usize * 2];
        for (texel, grey) in texels.chunks_exact_mut(2).zip(&state.part_overlay_3d.grey) {
            texel[0] = *grey;
        }
        subcraft_app::parts::raster_uv_triangles(SIZE, &tinted, &mut texels, 2, 1);
        (slot, texels)
    });
    state.part_overlay_3d.key = key;
    Some(change)
}
