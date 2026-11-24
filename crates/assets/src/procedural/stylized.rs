//! Stylized, hand-painted materials for game art: large flat colour patches and bold strokes
//! instead of fine photo detail, a light line painted along the edges, a dark tint painted into
//! the creases, and the Baked Lighting Stylized filter on top for a sun and sky painted into
//! the Base Color. Same rules as the parent module: built at the parameter defaults, bake first
//! (the edge line reads the curvature, the shade and the light read the ambient occlusion).

use super::weathered::{built_coat, filter};
use super::*;
use subcraft_doc::{FilterSetting as S, MaskBuilder, MaterialFilter};

/// The three sliders every stylized material ends with.
fn paint_params(r: &mut Recipe, highlight: f32, shade: f32, light: f32) {
    r.param(unit("highlight", "Edge highlight", highlight).group("Painting").tip("A light line painted along the edges and corners (reads the baked curvature)"));
    r.param(unit("shade", "Cavity shade", shade).group("Painting").tip("A dark tint painted into creases and corners (reads the baked ambient occlusion)"));
    r.param(unit("light", "Baked light", light).group("Painting").tip("Sun from above and a cool sky painted into the Base Color; 0 leaves the colours unlit for a lit engine"));
    r.pattern_params();
}

/// Wrap the surface layers with the painted edge line, the cavity shade and the baked light.
/// `metal` is the formula of the Metallic of the two painted coats.
fn painted(mut r: Recipe, ts: &TextureSet, name: &str, layers: Vec<Layer>, seed: u32, metal: &str) -> Layer {
    let shade = built_coat(&mut r, ts, "Cavity Shade", seed, MaskBuilder { grunge: 0.15, ao: 1.0, curvature: 0.35, convex_range: 0.0, concave_range: 1.0, ..MaskBuilder::default() }, "shade", 0.3, 0.8, 0.9);
    r.bind_fx(&shade, 0, Prop::Softness, "0.6");
    r.value(&shade, BaseColor, "color * 0.28");
    r.value(&shade, Roughness, "roughness");
    r.value(&shade, Metallic, metal);

    // A hard, thin line: low softness, so it reads as a brush stroke and not as a glow.
    let highlight = built_coat(&mut r, ts, "Edge Highlight", seed + 1, MaskBuilder { grunge: 0.2, ao: 0.0, curvature: 1.0, convex_range: 1.0, concave_range: 0.0, ..MaskBuilder::default() }, "highlight", 0.2, 0.5, 1.0);
    r.bind_fx(&highlight, 0, Prop::Softness, "0.12");
    r.value(&highlight, BaseColor, "min(color * 1.8 + 0.08, 1)");
    r.value(&highlight, Roughness, "roughness");
    r.value(&highlight, Metallic, metal);

    // The light sits on the material's own folder, above the surface and both painted coats.
    let mut surface = folder(&format!("{PREFIX}{name}"), ts, vec![folder("Surface", ts, layers), shade, highlight]);
    // Neutral at light 0: no sun, a white sky at full strength and no occlusion multiply by 1.
    let lit = filter(&mut surface, ts, MaterialFilter::BakedLighting {
        sun_angle: 0.125,
        sun_height: 0.6,
        sun_intensity: 0.0,
        sun_color: [1.0, 0.95, 0.85],
        sky_intensity: 1.0,
        sky_color: [1.0; 3],
        ground_color: [1.0; 3],
        ambient_occlusion: 0.0,
    });
    r.drive(&surface, lit, S::SunIntensity, "light * 0.75");
    r.drive(&surface, lit, S::SkyIntensity, "1 - light * 0.45");
    r.drive(&surface, lit, S::SkyColor, "mix(rgb(1, 1, 1), rgb(0.78, 0.88, 1), light)");
    r.drive(&surface, lit, S::GroundColor, "mix(rgb(1, 1, 1), rgb(0.42, 0.34, 0.3), light)");
    r.drive(&surface, lit, S::AmbientOcclusion, "light * 0.8");
    r.finish(surface)
}

fn stylized_metal(ts: &TextureSet) -> Layer {
    metal(ts, "Stylized Metal", [0.34, 0.42, 0.55], 0.42, "A cool steel by default; try a brown for bronze", "color * 0.45", 0)
}

fn stylized_gold(ts: &TextureSet) -> Layer {
    metal(ts, "Stylized Gold", [0.95, 0.6, 0.12], 0.3, "A rich yellow gold by default; paler for brass", "mix(color, rgb(0.6, 0.2, 0.03), 0.7)", 40)
}

/// A painted metal. `shadow` is the colour of the dark patches (a plain darker `color` turns a
/// yellow into a muddy olive, so gold shades towards orange). `k` shifts the seeds, so two
/// metals do not share their strokes.
fn metal(ts: &TextureSet, name: &str, tint: [f32; 3], rough: f32, tip: &str, shadow: &str, k: u32) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", tint).group("Metal").tip(tip));
    r.param(unit("roughness", "Roughness", rough).group("Metal"));
    r.param(unit("metallic", "Metallic", 1.0).group("Metal").tip("Lower it for a painted, non-PBR metal look"));
    r.param(unit("variation", "Colour patches", 0.45).group("Metal").tip("Large light and dark patches, as if blocked in with a wide brush"));
    r.param(unit("strokes", "Brush strokes", 0.5).group("Metal").tip("Bold light strokes across the faces"));
    paint_params(&mut r, 0.75, 0.6, 0.35);

    let base = mat(ts, "Metal", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 1.6, 801 + k, v(0.36, 0.4, 0.46), v(0.48, 0.52, 0.58))),
        (Roughness, proc(ProceduralKind::Clouds, 2.0, 802 + k, s(0.38), s(0.46))),
        (Metallic, uni(1.0)),
    ], triplanar(1.0));
    r.color(&base, "color", "variation * 1.2");
    r.roughness(&base, "roughness", "0.1");
    r.value(&base, Metallic, "metallic");
    r.follow_pattern(&base);

    let strokes = masked(coat(ts, "Brush Strokes", [0.6, 0.63, 0.68], 0.35, 1.0), ts, vec![
        fx(pattern(ProceduralKind::Brushed, 0.8, 803 + k, 0.52, 0.85)),
        fx_blend(pattern(ProceduralKind::Clouds, 1.5, 804 + k, 0.5, 0.7), BlendMode::Multiply),
    ]);
    r.value(&strokes, BaseColor, "min(color * 1.6 + 0.04, 1)");
    r.value(&strokes, Roughness, "clamp(roughness - 0.1, 0.02, 1)");
    r.value(&strokes, Metallic, "metallic");
    r.bind(&strokes, Prop::Opacity { channel: None }, "strokes * 1.4");
    r.follow_pattern(&strokes);

    let dents = masked(coat(ts, "Dark Patches", [0.2, 0.22, 0.26], 0.5, 1.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.4, 805 + k, 0.56, 0.9))]);
    r.value(&dents, BaseColor, shadow);
    r.value(&dents, Roughness, "clamp(roughness + 0.1, 0.02, 1)");
    r.value(&dents, Metallic, "metallic");
    r.bind(&dents, Prop::Opacity { channel: None }, "variation * 1.3");
    r.follow_pattern(&dents);

    painted(r, ts, name, vec![base, strokes, dents], 806 + k, "metallic")
}

fn stylized_wood(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.45, 0.2, 0.07]).group("Wood").tip("A warm brown by default"));
    r.param(unit("roughness", "Roughness", 0.7).group("Wood"));
    r.param(unit("variation", "Colour patches", 0.4).group("Wood").tip("Large light and dark patches, as if blocked in with a wide brush"));
    r.param(unit("grain", "Grain", 0.6).group("Wood").tip("Bold dark and light grain lines"));
    paint_params(&mut r, 0.7, 0.6, 0.35);

    let base = mat(ts, "Wood", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 1.4, 811, v(0.34, 0.2, 0.09), v(0.46, 0.27, 0.13))),
        (Roughness, proc(ProceduralKind::Clouds, 2.0, 812, s(0.66), s(0.74))),
        (Metallic, uni(0.0)),
    ], triplanar(1.0));
    r.color(&base, "color", "variation * 1.2");
    r.roughness(&base, "roughness", "0.08");
    r.follow_pattern(&base);

    let dark = with_height(masked(coat(ts, "Dark Grain", [0.2, 0.11, 0.05], 0.75, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 0.9, 813, 0.6, 0.85))]), ts);
    r.value(&dark, BaseColor, "color * 0.5");
    r.value(&dark, Roughness, "roughness");
    r.value(&dark, Height, "0 - grain * 0.03");
    r.bind(&dark, Prop::Opacity { channel: None }, "grain");
    r.follow_pattern(&dark);

    let light = masked(coat(ts, "Light Grain", [0.6, 0.4, 0.2], 0.65, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 0.7, 814, 0.66, 0.85))]);
    r.value(&light, BaseColor, "min(color * 1.45 + 0.03, 1)");
    r.value(&light, Roughness, "roughness");
    r.bind(&light, Prop::Opacity { channel: None }, "grain * 0.7");
    r.follow_pattern(&light);

    painted(r, ts, "Stylized Wood", vec![base, dark, light], 815, "0")
}

fn stylized_stone(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.33, 0.38, 0.46]).group("Stone").tip("A cool grey by default"));
    r.param(unit("roughness", "Roughness", 0.85).group("Stone"));
    r.param(unit("variation", "Colour patches", 0.5).group("Stone").tip("Large light and dark patches, as if blocked in with a wide brush"));
    r.param(unit("cracks", "Cracks", 0.5).group("Stone").tip("A few bold painted cracks"));
    paint_params(&mut r, 0.8, 0.65, 0.35);

    let base = mat(ts, "Stone", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 1.4, 821, v(0.34, 0.35, 0.36), v(0.46, 0.47, 0.48))),
        (Roughness, proc(ProceduralKind::Clouds, 2.0, 822, s(0.8), s(0.9))),
        (Metallic, uni(0.0)),
    ], triplanar(1.0));
    r.color(&base, "color", "variation * 1.1");
    r.roughness(&base, "roughness", "0.1");
    r.follow_pattern(&base);

    // Hard-edged facets: flat planes of slightly different value, like chiselled faces.
    let facets = masked(coat(ts, "Facets", [0.5, 0.51, 0.52], 0.85, 0.0), ts, vec![fx(pattern(ProceduralKind::Cells, 2.2, 823, 0.5, 0.95))]);
    r.value(&facets, BaseColor, "min(color * 1.5 + 0.03, 1)");
    r.value(&facets, Roughness, "roughness");
    r.bind(&facets, Prop::Opacity { channel: None }, "variation * 1.2");
    r.follow_pattern(&facets);

    let cracks = with_height(masked(coat(ts, "Cracks", [0.1, 0.1, 0.11], 0.95, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Cracks, 2.0, 824, 0.5, 0.9)),
        fx_blend(pattern(ProceduralKind::Clouds, 1.5, 825, 0.5, 0.7), BlendMode::Multiply),
    ]), ts);
    r.value(&cracks, BaseColor, "color * 0.18");
    r.value(&cracks, Height, "0 - 0.05");
    r.bind_fx(&cracks, 1, Prop::PatternBalance { channel: None }, "remap(cracks, 0.8, 0.15)");
    r.bind(&cracks, Prop::Opacity { channel: None }, "saturate(cracks * 5)");
    r.follow_pattern(&cracks);

    painted(r, ts, "Stylized Stone", vec![base, facets, cracks], 826, "0")
}

fn stylized_cloth(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.5, 0.1, 0.1]).group("Cloth").tip("A deep red by default"));
    r.param(unit("roughness", "Roughness", 0.9).group("Cloth"));
    r.param(unit("variation", "Colour patches", 0.4).group("Cloth").tip("Large light and dark patches, as if blocked in with a wide brush"));
    r.param(unit("folds", "Painted folds", 0.5).group("Cloth").tip("Soft dark bands, as if the cloth hung in folds"));
    r.param(unit("weave", "Weave strokes", 0.4).group("Cloth").tip("Short light strokes that suggest the threads"));
    paint_params(&mut r, 0.6, 0.7, 0.35);

    let base = mat(ts, "Cloth", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 1.4, 831, v(0.42, 0.08, 0.08), v(0.58, 0.12, 0.12))),
        (Roughness, proc(ProceduralKind::Clouds, 2.0, 832, s(0.86), s(0.94))),
        (Metallic, uni(0.0)),
    ], triplanar(1.0));
    r.color(&base, "color", "variation * 1.1");
    r.roughness(&base, "roughness", "0.08");
    r.follow_pattern(&base);

    let folds = masked(coat(ts, "Painted Folds", [0.25, 0.05, 0.05], 0.92, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Brushed, 0.35, 833, 0.55, 0.45)),
        fx_blend(pattern(ProceduralKind::Clouds, 1.2, 834, 0.45, 0.5), BlendMode::Multiply),
    ]);
    r.value(&folds, BaseColor, "color * 0.45");
    r.value(&folds, Roughness, "roughness");
    r.bind(&folds, Prop::Opacity { channel: None }, "folds * 1.3");
    r.follow_pattern(&folds);

    let weave = masked(coat(ts, "Weave Strokes", [0.7, 0.3, 0.3], 0.85, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 3.0, 835, 0.62, 0.85))]);
    r.value(&weave, BaseColor, "min(color * 1.4 + 0.04, 1)");
    r.value(&weave, Roughness, "roughness");
    r.bind(&weave, Prop::Opacity { channel: None }, "weave * 0.8");
    r.follow_pattern(&weave);

    painted(r, ts, "Stylized Cloth", vec![base, folds, weave], 836, "0")
}

fn stylized_leather(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.36, 0.16, 0.07]).group("Leather").tip("A saddle brown by default"));
    r.param(unit("roughness", "Roughness", 0.6).group("Leather"));
    r.param(unit("variation", "Colour patches", 0.45).group("Leather").tip("Large light and dark patches, as if blocked in with a wide brush"));
    r.param(unit("creases", "Creases", 0.5).group("Leather").tip("A few bold painted crease lines"));
    r.param(unit("scuffs", "Scuffs", 0.4).group("Leather").tip("Pale rubbed patches"));
    paint_params(&mut r, 0.7, 0.65, 0.35);

    let base = mat(ts, "Leather", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 1.4, 841, v(0.3, 0.13, 0.06), v(0.42, 0.19, 0.08))),
        (Roughness, proc(ProceduralKind::Clouds, 2.0, 842, s(0.55), s(0.65))),
        (Metallic, uni(0.0)),
    ], triplanar(1.0));
    r.color(&base, "color", "variation * 1.2");
    r.roughness(&base, "roughness", "0.1");
    r.follow_pattern(&base);

    let scuffs = masked(coat(ts, "Scuffs", [0.55, 0.35, 0.2], 0.75, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.6, 843, 0.6, 0.9))]);
    r.value(&scuffs, BaseColor, "min(color * 1.5 + 0.05, 1)");
    r.value(&scuffs, Roughness, "clamp(roughness + 0.15, 0.02, 1)");
    r.bind(&scuffs, Prop::Opacity { channel: None }, "scuffs * 1.2");
    r.follow_pattern(&scuffs);

    let creases = with_height(masked(coat(ts, "Creases", [0.1, 0.04, 0.02], 0.7, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Cracks, 3.0, 844, 0.5, 0.9)),
        fx_blend(pattern(ProceduralKind::Clouds, 1.6, 845, 0.5, 0.7), BlendMode::Multiply),
    ]), ts);
    r.value(&creases, BaseColor, "color * 0.3");
    r.value(&creases, Height, "0 - 0.03");
    r.bind_fx(&creases, 1, Prop::PatternBalance { channel: None }, "remap(creases, 0.8, 0.2)");
    r.bind(&creases, Prop::Opacity { channel: None }, "saturate(creases * 5) * 0.9");
    r.follow_pattern(&creases);

    painted(r, ts, "Stylized Leather", vec![base, scuffs, creases], 846, "0")
}

/// The stylized family, in shelf order.
pub(super) fn smart_materials() -> Vec<SmartMaterial> {
    let named = |name: &str, build: fn(&TextureSet) -> Layer| smart(&format!("{PREFIX}{name}"), build);
    vec![named("Stylized Metal", stylized_metal), named("Stylized Wood", stylized_wood), named("Stylized Stone", stylized_stone), named("Stylized Gold", stylized_gold), named("Stylized Cloth", stylized_cloth), named("Stylized Leather", stylized_leather)]
}
