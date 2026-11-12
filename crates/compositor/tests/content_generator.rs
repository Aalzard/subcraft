//! A generator in a layer's content stack: its gray result written into the channels through
//! the effect's per-channel blending.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{ContentEffect, ContentEffectKind, FillChannel, FillSource, Generator, GeneratorKind, GradientAxis, Layer, LayerContent, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

#[test]
fn a_content_generator_writes_its_gray_into_every_channel_but_normal() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    let ctx = EvalContext { geometry: &geo, resources: &res, mesh_maps: &maps };
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut l = Layer::new_fill("Fill", &tc);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        *ch = match k {
            ChannelKind::Roughness => FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.9))),
            ChannelKind::BaseColor => FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.2, 0.4, 0.6))),
            _ => FillChannel { enabled: false, ..ch.clone() },
        };
    }
    // A hard position gradient along X: 0 on the left half of the model, 1 on the right.
    let mut g = Generator::new(GeneratorKind::PositionGradient);
    g.axis = GradientAxis::X;
    g.softness = 0.0001;
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Generator(g), &tc)));
    ts.insert_above(None, l);
    let c = composite_full(&ts, &ctx);
    let rough = c.channel(ChannelKind::Roughness).unwrap();
    let color = c.channel(ChannelKind::BaseColor).unwrap();
    // The generator replaces the fill's values (Normal blending, full coverage): 0 on one side
    // of the model and 1 on the other, in roughness and as gray in base color. (Which texture
    // side X runs along depends on the plane's UVs: look at all four corners.)
    let corners = [(10, 10), (117, 10), (10, 117), (117, 117)];
    let values: Vec<f32> = corners.iter().map(|&(x, y)| rough.get(x, y)[0]).collect();
    let lo = values.iter().cloned().fold(1.0, f32::min);
    let hi = values.iter().cloned().fold(0.0, f32::max);
    assert!(lo.abs() < 2e-3 && (hi - 1.0).abs() < 2e-3, "{values:?}");
    let (x, y) = corners[values.iter().position(|v| *v == hi).unwrap()];
    assert!(color.get(x, y)[..3].iter().all(|v| (v - 1.0).abs() < 2e-3), "{:?}", color.get(x, y));
    assert_eq!(ContentEffectKind::Generator(g).label(), "Generator (Position Gradient)");
}
