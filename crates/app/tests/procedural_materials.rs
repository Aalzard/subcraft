//! Procedural materials (ADR-0047) in a session: parameters edit the applied material, the
//! composite follows, undo coalesces a slider drag, and projects / saved smart materials keep
//! the parameters.
use subcraft_app::{Command, Session};
use subcraft_assets::{Library, UserLibrary};
use subcraft_bake::surface::texels;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, LayerId, MeshMapKind};
use subcraft_doc::{Layer, MaterialControls, Param, ParamValue};
use subcraft_mesh::{Bvh, primitives};

const SIZE: u32 = 128;

fn baked_crate() -> Session {
    let mesh = primitives::sample_crate();
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), SIZE);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for (ts, slot) in sets {
        s.execute(Command::SelectTextureSet(ts)).unwrap();
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size: SIZE, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ..Default::default() },
            &Progress::new(),
        )
        .unwrap();
        s.set_mesh_maps(ts, maps);
    }
    let first = s.document().texture_sets[0].id;
    s.execute(Command::SelectTextureSet(first)).unwrap();
    s
}

/// Mean of each component of a channel over the texels the selected set covers.
fn mean(s: &mut Session, channel: ChannelKind) -> [f32; 3] {
    s.update_composites();
    let ts = s.selection().texture_set;
    let slot = s.document().texture_set(ts).unwrap().material_slot;
    let img = s.composite(ts).unwrap().channel(channel).unwrap().clone();
    let covered = texels(&s.scene().mesh, slot, SIZE);
    let mut sum = [0.0f64; 3];
    for t in &covered {
        let v = img.get(t.index % SIZE, t.index / SIZE);
        assert!(v.iter().all(|c| c.is_finite()), "{channel:?} is finite");
        for c in 0..3 {
            sum[c] += v[c] as f64;
        }
    }
    sum.map(|v| (v / covered.len() as f64) as f32)
}

fn apply(s: &mut Session, name: &str) -> LayerId {
    let lib = Library::builtin();
    let material = lib.smart_materials.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is on the shelf"));
    s.add_smart_material(material).unwrap();
    s.selection().layer.unwrap()
}

fn layer(s: &Session, id: LayerId) -> &Layer {
    s.selected_texture_set().find(id).unwrap()
}

fn controls(s: &Session, id: LayerId) -> &MaterialControls {
    layer(s, id).material.as_deref().expect("the layer has material parameters")
}

fn set(s: &mut Session, layer: LayerId, param: &str, value: ParamValue) {
    s.execute(Command::SetMaterialParam { layer, param: param.into(), value }).unwrap();
}

#[test]
fn sliders_change_the_composite_of_an_applied_material() {
    let mut s = baked_crate();
    let root = apply(&mut s, "Procedural Painted Metal");
    assert_eq!(s.history().undo_label(), Some("Smart Material: Procedural Painted Metal"));
    assert!(controls(&s, root).is_default());
    let (color0, metal0) = (mean(&mut s, ChannelKind::BaseColor), mean(&mut s, ChannelKind::Metallic)[0]);
    assert!(color0[2] > color0[0] * 1.3, "blue paint by default: {color0:?}");
    assert!(metal0 > 0.02 && metal0 < 0.6, "some bare metal shows through the paint: {metal0}");

    // Paint colour: the composite turns red; the bare metal does not change.
    set(&mut s, root, "paint_color", ParamValue::Color([0.6, 0.05, 0.04]));
    let red = mean(&mut s, ChannelKind::BaseColor);
    assert!(red[0] > red[2] * 1.3, "red paint: {red:?}");
    assert!((mean(&mut s, ChannelKind::Metallic)[0] - metal0).abs() < 1e-4);

    // More wear and chips: more bare metal. None, and no dirt: a clean coat of paint.
    set(&mut s, root, "wear", ParamValue::Number(1.0));
    set(&mut s, root, "chips", ParamValue::Number(1.0));
    let worn = mean(&mut s, ChannelKind::Metallic)[0];
    assert!(worn > metal0 + 0.1, "worn {worn} vs default {metal0}");
    set(&mut s, root, "wear", ParamValue::Number(0.0));
    set(&mut s, root, "chips", ParamValue::Number(0.0));
    assert!(mean(&mut s, ChannelKind::Metallic)[0] < 1e-3, "no wear, no chips: paint everywhere");
    let dirty = mean(&mut s, ChannelKind::BaseColor);
    set(&mut s, root, "dirt", ParamValue::Number(0.0));
    let clean = mean(&mut s, ChannelKind::BaseColor);
    assert!(clean[0] > dirty[0] * 1.05, "dirt darkens: {dirty:?} vs clean {clean:?}");
    // What is left is the paint colour itself (plus the soft occlusion layer, which only darkens).
    assert!(clean[0] > 0.3 && clean[0] < 0.7 && clean[1] < 0.1, "{clean:?}");

    // Seed and scale move the pattern, not the amount of anything by much.
    s.execute(Command::ResetMaterialParams(root)).unwrap();
    assert!(controls(&s, root).is_default());
    let back = mean(&mut s, ChannelKind::BaseColor);
    assert!((0..3).all(|c| (back[c] - color0[c]).abs() < 1e-5), "reset is exactly the applied material: {back:?} vs {color0:?}");
    let before = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    set(&mut s, root, "seed", ParamValue::Number(9.0));
    let _ = mean(&mut s, ChannelKind::BaseColor);
    let after = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    let moved = (0..SIZE * SIZE).filter(|i| before.get(i % SIZE, i / SIZE) != after.get(i % SIZE, i / SIZE)).count();
    assert!(moved > (SIZE * SIZE / 20) as usize, "another seed is another pattern: {moved} texels changed");
}

#[test]
fn a_slider_drag_is_one_undo_step_and_bad_input_is_refused() {
    let mut s = baked_crate();
    let root = apply(&mut s, "Procedural Rusted Steel");
    let applied = layer(&s, root).clone();
    let steps = s.history().cursor();
    for v in [0.55, 0.6, 0.7, 0.8] {
        set(&mut s, root, "rust", ParamValue::Number(v));
    }
    assert_eq!(s.history().cursor(), steps + 1, "a drag of one slider coalesces");
    assert_eq!(s.history().undo_label(), Some("Material"));
    set(&mut s, root, "pitting", ParamValue::Number(1.0));
    assert_eq!(s.history().cursor(), steps + 2, "another parameter is another step");
    s.execute(Command::Undo).unwrap();
    assert_eq!(controls(&s, root).param("rust").unwrap().value, ParamValue::Number(0.8));
    s.execute(Command::Undo).unwrap();
    assert_eq!(layer(&s, root), &applied, "undo restores the parameters and every bound layer");
    s.execute(Command::Redo).unwrap();
    assert_eq!(controls(&s, root).param("rust").unwrap().value, ParamValue::Number(0.8));

    assert!(s.execute(Command::SetMaterialParam { layer: root, param: "nope".into(), value: ParamValue::Number(1.0) }).is_err());
    assert!(s.execute(Command::SetMaterialParam { layer: root, param: "rust".into(), value: ParamValue::Toggle(true) }).is_err());
    let child = layer(&s, root).children()[0].id;
    assert!(s.execute(Command::SetMaterialParam { layer: child, param: "rust".into(), value: ParamValue::Number(1.0) }).is_err(), "a layer without parameters");
    // Out of range is clamped to the slider's range.
    set(&mut s, root, "rust", ParamValue::Number(7.0));
    assert_eq!(controls(&s, root).param("rust").unwrap().value, ParamValue::Number(1.0));
    // Resetting an untouched material adds no undo step.
    s.execute(Command::ResetMaterialParams(root)).unwrap();
    let steps = s.history().cursor();
    s.execute(Command::ResetMaterialParams(root)).unwrap();
    assert_eq!(s.history().cursor(), steps);
}

#[test]
fn parameters_survive_save_and_load_duplicate_and_the_user_library() {
    let mut s = baked_crate();
    let root = apply(&mut s, "Procedural Wood");
    set(&mut s, root, "painted", ParamValue::Toggle(true));
    set(&mut s, root, "paint_color", ParamValue::Color([0.1, 0.3, 0.35]));
    set(&mut s, root, "ring_size", ParamValue::Number(2.0));
    let teal = mean(&mut s, ChannelKind::BaseColor);
    let dir = std::env::temp_dir().join(format!("procedural-{}", subcraft_core::ObjectId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wood.subcraft");
    s.save(Some(&path)).unwrap();

    let mut back = Session::open_project(&path).unwrap();
    let m = controls(&back, root).clone();
    assert_eq!(&m, controls(&s, root), "parameters, values and bindings are in the project file");
    assert_eq!(m.check(layer(&back, root)), vec![], "every binding still finds its layer after loading");
    assert_eq!(layer(&back, root), layer(&s, root));
    // Still editable after loading: unpainting shows the wood again.
    back.execute(Command::SetMaterialParam { layer: root, param: "painted".into(), value: ParamValue::Toggle(false) }).unwrap();
    assert!(!layer(&back, root).children().iter().find(|c| c.name == "Paint").unwrap().visible);

    // A duplicate is its own material.
    s.execute(Command::DuplicateLayer(root)).unwrap();
    let copy = s.selection().layer.unwrap();
    assert_ne!(copy, root);
    assert_eq!(controls(&s, copy).check(layer(&s, copy)), vec![]);
    set(&mut s, copy, "paint_color", ParamValue::Color([0.7, 0.6, 0.1]));
    assert_eq!(controls(&s, root).param("paint_color").unwrap().value, ParamValue::Color([0.1, 0.3, 0.35]));
    s.execute(Command::DeleteLayer(copy)).unwrap();

    // Saved to the user library as a smart material, it keeps its sliders and current values.
    let lib = UserLibrary::new(dir.join("library"));
    let saved = s.save_smart_material(root, &lib).unwrap();
    let loaded = subcraft_app::library::load_smart_material_file(saved.user_file.as_ref().unwrap()).unwrap();
    let mut other = baked_crate();
    other.add_smart_material(&loaded).unwrap();
    let applied = other.selection().layer.unwrap();
    assert_ne!(applied, root, "fresh ids");
    let m = controls(&other, applied).clone();
    assert_eq!(m.param("ring_size").unwrap().value, ParamValue::Number(2.0));
    assert_eq!(m.check(layer(&other, applied)), vec![]);
    let again = mean(&mut other, ChannelKind::BaseColor);
    assert!((0..3).all(|c| (again[c] - teal[c]).abs() < 2e-3), "the saved material looks the same: {again:?} vs {teal:?}");
    other.execute(Command::SetMaterialParam { layer: applied, param: "painted".into(), value: ParamValue::Toggle(false) }).unwrap();
    let wood = mean(&mut other, ChannelKind::BaseColor);
    assert!(wood[0] > wood[2] * 1.5, "bare wood is brown: {wood:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn controls_can_be_attached_to_and_removed_from_any_layer() {
    let mut s = baked_crate();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    let controls = MaterialControls {
        params: vec![Param::color("tint", "Tint", [0.2, 0.4, 0.6])],
        bindings: vec![subcraft_doc::Binding::layer(fill, subcraft_doc::Prop::Value { channel: ChannelKind::BaseColor }, "tint")],
    };
    s.execute(Command::SetMaterialControls { layer: fill, controls: Some(Box::new(controls.clone())) }).unwrap();
    assert_eq!(s.history().undo_label(), Some("Material Parameters"));
    set(&mut s, fill, "tint", ParamValue::Color([0.9, 0.1, 0.1]));
    let c = mean(&mut s, ChannelKind::BaseColor);
    assert!((c[0] - 0.9).abs() < 2e-3 && (c[1] - 0.1).abs() < 2e-3, "{c:?}");
    // A definition with a duplicate id is cleaned, not stored broken.
    let mut twice = controls.clone();
    twice.params.push(Param::color("tint", "Again", [0.0; 3]));
    s.execute(Command::SetMaterialControls { layer: fill, controls: Some(Box::new(twice)) }).unwrap();
    assert_eq!(layer(&s, fill).material.as_ref().unwrap().params.len(), 1);
    // A new definition is applied at once: the layer follows the values it carries.
    let c = mean(&mut s, ChannelKind::BaseColor);
    assert!((c[0] - 0.2).abs() < 2e-3 && (c[2] - 0.6).abs() < 2e-3, "{c:?}");
    s.execute(Command::SetMaterialControls { layer: fill, controls: None }).unwrap();
    assert!(layer(&s, fill).material.is_none());
    let c = mean(&mut s, ChannelKind::BaseColor);
    assert!((c[0] - 0.2).abs() < 2e-3, "removing the parameters leaves the layer as it is: {c:?}");
}

/// Every procedural material composites to sane PBR values on a baked model.
#[test]
fn every_procedural_material_composites_to_valid_values() {
    let lib = Library::builtin();
    for material in lib.smart_materials.iter().filter(|m| m.name.starts_with(subcraft_assets::procedural::PREFIX)) {
        let mut s = baked_crate();
        s.add_smart_material(material).unwrap();
        let color = mean(&mut s, ChannelKind::BaseColor);
        let rough = mean(&mut s, ChannelKind::Roughness)[0];
        let metal = mean(&mut s, ChannelKind::Metallic)[0];
        assert!(color.iter().all(|c| (0.005..=0.95).contains(c)), "{}: base colour {color:?}", material.name);
        assert!((0.05..=1.0).contains(&rough), "{}: roughness {rough}", material.name);
        assert!((0.0..=1.0).contains(&metal), "{}: metallic {metal}", material.name);
    }
}

/// Applying a cloth adds the sheen channels it fills to the texture set, in the same undo step;
/// other materials leave the set's channels alone.
#[test]
fn cloth_brings_the_sheen_channels_with_it() {
    let sheen = [ChannelKind::SheenColor, ChannelKind::SheenOpacity, ChannelKind::SheenRoughness];
    let mut s = baked_crate();
    let before = s.selected_texture_set().channels.len();
    apply(&mut s, "Procedural Rubber");
    assert_eq!(s.selected_texture_set().channels.len(), before, "no sheen for rubber");
    let canvas = apply(&mut s, "Procedural Canvas");
    assert!(sheen.iter().all(|k| s.selected_texture_set().has_channel(*k)));
    assert!(controls(&s, canvas).param("sheen").is_some());
    let v = mean(&mut s, ChannelKind::SheenOpacity)[0];
    assert!((v - 0.4).abs() < 0.02, "sheen opacity at the default: {v}");
    set(&mut s, canvas, "sheen", ParamValue::Number(0.8));
    let v = mean(&mut s, ChannelKind::SheenOpacity)[0];
    assert!((v - 0.8).abs() < 0.02, "the slider drives it: {v}");
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert!(sheen.iter().all(|k| !s.selected_texture_set().has_channel(*k)), "undo removes the channels with the layer");
}

/// A slider can drive a material filter: expose MatFx Rust Weathering's Amount on a fill layer,
/// and the composite gets rustier as the slider goes up.
#[test]
fn a_slider_drives_a_material_filter() {
    use subcraft_doc::{ContentEffectKind, FilterSetting, MaterialFilter, Prop};
    let mut s = baked_crate();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    s.execute(Command::AddContentEffect { layer: fill, kind: ContentEffectKind::Material(MaterialFilter::ALL[4]) }).unwrap();
    let offered = subcraft_doc::material::exposable(layer(&s, fill));
    let amount = offered.iter().find(|e| e.prop == Prop::Filter { setting: FilterSetting::Amount }).expect("the filter's Amount is offered");
    let mut controls = MaterialControls::default();
    let id = controls.expose(amount).unwrap();
    s.execute(Command::SetMaterialControls { layer: fill, controls: Some(Box::new(controls)) }).unwrap();

    let start = mean(&mut s, ChannelKind::Roughness)[0];
    set(&mut s, fill, &id, ParamValue::Number(0.0));
    let clean = mean(&mut s, ChannelKind::Roughness)[0];
    set(&mut s, fill, &id, ParamValue::Number(1.0));
    let rusty = mean(&mut s, ChannelKind::Roughness)[0];
    assert!(rusty > clean + 0.1, "more rust is rougher: {clean} -> {rusty}");
    let ContentEffectKind::Material(MaterialFilter::RustWeathering { amount, .. }) = layer(&s, fill).content_effects[0].kind else { panic!("the filter is still there") };
    assert_eq!(amount, 1.0);
    s.execute(Command::Undo).unwrap();
    // The two moves of one slider are one undo step, back to the filter's own Amount.
    let back = mean(&mut s, ChannelKind::Roughness)[0];
    assert!((back - start).abs() < 1e-4 && rusty > start + 0.01, "undo puts the filter back: {back} vs {start}, rusty {rusty}");
}

/// The weathered materials are built on material filters, and each of the three macros moves
/// the composite on its own: at 0 the surface is new, at 1 it is clearly changed.
#[test]
fn weathered_materials_follow_their_wear_age_and_dirt_macros() {
    use subcraft_doc::ContentEffectKind;
    fn filters(l: &Layer) -> usize {
        l.content_effects.iter().filter(|e| matches!(e.kind, ContentEffectKind::Material(_))).count() + l.children().iter().map(|c| filters(c)).sum::<usize>()
    }
    for name in ["Rusty Painted Steel", "Chipped Enamel", "Scuffed Gun Metal", "Worn Leather Strap", "Dusty Plastic", "Galvanized Steel", "Powder Coated Metal", "Hammered Copper", "Raw Steel Plate", "Weathered Painted Wood", "Worn Canvas", "Weathered Concrete", "Perished Rubber", "Chipped Painted Plastic"] {
        let mut s = baked_crate();
        let id = apply(&mut s, &format!("Procedural {name}"));
        assert!(filters(layer(&s, id)) >= 1, "{name} uses a material filter");
        assert!(controls(&s, id).check(layer(&s, id)).is_empty(), "{name}: every binding has a target");
        let sample = |s: &mut Session| {
            let (c, r) = (mean(s, ChannelKind::BaseColor), mean(s, ChannelKind::Roughness)[0]);
            [c[0], c[1], c[2], r]
        };
        let distance = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        for m in ["wear", "age", "dirt"] {
            set(&mut s, id, m, ParamValue::Number(0.0));
        }
        let clean = sample(&mut s);
        for m in ["wear", "age", "dirt"] {
            set(&mut s, id, m, ParamValue::Number(1.0));
            let on = sample(&mut s);
            assert!(distance(on, clean) > 0.01, "{name}: {m} changes the surface: {clean:?} -> {on:?}");
            set(&mut s, id, m, ParamValue::Number(0.0));
            let off = sample(&mut s);
            assert!(distance(off, clean) < 1e-4, "{name}: {m} back at 0 is the new surface again");
        }
    }
}
