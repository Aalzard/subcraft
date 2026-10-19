//! Actual default-crate acceptance fixture, or CPU-only 4K edit profile (no owner UI input).
//! cargo run -p subcraft-app --example edge_spread -- <project.subcraft> [spread %] [variation] [level] [legacy]
//! cargo run -p subcraft-app --example edge_spread -- --profile 4096
use std::time::{Duration, Instant};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind, ProceduralKind, ProceduralParams};
use subcraft_mesh::{Bvh, primitives};

// Match the desktop app and perf_slider: Windows' system heap serializes tile allocations.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn fixture(size: u32, spread: f32, variation: f32, bake_maps: bool) -> Result<(Session, Vec<LayerId>), String> {
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh, size); let mut layers = Vec::new();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for (ts, slot) in sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
        if bake_maps {
            let scene = s.scene();
            let maps = bake(&BakeInput { low: Geometry { mesh: &scene.mesh, bvh: &bvh }, high: None, material_slot: slot },
                &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
            s.set_mesh_maps(ts, maps);
        }
        for (name, color, wear) in [("Charcoal paint", [0.045, 0.055, 0.065], false), ("Natural edge wear", [0.6, 0.38, 0.14], true)] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::Rename { layer, name: name.into() }).map_err(|e| format!("{e:?}"))?;
            for (channel, value) in [(ChannelKind::BaseColor, ChannelValue::rgb(color[0], color[1], color[2])), (ChannelKind::Roughness, ChannelValue::scalar(0.6))] {
                let source = if !wear && channel == ChannelKind::Roughness {
                    FillSource::Procedural { params: ProceduralParams { scale: 8.0, ..ProceduralParams::new(ProceduralKind::Grunge) },
                        low: ChannelValue::scalar(0.55), high: ChannelValue::scalar(0.65) }
                } else { FillSource::uniform(value) };
                s.execute(Command::SetFillChannel { layer, channel, fill: FillChannel::new(source) }).map_err(|e| format!("{e:?}"))?;
            }
            if wear {
                s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
                let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0;
                g.edge_spread.as_mut().unwrap().distance = spread; g.edge_spread.as_mut().unwrap().variation = variation;
                s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).map_err(|e| format!("{e:?}"))?;
                layers.push(layer);
            }
        }
    }
    let ts = s.document().texture_sets[0].id; s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
    s.execute(Command::SelectLayer(Some(layers[0]))).map_err(|e| format!("{e:?}"))?;
    Ok((s, layers))
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--profile") {
        let size = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4096);
        let (mut s, layers) = fixture(size, 2.5, 0.85, false)?;
        let cold = Instant::now(); s.update_composites(); println!("{size}² cold full composite: {:.1} ms", cold.elapsed().as_secs_f64() * 1000.0);
        let mut polls = Vec::new(); let start = Instant::now();
        for frame in 0.. {
            let now = Instant::now();
            if frame < 8 {
                for &layer in &layers {
                    let ts = s.document().texture_sets.iter().find(|ts| ts.find(layer).is_some()).unwrap().id;
                    s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
                    let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                    let MaskEffectKind::Generator(g) = &mut effect.kind else { unreachable!() };
                    g.edge_spread.as_mut().unwrap().distance = 1.0 + frame as f32 * 0.5;
                    s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
                }
            }
            s.update_composites_within(Some(Duration::from_millis(10)));
            polls.push(now.elapsed().as_secs_f64() * 1000.0);
            if frame >= 8 && !s.composites_pending() { break; }
            if start.elapsed() > Duration::from_secs(120) { return Err("profile convergence timeout".into()); }
            std::thread::sleep(Duration::from_millis(16).saturating_sub(now.elapsed()));
        }
        polls.sort_by(f64::total_cmp);
        println!("CPU edit/poll p50 {:.3} / p95 {:.3} / max {:.3} ms; convergence {:.1} ms; {} frames. GPU/render/upload excluded; no long-stroke claim.",
            polls[polls.len()/2], polls[polls.len()*95/100], polls[polls.len()-1], start.elapsed().as_secs_f64()*1000.0, polls.len());
    } else {
        let path = args.first().ok_or("expected output .subcraft path")?;
        let spread = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2.5);
        let variation = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.85);
        let level = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.5);
        let natural = !args.get(4).is_some_and(|s| s == "legacy");
        let (mut s, layers) = fixture(512, spread, variation, true)?;
        for layer in layers {
            let ts = s.document().texture_sets.iter().find(|ts| ts.find(layer).is_some()).unwrap().id;
            s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
            let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
            let MaskEffectKind::Generator(g) = &mut effect.kind else { unreachable!() };
            g.level = level; if !natural { g.edge_spread = None; }
            s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
        }
        s.execute(Command::SelectTextureSet(s.document().texture_sets[0].id)).map_err(|e| format!("{e:?}"))?;
        s.save(Some(std::path::Path::new(path)))?;
        println!("Saved actual crate, baked curvature, Breakup 0, Spread {spread}%, Variation {variation}, Level {level}, Natural {natural}");
    }
    Ok(())
}
