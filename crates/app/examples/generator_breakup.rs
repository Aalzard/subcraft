//! Native acceptance fixtures and cold/warm organic breakup timing, using a real baked model.
use std::{path::Path, time::Instant};
use subcraft_app::{Command, Session};
use subcraft_doc::{Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = Path::new(args.get(1).expect("source project"));
    let out = Path::new(args.get(2).expect("output folder")); std::fs::create_dir_all(out).unwrap();
    let resolution = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let original = Session::open_project(source).unwrap();
    let source_maps = original.mesh_maps(original.selection().texture_set).unwrap();
    let maps = source_maps.iter().filter(|(kind, _)| matches!(kind, subcraft_core::MeshMapKind::Normal | subcraft_core::MeshMapKind::Curvature | subcraft_core::MeshMapKind::AmbientOcclusion))
        .map(|(kind, image)| (kind, image.resampled(resolution, resolution))).collect();
    let mut s = Session::new((*original.scene().mesh).clone(), resolution);
    s.set_mesh_maps(s.selection().texture_set, maps); drop(original);
    let ids: Vec<_> = s.selected_texture_set().layers.iter().map(|l| l.id).collect();
    for id in ids { s.execute(Command::DeleteLayer(id)).unwrap(); }
    let mut base = Layer::new_fill("Steel base", s.selected_texture_set());
    if let LayerContent::Fill(f) = &mut base.content {
        f.channels.get_mut(&subcraft_core::ChannelKind::BaseColor).unwrap().source = subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::rgb(0.035, 0.05, 0.075));
        f.channels.get_mut(&subcraft_core::ChannelKind::Normal).unwrap().source = subcraft_doc::FillSource::MeshMap { map: subcraft_core::MeshMapKind::Normal };
    }
    s.execute(Command::InsertLayer { layer: Box::new(base), label: "Base".into() }).unwrap();
    let mut layer = Layer::new_fill("Edge wear", s.selected_texture_set());
    let mut g = Generator::new(GeneratorKind::Edges); g.level = 0.62; g.softness = 0.05;
    let mut mask = Mask::new(resolution, false); mask.effects.push(std::sync::Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))); layer.mask = Some(mask);
    let layer_id = layer.id;
    s.execute(Command::InsertLayer { layer: Box::new(layer), label: "Wear".into() }).unwrap();
    for (name, size, seed, organic) in [("Organic-default", 12.5, 1, true), ("Organic-large", 30., 1, true), ("Organic-seed-8", 12.5, 8, true), ("Legacy", 12.5, 1, false)] {
        let mut effect = (*s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0]).clone();
        g.set_pattern_size(size); g.noise.seed = seed; g.organic_breakup = organic; effect.kind = MaskEffectKind::Generator(g);
        s.execute(Command::SetMaskEffect { layer: layer_id, effect }).unwrap();
        let time = Instant::now(); s.update_composites(); let cold = time.elapsed();
        let time = Instant::now(); s.update_composites();
        println!("{name} {resolution}²: cold {:.1} ms; unchanged {:.2} ms", cold.as_secs_f64()*1000., time.elapsed().as_secs_f64()*1000.);
        if resolution <= 1024 { s.save(Some(&out.join(format!("{name}.subcraft")))).unwrap(); }
    }
    if resolution <= 1024 {
        s.execute(Command::ToggleFillChannel { layer: layer_id, effect: None, channel: subcraft_core::ChannelKind::BaseColor, solo: true }).unwrap();
        s.save(Some(&out.join("Channels-color-only.subcraft"))).unwrap();
    }
}
