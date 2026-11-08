//! Every built-in smart material and smart mask, applied to the baked default crate.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_assets::Library;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_bake::surface::texels;
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource};
use subcraft_mesh::{Bvh, primitives};

const SIZE: u32 = 128;

fn baked_crate() -> Session {
    let mesh = primitives::sample_crate();
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), SIZE);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for (ts, slot) in sets {
        s.execute(Command::SelectTextureSet(ts)).unwrap();
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size: SIZE, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness], ..Default::default() }, &Progress::new()).unwrap();
        s.set_mesh_maps(ts, maps);
    }
    let first = s.document().texture_sets[0].id;
    s.execute(Command::SelectTextureSet(first)).unwrap();
    s
}

/// (mean, variance) of a channel's first component over the texels the Body set covers.
fn stats(s: &mut Session, channel: ChannelKind) -> (f32, f32) {
    s.update_composites();
    let ts = s.selection().texture_set;
    let slot = s.document().texture_set(ts).unwrap().material_slot;
    let img = s.composite(ts).unwrap().channel(channel).unwrap().clone();
    let values: Vec<f32> = texels(&s.scene().mesh, slot, SIZE).iter().map(|t| img.get(t.index % SIZE, t.index / SIZE)[0]).collect();
    assert!(values.iter().all(|v| v.is_finite()), "{channel:?} must be finite");
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (mean, values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32)
}

#[test]
fn library_has_the_new_smart_materials_and_masks_with_unique_ids_and_names() {
    let lib = Library::builtin();
    assert!(lib.smart_materials.len() >= 20, "{}", lib.smart_materials.len());
    assert!(lib.smart_masks.len() >= 20, "{}", lib.smart_masks.len());
    for wanted in ["Chipped Painted Metal", "Brushed Steel", "Dusty Concrete", "Oxidized Copper", "Frosted Ice", "Hammered Brass"] {
        assert!(lib.smart_materials.iter().any(|m| m.name == wanted), "missing {wanted}");
    }
    for wanted in ["Edge Dirt", "Top-Down Dust", "Scratches", "Moss in Crevices", "Water Streaks"] {
        assert!(lib.smart_masks.iter().any(|m| m.name == wanted), "missing {wanted}");
    }
    let mut ids: Vec<_> = lib.smart_materials.iter().map(|m| m.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), lib.smart_materials.len());
    let mut names: Vec<_> = lib.smart_masks.iter().map(|m| m.name.clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), lib.smart_masks.len());
}

#[test]
fn every_smart_material_gives_a_varied_in_range_look_on_the_baked_crate() {
    let lib = Library::builtin();
    let mut looks = Vec::new();
    for m in &lib.smart_materials {
        let mut s = baked_crate();
        let before = s.document().clone();
        s.add_smart_material(m).unwrap_or_else(|e| panic!("{}: {e:?}", m.name));
        let (color, color_var) = stats(&mut s, ChannelKind::BaseColor);
        let (rough, _) = stats(&mut s, ChannelKind::Roughness);
        let (metal, _) = stats(&mut s, ChannelKind::Metallic);
        assert!((0.0..=1.5).contains(&color) && (0.0..=1.0).contains(&rough) && (0.0..=1.0).contains(&metal), "{}: {color} {rough} {metal}", m.name);
        assert!(color_var > 1e-5, "{} is not flat: variance {color_var}", m.name);
        looks.push((m.name.clone(), color, rough, metal));
        s.execute(Command::Undo).unwrap();
        assert_eq!(s.document(), &before, "{} is one undo step", m.name);
    }
    // No two smart materials may be the same look.
    for (i, a) in looks.iter().enumerate() {
        for b in &looks[i + 1..] {
            let d = (a.1 - b.1).abs() + (a.2 - b.2).abs() + (a.3 - b.3).abs();
            assert!(d > 0.004, "{} and {} look alike: {a:?} {b:?}", a.0, b.0);
        }
    }
}

#[test]
fn every_smart_mask_selects_part_of_the_baked_crate_and_is_deterministic() {
    let lib = Library::builtin();
    for m in &lib.smart_masks {
        let run = || {
            let mut s = baked_crate();
            s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
            s.execute(Command::AddMask { layer, white: false }).unwrap();
            s.apply_smart_mask(m).unwrap_or_else(|e| panic!("{}: {e:?}", m.name));
            // The layer is white over default grey: wear = how far the colour moved off the base.
            let (mean, var) = stats(&mut s, ChannelKind::BaseColor);
            (mean, var)
        };
        let (mean, var) = run();
        assert_eq!((mean, var), run(), "{} is deterministic", m.name);
        let (base, _) = { let mut s = baked_crate(); stats(&mut s, ChannelKind::BaseColor) };
        let covered = (mean - base) / (1.0 - base);
        assert!(covered > 0.002, "{} selects nothing ({covered})", m.name);
        assert!(covered < 0.97, "{} selects everything ({covered})", m.name);
        assert!(var > 0.0, "{}", m.name);
    }
}

const NEW_MATERIALS: [&str; 6] = ["Rusted Painted Metal", "Oily Metal", "Painted Steel Edge Wear", "Sandblasted Aluminium", "Galvanized Steel", "Pitted Cast Iron"];
const NEW_MASKS: [&str; 15] = ["Cavity Dust", "Heavy Drips", "Scratch Wear", "Edge Chips", "Rain Streaks", "Smudges", "Burn Marks", "Corner Wear", "Moss on Top", "Random Grime", "Fine Dust Speckle", "Oil Film", "Bottom Mud", "Lichen Patches", "Brushed Wear"];

#[test]
fn the_third_content_batch_is_in_the_library() {
    let lib = Library::builtin();
    for n in NEW_MATERIALS { assert!(lib.smart_materials.iter().any(|m| m.name == n), "missing smart material {n}"); }
    for n in NEW_MASKS { assert!(lib.smart_masks.iter().any(|m| m.name == n), "missing smart mask {n}"); }
}

#[test]
fn new_smart_materials_have_believable_overall_pbr_values_on_the_baked_crate() {
    let lib = Library::builtin();
    let look = |name: &str| {
        let m = lib.smart_materials.iter().find(|m| m.name == name).unwrap();
        let mut s = baked_crate();
        s.add_smart_material(m).unwrap();
        (stats(&mut s, ChannelKind::BaseColor).0, stats(&mut s, ChannelKind::Roughness).0, stats(&mut s, ChannelKind::Metallic).0)
    };
    let alu = look("Sandblasted Aluminium");
    let oily = look("Oily Metal");
    let zinc = look("Galvanized Steel");
    assert!(alu.0 > 0.6 && alu.2 > 0.5 && (0.35..0.8).contains(&alu.1), "sandblasted aluminium is bright, metallic, semi-rough: {alu:?}");
    assert!(oily.0 < alu.0 && oily.2 > 0.5, "oily metal is darker than aluminium and still metallic: {oily:?} {alu:?}");
    assert!(zinc.2 > 0.4, "galvanized is metallic: {zinc:?}");
}

#[test]
fn new_smart_materials_and_masks_survive_a_project_save_and_load() {
    let lib = Library::builtin();
    let mut s = baked_crate();
    for n in NEW_MATERIALS {
        s.add_smart_material(lib.smart_materials.iter().find(|m| m.name == n).unwrap()).unwrap();
    }
    for n in NEW_MASKS {
        s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        let layer = s.selection().layer.unwrap();
        s.execute(Command::AddMask { layer, white: false }).unwrap();
        s.apply_smart_mask(lib.smart_masks.iter().find(|m| m.name == n).unwrap()).unwrap();
    }
    let path = std::env::temp_dir().join(format!("subcraft-smart-content-{}.subcraft", std::process::id()));
    subcraft_project::save_project(&path, &s.scene().mesh, s.document(), s.resources(), &Default::default()).unwrap();
    let loaded = subcraft_project::load_project(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(&loaded.document, s.document(), "the document with all new smart content round-trips exactly");
}

/// Projects store the layers a smart material made, never a reference to the library entry, so a
/// project made with a material that was later removed (or renamed) still opens. The image-backed
/// materials (PBR pack, Bricks, Brown Leather) keep their textures through save and load.
#[test]
fn image_backed_smart_materials_survive_save_and_load_without_the_library_entry() {
    let lib = Library::builtin();
    let mut s = baked_crate();
    for n in ["Black Basalt", "Hex Armor", "Worn Leather PBR", "Bricks", "Brown Leather"] {
        s.add_smart_material(lib.smart_materials.iter().find(|m| m.name == n).unwrap_or_else(|| panic!("missing {n}"))).unwrap();
    }
    let path = std::env::temp_dir().join(format!("subcraft-smart-pbr-{}.subcraft", std::process::id()));
    subcraft_project::save_project(&path, &s.scene().mesh, s.document(), s.resources(), &Default::default()).unwrap();
    let loaded = subcraft_project::load_project(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(&loaded.document, s.document());
    for removed in ["Muddy Leather", "Dirty Rubber", "Sandstone", "Old Wood", "Mossy Bricks", "Worn Brown Leather"] {
        assert!(!lib.smart_materials.iter().any(|m| m.name == removed), "{removed} should be gone from the library");
    }
}
