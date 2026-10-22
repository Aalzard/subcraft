//! Reproducible native-app fixture: cargo run -p subcraft-app --example edge_corners -- <project.subcraft>
use glam::Vec3;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

fn main() -> Result<(), String> {
    let path = std::env::args_os().nth(1).ok_or("expected output .subcraft path")?;
    let mesh = primitives::atlas_box(Vec3::splat(2.0), 1, "Edge corner acceptance");
    let bvh = Bvh::build(&mesh);
    let settings = BakeSettings { size: 512, maps: vec![MeshMapKind::Curvature], ..Default::default() };
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 }, &settings, &Progress::new()).map_err(|e| e.to_string())?;
    let mut s = Session::new(mesh, 512);
    let ts = s.selection().texture_set;
    for (name, color, masked) in [("Dark base", [0.035, 0.045, 0.06], false), ("Connected edge wear", [0.65, 0.42, 0.16], true)] {
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
        let layer = s.selection().layer.unwrap();
        s.execute(Command::Rename { layer, name: name.into() }).map_err(|e| format!("{e:?}"))?;
        for (channel, value) in [(ChannelKind::BaseColor, ChannelValue::rgb(color[0], color[1], color[2])),
            (ChannelKind::Roughness, ChannelValue::scalar(0.55))] {
            s.execute(Command::SetFillChannel { layer, channel, fill: FillChannel::new(FillSource::uniform(value)) }).map_err(|e| format!("{e:?}"))?;
        }
        if masked {
            s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
            let mut generator = Generator::new(GeneratorKind::Edges);
            generator.level = 0.8; generator.softness = 0.08; generator.noise_amount = 0.0;
            s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(generator) }).map_err(|e| format!("{e:?}"))?;
        }
    }
    s.set_mesh_maps(ts, maps);
    s.save(Some(std::path::Path::new(&path)))?;
    Ok(())
}
