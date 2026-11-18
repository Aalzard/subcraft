//! Issue #581: Painter's UDIM workflow. With "Paint across UDIM tiles" on, a 3D stroke crosses
//! tile borders, and materials, smart materials and new layers go to every tile of the group.
//! Off (the default, and every existing project) each tile paints on its own.
use glam::{Vec2, Vec3};
use subcraft_app::new_project::NewProjectSettings;
use subcraft_app::{Camera, Command, NewLayer, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::{Corner, Mesh, MeshBuilder};
use subcraft_paint::BrushMaterial;

/// Two quads sharing the edge x = 1 (one surface), the left on tile 1001, the right on 1002.
fn mesh() -> Mesh {
    let mut b = MeshBuilder::new("Strip");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

fn session() -> Session {
    let mut s = Session::from_mesh_with(mesh(), &NewProjectSettings { resolution: 256, ..Default::default() }).unwrap();
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 10.0;
    s.brush.settings.hardness = 1.0;
    s
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(1.0, 3.0, 0.5), Vec3::new(1.0, 0.0, 0.5), Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}

/// A stroke on the selected set from x = 0.3 (tile 1001) to x = 1.7 (tile 1002).
fn stroke_across(s: &mut Session) {
    let cam = camera();
    s.begin_stroke().unwrap();
    for i in 0..=14 {
        let x = 0.3 + 0.1 * i as f32;
        let p = cam.project(Vec3::new(x, 0.0, 0.5)).unwrap().truncate();
        s.stroke_to(&cam, p, false).unwrap();
    }
    s.end_stroke();
}

fn painted_texels(s: &Session, set: usize) -> usize {
    let ts = &s.document().texture_sets[set];
    let LayerContent::Paint(p) = &ts.layers.last().unwrap().content else { panic!("paint layer on top") };
    p.channels.get(&K::BaseColor).map_or(0, |img| img.to_dense().chunks(4).filter(|px| px[3] > 0.5).count())
}

fn names(s: &Session, set: usize) -> Vec<String> {
    s.document().texture_sets[set].layers.iter().map(|l| l.name.clone()).collect()
}

fn turn_on(s: &mut Session) {
    let first = s.document().texture_sets[0].id;
    s.execute(Command::SetPaintAcrossUdims { texture_set: first, on: true }).unwrap();
}

#[test]
fn by_default_a_stroke_stays_on_its_tile() {
    let mut s = session();
    assert!(s.document().texture_sets.iter().all(|t| !t.paint_across_udims));
    assert!(s.udim_paint_partners().is_empty());
    stroke_across(&mut s);
    assert!(painted_texels(&s, 0) > 100);
    assert_eq!(painted_texels(&s, 1), 0, "unchanged default: the other tile is not painted");
}

#[test]
fn the_option_covers_the_whole_group_in_one_undo_step() {
    let mut s = session();
    turn_on(&mut s);
    assert!(s.document().texture_sets.iter().all(|t| t.paint_across_udims), "set on every tile of the group");
    assert_eq!(s.udim_paint_partners(), vec![s.document().texture_sets[1].id]);
    assert_eq!(s.history().undo_label(), Some("Paint Across UDIM Tiles"));
    s.execute(Command::Undo).unwrap();
    assert!(s.document().texture_sets.iter().all(|t| !t.paint_across_udims));
}

#[test]
fn ordinary_sets_cannot_take_the_option() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let id = s.selected_texture_set().id;
    assert!(s.execute(Command::SetPaintAcrossUdims { texture_set: id, on: true }).is_err());
    assert!(!s.selected_texture_set().paint_across_udims);
}

#[test]
fn a_stroke_crosses_the_tile_border_as_one_undo_step() {
    let mut s = session();
    turn_on(&mut s);
    let before = s.history().entries().len();
    stroke_across(&mut s);
    let (a, b) = (painted_texels(&s, 0), painted_texels(&s, 1));
    assert!(a > 100 && b > 100, "both tiles painted: {a} / {b}");
    // Both halves of the stroke are about as long: neither tile got far more than the other.
    assert!(a.max(b) < 3 * a.min(b), "{a} / {b}");
    assert_eq!(s.history().entries().len(), before + 1, "one history entry");
    s.execute(Command::Undo).unwrap();
    assert_eq!(painted_texels(&s, 0) + painted_texels(&s, 1), 0, "undo removes the paint on both tiles");
}

#[test]
fn a_stroke_from_the_other_tile_paints_the_first_too() {
    let mut s = session();
    turn_on(&mut s);
    let second = s.document().texture_sets[1].id;
    s.execute(Command::SelectTextureSet(second)).unwrap();
    stroke_across(&mut s);
    assert!(painted_texels(&s, 0) > 100 && painted_texels(&s, 1) > 100);
    assert_eq!(s.selection().texture_set, second, "the selection stays where it was");
}

#[test]
fn new_layers_renames_and_deletions_happen_on_every_tile() {
    let mut s = session();
    turn_on(&mut s);
    let before = s.history().entries().len();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert_eq!(s.history().entries().len(), before + 1, "one undo step");
    assert_eq!(names(&s, 0), names(&s, 1), "the stacks stay alike");
    assert_eq!(s.selection().texture_set, s.document().texture_sets[0].id);
    let fill = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer: fill, name: "Rust".into() }).unwrap();
    assert_eq!(names(&s, 0).last().unwrap(), "Rust");
    assert_eq!(names(&s, 1).last().unwrap(), "Rust");
    s.execute(Command::DeleteLayer(fill)).unwrap();
    assert_eq!(names(&s, 0), names(&s, 1));
    assert!(!names(&s, 1).contains(&"Rust".to_string()));
    s.execute(Command::Undo).unwrap();
    assert_eq!(names(&s, 1).last().unwrap(), "Rust", "one undo brings it back on every tile");
}

#[test]
fn a_new_paint_layer_then_a_stroke_paints_both_tiles() {
    let mut s = session();
    turn_on(&mut s);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    assert_eq!(names(&s, 0), names(&s, 1));
    stroke_across(&mut s);
    assert!(painted_texels(&s, 0) > 100 && painted_texels(&s, 1) > 100);
    // The first paint layer (under the new one) stays empty on both tiles.
    for set in 0..2 {
        let LayerContent::Paint(p) = &s.document().texture_sets[set].layers[0].content else { panic!() };
        assert!(p.channels.get(&K::BaseColor).is_none_or(|img| img.to_dense().chunks(4).all(|px| px[3] <= 0.5)));
    }
}

#[test]
fn materials_and_smart_materials_go_to_every_tile() {
    let mut s = session();
    let library = subcraft_assets::presets::materials();
    // Off: only the selected tile.
    s.add_material(&library[0]).unwrap();
    assert_eq!(s.document().texture_sets[1].layers.len(), 1);
    s.execute(Command::Undo).unwrap();

    turn_on(&mut s);
    let before = s.history().entries().len();
    s.add_material(&library[0]).unwrap();
    assert_eq!(names(&s, 0), names(&s, 1));
    assert_eq!(names(&s, 1).last().unwrap(), &library[0].name);
    assert_eq!(s.history().entries().len(), before + 1, "one undo step");
    assert_eq!(s.history().undo_label().unwrap(), format!("Material: {}", library[0].name));

    let smart = subcraft_assets::Library::builtin().smart_materials[0].clone();
    s.add_smart_material(&smart).unwrap();
    assert_eq!(names(&s, 0), names(&s, 1));
    assert_eq!(names(&s, 1).last().unwrap(), &smart.name);
    s.execute(Command::Undo).unwrap();
    assert_eq!(names(&s, 1).last().unwrap(), &library[0].name);
}

/// Issue #581 follow-up: with the option off a material goes onto one tile only, so the stacks
/// differ. Turning the option back on must still pair the layers both tiles have.
#[test]
fn strokes_still_cross_after_a_material_went_onto_one_tile_only() {
    let mut s = session();
    let library = subcraft_assets::presets::materials();
    turn_on(&mut s);
    let first = s.document().texture_sets[0].id;
    s.execute(Command::SetPaintAcrossUdims { texture_set: first, on: false }).unwrap();
    s.add_material(&library[0]).unwrap();
    assert_eq!((names(&s, 0).len(), names(&s, 1).len()), (2, 1), "the material is on the selected tile only");
    turn_on(&mut s);

    // A new paint layer goes onto both tiles (above the material on one, straight on the base
    // layer on the other), and a stroke on it crosses the border.
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    assert_eq!(names(&s, 0).last(), names(&s, 1).last());
    assert_eq!((names(&s, 0).len(), names(&s, 1).len()), (3, 2));
    stroke_across(&mut s);
    let (a, b) = (painted_texels(&s, 0), painted_texels(&s, 1));
    assert!(a > 100 && b > 100, "both tiles painted on the new layer: {a} / {b}");

    // The layer under the material, which both tiles always had, pairs up too.
    let base = s.document().texture_sets[0].layers[0].id;
    let other = &s.document().texture_sets[1];
    assert_eq!(other.corresponding_layer(&s.document().texture_sets[0], base), Some(other.layers[0].id));
    // The material has no counterpart on the other tile: a stroke on it stays on its tile.
    let material = s.document().texture_sets[0].layers[1].id;
    assert_eq!(other.corresponding_layer(&s.document().texture_sets[0], material), None);
    // Renaming the shared layer still reaches both tiles.
    let top = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer: top, name: "Details".into() }).unwrap();
    assert_eq!(names(&s, 0).last().unwrap(), "Details");
    assert_eq!(names(&s, 1).last().unwrap(), "Details");
}

#[test]
fn layers_with_the_same_name_pair_up_by_their_rank() {
    use subcraft_doc::{Layer, TextureSet};
    let stack = |names: &[&str]| {
        let mut ts = TextureSet::new("t", 0, 64);
        for n in names {
            let l = Layer::new_paint(*n, &ts);
            ts.insert_above(None, l);
        }
        ts
    };
    let a = stack(&["Paint", "Extra", "Paint", "Paint"]);
    let b = stack(&["Paint", "Paint", "Paint"]);
    for (ia, ib) in [(0, 0), (2, 1), (3, 2)] {
        assert_eq!(b.corresponding_layer(&a, a.layers[ia].id), Some(b.layers[ib].id), "a[{ia}] pairs with b[{ib}]");
    }
    assert_eq!(b.corresponding_layer(&a, a.layers[1].id), None, "no layer of that name on the other tile");
    // Identical stacks pair up position for position, as before.
    let c = stack(&["Paint", "Extra", "Paint", "Paint"]);
    for i in 0..4 {
        assert_eq!(c.corresponding_layer(&a, a.layers[i].id), Some(c.layers[i].id));
    }
}

#[test]
fn the_option_is_saved_with_the_project() {
    let dir = std::env::temp_dir().join(format!("subcraft-udim-across-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiles.subcraft");
    let mut s = session();
    turn_on(&mut s);
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    assert!(reopened.document().texture_sets.iter().all(|t| t.paint_across_udims));
    assert_eq!(reopened.document(), s.document());
    std::fs::remove_dir_all(&dir).unwrap();
}
