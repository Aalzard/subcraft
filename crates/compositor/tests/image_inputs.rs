//! Image inputs: Gradient Dynamic's gradient source and a generator's custom grunge.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, ResourceId};
use subcraft_doc::{ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, PainterControls, Resource, ResourceKind, ResourceStore, TextureSet};
use subcraft_image::Bitmap;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fx {
    fn new() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
    fn image(&mut self, w: u32, h: u32, comps: u32, data: Vec<u8>) -> ResourceId {
        let id = ResourceId::new();
        self.res.insert(Resource { id, name: "i.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(Bitmap::new_u8(w, h, comps, data)), source_path: None, encoded: None });
        id
    }
}

/// A roughness fill (`value`) with `content` effects and `mask` effects on a mask of `base`.
fn fill(value: f32, content: Vec<ContentEffectKind>, base: Option<f32>, mask: Vec<MaskEffectKind>) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut l = Layer::new_fill("F", &tc);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::Roughness;
        if ch.enabled {
            *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(value)));
        }
    }
    for kind in content {
        l.content_effects.push(Arc::new(ContentEffect::new(kind, &tc)));
    }
    if let Some(b) = base {
        let mut m = Mask::new(RES, b >= 0.5);
        for y in 0..RES {
            for x in 0..RES {
                m.base.set(x, y, [b, 0.0, 0.0, 0.0]);
            }
        }
        m.effects.extend(mask.into_iter().map(|k| Arc::new(MaskEffect::new(k))));
        l.mask = Some(m);
    }
    ts.insert_above(None, l);
    ts
}

fn roughness(fx: &Fx, ts: &TextureSet) -> Vec<f32> {
    let c = composite_full(ts, &fx.ctx());
    let img = c.channel(ChannelKind::Roughness).unwrap();
    (0..RES * RES).map(|i| img.get(i % RES, i / RES)[0]).collect()
}

#[test]
fn gradient_dynamic_maps_values_through_a_row_of_its_image() {
    let mut fx = Fx::new();
    // A gray ramp 0, 1/3, 2/3, 1 across four texels.
    let ramp = fx.image(4, 1, 1, vec![0, 85, 170, 255]);
    let gd = |vertical| Filter::GradientDynamic { source: ramp, vertical, position: 0.5 };
    // Content: roughness 0.25 reads u = 0.25, half way between the first two texels: 1/6.
    let ts = fill(0.25, vec![ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: gd(false) }], None, vec![]);
    assert!(roughness(&fx, &ts).iter().all(|v| (v - 1.0 / 6.0).abs() < 2e-3));
    // Mask: 0.75 reads 5/6; the roughness-1 fill over the 0.3 default shows 0.3 + 0.7 * 5/6.
    let ts = fill(1.0, vec![], Some(0.75), vec![MaskEffectKind::Filter(gd(false))]);
    let want = 0.3 + 0.7 * 5.0 / 6.0;
    assert!(roughness(&fx, &ts).iter().all(|v| (v - want).abs() < 3e-3));
    // Vertical on a one-row image: the input picks along v, which is constant: u = 0.5 -> 0.5.
    let ts = fill(0.25, vec![ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: gd(true) }], None, vec![]);
    assert!(roughness(&fx, &ts).iter().all(|v| (v - 0.5).abs() < 2e-3));
    // Without a source image the filter changes nothing.
    let ts = fill(0.25, vec![ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: Filter::gradient_dynamic() }], None, vec![]);
    assert!(roughness(&fx, &ts).iter().all(|v| (v - 0.25).abs() < 2e-3));
    assert_eq!(Filter::gradient_dynamic().resource(), None);
    assert_eq!(gd(false).resource(), Some(ramp));
}

#[test]
fn a_custom_grunge_image_replaces_the_breakup_noise() {
    let mut fx = Fx::new();
    let white = fx.image(1, 1, 1, vec![255]);
    let black = fx.image(1, 1, 1, vec![0]);
    let dirt = |grunge: Option<ResourceId>| {
        let mut g = Generator::new(GeneratorKind::Cavity);
        g.painter = Some(PainterControls { custom_grunge: grunge, ..g.painter.unwrap_or_default() });
        MaskEffectKind::Generator(g)
    };
    let with = |grunge| roughness(&fx, &fill(1.0, vec![], Some(0.0), vec![dirt(grunge)]));
    let (w, b, noise) = (with(Some(white)), with(Some(black)), with(None));
    // A flat image is a flat breakup: the mask is the same everywhere.
    assert!(w.iter().all(|v| (v - w[0]).abs() < 2e-3) && b.iter().all(|v| (v - b[0]).abs() < 2e-3));
    // White and black grunge shift the threshold opposite ways.
    assert!((w[0] - b[0]).abs() > 0.01, "{} {}", w[0], b[0]);
    // The default breakup noise is not flat.
    let (lo, hi) = noise.iter().fold((1.0f32, 0.0f32), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(hi - lo > 0.01, "{lo} {hi}");
    let mut g = Generator::new(GeneratorKind::Cavity);
    g.painter = Some(PainterControls { custom_grunge: Some(white), ..PainterControls::default() });
    assert_eq!(g.resources(), vec![white]);
}
