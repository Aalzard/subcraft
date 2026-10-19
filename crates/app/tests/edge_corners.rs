//! Bake -> generator -> composite -> save/reopen acceptance for hard corners and UV gutters.
use glam::Vec3;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake, surface::texels};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

#[test]
fn edge_generator_has_no_corner_holes_and_survives_reopen() {
    let mesh = primitives::atlas_box(Vec3::splat(2.0), 1, "Corner acceptance");
    let bvh = Bvh::build(&mesh);
    let settings = BakeSettings { size: 128, maps: vec![MeshMapKind::Curvature], ..Default::default() };
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 }, &settings, &Progress::new()).unwrap();
    let samples = texels(&mesh, 0, 128);
    let radius = settings.curvature_radius * mesh.bounds.diagonal();
    let mut session = Session::new(mesh, 128);
    let ts = session.selection().texture_set;
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = session.selection().layer.unwrap();
    session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    let mut generator = Generator::new(GeneratorKind::Edges);
    generator.level = 0.93; generator.softness = 0.02; generator.noise_amount = 0.0;
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(generator) }).unwrap();
    session.set_mesh_maps(ts, maps);
    session.update_composites();
    let before = session.composite(ts).unwrap().clone();
    let albedo = before.channel(ChannelKind::BaseColor).unwrap();
    let (mut corners, mut centers) = (0, 0);
    for sample in samples {
        let p = sample.position.abs();
        let value = albedo.get(sample.index % 128, sample.index / 128)[0];
        if (Vec3::ONE - p).max_element() < radius * 0.1 {
            assert!(value > 0.99, "corner/gutter loses wear at {:?}: {value}", sample.position);
            corners += 1;
        } else if [p.x, p.y, p.z].iter().filter(|&&v| v < 0.5).count() == 2 {
            assert!((value - 0.5).abs() < 0.001, "flat face should stay untouched");
            centers += 1;
        }
    }
    assert!(corners >= 12 && centers > 100);
    let path = std::env::temp_dir().join(format!("subcraft-edge-corners-{}.subcraft", layer.0));
    session.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    reopened.update_composites();
    assert_eq!(reopened.composite(ts).unwrap(), &before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn default_crate_overlap_gap_is_filled_and_spread_edits_undo_reopen_and_converge() {
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let samples = texels(&mesh, 0, 256);
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 },
        &BakeSettings { size: 256, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).unwrap();
    let curvature = maps[&MeshMapKind::Curvature].clone();
    let mut session = Session::new(mesh, 256); let ts = session.selection().texture_set;
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer = session.selection().layer.unwrap();
    session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0; g.level = 0.85;
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
    session.set_mesh_maps(ts, maps); session.update_composites();
    let before = session.composite(ts).unwrap().clone(); let mut repaired = 0;
    for sample in samples {
        let p = sample.position;
        if p.y > 0.49999 && p.z > 0.49999 && p.x.abs() > 0.66 && p.x.abs() < 0.799 {
            let (x, y) = (sample.index % 256, sample.index / 256);
            if curvature.get(x, y)[0] < 0.8 { repaired += 1; }
            assert!(before.channel(ChannelKind::BaseColor).unwrap().get(x, y)[0] > 0.99,
                "default crate body/post join still has a hole at {p:?}");
        }
    }
    assert!(repaired > 0, "must reproduce weak baked curvature beside the actual crate posts");
    session.execute(Command::Seal).unwrap(); let doc = session.document().clone();
    let mut effect = session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
    g.edge_spread.as_mut().unwrap().distance = 5.0; effect.kind = MaskEffectKind::Generator(g);
    session.execute(Command::SetMaskEffect { layer, effect: effect.clone() }).unwrap();
    session.execute(Command::Undo).unwrap(); assert_eq!(session.document(), &doc);
    session.update_composites(); assert_eq!(session.composite(ts).unwrap(), &before);
    session.execute(Command::Redo).unwrap();
    for distance in [2.0, 6.0, 3.0] {
        g.edge_spread.as_mut().unwrap().distance = distance; effect.kind = MaskEffectKind::Generator(g);
        session.execute(Command::SetMaskEffect { layer, effect: effect.clone() }).unwrap();
        session.update_composites_within(Some(std::time::Duration::from_millis(2)));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while session.composites_pending() {
        assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(1));
        session.update_composites_within(Some(std::time::Duration::from_millis(2)));
    }
    let worker = session.composite(ts).unwrap().clone();
    let path = std::env::temp_dir().join(format!("subcraft-crate-spread-{}.subcraft", layer.0));
    session.save(Some(&path)).unwrap(); let mut reopened = Session::open_project(&path).unwrap();
    reopened.update_composites(); assert_eq!(reopened.composite(ts).unwrap(), &worker);
    std::fs::remove_file(path).unwrap();
}
