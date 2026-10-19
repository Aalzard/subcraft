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
