//! What edits cost on a saved project, in a fresh process:
//! `cargo run --release -p subcraft-app --example edit_timing -- file.subcraft`
//!
//! Opens the project, composites it, then times (blocking, to the exact result) a mask generator
//! level change, a procedural fill scale change (new noise) and a fill color change (noise
//! reused) on the lowest, a middle and the highest top-level layer that has one. With
//! `SUBCRAFT_COMPOSITE_PROFILE=1` each edit lists the node kinds it evaluated.
use std::time::Instant;

use subcraft_app::{Command, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{FillSource, Layer, LayerContent, MaskEffectKind};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// Node evaluations by kind (needs `SUBCRAFT_COMPOSITE_PROFILE=1`); time is summed over threads.
fn profile() {
    let rows = subcraft_compositor::profile::take();
    if rows.is_empty() {
        return;
    }
    let list: Vec<String> = rows.iter().take(7).map(|(k, n, t)| format!("{k} {t:.0} ms/{n}")).collect();
    println!("      {}", list.join(", "));
}

fn has_generator(l: &Layer) -> bool {
    l.mask.as_ref().is_some_and(|m| m.effects.iter().any(|e| matches!(e.kind, MaskEffectKind::Generator(_))))
}

fn has_procedural(l: &Layer) -> bool {
    matches!(&l.content, LayerContent::Fill(f) if f.channels.get(&ChannelKind::BaseColor).is_some_and(|c| c.enabled && matches!(c.source, FillSource::Procedural { .. })))
}

/// The lowest, a middle and the highest of `ids` (fewer when there are fewer).
fn places(ids: &[(usize, LayerId)]) -> Vec<(&'static str, usize, LayerId)> {
    let mut out = Vec::new();
    for (name, at) in [("bottom", 0), ("middle", ids.len() / 2), ("top", ids.len().saturating_sub(1))] {
        if let Some(&(pos, id)) = ids.get(at)
            && !out.iter().any(|(_, _, other)| *other == id)
        {
            out.push((name, pos, id));
        }
    }
    out
}

fn timed(s: &mut Session, label: &str, mut edit: impl FnMut(&mut Session, usize)) {
    let mut times = Vec::new();
    for i in 0..2 {
        let t = Instant::now();
        edit(s, i);
        let tiles = s.update_composites().iter().map(|u| u.tiles.len()).sum::<usize>();
        times.push(format!("{:.0}", ms(t)));
        if i == 1 {
            println!("    {label}: {} ms ({tiles} tiles)", times.join(" / "));
            profile();
        } else {
            let _ = subcraft_compositor::profile::take();
        }
    }
}

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("project path"));
    let t = Instant::now();
    let mut s = Session::open_project(&path).expect("open");
    println!("open: {:.0} ms", ms(t));
    let t = Instant::now();
    let tiles: usize = s.update_composites().iter().map(|u| u.tiles.len()).sum();
    println!("first composite: {:.0} ms, {tiles} tiles", ms(t));
    profile();

    let ts = &s.document().texture_sets[0];
    let n = ts.layers.len();
    println!("{} top-level layers, {}²", n, ts.resolution);
    let with = |f: fn(&Layer) -> bool| -> Vec<(usize, LayerId)> { ts.layers.iter().enumerate().filter(|(_, l)| f(l)).map(|(i, l)| (i, l.id)).collect() };
    let generators = with(has_generator);
    let procedurals = with(has_procedural);

    println!("mask generator level:");
    for (place, pos, id) in places(&generators) {
        timed(&mut s, &format!("{place} (layer {pos} of {n})"), |s, i| {
            let layer = s.document().texture_sets[0].find(id).expect("layer");
            let effect = layer.mask.as_ref().unwrap().effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Generator(_))).unwrap();
            let mut e = (**effect).clone();
            if let MaskEffectKind::Generator(g) = &mut e.kind {
                g.level = if i == 0 { 0.37 } else { 0.63 };
            }
            s.execute(Command::SetMaskEffect { layer: id, effect: e }).expect("edit");
        });
    }
    let fill_edit = |s: &mut Session, id: LayerId, change: &dyn Fn(&mut FillSource)| {
        let layer = s.document().texture_sets[0].find(id).expect("layer");
        let LayerContent::Fill(f) = &layer.content else { unreachable!() };
        let mut ch = f.channels[&ChannelKind::BaseColor].clone();
        change(&mut ch.source);
        s.execute(Command::SetFillChannel { layer: id, channel: ChannelKind::BaseColor, fill: ch }).expect("edit");
    };
    println!("procedural fill scale (new noise):");
    for (place, pos, id) in places(&procedurals) {
        timed(&mut s, &format!("{place} (layer {pos} of {n})"), |s, i| {
            fill_edit(s, id, &|src| {
                if let FillSource::Procedural { params, .. } = src {
                    params.scale = if i == 0 { 5.5 } else { 7.25 };
                }
            });
        });
    }
    println!("procedural fill color (noise reused):");
    for (place, pos, id) in places(&procedurals) {
        timed(&mut s, &format!("{place} (layer {pos} of {n})"), |s, i| {
            fill_edit(s, id, &|src| {
                if let FillSource::Procedural { low, .. } = src {
                    *low = ChannelValue::rgb(if i == 0 { 0.2 } else { 0.3 }, 0.1, 0.05);
                }
            });
        });
    }
}
