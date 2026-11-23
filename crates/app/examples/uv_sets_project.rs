//! Writes a .subcraft of the default model carrying two UV sets: the file's own (UV0) and a
//! generated "Auto unwrap" layout, with a checker fill so the layout shows in the viewport.
//! With `1`, every texture set is switched to the second set before saving.
//! cargo run --release -p subcraft-app --example uv_sets_project -- <out.subcraft> [0 | 1]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, ProceduralKind, ProceduralParams, Projection};
use subcraft_mesh::UvSet;
use subcraft_mesh::unwrap::{UnwrapSettings, unwrap};

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?.clone();
    let set: usize = args.get(1).map_or(Ok(0), |s| s.parse()).map_err(err)?;
    let mut mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let (unwrapped, stats) = unwrap(&mesh, &UnwrapSettings::default()).map_err(err)?;
    println!("unwrap: {}", stats.summary());
    let corners = |m: &subcraft_mesh::Mesh| m.indices.iter().map(|&i| m.uvs[i as usize]).collect::<Vec<_>>();
    assert_eq!(unwrapped.triangle_count(), mesh.triangle_count(), "the unwrap keeps the triangles");
    mesh.uv_sets = vec![UvSet { name: "UV0".into(), corners: corners(&mesh) }, UvSet { name: "Auto unwrap".into(), corners: corners(&unwrapped) }];
    let mut s = Session::new(mesh, 1024);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    for &ts in &sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        let layer = s.selection().layer.unwrap();
        s.execute(Command::Rename { layer, name: "Checker".into() }).map_err(err)?;
        let fill = FillSource::Procedural { params: ProceduralParams::new(ProceduralKind::Checker), low: ChannelValue::rgb(0.05, 0.05, 0.05), high: ChannelValue::rgb(0.8, 0.8, 0.8) };
        s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(fill) }).map_err(err)?;
        s.execute(Command::SetFillProjection { layer, projection: Projection { scale: [12.0, 12.0], ..Projection::default() } }).map_err(err)?;
    }
    if set > 0 {
        for &ts in &sets {
            println!("{}", s.set_uv_set(ts, set)?);
        }
    }
    let path = s.save(Some(std::path::Path::new(&out)))?;
    println!("wrote {}", path.display());
    Ok(())
}
