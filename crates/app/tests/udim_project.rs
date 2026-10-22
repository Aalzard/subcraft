//! UDIM end to end: one texture set per tile, painting stays inside its tile, export layout in both
//! folder modes, save/reopen, and importing a multi-tile model.
use glam::{Vec2, Vec3};
use subcraft_app::new_project::NewProjectSettings;
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_export::ExportSettings;
use subcraft_mesh::{Corner, Mesh, MeshBuilder};
use subcraft_paint::BrushMaterial;

/// Three quads side by side along x (0..1, 2..3, 4..5), one material, UVs on tiles 1001, 1002 and 1011.
fn mesh() -> Mesh {
    let mut b = MeshBuilder::new("Tiled");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0)].into_iter().enumerate() {
        let origin = Vec3::new(2.0 * i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

fn session() -> Session {
    let mut s = Session::from_mesh_with(mesh(), &NewProjectSettings { resolution: 256, ..Default::default() }).unwrap();
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 30.0;
    s.brush.settings.hardness = 1.0;
    s
}

fn painted_texels(s: &Session, set: usize) -> usize {
    let ts = &s.document().texture_sets[set];
    let LayerContent::Paint(p) = &ts.layers[0].content else { panic!("paint layer") };
    p.channels.get(&K::BaseColor).map_or(0, |img| img.to_dense().chunks(4).filter(|px| px[3] > 0.5).count())
}

#[test]
fn each_tile_is_a_texture_set_with_its_own_layer_stack() {
    let s = session();
    let sets = &s.document().texture_sets;
    assert_eq!(sets.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Body.1001", "Body.1002", "Body.1011"]);
    assert_eq!(s.udim_tile_count(), 3);
    assert!(sets.iter().all(|t| t.layers.len() == 1 && t.resolution == 256));
    let ids: std::collections::BTreeSet<_> = sets.iter().map(|t| t.layers[0].id).collect();
    assert_eq!(ids.len(), 3, "layer stacks are independent");
}

#[test]
fn painting_on_one_tile_does_not_touch_the_others() {
    let mut s = session();
    let second = s.document().texture_sets[1].id;
    s.execute(Command::SelectTextureSet(second)).unwrap();
    let cam = Camera::perspective(Vec3::new(2.5, 3.0, 0.5), Vec3::new(2.5, 0.0, 0.5), Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    s.begin_stroke().unwrap();
    for x in [2.3, 2.5, 2.7] {
        let p = cam.project(Vec3::new(x, 0.0, 0.5)).unwrap().truncate();
        s.stroke_to(&cam, p, false).unwrap();
    }
    s.end_stroke();
    assert!(painted_texels(&s, 1) > 100, "the stroke landed on tile 1002");
    assert_eq!(painted_texels(&s, 0), 0);
    assert_eq!(painted_texels(&s, 2), 0);
    // ... and it stays on the tile's own 0..1 square (the stored mesh holds every tile in 0..1)
    let uvs = &s.scene().mesh.uvs;
    assert!(uvs.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
}

fn names_in(dir: &std::path::Path) -> Vec<String> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { walk(base, &p, out) } else { out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/")) }
        }
    }
    let mut v = Vec::new();
    walk(dir, dir, &mut v);
    v.sort();
    v
}

#[test]
fn export_layout_follows_the_separate_folder_checkbox() {
    let mut s = session();
    let settings = ExportSettings::default();
    s.edit_export_selection(&settings, |sel, all| {
        sel.select_none(all);
        for (set, _) in all.iter() {
            sel.set_on(set, "BaseColor", true);
        }
    });
    let root = std::env::temp_dir().join(format!("subcraft-udim-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    s.export_textures(&root.join("flat"), &settings).unwrap();
    assert_eq!(names_in(&root.join("flat")), ["Tiled_Body_BaseColor.1001.png", "Tiled_Body_BaseColor.1002.png", "Tiled_Body_BaseColor.1011.png"]);
    let mut project = s.settings().clone();
    project.udim_folders = true;
    s.set_settings(project);
    s.export_textures(&root.join("folders"), &settings).unwrap();
    assert_eq!(names_in(&root.join("folders")), ["1001/Tiled_Body_BaseColor.png", "1002/Tiled_Body_BaseColor.png", "1011/Tiled_Body_BaseColor.png"]);
    assert_eq!(s.export_plan(&settings).len(), 3);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn save_and_reopen_keep_tiles_paint_and_the_folder_choice() {
    let mut s = session();
    let second = s.document().texture_sets[1].id;
    s.execute(Command::SelectTextureSet(second)).unwrap();
    let cam = Camera::perspective(Vec3::new(2.5, 3.0, 0.5), Vec3::new(2.5, 0.0, 0.5), Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, cam.project(Vec3::new(2.5, 0.0, 0.5)).unwrap().truncate(), false).unwrap();
    s.end_stroke();
    let mut project = s.settings().clone();
    project.udim_folders = true;
    s.set_settings(project);
    let path = std::env::temp_dir().join(format!("subcraft-udim-reopen-{}.subcraft", std::process::id()));
    s.save(Some(&path)).unwrap();
    let r = Session::open_project(&path).unwrap();
    assert_eq!(r.document(), s.document());
    assert!(r.settings().udim_folders);
    assert_eq!(r.udim_tile_count(), 3);
    assert!(painted_texels(&r, 1) > 0 && painted_texels(&r, 0) == 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn opening_a_multi_tile_model_warns_and_splits_it_into_tile_sets() {
    let dir = std::env::temp_dir().join(format!("subcraft-udim-obj-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let m = mesh();
    let mut obj = String::new();
    for t in 0..m.triangle_count() {
        let (p, uv) = (m.tri_positions(t), m.tri_uvs(t));
        for k in 0..3 {
            obj += &format!("v {} {} {}\nvt {} {}\n", p[k].x, p[k].y, p[k].z, uv[k].x, uv[k].y);
        }
        obj += &format!("f {0}/{0} {1}/{1} {2}/{2}\n", t * 3 + 1, t * 3 + 2, t * 3 + 3);
    }
    let path = dir.join("tiled.obj");
    std::fs::write(&path, obj).unwrap();
    let (s, warnings) = Session::open_mesh(&path, 256).unwrap();
    assert_eq!(s.document().texture_sets.len(), 3, "{:?} {warnings:?}", s.document().texture_sets.iter().map(|t| &t.name).collect::<Vec<_>>());
    assert!(warnings.iter().any(|w| w.contains("3 UDIM tiles")), "{warnings:?}");
    std::fs::remove_dir_all(dir).unwrap();
}
