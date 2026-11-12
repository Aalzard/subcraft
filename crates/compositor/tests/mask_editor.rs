//! Mask Editor generator: contributions, blending, global remap and the staged global blur.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind, ResourceId};
use subcraft_doc::{
    BlendMode, CurvatureMode, EditorDirection, FillChannel, FillSource, Generator, GeneratorKind, GrayscaleType, Layer, LayerContent, Mask,
    MaskEditor, MaskEffect, MaskEffectKind, Resource, ResourceKind, ResourceStore, TextureSet,
};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fx {
    /// A plane covering the whole texture, with baked maps: curvature rising left to right,
    /// occlusion rising top to bottom, thickness 0.25 everywhere.
    fn plane() -> Self {
        let mut maps = MeshMaps::new();
        let mut map = |kind, f: &dyn Fn(u32, u32) -> f32| {
            let mut img = TiledImage::new(RES, RES, 1, [0.0; 4]);
            for y in 0..RES {
                for x in 0..RES {
                    img.set(x, y, [f(x, y), 0.0, 0.0, 0.0]);
                }
            }
            maps.insert(kind, img);
        };
        map(MeshMapKind::Curvature, &curvature);
        map(MeshMapKind::AmbientOcclusion, &occlusion);
        map(MeshMapKind::Thickness, &|_, _| 0.25);
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
    /// The mask a Mask Editor with `editor` gives, read back from a layer it masks.
    fn mask(&self, editor: MaskEditor) -> Vec<f32> {
        let mut ts = TextureSet::new("T", 0, RES);
        let mut l = Layer::new_fill("Fill", &ts);
        let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
        for (k, ch) in f.channels.iter_mut() {
            ch.enabled = *k == ChannelKind::Roughness;
            if ch.enabled {
                *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0)));
            }
        }
        let mut mask = Mask::new(RES, false);
        let mut g = Generator::new(GeneratorKind::MaskEditor);
        g.editor = Some(editor);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
        l.mask = Some(mask);
        ts.insert_above(None, l);
        let c = composite_full(&ts, &self.ctx());
        let img = c.channel(ChannelKind::Roughness).unwrap();
        // Roughness 1 over the 0.3 default: value = 0.3 + 0.7 * mask.
        (0..RES * RES).map(|i| (img.get(i % RES, i / RES)[0] - 0.3) / 0.7).collect()
    }
}

fn curvature(x: u32, _: u32) -> f32 {
    x as f32 / (RES - 1) as f32
}
fn occlusion(_: u32, y: u32) -> f32 {
    y as f32 / (RES - 1) as f32
}
fn edges(x: u32) -> f32 {
    ((curvature(x, 0) - 0.5) * 2.0).clamp(0.0, 1.0)
}

fn assert_mask(got: &[f32], want: impl Fn(u32, u32) -> f32, tol: f32) {
    for y in 0..RES {
        for x in 0..RES {
            let (g, w) = (got[(y * RES + x) as usize], want(x, y));
            assert!((g - w).abs() < tol, "at {x},{y}: {g} vs {w}");
        }
    }
}

/// Everything off: the starting point of the single-contribution checks.
fn silent() -> MaskEditor {
    let mut e = MaskEditor::default();
    e.curvature.opacity = 0.0;
    e
}

#[test]
fn default_mask_editor_is_the_curvature_edges() {
    let fx = Fx::plane();
    assert_mask(&fx.mask(MaskEditor::default()), |x, _| edges(x), 2e-3);
    assert_mask(&fx.mask(silent()), |_, _| 0.0, 2e-3);
}

#[test]
fn each_map_group_contributes_through_its_own_controls() {
    let fx = Fx::plane();
    // Curvature: modes, opacity, invert.
    let mut e = MaskEditor::default();
    e.curvature.mode = CurvatureMode::Cavities;
    assert_mask(&fx.mask(e), |x, _| ((0.5 - curvature(x, 0)) * 2.0).clamp(0.0, 1.0), 2e-3);
    e.curvature.mode = CurvatureMode::Dual;
    e.curvature.opacity = 0.5;
    assert_mask(&fx.mask(e), |x, _| (curvature(x, 0) - 0.5).abs(), 2e-3);
    e.curvature.mode = CurvatureMode::Unprocessed;
    e.curvature.opacity = 1.0;
    e.curvature.invert = true;
    assert_mask(&fx.mask(e), |x, _| 1.0 - curvature(x, 0), 2e-3);
    // Ambient occlusion: the map, then balance / contrast (ramp 0.5 wide centred on 0.25), invert.
    let mut e = silent();
    e.occlusion.opacity = 1.0;
    assert_mask(&fx.mask(e), |_, y| occlusion(0, y), 2e-3);
    e.occlusion.balance = 0.75;
    e.occlusion.contrast = 0.5;
    e.occlusion.invert = true;
    assert_mask(&fx.mask(e), |_, y| 1.0 - (occlusion(0, y) / 0.5).clamp(0.0, 1.0), 2e-3);
    // Thickness: 0.25 everywhere, contrast 0.5 doubles the distance to mid gray, brightness adds.
    let mut e = silent();
    e.thickness.opacity = 1.0;
    assert_mask(&fx.mask(e), |_, _| 0.25, 2e-3);
    e.thickness.contrast = 0.5;
    e.thickness.brightness = 0.2;
    assert_mask(&fx.mask(e), |_, _| 0.2, 2e-3);
    e.thickness.invert = true;
    e.thickness.opacity = 0.5;
    assert_mask(&fx.mask(e), |_, _| 0.4, 2e-3);
}

#[test]
fn groups_blend_bottom_to_top_with_their_blending_modes() {
    let fx = Fx::plane();
    // Occlusion below, curvature edges multiplied on top.
    let mut e = MaskEditor::default();
    e.occlusion.opacity = 1.0;
    e.curvature.blend = BlendMode::Multiply;
    assert_mask(&fx.mask(e), |x, y| occlusion(0, y) * edges(x), 3e-3);
    // Half opacity: half way from the occlusion to the product.
    e.curvature.opacity = 0.5;
    assert_mask(&fx.mask(e), |x, y| occlusion(0, y) * (0.5 + 0.5 * edges(x)), 3e-3);
    // Normal blending replaces what is below; thickness (always normal) comes last.
    e.curvature.blend = BlendMode::Normal;
    e.curvature.opacity = 1.0;
    assert_mask(&fx.mask(e), |x, _| edges(x), 3e-3);
    e.thickness.opacity = 0.5;
    assert_mask(&fx.mask(e), |x, _| 0.5 * edges(x) + 0.125, 3e-3);
    // Global balance / contrast / invert act on the blended result.
    let mut e = MaskEditor::default();
    e.global_contrast = 0.5;
    e.global_invert = true;
    assert_mask(&fx.mask(e), |x, _| 1.0 - ((edges(x) - 0.25) / 0.5).clamp(0.0, 1.0), 4e-3);
}

#[test]
fn position_and_world_normal_groups_follow_the_model() {
    let fx = Fx::plane();
    let column = |m: &[f32], x: u32| m[(64 * RES + x) as usize];
    // Position, Right to Left only: a ramp across the plane, the same on every row.
    let mut e = silent();
    e.position.opacity = 1.0;
    e.position.axes = [EditorDirection { amount: 1.0, ..Default::default() }, EditorDirection::default(), EditorDirection::default()];
    let m = fx.mask(e);
    let (a, b) = (column(&m, 4), column(&m, RES - 5));
    assert!((a - b).abs() > 0.85, "{a} .. {b}");
    let rising = a < b;
    for x in 5..RES - 4 {
        let (p, q) = (column(&m, x - 1), column(&m, x));
        assert!(if rising { q >= p - 1e-3 } else { q <= p + 1e-3 }, "not monotonic at {x}: {p} {q}");
        assert!((m[(20 * RES + x) as usize] - q).abs() < 2e-3, "rows differ at {x}");
    }
    // Inverting the direction mirrors the ramp.
    e.position.axes[0].invert = true;
    let inverted = fx.mask(e);
    for x in (4..RES - 4).step_by(9) {
        assert!((column(&inverted, x) - (1.0 - column(&m, x))).abs() < 3e-3);
    }
    // World space normal: a flat plane has one normal, so the group is one value: n * 0.5 + 0.5
    // of the facing axis is 1 (0 inverted) and the two other axes sit at 0.5.
    let mut e = silent();
    e.world_normal.opacity = 1.0;
    let n = fx.geo.tile(0).normal(65);
    let facing = (0..3).find(|&a| n[a].abs() > 0.99).expect("axis-aligned plane");
    for axis in 0..3 {
        e.world_normal.axes = std::array::from_fn(|a| EditorDirection { amount: if a == axis { 1.0 } else { 0.0 }, ..Default::default() });
        let want = if axis == facing { n[axis] * 0.5 + 0.5 } else { 0.5 };
        assert_mask(&fx.mask(e), |_, _| want, 2e-3);
    }
}

#[test]
fn texture_groups_read_an_image_through_scale_grayscale_and_blending() {
    let mut fx = Fx::plane();
    // 4 x 1: two dark red texels, then two white ones (samples below fall between equal texels).
    let id = ResourceId::new();
    fx.res.insert(Resource { id, name: "t.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(Bitmap::new_u8(4, 1, 3, vec![102, 0, 0, 102, 0, 0, 255, 255, 255, 255, 255, 255])), source_path: None, encoded: None });
    let mut e = silent();
    e.texture.resource = Some(id);
    e.texture.opacity = 1.0;
    e.texture.grayscale = GrayscaleType::Red;
    let m = fx.mask(e);
    // u = 0.25 is red (0.4, raw: masks are never color-decoded), u = 0.75 white.
    let at = |m: &[f32], x: u32| m[(40 * RES + x) as usize];
    assert!((at(&m, 32) - 0.4).abs() < 3e-3 && (at(&m, 96) - 1.0).abs() < 3e-3, "{} {}", at(&m, 32), at(&m, 96));
    // Luma of the red texel is 0.2126 * 0.4.
    e.texture.grayscale = GrayscaleType::Luma;
    assert!((at(&fx.mask(e), 32) - 0.2126 * 0.4).abs() < 3e-3);
    // Scale 2 repeats the image twice across: u = 0.125 red, 0.375 white, 0.625 red.
    e.texture.grayscale = GrayscaleType::Red;
    e.texture.scale = 2.0;
    let m = fx.mask(e);
    assert!((at(&m, 16) - 0.4).abs() < 3e-3 && (at(&m, 48) - 1.0).abs() < 3e-3 && (at(&m, 80) - 0.4).abs() < 3e-3);
    // Invert and opacity; the second texture multiplies the first.
    e.texture.scale = 1.0;
    e.texture.invert = true;
    e.texture.opacity = 0.5;
    assert!((at(&fx.mask(e), 32) - 0.3).abs() < 3e-3);
    e.texture.invert = false;
    e.texture.opacity = 1.0;
    e.texture2 = subcraft_doc::EditorTexture { resource: Some(id), opacity: 1.0, grayscale: GrayscaleType::Red, blend: BlendMode::Multiply, ..Default::default() };
    assert!((at(&fx.mask(e), 32) - 0.16).abs() < 3e-3);
    // A group without an image, or with a missing one, contributes nothing.
    e.texture2.resource = None;
    e.texture.resource = Some(ResourceId::new());
    assert_mask(&fx.mask(e), |_, _| 0.0, 2e-3);
}

#[test]
fn global_blur_is_a_gaussian_of_the_blended_contributions() {
    let fx = Fx::plane();
    // A hard vertical step: unprocessed curvature thresholded by the group's contrast.
    let mut e = MaskEditor::default();
    e.curvature.mode = CurvatureMode::Unprocessed;
    e.curvature.contrast = 1.0;
    let step = |x: u32| if curvature(x, 0) > 0.5 { 1.0 } else { 0.0 };
    assert_mask(&fx.mask(e), |x, _| step(x), 2e-3);
    e.global_blur = 6.0;
    let weights = subcraft_materials::filter::gaussian_weights(6.0);
    let r = (weights.len() / 2) as i32;
    let blurred = |x: u32| -> f32 { weights.iter().enumerate().map(|(k, w)| w * step((x as i32 + k as i32 - r).clamp(0, RES as i32 - 1) as u32)).sum() };
    assert_mask(&fx.mask(e), |x, _| blurred(x), 3e-3);
    // The step is at 63 | 64: the blur is symmetric around it and untouched far away.
    assert!((blurred(63) + blurred(64) - 1.0).abs() < 1e-5 && blurred(40) == 0.0 && (blurred(90) - 1.0).abs() < 1e-6);
    // Balance / contrast / invert come after the blur: a hard threshold restores the step.
    e.global_contrast = 1.0;
    e.global_invert = true;
    assert_mask(&fx.mask(e), |x, _| 1.0 - step(x), 3e-3);
}
