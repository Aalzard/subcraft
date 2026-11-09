//! More built-in smart materials and smart masks: stylized, game-art looks built only from this
//! project's own fills, procedural generators and mask effects (no third-party assets).
//! Edges / Cavity read baked curvature / AO, so bake first; without maps those layers stay clean.

use subcraft_core::{ChannelKind::*, ChannelValue};
use subcraft_doc::{BlendMode, GeneratorKind, Layer, MaskEffectKind, ProceduralKind, ProceduralParams, Projection, TextureSet};

use crate::presets::{dirt_generator, SmartMask, generator, mask, proc, rgb, triplanar, uni};
use crate::smart::{SmartMaterial, coat, occlusion, fx, fx_blend, folder, masked, smart};

pub(crate) type V = ChannelValue;

pub(crate) fn v(r: f32, g: f32, b: f32) -> V { ChannelValue::rgb(r, g, b) }
pub(crate) fn s(x: f32) -> V { ChannelValue::scalar(x) }

/// A fill layer with these channel sources.
pub(crate) fn mat(ts: &TextureSet, name: &str, channels: Vec<(subcraft_core::ChannelKind, subcraft_doc::FillSource)>, projection: Projection) -> Layer {
    crate::presets::preset(name, "", channels, projection).build(ts)
}

/// A procedural mask fill: `contrast` / `balance` shape it, projected in 3D so it ignores UV seams.
pub(crate) fn pattern(kind: ProceduralKind, scale: f32, seed: u32, balance: f32, contrast: f32) -> MaskEffectKind {
    MaskEffectKind::Fill {
        value: 1.0,
        procedural: Some(ProceduralParams { scale, seed, balance, contrast, ..ProceduralParams::new(kind) }),
        projection: triplanar(2.0),
    }
}

pub(crate) fn all_white() -> MaskEffectKind {
    MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Projection::default() }
}

/// Wear on edges: legacy threshold on baked curvature (no natural spread), so it stays a thin chip line.
pub(crate) fn edges(level: f32, softness: f32, noise: f32) -> MaskEffectKind {
    let mut g = match generator(GeneratorKind::Edges, level, softness, noise, false) {
        MaskEffectKind::Generator(g) => g,
        _ => unreachable!(),
    };
    g.edge_spread = None;
    MaskEffectKind::Generator(g)
}

pub(crate) fn cavity(level: f32, softness: f32, noise: f32) -> MaskEffectKind { generator(GeneratorKind::Cavity, level, softness, noise, false) }
pub(crate) fn top(level: f32, softness: f32, noise: f32) -> MaskEffectKind { generator(GeneratorKind::Directional, level, softness, noise, false) }

// ----- smart materials -----------------------------------------------------------------------

fn chipped_painted_metal(ts: &TextureSet) -> Layer {
    let steel = mat(ts, "Bare Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 9.0, 71, v(0.42, 0.42, 0.43), v(0.58, 0.58, 0.59))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, 72, s(0.35), s(0.65))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    // Paint everywhere, minus chips along edges and a few flaked patches.
    let paint = masked(coat(ts, "Paint", [0.5, 0.07, 0.05], 0.42, 0.0), ts, vec![
        fx(all_white()),
        fx_blend(edges(0.55, 0.12, 0.55), BlendMode::Subtract),
        fx_blend(pattern(ProceduralKind::Grunge, 5.0, 14, 0.62, 0.7), BlendMode::Subtract),
    ]);
    let grime = masked(coat(ts, "Grime", [0.05, 0.04, 0.035], 0.85, 0.0), ts, vec![fx(dirt_generator())]);
    folder("Chipped Painted Metal", ts, vec![steel, paint, occlusion(ts), grime])
}

fn brushed_steel(ts: &TextureSet) -> Layer {
    let steel = mat(ts, "Brushed Steel", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 6.0, 91, v(0.3, 0.31, 0.33), v(0.44, 0.45, 0.47))),
        (Roughness, proc(ProceduralKind::Brushed, 6.0, 92, s(0.2), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Brushed, 6.0, 93, s(0.0), s(0.015))),
    ], triplanar(2.0));
    let scratches = masked(coat(ts, "Scratches", [0.78, 0.79, 0.8], 0.45, 1.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 5.0, 94, 0.5, 0.6))]);
    let smudge = masked(coat(ts, "Smudges", [0.35, 0.35, 0.36], 0.6, 1.0), ts, vec![fx_blend(pattern(ProceduralKind::Dirt, 4.0, 95, 0.55, 0.1), BlendMode::Multiply), fx(dirt_generator())]);
    folder("Brushed Steel", ts, vec![steel, smudge, scratches])
}

fn dusty_concrete(ts: &TextureSet) -> Layer {
    let concrete = mat(ts, "Concrete", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 101, v(0.28, 0.28, 0.27), v(0.46, 0.45, 0.43))),
        (Roughness, proc(ProceduralKind::Grunge, 12.0, 102, s(0.82), s(0.97))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Speckles, 12.0, 103, s(0.0), s(0.05))),
    ], triplanar(2.0));
    let dust = masked(coat(ts, "Dust", [0.52, 0.49, 0.44], 0.98, 0.0), ts, vec![fx(top(0.62, 0.35, 0.45)), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 104, 0.45, 0.2), BlendMode::Multiply)]);
    let chips = masked(coat(ts, "Chipped Corners", [0.6, 0.59, 0.56], 0.9, 0.0), ts, vec![fx(edges(0.6, 0.15, 0.5))]);
    let stains = masked(coat(ts, "Stains", [0.2, 0.19, 0.18], 0.9, 0.0), ts, vec![fx(pattern(ProceduralKind::Streaks, 4.0, 105, 0.68, 0.4))]);
    folder("Dusty Concrete", ts, vec![concrete, stains, dust, chips])
}

fn oxidized_copper(ts: &TextureSet) -> Layer {
    let copper = mat(ts, "Copper", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 111, v(0.6, 0.29, 0.19), v(0.8, 0.42, 0.27))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 112, s(0.25), s(0.5))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    let verdigris = masked(
        mat(ts, "Verdigris", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 12.0, 113, v(0.08, 0.38, 0.3), v(0.2, 0.62, 0.5))),
            (Roughness, proc(ProceduralKind::Grunge, 16.0, 114, s(0.7), s(0.9))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Grunge, 20.0, 115, s(0.0), s(0.06))),
        ], triplanar(2.0)),
        ts,
        vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Grunge, 7.0, 116, 0.62, 0.5), BlendMode::LinearDodge), fx_blend(pattern(ProceduralKind::Clouds, 8.0, 117, 0.3, 0.4), BlendMode::Multiply)],
    );
    folder("Oxidized Copper", ts, vec![copper, verdigris])
}

fn frosted_ice(ts: &TextureSet) -> Layer {
    let ice = mat(ts, "Ice", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 141, v(0.28, 0.52, 0.72), v(0.5, 0.75, 0.9))),
        (Roughness, proc(ProceduralKind::Clouds, 9.0, 142, s(0.06), s(0.2))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 7.0, 143, s(0.0), s(0.05))),
    ], triplanar(1.5));
    let frost = masked(
        mat(ts, "Frost", vec![
            (BaseColor, rgb(0.85, 0.92, 0.98)),
            (Roughness, proc(ProceduralKind::Speckles, 10.0, 144, s(0.6), s(0.9))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Speckles, 14.0, 145, s(0.0), s(0.04))),
        ], triplanar(2.0)),
        ts,
        vec![fx(pattern(ProceduralKind::Clouds, 7.0, 146, 0.6, 0.5)), fx_blend(edges(0.5, 0.3, 0.5), BlendMode::LinearDodge), fx_blend(dirt_generator(), BlendMode::LinearDodge)],
    );
    let fractures = masked(coat(ts, "Fractures", [0.82, 0.92, 0.98], 0.3, 0.0), ts, vec![fx(pattern(ProceduralKind::Cracks, 4.0, 147, 0.5, 0.9))]);
    folder("Frosted Ice", ts, vec![ice, fractures, frost])
}

fn hammered_brass(ts: &TextureSet) -> Layer {
    let brass = mat(ts, "Brass", vec![
        (BaseColor, proc(ProceduralKind::Cells, 12.0, 171, v(0.62, 0.45, 0.16), v(0.85, 0.66, 0.28))),
        (Roughness, proc(ProceduralKind::Cells, 12.0, 171, s(0.22), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Cells, 12.0, 171, s(0.0), s(0.1))),
    ], triplanar(2.0));
    let tarnish = masked(coat(ts, "Tarnish", [0.14, 0.09, 0.04], 0.6, 0.6), ts, vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 172, 0.5, 0.2), BlendMode::Multiply)]);
    let polish = masked(coat(ts, "Polished Edges", [1.0, 0.8, 0.4], 0.15, 1.0), ts, vec![fx(edges(0.55, 0.15, 0.4))]);
    folder("Hammered Brass", ts, vec![brass, tarnish, polish])
}

pub fn smart_materials() -> Vec<SmartMaterial> {
    vec![
        smart("Chipped Painted Metal", chipped_painted_metal),
        smart("Brushed Steel", brushed_steel),
        smart("Dusty Concrete", dusty_concrete),
        smart("Oxidized Copper", oxidized_copper),
        smart("Frosted Ice", frosted_ice),
        smart("Hammered Brass", hammered_brass),
    ]
}

// ----- smart masks ---------------------------------------------------------------------------

pub fn smart_masks() -> Vec<SmartMask> {
    // A smart mask is a list of plain effect kinds laid over a black mask (a later Normal effect
    // replaces an earlier one), so each recipe is one effect, shaped by its own breakup noise.
    vec![
        mask("Edge Dirt", vec![edges(0.45, 0.4, 0.65)]),
        mask("Top-Down Dust", vec![top(0.7, 0.3, 0.45)]),
        mask("Scratches", vec![pattern(ProceduralKind::Scratches, 5.0, 302, 0.5, 0.65)]),
        mask("Moss in Crevices", vec![cavity(0.3, 0.35, 0.5)]),
        mask("Water Streaks", vec![pattern(ProceduralKind::Streaks, 4.0, 303, 0.45, 0.35)]),
        mask("Drip Stains", vec![pattern(ProceduralKind::Streaks, 7.0, 304, 0.55, 0.5)]),
        mask("Chipped Edges", vec![edges(0.6, 0.12, 0.6)]),
        mask("Cracks", vec![pattern(ProceduralKind::Cracks, 5.0, 305, 0.5, 0.9)]),
        mask("Speckled Dirt", vec![pattern(ProceduralKind::Speckles, 5.0, 306, 0.5, 0.3)]),
        mask("Rust Bleed", vec![cavity(0.28, 0.3, 0.75)]),
        mask("Bottom Splash", vec![generator(GeneratorKind::PositionGradient, 0.35, 0.4, 0.5, true)]),
        mask("Sun Fade", vec![top(0.55, 0.5, 0.35)]),
        mask("Soot Crevices", vec![cavity(0.15, 0.4, 0.3)]),
        mask("Peeling Edges", vec![edges(0.45, 0.35, 0.85)]),
        mask("Pitting", vec![pattern(ProceduralKind::Pores, 6.0, 309, 0.5, 0.6)]),
        mask("Grime Blotches", vec![pattern(ProceduralKind::Dirt, 5.0, 310, 0.5, 0.4)]),
    ]
}
