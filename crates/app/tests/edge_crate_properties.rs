//! Baked default crate: Edge generator properties that must hold at any texture-set resolution.
use std::collections::HashMap;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake, surface::texels};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

struct Run { wear: HashMap<u32, f32>, texels: Vec<subcraft_bake::surface::Texel> }

/// Bake + composite the first texture set with a white fill masked by Edges.
fn run(size: u32, natural: bool, level: f32) -> Run {
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut session = Session::new(mesh.clone(), size);
    let (ts, slot) = { let t = &session.document().texture_sets[0]; (t.id, t.material_slot) };
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
        &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).unwrap();
    session.set_mesh_maps(ts, maps);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer = session.selection().layer.unwrap();
    session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0; g.level = level;
    if !natural { g.edge_spread = None; }
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
    session.update_composites();
    let texels = texels(&mesh, slot, size);
    let comp = session.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap();
    let wear = texels.iter().map(|t| (t.index, (comp.get(t.index % size, t.index / size)[0] - 0.5) * 2.0)).collect();
    Run { wear, texels }
}

/// (rim fraction covered, flat-panel max, inner-band mean, top-face mean) for the crate body.
fn stats(r: &Run) -> (f32, f32, f32, f32) {
    let (mut rim, mut rim_hit, mut panel_max, mut band, mut band_sum, mut top, mut top_sum) = (0, 0, 0.0f32, 0, 0.0, 0, 0.0);
    for t in &r.texels {
        let p = t.position; let w = r.wear[&t.index];
        let side = (p.x.abs() - 0.8).abs() < 1e-4 || (p.z.abs() - 0.5).abs() < 1e-4;
        let on_top = (p.y - 0.5).abs() < 1e-4 && !(p.x.abs() > 0.65 && p.z.abs() > 0.35);
        if side && p.y.abs() < 0.08 && p.x.abs() < 0.7 { panel_max = panel_max.max(w); }
        else if on_top {
            let edge = (0.8 - p.x.abs()).min(0.5 - p.z.abs());
            top += 1; top_sum += w;
            if edge < 0.01 { rim += 1; if w > 0.9 { rim_hit += 1; } }
            if (0.0..0.04).contains(&edge) { band += 1; band_sum += w; }
        }
    }
    assert!(rim > 10 && band > 20 && top > 100, "{rim} {band} {top}");
    (rim_hit as f32 / rim as f32, panel_max, band_sum / band as f32, top_sum / top as f32)
}

#[test]
fn wear_sits_on_convex_edges_not_flat_faces_at_every_resolution() {
    for natural in [false, true] {
        let mut all = Vec::new();
        for size in [512, 1024] {
            let s = stats(&run(size, natural, 0.85));
            eprintln!("natural={natural} size={size}: rim {:.3} panel_max {:.4} band {:.3} top {:.3}", s.0, s.1, s.2, s.3);
            assert!(s.0 > 0.97, "convex rim not covered (natural={natural}, {size}): {}", s.0);
            assert!(s.1 < 0.01, "flat panel carries wear (natural={natural}, {size}): {}", s.1);
            all.push(s);
        }
        // Resolution independence: coverage statistics agree within a few percent.
        for s in &all[1..] {
            assert!((s.2 - all[0].2).abs() < 0.1, "inner band differs across resolution: {all:?}");
            assert!((s.3 - all[0].3).abs() < 0.03, "top face coverage differs across resolution: {all:?}");
        }
    }
}

#[test]
fn wear_is_continuous_across_uv_seams() {
    for natural in [false, true] {
        let r = run(512, natural, 0.85);
        // Texels from different triangles whose 3D positions nearly coincide (a seam or an
        // island edge) must carry nearly the same wear.
        let cell = 0.004f32;
        let key = |p: glam::Vec3| ((p.x / cell).round() as i32, (p.y / cell).round() as i32, (p.z / cell).round() as i32);
        let mut grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
        for (i, t) in r.texels.iter().enumerate() { grid.entry(key(t.position)).or_default().push(i); }
        let (mut pairs, mut bad, mut worst) = (0, 0, 0.0f32);
        for cellv in grid.values() {
            for (a, &i) in cellv.iter().enumerate() {
                for &j in &cellv[a + 1..] {
                    let (ta, tb) = (&r.texels[i], &r.texels[j]);
                    if (ta.position - tb.position).length() > cell * 0.6 || ta.normal.dot(tb.normal) < 0.99 { continue; }
                    // Same island neighbours differ by gradient only; seams are what we look for.
                    let d = (r.wear[&ta.index] - r.wear[&tb.index]).abs();
                    pairs += 1; worst = worst.max(d);
                    if d > 0.25 { bad += 1; }
                }
            }
        }
        eprintln!("natural={natural}: {pairs} coincident pairs, worst {worst:.3}, bad {bad}");
        assert!(pairs > 50, "no seam pairs found");
        assert!(bad as f32 <= pairs as f32 * 0.01, "wear jumps across seams: {bad}/{pairs}, worst {worst}");
    }
}
