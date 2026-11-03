//! Writes one project per scratch generator preset (sphere, or the sample crate with `--crate`),
//! for scripted screenshots: `subcraft out/<name>.subcraft --screenshot out/<name>.png`.
//! Usage: `scratch_presets <out dir> [--crate]`.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, ProceduralKind, ProceduralParams, ScratchPreset};

fn run(s: &mut Session, cmd: Command) -> Result<(), String> {
    s.execute(cmd).map(|_| ()).map_err(|e| format!("{e:?}"))
}

fn main() -> Result<(), String> {
    let dir = std::env::args().nth(1).ok_or("output directory required")?;
    let use_crate = std::env::args().any(|a| a == "--crate");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for preset in ScratchPreset::ALL {
        let mesh = if use_crate { subcraft_mesh::primitives::sample_crate() } else { subcraft_mesh::primitives::uv_sphere(1.0, 96, 48) };
        let mut s = Session::new(mesh, 1024);
        let set = s.document().texture_sets[0].id;
        run(&mut s, Command::SelectTextureSet(set))?;
        run(&mut s, Command::AddLayer(NewLayer::Fill))?;
        let base = s.selection().layer.unwrap();
        run(&mut s, Command::Rename { layer: base, name: "Base".into() })?;
        run(&mut s, Command::SetFillChannel { layer: base, channel: C::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.05, 0.07, 0.1))) })?;
        run(&mut s, Command::AddLayer(NewLayer::Fill))?;
        let layer = s.selection().layer.unwrap();
        run(&mut s, Command::Rename { layer, name: preset.label().into() })?;
        let mut params = ProceduralParams::new(ProceduralKind::Scratch);
        preset.apply(&mut params);
        params.contrast = 0.9;
        params.balance = 0.2;
        let procedural = |low: ChannelValue, high: ChannelValue| FillChannel::new(FillSource::Procedural { params, low, high });
        run(&mut s, Command::SetFillChannel { layer, channel: C::BaseColor, fill: procedural(ChannelValue::rgb(0.05, 0.07, 0.1), ChannelValue::rgb(0.85, 0.85, 0.8)) })?;
        run(&mut s, Command::SetFillChannel { layer, channel: C::Height, fill: procedural(ChannelValue::scalar(0.5), ChannelValue::scalar(0.1)) })?;
        let name = preset.label().to_lowercase().replace(' ', "-");
        s.save(Some(&std::path::Path::new(&dir).join(format!("{name}.subcraft")))).map(|_| ())?;
    }
    Ok(())
}
