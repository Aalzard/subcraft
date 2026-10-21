//! Writes a .subcraft crate with Old Wood and glowing orange edges (an Emissive fill masked by
//! the Edges generator), for native render-quality captures.
//! cargo run -p subcraft-app --example glow_project -- <out.subcraft> [size]
use std::sync::Arc;
use subcraft_app::{Command, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{ChannelDef, FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?;
    let size = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024u32);
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let smart = subcraft_assets::smart::smart_materials();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for (i, (ts, slot)) in sets.iter().enumerate() {
        s.execute(Command::SelectTextureSet(*ts)).map_err(|e| format!("{e:?}"))?;
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: *slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
        s.set_mesh_maps(*ts, maps);
        let name = if i == 0 { "Rough Wood" } else { "Brushed Steel" };
        s.add_smart_material(smart.iter().find(|m| m.name == name).ok_or("smart material")?).map_err(|e| format!("{e:?}"))?;
        // The default texture set has no Emissive channel: add it together with the glow layer.
        let mut with_emission = s.selected_texture_set().clone();
        with_emission.channels.push(ChannelDef::default_for(ChannelKind::Emissive));
        let mut layer = Layer::new_fill("Glowing edges", &with_emission);
        if let LayerContent::Fill(f) = &mut layer.content {
            for ch in f.channels.values_mut() { ch.enabled = false; }
            let mut glow = FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.42, 0.08)));
            glow.strength = 3.0;
            f.channels.insert(ChannelKind::Emissive, glow);
        }
        let mut mask = Mask::new(size, false);
        let mut g = Generator::new(GeneratorKind::Edges);
        g.level = 0.8;
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
        layer.mask = Some(mask);
        s.execute(Command::InsertLayerWithChannels { layer: Box::new(layer), channels: vec![ChannelKind::Emissive], label: "Glowing edges".into() }).map_err(|e| format!("{e:?}"))?;
    }
    s.update_composites();
    s.save(Some(std::path::Path::new(out)))?;
    println!("wrote {out}");
    Ok(())
}
