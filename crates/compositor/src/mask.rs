//! Mask evaluation: base raster followed by the effect stack.
//!
//! Each visible effect produces a value `e` and coverage `ea` per texel and is composited onto
//! the running mask with its blend mode and opacity (`m = lerp(m, B(m, e), ea·opacity)`).

use glam::Vec3;
use subcraft_core::MeshMapKind;
use subcraft_doc::{Generator, GeneratorKind, Mask, MaskEffectKind, Projection, ProjectionMode};
use subcraft_image::TILE_TEXELS;
use subcraft_image::Wrap;
use subcraft_materials::sample::{procedural_tile, sample_projected};

use crate::TileContext;
use crate::blend::{BlendTarget, composite};
use crate::util::read_tile_rgba;

/// Mask values (0..1) for every texel of the tile, or `None` when the mask is absent/disabled.
pub fn eval_mask(mask: Option<&Mask>, tc: &TileContext) -> Option<Vec<f32>> {
    eval_mask_scaled(mask,tc,1.0)
}

/// Reduced/enlarged UV inspection scales only neighborhood radii, never document data.
pub fn eval_mask_scaled(mask: Option<&Mask>, tc: &TileContext, radius_scale:f32) -> Option<Vec<f32>> {
    let mask = mask.filter(|m| m.enabled)?;
    if crate::stages::mask_spatial(mask) { return Some(crate::stages::mask_preview(mask,tc,radius_scale)); }
    let mut px = vec![[0.0f32; 4]; TILE_TEXELS];
    read_tile_rgba(&mask.base, tc.geo, &mut px);
    let mut m: Vec<f32> = px.iter().map(|p| p[0]).collect();
    let mut e = vec![[0.0f32; 4]; TILE_TEXELS];
    for effect in mask.effects.iter().filter(|e| e.visible && e.opacity > 0.0) {
        // e[i] = [value, alpha, _, _]
        evaluate_effect(&effect.kind, tc, &m, &mut e);
        for (mv, ev) in m.iter_mut().zip(&e) {
            let mut dst = [*mv, *mv, *mv, 1.0];
            composite(&mut dst, [ev[0]; 3], ev[1] * effect.opacity, effect.blend, BlendTarget::MASK);
            *mv = dst[0];
        }
    }
    for v in m.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
    Some(m)
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Generator coverage: a mesh-derived raw value thresholded at `level` (optionally broken up by
/// noise) with a `softness`-wide transition.
fn eval_generator(g: &Generator, tc: &TileContext, out: &mut [[f32; 4]]) {
    if g.kind == GeneratorKind::Edges && let Some(spread) = g.edge_spread {
        eval_edge_spread(g, spread, tc, out);
        return;
    }
    let geo = tc.geo;
    let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
    let map_kind = match g.kind {
        GeneratorKind::Edges | GeneratorKind::Concave => Some(MeshMapKind::Curvature),
        GeneratorKind::Cavity => Some(MeshMapKind::AmbientOcclusion),
        GeneratorKind::MeshMap(k) => Some(k),
        GeneratorKind::Directional | GeneratorKind::PositionGradient => None,
    };
    if let Some(k) = map_kind {
        tc.sample.mesh_maps.read_tile(k, geo, &mut map);
    }
    let dir = Vec3::from_array(g.direction).normalize_or(Vec3::Y);
    let set = tc.set_geometry;
    let axis = g.axis.index();
    let ext_axis = set.bounds.extent()[axis].max(1e-12);
    let half = g.softness.max(1e-4) * 0.5;
    // Breakup noise is projected in 3D (triplanar on object position), never in UV: UV noise
    // jumps at every seam and would make wear follow the islands instead of the shape.
    let noise_proj = Projection { mode: ProjectionMode::Triplanar, triplanar_sharpness: 4.0, ..Projection::default() };
    let noise_params = scaled_noise(g);
    let noise = (g.noise_amount > 0.0).then(|| if g.organic_breakup {
        subcraft_materials::sample::organic_tile(&noise_params, geo)
    } else { procedural_tile(&noise_params, &noise_proj, geo) });
    // Painter's dirt: convex edges are kept clear of dirt by `edges_masking`.
    let edges_masking = g.painter.map_or(0.0, |p| p.edges_masking.clamp(0.0, 1.0));
    let mut curvature = Vec::new();
    if edges_masking > 0.0 && g.kind == GeneratorKind::Cavity {
        curvature = vec![[0.0f32; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, geo, &mut curvature);
    }
    let floor = g.painter.map_or(0.0, |p| p.grunge_floor.clamp(0.0, 1.0));
    // Grain speckles dirt's threshold; the plain Edges threshold (no spread model) keeps its exact look.
    let grain = if g.kind == GeneratorKind::Edges { 0.0 } else { g.painter.map_or(0.0, |p| p.grain.clamp(0.0, 1.0)) };
    let fine = (grain > 0.0).then(|| fine_noise(g, geo));
    for (i, o) in out.iter_mut().enumerate() {
        let raw = match g.kind {
            GeneratorKind::Edges | GeneratorKind::MeshMap(_) => map[i][0],
            GeneratorKind::Cavity if !curvature.is_empty() => (1.0 - map[i][0]) * (1.0 - edges_masking * ((curvature[i][0] - 0.5) * 2.0).clamp(0.0, 1.0)),
            GeneratorKind::Cavity => 1.0 - map[i][0],
            GeneratorKind::Directional => Vec3::from_array(geo.normal(i)).dot(dir) * 0.5 + 0.5,
            GeneratorKind::Concave => 1.0 - map[i][0],
            GeneratorKind::PositionGradient => (geo.position(i)[axis] * set.max_extent / ext_axis).clamp(0.0, 1.0),
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
            v += (1.0 - v) * floor * ((noise[i] - 0.5) * 3.0 + 0.5).clamp(0.0, 1.0);
        }
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
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
fn fine_noise(g: &Generator, geo: &subcraft_materials::sample::TileGeometry) -> std::sync::Arc<subcraft_materials::sample::field::Field> {
    let mut p = scaled_noise(g);
    p.scale = (p.scale * 6.0).clamp(0.01, 256.0);
    p.seed = p.seed.wrapping_add(0x9e37);
    subcraft_materials::sample::organic_tile(&p, geo)
}

fn eval_edge_spread(g: &Generator, spread: subcraft_doc::EdgeSpread, tc: &TileContext, out: &mut [[f32; 4]]) {
    let distances = tc.set_geometry.edge_distance_tile(tc.geo);
    // Variation changes the boundary even at zero Breakup. Breakup additionally erodes patches;
    // both share one cached non-periodic volume field and the existing Pattern size/Seed.
    let noise_params = scaled_noise(g);
    let noise = subcraft_materials::sample::organic_tile(&noise_params, tc.geo);
    let painter = g.painter;
    let mut ao_map = Vec::new();
    if painter.is_some_and(|p| p.ao_masking > 0.0) {
        ao_map = vec![[0.0; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(MeshMapKind::AmbientOcclusion, tc.geo, &mut ao_map);
    }
    let curvature_weight = painter.map_or(1.0, |p| p.curvature_weight.clamp(0.0, 2.0));
    let ao_masking = painter.map_or(0.0, |p| p.ao_masking.clamp(0.0, 1.0));
    let mut curvature = vec![[0.0; 3]; TILE_TEXELS];
    tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, tc.geo, &mut curvature);
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
        subcraft_materials::sample::organic_tile(&p, tc.geo)
    });
    let breakup = g.noise_amount.clamp(0.0, 1.0);
    let grain = painter.map_or(0.0, |p| p.grain.clamp(0.0, 1.0));
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
        let holes = if breakup > 0.0 { smoothstep(breakup * 0.65 - 0.2, breakup * 0.65, n) } else { 1.0 };
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
    let geo = tc.geo;
    for (i, o) in e.iter_mut().enumerate() {
        let s = sample_projected(projection, geo.uv(i), geo.position(i), geo.normal(i), |p| bmp.sample(p[0], p[1], Wrap::Repeat, false));
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

pub(crate) fn evaluate_effect(kind: &MaskEffectKind, tc: &TileContext, m: &[f32], e: &mut [[f32; 4]]) {
        match kind {
            MaskEffectKind::Paint { image } => {
                read_tile_rgba(image, tc.geo, e);
            }
            MaskEffectKind::Fill { value, procedural, projection } => {
                let field = procedural.as_ref().map(|p| procedural_tile(p, projection, tc.geo));
                for (i, o) in e.iter_mut().enumerate() {
                    let v = field.as_ref().map_or(*value, |f| f[i]);
                    *o = [v, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Generator(g) => eval_generator(g, tc, e),
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
