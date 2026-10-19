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
    let mut effect = session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
    // The same baked corner must survive with Natural disabled: no geometric spread fallback.
    for natural in [false, true] {
        let mut edited = g; if !natural { edited.edge_spread = None; }
        effect.kind = MaskEffectKind::Generator(edited);
        session.execute(Command::SetMaskEffect { layer, effect: effect.clone() }).unwrap();
        session.update_composites(); let mut checked = 0;
        for sample in &samples {
            let p = sample.position;
            if [p.x.abs() - 0.8, p.y.abs() - 0.5, p.z.abs() - 0.5].into_iter().filter(|d| d.abs() < 1e-5).count() >= 2 {
                let (x, y) = (sample.index % 256, sample.index / 256);
                assert!(curvature.get(x, y)[0] > 0.97, "bake still cancels its own edge at {p:?}");
                assert!(session.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().get(x, y)[0] > 0.99,
                    "body/post join has a hole at {p:?}, natural={natural}");
                checked += 1;
            }
        }
        assert!(checked > 100);
    }
    let before = session.composite(ts).unwrap().clone();
    session.execute(Command::Seal).unwrap(); let doc = session.document().clone();
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

/// Owner's settings on the actual baked crate: wear starts only at exposed edges (never at the
/// body edges buried in the posts, nor at bands hidden behind post faces), reaches broadly at
/// Level 0.065 / Spread 12.5%, and still leaves the middle of flat panels clean.
#[test]
fn default_crate_wear_starts_at_exposed_edges_and_reaches_at_low_level() {
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let size = 256;
    let mut session = Session::new(mesh.clone(), size);
    let sets: Vec<_> = session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let mut layers = Vec::new();
    for &(ts, slot) in &sets {
        session.execute(Command::SelectTextureSet(ts)).unwrap();
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).unwrap();
        session.set_mesh_maps(ts, maps);
        session.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer = session.selection().layer.unwrap();
        session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
            fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
        session.execute(Command::AddMask { layer, white: false }).unwrap();
        let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0; g.level = 0.065;
        let spread = g.edge_spread.as_mut().unwrap(); spread.distance = 12.5; spread.variation = 0.85;
        session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
        layers.push((ts, slot, layer, g));
    }
    session.update_composites();
    let wear = |session: &Session, ts, index: u32| (session.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap()
        .get(index % size, index / size)[0] - 0.5) * 2.0;
    let (ts, slot, ..) = layers[0];
    let (mut panel, mut rim, mut broad, mut broad_sum) = (0, 0, 0, 0.0);
    for t in texels(&mesh, slot, size) {
        let p = t.position; let value = wear(&session, ts, t.index);
        let side = (p.x.abs() - 0.8).abs() < 1e-4 || (p.z.abs() - 0.5).abs() < 1e-4;
        // Body top, away from the corner posts' footprints (their x/z bands near the corners).
        let top = (p.y - 0.5).abs() < 1e-4 && !(p.x.abs() > 0.65 && p.z.abs() > 0.35);
        if side && p.y.abs() < 0.08 && p.x.abs() < 0.7 {
            // Mid-height side panels, also right beside the posts that bury the vertical edges.
            assert!(value < 0.01, "flat panel covered at {p:?}: {value}"); panel += 1;
        } else if top {
            let edge = (0.8 - p.x.abs()).min(0.5 - p.z.abs());
            if edge < 0.04 { assert!(value > 0.99, "exposed rim lost at {p:?}: {value}"); rim += 1; }
            if (0.1..0.2).contains(&edge) { broad_sum += value; broad += 1; }
        }
    }
    assert!(panel > 100 && rim > 50 && broad > 100, "{panel} {rim} {broad}");
    assert!(broad_sum / broad as f32 > 0.4, "Spread 12.5% must reach 0.1–0.2 units inward: {}", broad_sum / broad as f32);
    // Natural off (legacy, baked detail only) at a typical level: no detached dashes on posts.
    let (ts, slot, layer, mut g) = layers[1];
    session.execute(Command::SelectTextureSet(ts)).unwrap();
    g.edge_spread = None; g.level = 0.85;
    let mut effect = session.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
    effect.kind = MaskEffectKind::Generator(g);
    session.execute(Command::SetMaskEffect { layer, effect }).unwrap();
    session.update_composites();
    let mut post = 0;
    for t in texels(&mesh, slot, size) {
        let p = t.position;
        let (dx, dz) = ((p.x.abs() - 0.8).abs(), (p.z.abs() - 0.5).abs());
        let lateral = if (p.x.abs() - 0.87).abs() < 1e-4 { dz } else if (p.z.abs() - 0.57).abs() < 1e-4 { dx } else { continue };
        if lateral < 0.05 && p.y.abs() < 0.5 {
            let value = wear(&session, ts, t.index);
            assert!(value < 0.01, "detached dash on a post face at {p:?}: {value}"); post += 1;
        }
    }
    assert!(post > 100);
}
