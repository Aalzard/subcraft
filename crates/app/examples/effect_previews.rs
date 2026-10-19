//! Effect-stage native fixtures and bounded 4K thumbnail request/settlement profile.
use std::time::{Duration, Instant};
use subcraft_app::{
    Command, NewLayer, Session,
    layer_previews::{LayerPreviews, PreviewKey},
};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{
    ContentEffectKind, FillChannel, FillSource, Filter, Generator, GeneratorKind, Levels,
    MaskEffectKind, ProceduralKind, ProceduralParams,
};
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
fn run(s: &mut Session, cmd: Command) -> Result<(), String> {
    s.execute(cmd).map(|_| ()).map_err(|e| format!("{e:?}"))
}
fn fixture(size: u32, mask: bool) -> Result<Session, String> {
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    for set in sets {
        run(&mut s, Command::SelectTextureSet(set))?;
        run(&mut s, Command::AddLayer(NewLayer::Fill))?;
        let layer = s.selection().layer.unwrap();
        run(
            &mut s,
            Command::Rename {
                layer,
                name: "Stage previews".into(),
            },
        )?;
        run(
            &mut s,
            Command::SetFillChannel {
                layer,
                channel: C::BaseColor,
                fill: FillChannel::new(FillSource::Procedural {
                    params: ProceduralParams {
                        scale: 12.0,
                        ..ProceduralParams::new(ProceduralKind::Clouds)
                    },
                    low: ChannelValue::rgb(0.01, 0.003, 0.001),
                    high: ChannelValue::rgb(0.8, 0.25, 0.03),
                }),
            },
        )?;
        for filter in [
            Filter::Hsl {
                hue: 100.0,
                saturation: 1.0,
                lightness: 0.0,
            },
            Filter::Highpass {
                radius: 8.0,
                amount: 1.0,
            },
            Filter::Contrast {
                gain: 1.5,
                pivot: 0.5,
            },
        ] {
            run(
                &mut s,
                Command::AddContentEffect {
                    layer,
                    kind: ContentEffectKind::Filter {
                        channel: C::BaseColor,
                        filter,
                    },
                },
            )?;
        }
        if mask {
            run(
                &mut s,
                Command::AddMask {
                    layer,
                    white: false,
                },
            )?;
            run(
                &mut s,
                Command::AddMaskEffect {
                    layer,
                    kind: MaskEffectKind::Fill {
                        value: 1.0,
                        procedural: Some(ProceduralParams {
                            scale: 5.0,
                            ..ProceduralParams::new(ProceduralKind::Clouds)
                        }),
                        projection: Default::default(),
                    },
                },
            )?;
            run(
                &mut s,
                Command::AddMaskEffect {
                    layer,
                    kind: MaskEffectKind::Generator(Generator::new(
                        GeneratorKind::PositionGradient,
                    )),
                },
            )?;
            run(
                &mut s,
                Command::AddMaskEffect {
                    layer,
                    kind: MaskEffectKind::Levels(Levels {
                        gamma: 0.6,
                        ..Levels::default()
                    }),
                },
            )?;
            run(
                &mut s,
                Command::AddMaskEffect {
                    layer,
                    kind: MaskEffectKind::Filter(Filter::Blur { radius: 12.0 }),
                },
            )?;
        }
    }
    let set = s.document().texture_sets[0].id;
    run(&mut s, Command::SelectTextureSet(set))?;
    Ok(s)
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let profile = args.first().is_some_and(|a| a == "--profile");
    let mut s = fixture(
        if profile { 4096 } else { 512 },
        profile || args.get(1).is_some_and(|a| a == "mask"),
    )?;
    if !profile {
        return s.save(Some(std::path::Path::new(
            args.first().ok_or("output project path required")?,
        ))).map(|_|());
    }
    let mut cache = LayerPreviews::default();
    let mut keys = Vec::new();
    let mut edits = Vec::new();
    for ts in &s.document().texture_sets {
        let layer = ts.layers.last().unwrap();
        let base = PreviewKey {
            layer: layer.id,
            channel: C::BaseColor,
            mask: false,
            effect: None,
            size: 64,
        };
        keys.push(base);
        keys.push(PreviewKey { mask: true, ..base });
        for e in &layer.content_effects {
            keys.push(PreviewKey {
                effect: Some(e.id),
                ..base
            });
        }
        for e in &layer.mask.as_ref().unwrap().effects {
            keys.push(PreviewKey {
                mask: true,
                effect: Some(e.id),
                ..base
            });
        }
        edits.push((layer.id, layer.content_effects[0].as_ref().clone()));
    }
    let start = Instant::now();
    let mut times = Vec::new();
    let mut frame = 0;
    loop {
        let now = Instant::now();
        if frame < 8 {
            for (layer, template) in &edits {
                let mut effect = template.clone();
                effect.kind = ContentEffectKind::Filter {
                    channel: C::BaseColor,
                    filter: Filter::Hsl {
                        hue: 20.0 + frame as f32 * 10.0,
                        saturation: 1.0,
                        lightness: 0.0,
                    },
                };
                run(
                    &mut s,
                    Command::SetContentEffect {
                        layer: *layer,
                        effect,
                    },
                )?;
            }
        }
        for key in &keys {
            cache.request(&s, *key);
        }
        cache.poll();
        times.push(now.elapsed().as_secs_f64() * 1000.0);
        if frame >= 8 && !cache.busy() {
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            return Err("preview settlement timeout".into());
        }
        std::thread::sleep(Duration::from_millis(16).saturating_sub(now.elapsed()));
        frame += 1;
    }
    times.sort_by(f64::total_cmp);
    println!(
        "4K two sets / {} stage+layer+mask previews, 8 parameter revisions. CPU request/poll p50 {:.3}, p95 {:.3}, max {:.3} ms; settled {:.1} ms, {} frames, cache {} entries. GPU/render/upload and full composite excluded.",
        keys.len(),
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1],
        start.elapsed().as_secs_f64() * 1000.0,
        times.len(),
        cache.len()
    );
    Ok(())
}
