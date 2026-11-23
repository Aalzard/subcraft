//! Smart masks made of several things at once, as Painter's are: Mask Builder recipes (one
//! generator that sums grunge, occlusion, curvature, height on the model, facing, scratches and
//! scattered spots, each with its own slider) and short stacks of effects with blend modes.
//! Every slider of the generator stays editable after the mask is applied.

use subcraft_doc::{BlendMode, DripSettings, Generator, GeneratorKind, MaskBuilder, MaskEffectKind, ProceduralKind};

use crate::presets::{SmartMask, mask, stacked};
use crate::smart_more::{all_white, cavity, pattern};

/// A Mask Builder generator: `level` shifts the summed contributions (0.5 leaves them as they
/// are), `softness` 1 keeps the sum and lower values harden it into a threshold.
fn builder(seed: u32, level: f32, softness: f32, b: MaskBuilder) -> MaskEffectKind {
    let mut g = Generator::new(GeneratorKind::MaskBuilder);
    g.noise.seed = seed;
    g.level = level;
    g.softness = softness;
    g.builder = Some(b);
    MaskEffectKind::Generator(g)
}

/// A Dripping Rust generator: rust on the edges and in the creases (`spreading` is how far it
/// goes) and the streaks that run down from it, which follow gravity.
fn drips(seed: u32, spreading: f32, softness: f32, d: DripSettings) -> MaskEffectKind {
    let mut g = Generator::new(GeneratorKind::DrippingRust);
    g.noise.seed = seed;
    g.level = spreading;
    g.softness = softness;
    g.drip = Some(d);
    MaskEffectKind::Generator(g)
}

fn off() -> MaskBuilder {
    MaskBuilder { ao: 0.0, curvature: 0.0, ..MaskBuilder::default() }
}

pub fn smart_masks() -> Vec<SmartMask> {
    use BlendMode::{LinearDodge, Multiply, Normal, Subtract};
    vec![
        mask("Dust Occlusion", vec![builder(701, 0.45, 0.7, MaskBuilder { ao: 1.0, grunge: 0.35, ao_noisiness: 0.4, ..off() })]),
        mask("Sharp Dirt", vec![builder(702, 0.4, 0.2, MaskBuilder { ao: 1.0, grunge: 0.5, curvature: 0.5, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.5, ..off() })]),
        mask("Ground Dirt", vec![builder(703, 0.4, 0.6, MaskBuilder { gradient: 0.9, gradient_invert: true, grunge: 0.5, ao: 0.5, ao_noisiness: 0.3, ..off() })]),
        mask("Dust from Above", vec![builder(704, 0.4, 0.5, MaskBuilder { world_normal: 0.8, ao: 0.5, grunge: 0.35, ao_noisiness: 0.3, ..off() })]),
        mask("Moss", vec![builder(705, 0.36, 0.4, MaskBuilder { ao: 0.9, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, world_normal: 0.35, grunge: 0.5, ao_noisiness: 0.6, ..off() })]),
        mask("Edges Scratched", vec![builder(706, 0.35, 0.35, MaskBuilder { curvature: 0.7, scratches: 0.7, grunge: 0.2, curvature_boost: 0.4, curvature_noisiness: 0.5, scratches_amount: 0.5, scratches_scale: 1.4, ..off() })]),
        mask("Surface Worn", vec![builder(707, 0.4, 0.5, MaskBuilder { grunge: 0.8, curvature: 0.35, curvature_noisiness: 0.7, ..off() })]),
        mask("Stains and Scratches", vec![builder(708, 0.38, 0.4, MaskBuilder { grunge: 0.7, scratches: 0.6, ao: 0.2, scratches_amount: 0.4, ..off() })]),
        mask("Oxidation", vec![builder(709, 0.35, 0.5, MaskBuilder { grunge: 0.6, ao: 0.6, curvature: 0.4, ao_noisiness: 0.5, curvature_noisiness: 0.5, ..off() })]),
        mask("Spots", vec![builder(710, 0.5, 0.5, MaskBuilder { scatter: 1.0, grunge: 0.2, scatter_density: 0.75, ..off() })]),
        // Gravity drips: these three are the Dripping Rust generator, so the streaks really hang
        // below the edges and creases they start from.
        mask("Edge Drips", vec![drips(719, 0.5, 0.3, DripSettings { contrast: 0.5, intensity: 0.8, smoothness: 0.25, length: 0.3 })]),
        // The streaks alone: the same generator without drips takes the rusted edges back out.
        stacked("Long Leaks", vec![
            (drips(720, 0.5, 0.3, DripSettings { contrast: 0.3, intensity: 1.0, smoothness: 0.12, length: 0.7 }), Normal),
            (drips(720, 0.5, 0.3, DripSettings { contrast: 0.3, intensity: 0.0, smoothness: 0.12, length: 0.7 }), Subtract),
        ]),
        mask("Soft Run-off Stains", vec![drips(721, 0.55, 0.5, DripSettings { contrast: 0.2, intensity: 1.0, smoothness: 0.85, length: 0.45 })]),
        // Streaks, kept only below the creases they would run out of.
        stacked("Leaks", vec![
            (pattern(ProceduralKind::Streaks, 7.0, 711, 0.55, 0.4), Normal),
            (builder(712, 0.5, 0.5, MaskBuilder { ao: 1.0, grunge: 0.3, ao_noisiness: 0.3, ..off() }), Multiply),
        ]),
        // Dirt in the creases, plus broken streaks running down from it.
        stacked("Dirt and Leaks", vec![
            (pattern(ProceduralKind::Streaks, 7.0, 715, 0.68, 0.4), Normal),
            (pattern(ProceduralKind::Clouds, 3.0, 716, 0.45, 0.5), Multiply),
            (cavity(0.3, 0.3, 0.4), LinearDodge),
        ]),
        // What is left of an old coat of paint: everything, minus the edges, minus flaked patches.
        stacked("Paint Old", vec![
            (all_white(), Normal),
            (builder(718, 0.4, 0.3, MaskBuilder { curvature: 0.8, grunge: 0.3, curvature_boost: 0.4, curvature_noisiness: 0.5, ..off() }), Subtract),
            (pattern(ProceduralKind::Dirt, 6.0, 717, 0.6, 0.9), Subtract),
        ]),
    ]
}
