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
    if g.kind.uses_shape() {
        eval_shape_generator(g, tc, out);
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
        GeneratorKind::Distance3D | GeneratorKind::LinearGradient3D | GeneratorKind::UvRandomColor | GeneratorKind::GrayscaleConversion | GeneratorKind::UvBorderDistance | GeneratorKind::UvChecker | GeneratorKind::UvTexelDensity => None,
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
    let attenuation = g.shape.map_or(0.0, |s| s.attenuation.clamp(0.0, 1.0));
    let extent = set.bounds.extent().to_array().map(|e| e.max(1e-12));
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
    let custom = custom_grunge(g, &noise_params, offset, tc);
    let noise: Option<Vec<f32>> = (g.noise_amount > 0.0).then(|| if let Some(c) = &custom {
        c.clone()
    } else if uses_grunge_map(g) && g.kind != GeneratorKind::Edges {
        subcraft_materials::sample::grunge_tile_at(&noise_params, offset, geo).to_vec()
    } else if g.organic_breakup {
        subcraft_materials::sample::organic_tile_at(&noise_params, offset, geo).to_vec()
    } else { subcraft_materials::sample::procedural_tile_at(&noise_params, &noise_proj, offset, geo).to_vec() });
    // Painter's dirt: convex edges are kept clear of dirt by `edges_masking`.
    let edges_masking = g.painter.map_or(0.0, |p| p.edges_masking.clamp(0.0, 1.0));
    let mut curvature = Vec::new();
    if edges_masking > 0.0 && g.kind == GeneratorKind::Cavity {
        curvature = vec![[0.0f32; 3]; TILE_TEXELS];
        tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, geo, &mut curvature);
    }
    let floor = g.painter.map_or(0.0, |p| p.grunge_floor.clamp(0.0, 1.0));
    let grunge_map = (uses_grunge_map(g) || custom.is_some()) && g.kind != GeneratorKind::Edges;
    // Grain speckles dirt's threshold; the plain Edges threshold (no spread model) keeps its exact look.
    // Grain is part of Breakup: it fades in over the first quarter of the Breakup slider, so
    // Breakup 0 is exactly smooth whatever the Grain value.
    let grain = if g.kind == GeneratorKind::Edges { 0.0 } else { g.painter.map_or(0.0, |p| p.grain.clamp(0.0, 1.0)) * (g.noise_amount * 4.0).clamp(0.0, 1.0) };
    let fine = (grain > 0.0).then(|| fine_noise(g, geo));
    // Curvature generator mode and detail sizes; absent or default: the baked map itself.
    let mut detail = Vec::new();
    if g.kind == GeneratorKind::MeshMap(MeshMapKind::Curvature) && let Some(d) = g.curvature.filter(|d| !d.is_identity()) {
        detail = vec![0.0f32; TILE_TEXELS];
        // With an anchor input the grids would describe the baked map, not the input: fine and mode only.
        let source = if input.is_some() { None } else { tc.sample.mesh_maps.get(MeshMapKind::Curvature) };
        crate::curvature::detail_values(&d, source, geo, &map, &mut detail);
    }
    for (i, o) in out.iter_mut().enumerate() {
        let raw = match g.kind {
            GeneratorKind::MeshMap(_) if !detail.is_empty() => detail[i],
            GeneratorKind::Edges | GeneratorKind::MeshMap(_) => map[i][0],
            GeneratorKind::Cavity if !curvature.is_empty() => (1.0 - map[i][0]) * (1.0 - edges_masking * ((curvature[i][0] - 0.5) * 2.0).clamp(0.0, 1.0)),
            GeneratorKind::Cavity | GeneratorKind::AmbientOcclusion => 1.0 - map[i][0],
            GeneratorKind::Directional => Vec3::from_array(geo.normal(i)).dot(dir) * 0.5 + 0.5,
            GeneratorKind::Concave => 1.0 - map[i][0],
            GeneratorKind::PositionGradient => (geo.position(i)[axis] * set.max_extent / ext_axis).clamp(0.0, 1.0),
            // Handled above.
            GeneratorKind::MaskEditor | GeneratorKind::Distance3D | GeneratorKind::LinearGradient3D | GeneratorKind::UvRandomColor | GeneratorKind::GrayscaleConversion | GeneratorKind::UvBorderDistance | GeneratorKind::UvChecker | GeneratorKind::UvTexelDensity => 0.0,
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
        // Light: attenuation fades the light toward the side of the model facing away from it.
        if g.kind == GeneratorKind::Directional && attenuation > 0.0 {
            let p = geo.position(i);
            // A flat axis (a plane) sits at the box centre.
            let centred: [f32; 3] = std::array::from_fn(|a| if extent[a] > set.max_extent * 1e-6 { p[a] * set.max_extent / extent[a] - 0.5 } else { 0.0 });
            // 1 at the lit end of the box, 0 at the far end (the half-diagonal spans 0.5).
            let t = ((centred[0] * dir.x + centred[1] * dir.y + centred[2] * dir.z) / 0.866_025_4 * 0.5 + 0.5).clamp(0.0, 1.0);
            v *= 1.0 - attenuation * (1.0 - t);
        }
        if floor > 0.0 && let Some(noise) = &noise {
            // Contrast-stretched around the mean: Painter's tail spans 0.02 to 0.18, not a narrow band.
            // The grunge map already spans the range: use it as is, minus its darkest third.
            let tail = if grunge_map { smoothstep(0.3, 1.0, noise[i]) } else { ((noise[i] - 0.5) * 3.0 + 0.5).clamp(0.0, 1.0) };
            v += (1.0 - v) * floor * tail;
        }
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}

/// The position, UV and conversion generators ([`GeneratorKind::uses_shape`]).
fn eval_shape_generator(g: &Generator, tc: &TileContext, out: &mut [[f32; 4]]) {
    let s = g.shape_settings();
    let geo = tc.geo;
    let set = tc.set_geometry;
    let extent = set.bounds.extent().to_array().map(|e| e.max(1e-12));
    // Box coordinates (0..1 per axis) to positions in units of the model's longest side.
    let to_units = |b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|a| b[a] * extent[a] / set.max_extent.max(1e-12)) };
    let finish = |v: f32| {
        let v = subcraft_doc::mask::balance_contrast(v, s.balance, s.contrast);
        if g.invert { 1.0 - v } else { v }
    };
    match g.kind {
        GeneratorKind::Distance3D => {
            let c = to_units(s.point);
            for (i, o) in out.iter_mut().enumerate() {
                let p = geo.position(i);
                let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt();
                *o = [finish(s.distance_value(d)), 1.0, 0.0, 0.0];
            }
        }
        GeneratorKind::LinearGradient3D => {
            let (a, b) = (to_units(s.start), to_units(s.end));
            for (i, o) in out.iter_mut().enumerate() {
                *o = [finish(s.gradient_value(geo.position(i), a, b)), 1.0, 0.0, 0.0];
            }
        }
        GeneratorKind::UvRandomColor => {
            for (i, o) in out.iter_mut().enumerate() {
                let v = geo.region(i, subcraft_core::PartKind::UvIsland).map_or(0.0, |island| s.island_value(island));
                *o = [finish(v), 1.0, 0.0, 0.0];
            }
        }
        GeneratorKind::GrayscaleConversion => {
            let Some(bmp) = s.texture.and_then(|id| tc.sample.resources.bitmap(id)) else {
                out.fill([finish(0.0), 1.0, 0.0, 0.0]);
                return;
            };
            let projection = Projection { mode: ProjectionMode::Uv, scale: [s.tile; 2], rotation_deg: s.effective_rotation(), ..Projection::default() };
            let mut px = vec![[0.0f32; 4]; TILE_TEXELS];
            subcraft_materials::sample::bitmap_tile(bmp, false, &projection, false, geo, &mut px);
            for (o, p) in out.iter_mut().zip(&px) {
                *o = [finish(s.grayscale.convert([p[0], p[1], p[2]])), 1.0, 0.0, 0.0];
            }
        }
        GeneratorKind::UvBorderDistance => {
            let d = border_distance(tc);
            for (o, px) in out.iter_mut().zip(&d) {
                *o = [finish(s.border_value(*px)), 1.0, 0.0, 0.0];
            }
        }
        GeneratorKind::UvChecker | GeneratorKind::UvTexelDensity => {
            // In a mask: the brightness of the colour the generator gives on content.
            let colors = generator_color(g, tc).expect("colour generator");
            for (o, c) in out.iter_mut().zip(&colors) {
                *o = [finish(0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]), 1.0, 0.0, 0.0];
            }
        }
        _ => unreachable!("not a shape generator"),
    }
}

/// The colour a generator gives on a layer's content, for the kinds that have one
/// ([`GeneratorKind::has_color`]): UV Random Color, UV Checker and UV Texel Density.
pub(crate) fn generator_color(g: &Generator, tc: &TileContext) -> Option<Vec<[f32; 3]>> {
    if !g.kind.has_color() {
        return None;
    }
    let s = g.shape_settings();
    let geo = tc.geo;
    let island = |i: usize| geo.region(i, subcraft_core::PartKind::UvIsland);
    let invert = |c: [f32; 3]| if g.invert { c.map(|v| 1.0 - v) } else { c };
    Some(match g.kind {
        GeneratorKind::UvRandomColor => (0..TILE_TEXELS).map(|i| invert(island(i).map_or([0.0; 3], |id| s.island_color(id)))).collect(),
        GeneratorKind::UvChecker => {
            let border = border_distance(tc);
            (0..TILE_TEXELS)
                .map(|i| {
                    let base = if s.checker_random { island(i).map_or(s.checker_color, |id| s.island_color(id)) } else { s.checker_color };
                    invert(s.checker_color_at(geo.uv(i), base, border[i]))
                })
                .collect()
        }
        _ => {
            // Texel density: texels per unit of model length, relative to the texture spanning
            // the model's longest side once (the shorter step to a covered neighbour, so a UV
            // seam next to the texel does not count).
            let n = subcraft_image::TILE_SIZE as usize;
            (0..TILE_TEXELS)
                .map(|i| {
                    let (x, y) = (i % n, i / n);
                    let p = geo.position(i);
                    let step = |j: usize| (geo.coverage(j) > 0.0).then(|| { let q = geo.position(j); ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt() });
                    let dx = step(if x + 1 < n { i + 1 } else { i - 1 });
                    let dy = step(if y + 1 < n { i + n } else { i - n });
                    let d = match (dx, dy) { (Some(a), Some(b)) => a.min(b), (Some(a), None) | (None, Some(a)) => a, (None, None) => 0.0 };
                    let relative = if d > 0.0 { 1.0 / (d * geo.resolution as f32) } else { 0.0 };
                    invert(s.density_color(subcraft_doc::ShapeSettings::density_position(relative)))
                })
                .collect()
        }
    })
}

/// Distance in pixels from every texel of the tile to the nearest UV island border, measured
/// over this tile and its eight neighbours (so it saturates one tile away). A border texel is a
/// covered texel next to another island or to nothing; uncovered texels read 0.
pub(crate) fn border_distance(tc: &TileContext) -> Vec<f32> {
    let n = subcraft_image::TILE_SIZE as i64;
    let set = tc.set_geometry;
    let (tiles_x, res) = (set.tiles_x as i64, set.resolution as i64);
    let tiles_y = (res + n - 1) / n;
    let (tx, ty) = (tc.geo.x0 as i64 / n, tc.geo.y0 as i64 / n);
    let w = (3 * n) as usize;
    // Island id + 1 per texel of the 3 x 3 tile window (0: nothing there).
    let mut ids = vec![0u32; w * w];
    for dy in -1..=1i64 {
        for dx in -1..=1i64 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= tiles_x || ny >= tiles_y {
                continue;
            }
            let tile = if dx == 0 && dy == 0 { None } else { Some(set.tile((ny * tiles_x + nx) as usize)) };
            let geo: &subcraft_materials::sample::TileGeometry = tile.as_deref().unwrap_or(tc.geo);
            let (ox, oy) = (((dx + 1) * n) as usize, ((dy + 1) * n) as usize);
            for i in 0..TILE_TEXELS {
                let (x, y) = (i % n as usize, i / n as usize);
                if (nx * n + x as i64) < res && (ny * n + y as i64) < res {
                    ids[(oy + y) * w + ox + x] = geo.region(i, subcraft_core::PartKind::UvIsland).map_or(0, |id| id + 1);
                }
            }
        }
    }
    let at = |x: i64, y: i64| if x < 0 || y < 0 || x >= w as i64 || y >= w as i64 { 0 } else { ids[y as usize * w + x as usize] };
    let border: Vec<bool> = (0..w * w)
        .map(|i| {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let id = ids[i];
            id != 0 && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| at(x + dx, y + dy) != id)
        })
        .collect();
    let d2 = subcraft_materials::filter::contour::squared_distance(&border, w, w, true);
    (0..TILE_TEXELS)
        .map(|i| {
            let (x, y) = (i % n as usize + n as usize, i / n as usize + n as usize);
            let at = y * w + x;
            if ids[at] == 0 { 0.0 } else { d2[at].sqrt().min(subcraft_doc::ShapeSettings::MAX_BORDER_DISTANCE) }
        })
        .collect()
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

/// Painter's custom grunge: the generator's image projected tri-planar on the model, the
/// pattern size setting how often it repeats. `None` without one (or when it is missing).
fn custom_grunge(g: &Generator, params: &subcraft_doc::ProceduralParams, offset: [f32; 3], tc: &TileContext) -> Option<Vec<f32>> {
    let bmp = tc.sample.resources.bitmap(g.painter?.custom_grunge?)?;
    let geo = tc.geo;
    let projection = Projection { mode: ProjectionMode::Triplanar, scale: [params.scale; 2], triplanar_sharpness: 4.0, ..Projection::default() };
    let sc = projection.rotation_sin_cos();
    Some(
        (0..TILE_TEXELS)
            .map(|i| {
                let p = geo.position(i);
                let p = [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]];
                let s = subcraft_materials::sample::sample_projected_sc(&projection, sc, geo.uv(i), p, geo.normal(i), |q| bmp.sample(q[0], q[1], subcraft_image::Wrap::Repeat, false));
                0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2]
            })
            .collect(),
    )
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
    let noise = custom_grunge(g, &noise_params, offset, tc).unwrap_or_else(|| breakup_field(g, &noise_params, offset, tc.geo).to_vec());
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
        let (v, a) = id.value_coverage(*rgb);
        *o = [v, a, 0.0, 0.0];
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
            MaskEffectKind::Filter(subcraft_doc::Filter::GradientDynamic { source, vertical, position }) => {
                let bmp = tc.sample.resources.bitmap(*source);
                for (o, &value) in e.iter_mut().zip(m) {
                    let v = bmp.map_or(value, |b| {
                        let c = crate::util::gradient_dynamic(b, value, *vertical, *position, false);
                        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
                    });
                    *o = [v, 1.0, 0.0, 0.0];
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
            // Without the layer stack (local previews), the layers read as 0.
            MaskEffectKind::Compare(c) => eval_compare(c, m, None, None, e),
        }
}

/// Compare Mask: `[value, 1]` per texel from its two operands. `below` / `own` are the compared
/// channel of the layers below and of this layer as gray (0 when not given).
pub(crate) fn eval_compare(c: &subcraft_doc::CompareMask, m: &[f32], below: Option<&[f32]>, own: Option<&[f32]>, e: &mut [[f32; 4]]) {
    use subcraft_doc::CompareOperand as O;
    let operand = |o: O, i: usize| match o {
        O::LayersBelow => below.map_or(0.0, |b| b[i]),
        O::ThisLayer => own.map_or(0.0, |b| b[i]),
        O::ThisMask => m[i],
        O::Constant => c.constant,
    };
    for (i, o) in e.iter_mut().enumerate() {
        *o = [c.eval(operand(c.left, i), operand(c.right, i)), 1.0, 0.0, 0.0];
    }
}
