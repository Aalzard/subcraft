//! Writes a .subcraft of the default model with a checker fill on every texture set, scaled by
//! Physical size (one repeat = the given centimeters on every part) or, with `tiling`, by plain
//! Tiling 8 (the checker size then follows each part's UV density). Open both to compare.
//! cargo run --release -p subcraft-app --example physical_size_project -- <out.subcraft> [cm | tiling]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, ProceduralKind, ProceduralParams, Projection};

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?.clone();
    let mode = args.get(1).cloned().unwrap_or_else(|| "10".into());
    let projection = match mode.as_str() {
        "tiling" => Projection { scale: [8.0, 8.0], ..Projection::default() },
        cm => {
            let cm: f32 = cm.parse().map_err(err)?;
            Projection { physical_size: Some([cm, cm]), ..Projection::default() }
        }
    };
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    for ts in sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        let layer = s.selection().layer.unwrap();
        s.execute(Command::Rename { layer, name: "Checker".into() }).map_err(err)?;
        let fill = FillSource::Procedural { params: ProceduralParams::new(ProceduralKind::Checker), low: ChannelValue::rgb(0.05, 0.05, 0.05), high: ChannelValue::rgb(0.8, 0.8, 0.8) };
        s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(fill) }).map_err(err)?;
        s.execute(Command::SetFillProjection { layer, projection }).map_err(err)?;
    }
    let path = s.save(Some(std::path::Path::new(&out)))?;
    println!("wrote {}", path.display());
    Ok(())
}
