//! Edit UV islands: moving, turning and scaling islands through the session keeps painted layers
//! on the surface, and changes nothing outside the selection.
use std::collections::BTreeSet;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind as K;
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_mesh::uv_edit::{UvPivot, UvTransform};
use subcraft_mesh::uv_raster::rasterize_surface;

const RES: u32 = 256;

fn color(p: Vec3) -> Vec3 {
    p + Vec3::splat(0.5)
}

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (fx, fy) = (uv.x * w - 0.5, uv.y * h - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |x: f32, y: f32| img.get(x.clamp(0.0, w - 1.0) as u32, y.clamp(0.0, h - 1.0) as u32);
    let mut out = [0.0; 4];
    for (x, y, wgt) in [(x0, y0, (1.0 - tx) * (1.0 - ty)), (x0 + 1.0, y0, tx * (1.0 - ty)), (x0, y0 + 1.0, (1.0 - tx) * ty), (x0 + 1.0, y0 + 1.0, tx * ty)] {
        let v = at(x, y);
        for c in 0..4 {
            out[c] += v[c] * wgt;
        }
    }
    out
}

/// A cube whose base colour is its surface position.
fn painted_cube() -> Session {
    let mut s = Session::new(primitives::cube(1.0), RES);
    let mesh = s.scene().mesh.clone();
    let ts = s.selected_texture_set().clone();
    let surface = rasterize_surface(&mesh, mesh.material_triangles(ts.material_slot), RES, RES, 2.0);
    let mut layer = (*ts.layers[0]).clone();
    let LayerContent::Paint(p) = &mut layer.content else { panic!("paint layer") };
    let img = p.channel_mut(K::BaseColor, RES);
    for y in 0..RES {
        for x in 0..RES {
            let i = (y * RES + x) as usize;
            if surface.covered(i) {
                let c = color(Vec3::from_slice(&surface.position[i][..3]));
                img.set(x, y, [c.x, c.y, c.z, 1.0]);
            }
        }
    }
    s.execute(Command::ReplaceLayer { layer: layer.id, replacement: Box::new(layer), label: "Paint".into() }).unwrap();
    s
}

/// Largest base colour error over sample points of every triangle (paint must follow the surface).
fn worst_error(s: &Session) -> f32 {
    let mesh = s.scene().mesh.clone();
    let LayerContent::Paint(p) = &s.document().texture_sets[0].layers[0].content else { panic!("paint layer") };
    let base = &p.channels[&K::BaseColor];
    let mut worst = 0.0f32;
    for t in 0..mesh.triangle_count() {
        for [u, v] in [[0.33, 0.33], [0.15, 0.6], [0.6, 0.15]] {
            let sp = mesh.interpolate(t, u, v);
            let got = bilinear(base, sp.uv);
            assert!(got[3] > 0.95, "coverage {} at triangle {t}", got[3]);
            worst = worst.max((Vec3::new(got[0], got[1], got[2]) - color(sp.position)).abs().max_element());
        }
    }
    worst
}

#[test]
fn turning_and_shrinking_an_island_moves_the_paint_with_it() {
    let mut s = painted_cube();
    assert!(worst_error(&s) < 0.05, "the test paint itself must match");
    let islands = s.uv_islands();
    let all = s.uv_islands_of_texture_set(&islands);
    assert!(all.len() >= 2, "the cube has several islands: {all:?}");
    let first = *all.iter().next().unwrap();
    let before = s.scene().mesh.clone();
    let id = s.document().texture_sets[0].id;
    let t = UvTransform { rotation_deg: 90.0, scale: Vec2::splat(0.7), pivot: UvPivot::EachIsland, ..Default::default() };
    // The preview shows exactly the selected island's triangles, moved.
    let shown = s.uv_edit_triangles(&islands, &BTreeSet::from([first]), Some(&t));
    assert_eq!(shown.len(), islands.iter().filter(|&&i| i == first).count());
    let summary = s.edit_uv_islands(BTreeSet::from([first]), t).unwrap();
    assert!(summary.contains("Edited 1 UV island") && summary.contains("reprojected"), "{summary}");
    assert_eq!(s.document().texture_sets[0].id, id, "identity is kept");
    let after = s.scene().mesh.clone();
    assert_eq!(after.triangle_count(), before.triangle_count());
    let mut moved = 0;
    for tri in 0..before.triangle_count() {
        let changed = before.tri_uvs(tri).iter().zip(after.tri_uvs(tri)).any(|(a, b)| (*a - b).length() > 1e-4);
        assert_eq!(changed, islands[tri] == first, "triangle {tri}: only the selected island changes");
        moved += changed as usize;
        // The preview is where the triangle went.
        if changed {
            assert!(shown.iter().any(|p| p.iter().zip(after.tri_uvs(tri)).all(|(a, b)| (*a - b).length() < 1e-4)), "triangle {tri} is not where the preview showed it");
        }
    }
    assert!(moved > 0);
    let worst = worst_error(&s);
    assert!(worst < 0.05, "largest color error {worst} after the edit");
    s.update_composites();
}

#[test]
fn every_island_can_move_together_and_bad_edits_are_refused() {
    let mut s = painted_cube();
    let islands = s.uv_islands();
    let all = s.uv_islands_of_texture_set(&islands);
    assert_eq!(s.uv_islands_in_box(&islands, Vec2::splat(-1.0), Vec2::splat(2.0)), all, "a box over the whole tile picks every island");
    assert!(s.start_uv_edit(all.clone(), UvTransform::default()).is_err(), "an identity edit is refused");
    assert!(s.start_uv_edit(BTreeSet::from([9999]), UvTransform { offset: Vec2::new(0.1, 0.0), ..Default::default() }).is_err(), "unknown islands are refused");
    // Shrink the whole layout about its centre: every island moves, the paint follows.
    let summary = s.edit_uv_islands(all.clone(), UvTransform { scale: Vec2::splat(0.8), rotation_deg: 180.0, ..Default::default() }).unwrap();
    assert!(summary.contains(&format!("Edited {} UV island", all.len())), "{summary}");
    let worst = worst_error(&s);
    assert!(worst < 0.05, "largest color error {worst} after the edit");
}

/// Two quads on UDIM tiles 1001 and 1002 of one material, "paint across UDIM tiles" on.
fn udim_strip() -> Session {
    let mut b = subcraft_mesh::MeshBuilder::new("Strip");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| subcraft_mesh::Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    let settings = subcraft_app::new_project::NewProjectSettings { resolution: 128, ..Default::default() };
    let mut s = Session::from_mesh_with(b.build().unwrap(), &settings).unwrap();
    let first = s.document().texture_sets[0].id;
    s.execute(Command::SetPaintAcrossUdims { texture_set: first, on: true }).unwrap();
    s
}

/// QA fuzz (UDIM walk): editing an island of a UDIM project dropped every set's UDIM tile while
/// "paint across UDIM tiles" stayed on, so the tiles stopped moving together and the option was
/// lost on save + open.
#[test]
fn editing_an_island_of_a_udim_project_keeps_the_tiles_and_their_grouping() {
    let mut s = udim_strip();
    let tiles = |s: &Session| s.document().texture_sets.iter().map(|t| (t.name.clone(), t.udim_tile, t.paint_across_udims)).collect::<Vec<_>>();
    let before = tiles(&s);
    assert!(before.iter().all(|t| t.1.is_some() && t.2), "{before:?}");
    let islands = s.uv_islands();
    let mine = s.uv_islands_of_texture_set(&islands);
    let one: BTreeSet<u32> = mine.iter().take(1).copied().collect();
    s.edit_uv_islands(one, UvTransform { rotation_deg: 90.0, ..Default::default() }).unwrap();
    let after = tiles(&s);
    assert_eq!(after, before, "the island stayed on its tile: same sets, tiles and grouping");
    let partners = s.udim_paint_partners();
    assert_eq!(partners.len(), 1, "the other tile still paints together with the selected one");
    let dir = std::env::temp_dir().join(format!("subcraft-uv-edit-udim-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("udim.subcraft");
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document(), "save + open keeps the document");
    let _ = std::fs::remove_dir_all(&dir);
}
