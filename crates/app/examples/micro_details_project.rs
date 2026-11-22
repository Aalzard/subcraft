//! Writes a .subcraft demo of generator micro details on the default model, ready to open in the
//! app: a painted-metal surface with a procedural Height (raised plates, thin grooves between
//! them), then Dirt and Edge wear layers whose generators have Micro details on. Untick
//! "Micro details" in a generator's Properties to compare.
//! cargo run --release -p subcraft-app --example micro_details_project -- <out.subcraft> [size]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind, ProceduralKind, ProceduralParams, Projection, ProjectionMode};
use subcraft_mesh::Bvh;

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let bvh = Bvh::build(&mesh);
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    for &(ts, slot) in &sets {
        if size != 2048 {
            s.execute(Command::SetResolution { texture_set: Some(ts), resolution: size }).map_err(err)?;
        }
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ..Default::default() },
            &Progress::new(),
        )
        .map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        let surface = s.selection().layer.unwrap();
        s.execute(Command::Rename { layer: surface, name: "Painted metal plates (height)".into() }).map_err(err)?;
        for (channel, value) in [(ChannelKind::BaseColor, ChannelValue::rgb(0.22, 0.30, 0.36)), (ChannelKind::Roughness, ChannelValue::scalar(0.45)), (ChannelKind::Metallic, ChannelValue::scalar(0.0))] {
            s.execute(Command::SetFillChannel { layer: surface, channel, fill: FillChannel::new(FillSource::uniform(value)) }).map_err(err)?;
        }
        let mut cells = ProceduralParams::new(ProceduralKind::Cells);
        cells.scale = 9.0;
        cells.balance = 0.78;
        cells.contrast = 0.1;
        s.execute(Command::SetFillChannel {
            layer: surface,
            channel: ChannelKind::Height,
            fill: FillChannel::new(FillSource::Procedural { params: cells, low: ChannelValue::scalar(0.0), high: ChannelValue::scalar(-0.02) }),
        })
        .map_err(err)?;
        s.execute(Command::SetFillProjection { layer: surface, projection: Projection { mode: ProjectionMode::Triplanar, scale: [1.0, 1.0], triplanar_sharpness: 8.0, ..Projection::default() } }).map_err(err)?;
        for (name, color, rough, metal, kind) in [
            ("Dirt (micro details on)", [0.10, 0.07, 0.04], 0.9, 0.0, GeneratorKind::Cavity),
            ("Edge wear (micro details on)", [0.80, 0.78, 0.74], 0.3, 1.0, GeneratorKind::Edges),
        ] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::Rename { layer, name: name.into() }).map_err(err)?;
            for (channel, value) in [(ChannelKind::BaseColor, ChannelValue::rgb(color[0], color[1], color[2])), (ChannelKind::Roughness, ChannelValue::scalar(rough)), (ChannelKind::Metallic, ChannelValue::scalar(metal))] {
                s.execute(Command::SetFillChannel { layer, channel, fill: FillChannel::new(FillSource::uniform(value)) }).map_err(err)?;
            }
            s.execute(Command::AddMask { layer, white: false }).map_err(err)?;
            let mut g = Generator::new(kind);
            g.set_micro_details_enabled(true);
            s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).map_err(err)?;
        }
    }
    s.update_composites();
    let path = s.save(Some(std::path::Path::new(&out)))?;
    println!("wrote {}", path.display());
    Ok(())
}
