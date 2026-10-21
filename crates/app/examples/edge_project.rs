//! Writes a .subcraft project of the baked default crate with charcoal + gold Edges wear, so the
//! real app can open it (`subcraft out.subcraft --screenshot x.png`).
//! cargo run -p subcraft-app --example edge_project -- <out.subcraft> [level] [spread %] [variation] [breakup] [natural|legacy] [size]
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?;
    let arg = |i: usize, d: f32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (level, spread, variation, breakup) = (arg(1, 0.065), arg(2, 12.5), arg(3, 0.85), arg(4, 0.0));
    let natural = !args.get(5).is_some_and(|s| s == "legacy");
    let size = arg(6, 1024.0) as u32;
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for &(ts, slot) in &sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        for (color, wear) in [([0.045, 0.055, 0.065], false), ([0.9, 0.62, 0.2], true)] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
                fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))) }).map_err(|e| format!("{e:?}"))?;
            if wear {
                s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
                let mut g = Generator::new(GeneratorKind::Edges);
                g.level = level; g.noise_amount = breakup;
                if natural { let e = g.edge_spread.as_mut().unwrap(); e.distance = spread; e.variation = variation; } else { g.edge_spread = None; }
                s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).map_err(|e| format!("{e:?}"))?;
            }
        }
    }
    s.update_composites();
    s.save(Some(std::path::Path::new(out)))?;
    println!("level {level} spread {spread}% variation {variation} breakup {breakup} natural {natural} size {size} -> {out}");
    Ok(())
}
