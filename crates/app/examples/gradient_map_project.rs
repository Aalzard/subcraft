//! Writes a small project whose top layer has a Gradient Map effect (Heat preset), for the
//! scripted Properties-panel screenshot: `subcraft out.subcraft --effect-panel --screenshot x.png`.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, Filter, Gradient, ProceduralKind, ProceduralParams};

fn run(s: &mut Session, cmd: Command) -> Result<(), String> {
    s.execute(cmd).map(|_| ()).map_err(|e| format!("{e:?}"))
}

fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).ok_or("output project path required")?;
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), 512);
    let set = s.document().texture_sets[0].id;
    run(&mut s, Command::SelectTextureSet(set))?;
    run(&mut s, Command::AddLayer(NewLayer::Fill))?;
    let layer = s.selection().layer.unwrap();
    run(&mut s, Command::Rename { layer, name: "Gradient Map".into() })?;
    run(&mut s, Command::SetFillChannel {
        layer,
        channel: C::BaseColor,
        fill: FillChannel::new(FillSource::Procedural {
            params: ProceduralParams { scale: 10.0, ..ProceduralParams::new(ProceduralKind::Clouds) },
            low: ChannelValue::rgb(0.0, 0.0, 0.0),
            high: ChannelValue::rgb(1.0, 1.0, 1.0),
        }),
    })?;
    run(&mut s, Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Filter {
            channel: C::BaseColor,
            filter: Filter::GradientMap { gradient: Gradient::preset(1).unwrap(), to_gray: false },
        },
    })?;
    s.save(Some(std::path::Path::new(&out))).map(|_| ())
}
