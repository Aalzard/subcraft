//! Writes a .subcraft project of the default crate whose rust is masked by an anchor point
//! (ADR-0045): a "Rivets" fill carries a procedural height and a content anchor; the "Rust"
//! fill above it has a black mask with an Anchor Reference to that height, through Levels, so rust
//! sits exactly on the raised rivets. Open it in the app to see it:
//! `subcraft out.subcraft --select-layer Rust --effect-panel --screenshot x.png`.
//! cargo run --release -p subcraft-app --example anchor_project -- <out.subcraft> [size]
use subcraft_app::{Command, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{AnchorInput, ContentEffectKind, FillChannel, FillSource, Layer, LayerContent, Levels, MaskEffectKind, ProceduralKind, ProceduralParams};
use subcraft_mesh::primitives;

fn fill(s: &Session, name: &str, channels: &[(ChannelKind, FillSource)]) -> Layer {
    let ts = s.selected_texture_set();
    let mut l = Layer::new_fill(name, ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if let Some((_, src)) = channels.iter().find(|(kk, _)| kk == k) {
            *ch = FillChannel::new(src.clone());
        }
    }
    l
}

fn run(s: &mut Session, c: Command) -> Result<(), String> {
    s.execute(c).map_err(|e| format!("{e:?}"))
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?;
    let size = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024u32);
    let mut s = Session::new(primitives::sample_crate(), size);

    let paint = |v: [f32; 3]| FillSource::uniform(ChannelValue::rgb(v[0], v[1], v[2]));
    let base = fill(&s, "Painted metal", &[
        (ChannelKind::BaseColor, paint([0.18, 0.32, 0.45])),
        (ChannelKind::Roughness, FillSource::uniform(ChannelValue::scalar(0.45))),
        (ChannelKind::Metallic, FillSource::uniform(ChannelValue::scalar(0.0))),
    ]);
    run(&mut s, Command::InsertLayer { layer: Box::new(base), label: "Base".into() })?;

    // Raised rivets: a procedural height, captured by an anchor point.
    let mut pores = ProceduralParams::new(ProceduralKind::Pores);
    pores.scale = 14.0;
    let rivets = fill(&s, "Rivets", &[(ChannelKind::Height, FillSource::Procedural { params: pores, low: ChannelValue::scalar(0.0), high: ChannelValue::scalar(1.0) })]);
    run(&mut s, Command::InsertLayer { layer: Box::new(rivets), label: "Rivets".into() })?;
    let rivets = s.selection().layer.ok_or("no layer")?;
    run(&mut s, Command::AddContentEffect { layer: rivets, kind: ContentEffectKind::Anchor })?;
    let anchor = s.selected_texture_set().find(rivets).unwrap().content_effects[0].id;

    // Rust only where the anchored height is raised.
    let rust = fill(&s, "Rust", &[
        (ChannelKind::BaseColor, paint([0.42, 0.13, 0.03])),
        (ChannelKind::Roughness, FillSource::uniform(ChannelValue::scalar(0.92))),
    ]);
    run(&mut s, Command::InsertLayer { layer: Box::new(rust), label: "Rust".into() })?;
    let rust = s.selection().layer.ok_or("no layer")?;
    run(&mut s, Command::AddMask { layer: rust, white: false })?;
    let levels = Levels { in_low: 0.15, in_high: 0.55, ..Levels::default() };
    run(&mut s, Command::AddMaskEffect { layer: rust, kind: MaskEffectKind::AnchorRef { input: AnchorInput::new(anchor, ChannelKind::Height), levels: Some(levels) } })?;

    let index = subcraft_doc::AnchorIndex::build(&s.selected_texture_set().layers);
    assert!(index.problems().next().is_none(), "the reference resolves");
    s.update_composites();
    let ts = s.selection().texture_set;
    let color = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().to_dense();
    let rusty = color.as_chunks::<4>().0.iter().filter(|p| p[0] > p[2] + 0.1).count();
    s.save(Some(std::path::Path::new(out)))?;
    println!("{} anchor(s), {} reference(s); {rusty} of {} texels rusty -> {out}", index.anchors.len(), index.refs.len(), color.len() / 4);
    Ok(())
}
