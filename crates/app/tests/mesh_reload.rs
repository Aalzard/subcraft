//! File > Reload mesh: Keep UVs is bit-exact, Reproject moves paint through the surface onto a
//! different UV layout (seams included, sparse tiles), texture sets match materials by name, and
//! the result saves and reopens.
use std::path::PathBuf;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, MeshMapKind, ReloadMode, Session};
use subcraft_core::ChannelKind as K;
use subcraft_doc::{LayerContent, Mask};
use subcraft_image::{TILE_SIZE, TiledImage};
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{Mesh, import::write_obj, primitives};

const RES: u32 = 256;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-reload-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Color painted at a surface point: a smooth function of position, so any misplacement shows.
fn color(p: Vec3) -> Vec3 {
    p + Vec3::splat(0.5)
}

/// The same cube with every UV rotated by 90 degrees about the square's center.
fn rotated(mesh: &Mesh) -> Mesh {
    let mut m = mesh.clone();
    for uv in &mut m.uvs {
        *uv = Vec2::new(1.0 - uv.y, uv.x);
    }
    m
}

/// Paint the selected set's first layer: base color = `color(position)` wherever `keep(position)`,
/// and a black mask whose painted value is the red component.
fn paint(s: &mut Session, keep: impl Fn(Vec3) -> bool) {
    let mesh = s.scene().mesh.clone();
    let ts = s.selected_texture_set().clone();
    let surface = rasterize_surface(&mesh, mesh.material_triangles(ts.material_slot), RES, RES, 2.0);
    let mut layer = (*ts.layers[0]).clone();
    let LayerContent::Paint(p) = &mut layer.content else { panic!("paint layer") };
    let img = p.channel_mut(K::BaseColor, RES);
    let mut mask = Mask::new(RES, false);
    for y in 0..RES {
        for x in 0..RES {
            let i = (y * RES + x) as usize;
            if !surface.covered(i) {
                continue;
            }
            let pos = Vec3::from_slice(&surface.position[i][..3]);
            if keep(pos) {
                let c = color(pos);
                img.set(x, y, [c.x, c.y, c.z, 1.0]);
                mask.base.set(x, y, [c.x, 0.0, 0.0, 0.0]);
            }
        }
    }
    layer.mask = Some(mask);
    s.execute(Command::ReplaceLayer { layer: layer.id, replacement: Box::new(layer), label: "Paint".into() }).unwrap();
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

fn layer_images(s: &Session) -> (TiledImage, TiledImage) {
    let layer = &s.document().texture_sets[0].layers[0];
    let LayerContent::Paint(p) = &layer.content else { panic!("paint layer") };
    (p.channels[&K::BaseColor].clone(), layer.mask.as_ref().unwrap().base.clone())
}

fn cube_session() -> Session {
    Session::new(primitives::cube(1.0), RES)
}

#[test]
fn reproject_follows_the_surface_across_a_new_uv_layout_and_its_seams() {
    let mut s = cube_session();
    paint(&mut s, |_| true);
    let old_id = s.document().texture_sets[0].id;
    let new_mesh = rotated(&s.scene().mesh);
    let dir = tmp("reproject");
    let path = dir.join("cube_v2.obj");
    write_obj(&new_mesh, &path).unwrap();

    let prepared = Arc::new(s.start_mesh_reload_check(&path).wait().unwrap());
    assert_eq!(prepared.suggested, ReloadMode::Reproject, "the layout moved: {:?}", prepared.sets);
    assert_eq!(prepared.matched(), 1);
    let report = s.apply_mesh_reload(prepared, ReloadMode::Reproject).unwrap();
    assert_eq!(report.matched, ["Cube"]);
    assert_eq!(report.rasters_transferred, 2, "base color + mask base");
    assert_eq!(s.document().texture_sets[0].id, old_id, "identity is kept");
    assert!(s.is_modified());
    assert!(!s.history().can_undo(), "reload is not undoable (ADR-0042)");

    // Sample the new mesh at many surface points, including next to every edge (UV seams).
    let mesh = s.scene().mesh.clone();
    let (base, mask) = layer_images(&s);
    let bary = [[0.33, 0.33], [0.01, 0.5], [0.5, 0.01], [0.495, 0.495], [0.02, 0.02], [0.9, 0.05], [0.05, 0.9]];
    let (mut worst, mut worst_mask, mut n) = (0.0f32, 0.0f32, 0);
    for t in 0..mesh.triangle_count() {
        for [u, v] in bary {
            let sp = mesh.interpolate(t, u, v);
            let got = bilinear(&base, sp.uv);
            let want = color(sp.position);
            worst = worst.max((Vec3::new(got[0], got[1], got[2]) - want).abs().max_element());
            assert!(got[3] > 0.97, "coverage {} at triangle {t} ({u}, {v})", got[3]);
            worst_mask = worst_mask.max((bilinear(&mask, sp.uv)[0] - want.x).abs());
            n += 1;
        }
    }
    assert!(worst < 0.03, "largest color error {worst} over {n} surface points");
    assert!(worst_mask < 0.03, "largest mask error {worst_mask}");
    s.update_composites();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn reproject_writes_only_tiles_that_receive_paint() {
    let mut s = cube_session();
    // A small spot in the middle of the +X face.
    let spot = |p: Vec3| p.x > 0.49 && p.y.abs() < 0.08 && p.z.abs() < 0.08;
    paint(&mut s, spot);
    let (before, _) = layer_images(&s);
    let new_mesh = rotated(&s.scene().mesh);
    let prepared = Arc::new(s.check_mesh_reload(&PathBuf::from("cube_v2.obj"), new_mesh).unwrap());
    s.apply_mesh_reload(prepared, ReloadMode::Reproject).unwrap();
    let (after, mask) = layer_images(&s);
    assert!(after.allocated_tiles() > 0 && before.allocated_tiles() <= 4);
    // Every allocated tile shows (part of) the spot on the new layout: nothing else is written.
    let mesh = s.scene().mesh.clone();
    let surface = rasterize_surface(&mesh, mesh.material_triangles(0), RES, RES, 2.0);
    let near = |p: Vec3| p.x > 0.45 && p.y.abs() < 0.12 && p.z.abs() < 0.12;
    let tiles_x = RES / TILE_SIZE;
    for tile in (0..after.tile_count()).filter(|&t| after.is_allocated(t)) {
        let (tx, ty) = (tile as u32 % tiles_x, tile as u32 / tiles_x);
        let shows_spot = (0..TILE_SIZE * TILE_SIZE).any(|k| {
            let i = ((ty * TILE_SIZE + k / TILE_SIZE) * RES + tx * TILE_SIZE + k % TILE_SIZE) as usize;
            surface.covered(i) && near(Vec3::from_slice(&surface.position[i][..3]))
        });
        assert!(shows_spot, "tile {tile} was allocated but shows no paint");
    }
    assert!(after.allocated_tiles() <= 4, "{} tiles", after.allocated_tiles());
    assert_eq!(after.fill(), before.fill());
    assert_eq!(mask.fill(), [0.0; 4]);
    // Painted texel count is preserved (same surface area, same resolution) within rim noise.
    let painted = |img: &TiledImage| img.to_dense().chunks(4).filter(|px| px[3] > 0.5).count() as f32;
    let ratio = painted(&after) / painted(&before);
    assert!((0.8..1.25).contains(&ratio), "painted texels {} -> {}", painted(&before), painted(&after));
}

#[test]
fn keep_uvs_leaves_every_raster_bit_exact() {
    let mut s = cube_session();
    paint(&mut s, |p| p.y > 0.0);
    let before = s.document().texture_sets[0].clone();
    let mesh = s.scene().mesh.as_ref().clone();
    let prepared = Arc::new(s.check_mesh_reload(&PathBuf::from("same.obj"), mesh).unwrap());
    assert_eq!(prepared.suggested, ReloadMode::KeepUvs, "{:?}", prepared.sets);
    let report = s.apply_mesh_reload(prepared, ReloadMode::KeepUvs).unwrap();
    assert_eq!(report.rasters_transferred, 0);
    let after = &s.document().texture_sets[0];
    assert_eq!(after.id, before.id);
    assert_eq!(after.layers.len(), before.layers.len());
    for (a, b) in after.layers.iter().zip(&before.layers) {
        assert!(Arc::ptr_eq(a, b), "layers are shared untouched");
    }
    assert_eq!(*after, before);
}

#[test]
fn texture_sets_follow_material_names() {
    let mut s = Session::new(primitives::sample_crate(), RES);
    let names = |s: &Session| s.document().texture_sets.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), ["Body", "Trim"]);
    let old = s.document().texture_sets.clone();
    // The new version renames Body to Lid and lists Trim first.
    let mesh = s.scene().mesh.clone();
    let slot_of: Vec<u32> = mesh.tri_material.iter().map(|&m| if m == 0 { 1 } else { 0 }).collect();
    let (new_mesh, _) = mesh.regroup(&["Trim".to_string(), "Lid".to_string()], &slot_of, &[]);
    let prepared = Arc::new(s.check_mesh_reload(&PathBuf::from("crate_v2.obj"), new_mesh).unwrap());
    assert_eq!(prepared.added(), ["Lid"]);
    assert_eq!(prepared.unmatched_old, ["Body"]);
    let report = s.apply_mesh_reload(prepared, ReloadMode::KeepUvs).unwrap();
    assert_eq!(report.matched, ["Trim"]);
    assert_eq!(report.added, ["Lid"]);
    assert_eq!(report.kept_without_geometry, ["Body"]);
    assert_eq!(names(&s), ["Trim", "Lid", "Body"]);
    let sets = s.document().texture_sets.clone();
    assert_eq!(sets[0].id, old[1].id, "Trim keeps its identity and layers");
    assert_eq!(sets[0].layers, old[1].layers);
    assert_eq!(sets[0].material_slot, 0);
    assert_eq!(sets[1].layers.len(), 1, "new material starts with one empty paint layer");
    assert_eq!(sets[1].channels, old[0].channels);
    assert_eq!(sets[2].id, old[0].id, "unmatched set is kept");
    assert_eq!(s.scene().mesh.materials.len(), 3);
    assert!(s.scene().mesh.material_triangles(2).is_empty());
    // Compositing copes with the set that has no geometry.
    let updates = s.update_composites();
    assert!(updates.iter().any(|u| u.texture_set == sets[1].id));
}

#[test]
fn reloaded_project_saves_and_reopens_with_stale_maps_flagged() {
    let mut s = cube_session();
    paint(&mut s, |_| true);
    let ts = s.document().texture_sets[0].id;
    let mut maps = std::collections::BTreeMap::new();
    maps.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.5; 4]));
    s.set_mesh_maps(ts, maps);
    let dir = tmp("save");
    let path = dir.join("cube_v2.obj");
    write_obj(&rotated(&s.scene().mesh), &path).unwrap();
    let report = s.reload_mesh(&path, ReloadMode::Reproject).unwrap();
    assert_eq!(report.stale_maps, ["Cube"]);
    assert!(s.mesh_maps_stale(ts));
    assert!(s.mesh_maps(ts).is_some(), "maps are kept (reprojected) until rebaked");
    assert_eq!(s.mesh_source(), Some(path.as_path()));

    let project = dir.join("reloaded.subcraft");
    s.save(Some(&project)).unwrap();
    assert!(!s.is_modified());
    let reopened = Session::open_project(&project).unwrap();
    assert_eq!(reopened.document(), s.document());
    assert_eq!(reopened.scene().mesh.triangle_count(), s.scene().mesh.triangle_count());
    assert_eq!(reopened.scene().mesh.uvs, s.scene().mesh.uvs, "the new UV layout is saved");
    assert!(reopened.mesh_maps_stale(ts));
    assert_eq!(reopened.mesh_source(), Some(path.as_path()));

    // A bake clears the flag.
    let mut reopened = reopened;
    let mut maps = std::collections::BTreeMap::new();
    maps.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [1.0; 4]));
    reopened.set_mesh_maps(ts, maps);
    assert!(!reopened.mesh_maps_stale(ts));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_reload_prepared_before_an_edit_is_refused() {
    let mut s = cube_session();
    let mesh = rotated(&s.scene().mesh);
    let prepared = Arc::new(s.check_mesh_reload(&PathBuf::from("v2.obj"), mesh).unwrap());
    let job = s.start_mesh_reload(prepared, ReloadMode::Reproject);
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    let err = s.finish_mesh_reload(job.wait().unwrap()).unwrap_err();
    assert!(err.contains("changed"), "{err}");
    assert_eq!(s.document().texture_sets[0].layers.len(), 2, "nothing was replaced");
}
