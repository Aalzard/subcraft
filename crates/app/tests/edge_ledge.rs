//! Edge wear along long straight rims of a block standing on a plate (the user-reported case of
//! wear that stops and starts along a ledge), with the generator as a user adds it.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake, surface::texels};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};
use glam::Vec3;

fn block_on_plate() -> Mesh {
    let mut builder = MeshBuilder::new("Block on plate");
    let material = builder.material("M");
    // Each box gets its own half of the UV square (the atlas layouts would overlap).
    for (name, size, offset, uv_offset) in [("Plate", Vec3::new(4.0, 0.4, 2.0), Vec3::ZERO, glam::Vec2::ZERO), ("Block", Vec3::new(2.0, 1.0, 1.0), Vec3::new(0.3, 0.7, -0.2), glam::Vec2::new(0.5, 0.0))] {
        builder.object(name);
        let source = primitives::atlas_box(size, 4, name);
        for t in 0..source.triangle_count() {
            let p = source.tri_positions(t);
            let uv = source.tri_uvs(t);
            builder.push_triangle(material, [0, 1, 2].map(|i| Corner { position: p[i] + offset, uv: Some(glam::Vec2::new(uv[i].x * 0.5, uv[i].y) + uv_offset), normal: Some(source.geometric_normal(t)) }));
        }
    }
    builder.build().unwrap()
}

/// Wear per texel (0..1) and the texels, for a generator as given.
fn run(size: u32, g: Generator) -> (Vec<f32>, Vec<subcraft_bake::surface::Texel>) {
    let mesh = block_on_plate();
    let bvh = Bvh::build(&mesh);
    let mut session = Session::new(mesh.clone(), size);
    let (ts, slot) = { let t = &session.document().texture_sets[0]; (t.id, t.material_slot) };
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
        &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ..Default::default() }, &Progress::new()).unwrap();
    session.set_mesh_maps(ts, maps);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = session.selection().layer.unwrap();
    session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
    session.update_composites();
    let texels = texels(&mesh, slot, size);
    let comp = session.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap();
    let wear = texels.iter().map(|t| ((comp.get(t.index % size, t.index / size)[0] - 0.5) * 2.0).clamp(0.0, 1.0)).collect();
    (wear, texels)
}

/// Along a rim: coverage by 1% bins of its length (the best texel within `band` of the rim per
/// bin), and the number of runs of uncovered bins.
fn rim(wear: &[f32], texels: &[subcraft_bake::surface::Texel], on: impl Fn(Vec3) -> Option<f32>) -> (f32, usize) {
    let mut bins = [0.0f32; 100];
    let mut seen = [false; 100];
    for (t, w) in texels.iter().zip(wear) {
        if let Some(u) = on(t.position) {
            let b = ((u * 100.0) as usize).min(99);
            bins[b] = bins[b].max(*w);
            seen[b] = true;
        }
    }
    let covered: Vec<bool> = (0..100).filter(|&b| seen[b]).map(|b| bins[b] > 0.5).collect();
    let gaps = covered.windows(2).filter(|w| w[0] && !w[1]).count() + usize::from(covered.first() == Some(&false));
    (covered.iter().filter(|c| **c).count() as f32 / covered.len().max(1) as f32, gaps)
}

#[test]
fn default_edge_wear_covers_long_straight_rims_without_gaps() {
    for size in [512, 1024] {
        let (wear, texels) = run(size, Generator::new(GeneratorKind::Edges));
        let band = 0.03;
        // The plate's front top rim (z = 1, y = 0.2) and the block's front top rim (z = 0.3, y = 1.2).
        let plate = rim(&wear, &texels, |p| ((p.y - 0.2).abs() < 1e-3 && (1.0 - p.z) < band && p.x.abs() < 1.9).then(|| (p.x + 1.9) / 3.8));
        let block = rim(&wear, &texels, |p| ((p.y - 1.2).abs() < 1e-3 && (0.3 - p.z) < band && (-0.6..1.2).contains(&p.x)).then(|| (p.x + 0.6) / 1.8));
        eprintln!("size {size}: plate rim {plate:?}, block rim {block:?}");
        for (name, (coverage, gaps)) in [("plate", plate), ("block", block)] {
            assert!(coverage > 0.97 && gaps == 0, "{name} rim at {size}: covered {coverage}, {gaps} gaps");
        }
    }
}
