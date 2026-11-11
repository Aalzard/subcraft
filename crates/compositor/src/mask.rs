//! Mask evaluation: base raster followed by the effect stack.
//!
//! Each visible effect produces a value `e` and coverage `ea` per texel and is composited onto
//! the running mask with its blend mode and opacity (`m = lerp(m, B(m, e), ea·opacity)`).

use glam::Vec3;
use subcraft_core::MeshMapKind;
use subcraft_doc::{Generator, GeneratorKind, Mask, MaskEffectKind, Projection, ProjectionMode};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::sample::procedural_tile;

use crate::TileContext;

use crate::util::read_tile_rgba;

/// Mask values (0..1) for every texel of the tile, or `None` when the mask is absent/disabled.
pub fn eval_mask(mask: Option<&Mask>, tc: &TileContext) -> Option<Vec<f32>> {
    eval_mask_scaled(mask,tc,1.0)
}

/// Reduced/enlarged UV inspection scales only neighborhood radii, never document data.
pub fn eval_mask_scaled(mask: Option<&Mask>, tc: &TileContext, radius_scale:f32) -> Option<Vec<f32>> {
    let mask = mask.filter(|m| m.enabled)?;
    if crate::stages::mask_spatial(mask) { return Some(crate::stages::mask_preview(mask,tc,radius_scale)); }
    let mut m = crate::tile::prof("mask base", || read_mask_base(&mask.base, tc));
    let mut e = vec![[0.0f32; 4]; TILE_TEXELS];
    // Anchor points change nothing and references cannot resolve without the whole texture set
    // (the stage graph evaluates them, ADR-0045): both are skipped here.
    for effect in mask.effects.iter().filter(|e| e.visible && e.opacity > 0.0 && !matches!(e.kind, MaskEffectKind::Anchor | MaskEffectKind::AnchorRef { .. })) {
        // e[i] = [value, alpha, _, _]
        let label = match &effect.kind {
            MaskEffectKind::Generator(_) => "mask generator (local)",
            MaskEffectKind::Fill { .. } => "mask fill (local)",
            _ => "mask effect (local)",
        };
        crate::tile::prof(label, || evaluate_effect(&effect.kind, tc, &m, &mut e, None));
        crate::tile::prof("mask blend (local)", || crate::blend::composite_mask_run(&mut m, &e, effect.opacity, effect.blend));
    }
    for v in m.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
    Some(m)
}

/// The first component of the mask's base raster on this tile (what `read_tile_rgba` gives in
/// `[0]`), decoded straight into the mask buffer when the raster matches the set.
fn read_mask_base(base: &subcraft_image::TiledImage, tc: &TileContext) -> Vec<f32> {
    let geo = tc.geo;
    if base.comps() == 1 && base.width() == geo.resolution && base.height() == geo.resolution {
        let mut m = vec![0.0f32; TILE_TEXELS];
        base.read_tile(base.tile_index(geo.x0 / subcraft_image::TILE_SIZE, geo.y0 / subcraft_image::TILE_SIZE), &mut m);
        return m;
    }
    let mut px = vec![[0.0f32; 4]; TILE_TEXELS];
    read_tile_rgba(base, geo, &mut px);
    px.iter().map(|p| p[0]).collect()
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Generator coverage: a mesh-derived raw value thresholded at `level` (optionally broken up by
/// noise) with a `softness`-wide transition.
///
/// `input` (an anchor read as gray, ADR-0045) replaces the baked map the generator reads.
fn eval_generator(g: &Generator, tc: &TileContext, out: &mut [[f32; 4]], input: Option<&[f32]>) {
    if g.kind == GeneratorKind::MaskEditor {
        // Without a global blur the editor is local to the tile; with one, the stage graph
        // blurs `editor_raw` between these two steps.
        let mut raw = vec![0.0f32; TILE_TEXELS];
        editor_raw(g, tc, &mut raw);
        let editor = g.editor_settings();
        for (o, v) in out.iter_mut().zip(raw) {
            *o = [editor.finish(v), 1.0, 0.0, 0.0];
        }
        return;
    }
    let input = input.filter(|_| g.accepts_input());
    if g.kind == GeneratorKind::Edges && let Some(spread) = g.edge_spread {
        eval_edge_spread(g, spread, tc, out, input);
        return;
    }
    let geo = tc.geo;
    let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
    let map_kind = match g.kind {
        GeneratorKind::Edges | GeneratorKind::Concave => Some(MeshMapKind::Curvature),
        GeneratorKind::Cavity | GeneratorKind::AmbientOcclusion => Some(MeshMapKind::AmbientOcclusion),
        GeneratorKind::MeshMap(k) => Some(k),
        GeneratorKind::Directional | GeneratorKind::PositionGradient | GeneratorKind::MaskEditor => None,
    };
    match (input, map_kind) {
        (Some(values), _) => {
            for (m, &v) in map.iter_mut().zip(values) {
                *m = [v; 3];
            }
        }
        (None, Some(k)) => tc.sample.mesh_maps.read_tile(k, geo, &mut map),
        (None, None) => {}
    }
    let dir = Vec3::from_array(g.direction).normalize_or(Vec3::Y);
    let set = tc.set_geometry;
    let axis = g.axis.index();
    let ext_axis = set.bounds.extent()[axis].max(1e-12);
    let half = g.softness.max(1e-4) * 0.5;
    // Breakup noise is projected in 3D (triplanar on object position), never in UV: UV noise
    // jumps at every seam and would make wear follow the islands instead of the shape.
    let noise_proj = Projection { mode: ProjectionMode::Triplanar, triplanar_sharpness: 4.0, ..Projection::default() };
    let offset = noise_offset(g);
    let noise_params = scaled_noise(g);
    // The plain Edges threshold (no spread model) keeps its organic noise: the grunge map's broad
    // range would detach dashes from the baked curvature lines.
    let noise = (g.noise_amount > 0.0).then(|| if uses_grunge_map(g) && g.kind != GeneratorKind::Edges {
        subcraft_materials::sample::grunge_tile_at(&noise_params, offset, geo)
    } else if g.organic_breakup {
        subcraft_materials::sample::organic_tile_at(&noise_params, offset, geo)
    } else { subcraft_materials::sample::procedural_tile_at(&noise_params, &noise_proj, offset, geo) });
    // Painter's dirt: convex edges are kept clear of dirt by `edges_masking`.
    let edges_masking = g.painter.map_or(0.0, |p| p.edges_masking.clamp(0.0, 1.0));
    let mut curvature = Vec::new();
    if edges_masking > 0.0 && g.kind == GeneratorKind::Cavity {
        curvature = vec![[0.0f32; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, geo, &mut curvature);
    }
    let floor = g.painter.map_or(0.0, |p| p.grunge_floor.clamp(0.0, 1.0));
    let grunge_map = uses_grunge_map(g) && g.kind != GeneratorKind::Edges;
    // Grain speckles dirt's threshold; the plain Edges threshold (no spread model) keeps its exact look.
    // Grain is part of Breakup: it fades in over the first quarter of the Breakup slider, so
    // Breakup 0 is exactly smooth whatever the Grain value.
    let grain = if g.kind == GeneratorKind::Edges { 0.0 } else { g.painter.map_or(0.0, |p| p.grain.clamp(0.0, 1.0)) * (g.noise_amount * 4.0).clamp(0.0, 1.0) };
    let fine = (grain > 0.0).then(|| fine_noise(g, geo));
    for (i, o) in out.iter_mut().enumerate() {
        let raw = match g.kind {
            GeneratorKind::Edges | GeneratorKind::MeshMap(_) => map[i][0],
            GeneratorKind::Cavity if !curvature.is_empty() => (1.0 - map[i][0]) * (1.0 - edges_masking * ((curvature[i][0] - 0.5) * 2.0).clamp(0.0, 1.0)),
            GeneratorKind::Cavity | GeneratorKind::AmbientOcclusion => 1.0 - map[i][0],
            GeneratorKind::Directional => Vec3::from_array(geo.normal(i)).dot(dir) * 0.5 + 0.5,
            GeneratorKind::Concave => 1.0 - map[i][0],
            GeneratorKind::PositionGradient => (geo.position(i)[axis] * set.max_extent / ext_axis).clamp(0.0, 1.0),
            GeneratorKind::MaskEditor => 0.0, // handled above
        };
        let level = if let Some(noise) = &noise {
            g.level + (noise[i] - 0.5) * g.noise_amount
        } else {
            g.level
        };
        let level = match &fine {
            Some(f) => level + ((f[i] - 0.5) * 3.0).clamp(-0.5, 0.5) * grain * 0.5,
            None => level,
        };
        let mut v = smoothstep(level - half, level + half, raw);
        if floor > 0.0 && let Some(noise) = &noise {
            // Contrast-stretched around the mean: Painter's tail spans 0.02 to 0.18, not a narrow band.
            // The grunge map already spans the range: use it as is, minus its darkest third.
            let tail = if grunge_map { smoothstep(0.3, 1.0, noise[i]) } else { ((noise[i] - 0.5) * 3.0 + 0.5).clamp(0.0, 1.0) };
            v += (1.0 - v) * floor * tail;
        }
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}

/// Grayscale blend of two Mask Editor values with a layer blending mode.
fn gray_blend(mode: subcraft_doc::BlendMode, below: f32, value: f32) -> f32 {
    if mode == subcraft_doc::BlendMode::Disable {
        return below;
    }
    crate::blend::blend_fn(mode, [below; 3], [value; 3], crate::blend::BlendTarget::MASK)[0]
}

/// Blend one Mask Editor contribution over the running value: `out += (blend(out, v) - out) * opacity`.
fn editor_mix(out: &mut [f32], opacity: f32, mode: subcraft_doc::BlendMode, value: impl Fn(usize) -> f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 || !opacity.is_finite() {
        return;
    }
    for (i, o) in out.iter_mut().enumerate() {
        let v = value(i).clamp(0.0, 1.0);
        *o += (gray_blend(mode, *o, v).clamp(0.0, 1.0) - *o) * opacity;
    }
}

/// Mask Editor (`GeneratorKind::MaskEditor`): its seven contributions blended bottom to top over
/// black, before the global Blur / Balance / Contrast / Invert (`MaskEditor::finish`).
///
/// Order: Texture, Texture 2, Ambient Occlusion, Curvature, World Space Normal, Position
/// Gradient, Thickness. Maps that are not baked read their neutral value; a texture group
/// without an image (or whose image is missing) contributes nothing.
pub(crate) fn editor_raw(g: &Generator, tc: &TileContext, out: &mut [f32]) {
    let editor = g.editor_settings();
    let geo = tc.geo;
    out.fill(0.0);
    for texture in [&editor.texture, &editor.texture2] {
        let Some(bmp) = texture.resource.and_then(|id| tc.sample.resources.bitmap(id)) else { continue };
        if texture.opacity <= 0.0 {
            continue;
        }
        let projection = Projection {
            mode: if texture.triplanar { ProjectionMode::Triplanar } else { ProjectionMode::Uv },
            scale: texture.tiling(bmp.width, bmp.height),
            triplanar_sharpness: texture.triplanar_sharpness(),
            ..Projection::default()
        };
        let sc = projection.rotation_sin_cos();
        editor_mix(out, texture.opacity, texture.blend, |i| {
            let s = subcraft_materials::sample::sample_projected_sc(&projection, sc, geo.uv(i), geo.position(i), geo.normal(i), |p| bmp.sample(p[0], p[1], subcraft_image::Wrap::Repeat, false));
            texture.shape([s[0], s[1], s[2]])
        });
    }
    let read = |kind: MeshMapKind| {
        let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(kind, geo, &mut map);
        map
    };
    if editor.occlusion.opacity > 0.0 {
        let map = read(MeshMapKind::AmbientOcclusion);
        editor_mix(out, editor.occlusion.opacity, editor.occlusion.blend, |i| editor.occlusion.shape(map[i][0]));
    }
    if editor.curvature.opacity > 0.0 {
        let map = read(MeshMapKind::Curvature);
        editor_mix(out, editor.curvature.opacity, editor.curvature.blend, |i| editor.curvature.shape(map[i][0]));
    }
    if editor.world_normal.opacity > 0.0 {
        // The baked world-space normal map when there is one (it carries the high poly), else
        // the mesh normal; either way `n * 0.5 + 0.5` per axis: 1 facing right / up / front.
        let baked = tc.sample.mesh_maps.get(MeshMapKind::WorldSpaceNormal).is_some().then(|| read(MeshMapKind::WorldSpaceNormal));
        editor_mix(out, editor.world_normal.opacity, editor.world_normal.blend, |i| {
            let gradients = match &baked {
                Some(map) => map[i],
                None => geo.normal(i).map(|c| c * 0.5 + 0.5),
            };
            editor.world_normal.shape(gradients, gray_blend)
        });
    }
    if editor.position.opacity > 0.0 {
        // 0 at the model's minimum and 1 at its maximum along each axis.
        let set = tc.set_geometry;
        let extent = set.bounds.extent().to_array().map(|e| e.max(1e-12));
        editor_mix(out, editor.position.opacity, editor.position.blend, |i| {
            let p = geo.position(i);
            editor.position.shape(std::array::from_fn(|a| (p[a] * set.max_extent / extent[a]).clamp(0.0, 1.0)), gray_blend)
        });
    }
    if editor.thickness.opacity > 0.0 {
        let map = read(MeshMapKind::Thickness);
        editor_mix(out, editor.thickness.opacity, subcraft_doc::BlendMode::Normal, |i| editor.thickness.shape(map[i][0]));
    }
}

/// Whether the breakup reads the bundled grunge map (Painter-style Dirt / Edge wear).
fn uses_grunge_map(g: &Generator) -> bool {
    g.painter.is_some_and(|p| p.grunge_map)
}

/// The breakup field of natural edge wear: the grunge map or the organic volume noise.
fn breakup_field(g: &Generator, params: &subcraft_doc::ProceduralParams, offset: [f32; 3], geo: &subcraft_materials::sample::TileGeometry) -> std::sync::Arc<subcraft_materials::sample::field::Field> {
    if uses_grunge_map(g) {
        subcraft_materials::sample::grunge_tile_at(params, offset, geo)
    } else {
        subcraft_materials::sample::organic_tile_at(params, offset, geo)
    }
}

/// The generator's noise offset, with anything non-finite (a damaged file) ignored.
fn noise_offset(g: &Generator) -> [f32; 3] {
    if g.offset.iter().all(|v| v.is_finite()) { g.offset } else { [0.0; 3] }
}

/// The breakup noise's parameters with Painter's grunge scale applied (unchanged without it).
fn scaled_noise(g: &Generator) -> subcraft_doc::ProceduralParams {
    let mut p = g.noise;
    if let Some(pc) = g.painter {
        p.scale = (p.scale * pc.grunge_scale.clamp(0.05, 64.0)).clamp(0.01, 256.0);
    }
    p
}

/// Noise six times finer than the breakup pattern (same seed family), in object space.
pub(crate) fn fine_noise(g: &Generator, geo: &subcraft_materials::sample::TileGeometry) -> std::sync::Arc<subcraft_materials::sample::field::Field> {
    let mut p = scaled_noise(g);
    p.scale = (p.scale * 6.0).clamp(0.01, 256.0);
    p.seed = p.seed.wrapping_add(0x9e37);
    subcraft_materials::sample::organic_tile(&p, geo)
}

fn eval_edge_spread(g: &Generator, spread: subcraft_doc::EdgeSpread, tc: &TileContext, out: &mut [[f32; 4]], input: Option<&[f32]>) {
    let distances = tc.set_geometry.edge_distance_tile(tc.geo);
    // Variation changes the boundary even at zero Breakup. Breakup additionally erodes patches;
    // both share one cached non-periodic volume field and the existing Pattern size/Seed.
    let offset = noise_offset(g);
    let noise_params = scaled_noise(g);
    let noise = breakup_field(g, &noise_params, offset, tc.geo);
    let painter = g.painter;
    let mut ao_map = Vec::new();
    if painter.is_some_and(|p| p.ao_masking > 0.0) {
        ao_map = vec![[0.0; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(MeshMapKind::AmbientOcclusion, tc.geo, &mut ao_map);
    }
    let curvature_weight = painter.map_or(1.0, |p| p.curvature_weight.clamp(0.0, 2.0));
    let ao_masking = painter.map_or(0.0, |p| p.ao_masking.clamp(0.0, 1.0));
    let mut curvature = vec![[0.0; 3]; TILE_TEXELS];
    match input {
        Some(values) => curvature.iter_mut().zip(values).for_each(|(c, &v)| *c = [v; 3]),
        None => tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, tc.geo, &mut curvature),
    }
    // Spread is a physical reach, independent of the baked-detail threshold. Multiplying by
    // Level made even maximum Spread almost disappear at the owner's Level 0.065 setting.
    let radius = spread.distance.clamp(0.0, 12.5) * 0.01;
    // Level is the amount of wear: how much of that reach is uncovered (old projects: all of it).
    let extent = if spread.level_reach { subcraft_doc::EdgeSpread::level_extent(g.level) } else { 1.0 };
    let variation = spread.variation.clamp(0.0, 1.0);
    // Pattern size alone wobbles over ~12% of the model, so a narrow band read as a straight
    // stripe. A second field four times finer chips the boundary. It follows Pattern size and
    // Seed but not Spread, so Spread drags reuse the cached field; tiles beyond any possible
    // reach skip it. Variation zero keeps the clean geometric band.
    let widest = radius * (1.0 + variation * 0.9);
    let chips = (variation > 0.0 && radius > 0.0 && distances.iter().any(|&d| d < widest)).then(|| {
        let mut p = noise_params;
        p.scale = (noise_params.scale * 4.0).clamp(0.01, 256.0);
        p.seed = p.seed.wrapping_add(0x5bd1);
        subcraft_materials::sample::organic_tile_at(&p, offset, tc.geo)
    });
    let breakup = g.noise_amount.clamp(0.0, 1.0);
    let grunge_map = uses_grunge_map(g);
    let grain = painter.map_or(0.0, |p| p.grain.clamp(0.0, 1.0)) * (breakup * 4.0).clamp(0.0, 1.0);
    let fine = (grain > 0.0).then(|| fine_noise(g, tc.geo));
    let softness = (g.softness + painter.map_or(0.0, |p| p.smoothness.clamp(0.0, 1.0))).clamp(0.02, 1.0);
    for (i, o) in out.iter_mut().enumerate() {
        let n = noise[i];
        // Fractal volume values cluster near 0.5. Expand their useful range so Variation has
        // visible width changes, rather than an almost straight band at small Spread values.
        let coarse = ((n - 0.5) * 3.0).clamp(-0.5, 0.5);
        let boundary = chips.as_ref().map_or(coarse, |c| coarse * 0.5 + ((c[i] - 0.5) * 4.0).clamp(-0.5, 0.5) * 0.5);
        let reach = radius * (1.0 + variation * boundary * 1.8) * (1.0 - breakup * 0.7 * (1.0 - n));
        let reach = reach * extent;
        // Fine grain erodes the band edge into chips and specks.
        let reach = match &fine { Some(f) => reach * (1.0 - grain * 0.65 * (1.0 - ((f[i] - 0.5) * 2.5 + 0.5).clamp(0.0, 1.0))), None => reach };
        let geometric = if reach > 0.0 { 1.0 - smoothstep(reach * (1.0 - softness), reach, distances[i]) } else { 0.0 };
        // Keep baked rounded/high-poly detail, but explicitly exclude the neutral flat baseline.
        // Geometric sources are independent of contact curvature from other crate components.
        let convex = (((curvature[i][0] - 0.5) * 2.0) * curvature_weight).clamp(0.0, 1.0);
        // Coarse only: chips are skipped on tiles out of reach, and must not seam baked detail.
        let threshold = 1.0 - g.level + (n - 0.5) * breakup + coarse * variation * 0.5;
        let baked = if convex > 1e-4 && g.level > 0.0 {
            smoothstep(threshold - softness * 0.5, threshold + softness * 0.5, convex)
        } else { 0.0 };
        let mut holes = if breakup > 0.0 { smoothstep(breakup * 0.65 - 0.2, breakup * 0.65, n) } else { 1.0 };
        if grunge_map && holes < 1.0 && distances[i] < reach * 0.4 {
            // Painter's wear keeps the edge apex worn and lets the grunge eat the band behind it:
            // holes fade in over the first 40% of the reach, so long rims never stop and start.
            holes = 1.0 - (1.0 - holes) * smoothstep(0.0, reach * 0.4, distances[i]);
        }
        let mut v = geometric.max(baked) * holes;
        if ao_masking > 0.0 {
            v *= 1.0 - ao_masking * (1.0 - ao_map[i][0]);
        }
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}

/// Image mask effect. With alpha: value 1 (0 inverted) with coverage = alpha, so a logo on a
/// transparent background stamps onto the mask. Without alpha: value = Rec.709 luminance of the
/// raw (not color-transformed) pixels. A missing resource contributes nothing.
fn eval_image(resource: subcraft_core::ResourceId, projection: &Projection, invert: bool, tc: &TileContext, e: &mut [[f32; 4]]) {
    let Some(bmp) = tc.sample.resources.bitmap(resource) else {
        e.fill([0.0; 4]);
        return;
    };
    let has_alpha = bmp.channels == 2 || bmp.channels == 4;
    let mut sampled = vec![[0.0f32; 4]; TILE_TEXELS];
    subcraft_materials::sample::bitmap_tile(bmp, false, projection, false, tc.geo, &mut sampled);
    let cover = subcraft_materials::sample::coverage_tile(projection, tc.geo);
    for (i, (o, s)) in e.iter_mut().zip(&sampled).enumerate() {
        // Outside a non-repeating / cropped projection the image contributes nothing.
        if cover.as_ref().is_some_and(|c| c[i] == 0.0) {
            *o = [0.0; 4];
            continue;
        }
        let (v, a) = if has_alpha { (1.0, s[3]) } else { (0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2], 1.0) };
        *o = [if invert { 1.0 - v } else { v }, a.clamp(0.0, 1.0), 0.0, 0.0];
    }
}

/// ID Color Mask: the baked ID map compared against the picked colors. Without a baked ID map
/// the effect is opaque black (not inverted: nothing can be selected), and the UI says so.
fn eval_id_color(id: &subcraft_doc::IdColorMask, tc: &TileContext, e: &mut [[f32; 4]]) {
    if tc.sample.mesh_maps.get(MeshMapKind::Id).is_none() {
        e.fill([0.0, 1.0, 0.0, 0.0]);
        return;
    }
    let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
    tc.sample.mesh_maps.read_tile(MeshMapKind::Id, tc.geo, &mut map);
    for (o, rgb) in e.iter_mut().zip(&map) {
        *o = [id.value(*rgb), 1.0, 0.0, 0.0];
    }
}

/// One mask effect's `[value, coverage, _, _]` per texel. `input` is the effect's anchor input as
/// gray (ADR-0045), when it has a resolved one.
pub(crate) fn evaluate_effect(kind: &MaskEffectKind, tc: &TileContext, m: &[f32], e: &mut [[f32; 4]], input: Option<&[f32]>) {
        match kind {
            MaskEffectKind::Paint { image } => {
                read_tile_rgba(image, tc.geo, e);
            }
            MaskEffectKind::Fill { value, procedural, projection } => {
                let field = procedural.as_ref().map(|p| procedural_tile(p, projection, tc.geo));
                // A uniform value has no texture to wrap or crop; a procedural does.
                let cover = field.as_ref().and_then(|_| subcraft_materials::sample::coverage_tile(projection, tc.geo));
                for (i, o) in e.iter_mut().enumerate() {
                    let v = field.as_ref().map_or(*value, |f| f[i]);
                    *o = [v, cover.as_ref().map_or(1.0, |c| c[i]), 0.0, 0.0];
                }
            }
            MaskEffectKind::Generator(g) => eval_generator(g, tc, e, input),
            MaskEffectKind::Anchor => e.fill([0.0; 4]),
            MaskEffectKind::AnchorRef { levels, .. } => match input {
                Some(values) => {
                    for (o, &v) in e.iter_mut().zip(values) {
                        *o = [levels.map_or(v, |l| l.apply(v)), 1.0, 0.0, 0.0];
                    }
                }
                None => e.fill([0.0; 4]),
            },
            MaskEffectKind::Levels(l) => {
                for (o, &v) in e.iter_mut().zip(m) {
                    *o = [l.apply(v), 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Filter(filter) => {
                let usable = filter.valid() && filter.supports_mask();
                for (o, &value) in e.iter_mut().zip(m) {
                    *o = [if usable { subcraft_materials::filter::apply_mask(*filter, value) } else { value }, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Invert => {
                for (o, &v) in e.iter_mut().zip(m) {
                    *o = [1.0 - v, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Image { resource, projection, invert } => eval_image(*resource, projection, *invert, tc, e),
            MaskEffectKind::IdColor(id) => eval_id_color(id, tc, e),
        }
}
