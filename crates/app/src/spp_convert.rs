//! Convert Substance Painter smart-material / mask layer trees into SubCraft layers.
#![allow(dead_code)]

use std::sync::Arc;
use subcraft_core::ChannelKind;
use subcraft_doc::{Layer, TextureSet};

fn spp_blend_mode_from_str(s: &str) -> subcraft_doc::BlendMode {
    use subcraft_doc::BlendMode;
    match s {
        "disable" => BlendMode::Disable,
        "passthrough" => BlendMode::Passthrough,
        "replace" => BlendMode::Replace,
        "multiply" => BlendMode::Multiply,
        "add" => BlendMode::LinearDodge,
        "subtract" => BlendMode::Subtract,
        "add_sub" => BlendMode::SignedAddition,
        "min" => BlendMode::Darken,
        "max" => BlendMode::Lighten,
        "overlay" => BlendMode::Overlay,
        "screen" => BlendMode::Screen,
        "soft_light" => BlendMode::SoftLight,
        "hard_light" => BlendMode::HardLight,
        "vivid_light" => BlendMode::VividLight,
        "linear_light" => BlendMode::LinearLight,
        "pin_light" => BlendMode::PinLight,
        "color_dodge" => BlendMode::ColorDodge,
        "color_burn" => BlendMode::ColorBurn,
        "linear_burn" => BlendMode::LinearBurn,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "divide" => BlendMode::Divide,
        "inverse_divide" => BlendMode::InverseDivide,
        "inverse_subtract" => BlendMode::InverseSubtract,
        "color" => BlendMode::Color,
        "tint" => BlendMode::Tint,
        "saturation" => BlendMode::Saturation,
        "value" => BlendMode::Value,
        "normal_combine" => BlendMode::NormalMapCombine,
        "normal_detail" => BlendMode::NormalMapDetail,
        "normal_inverse_detail" => BlendMode::NormalMapInverseDetail,
        _ => BlendMode::Normal,
    }
}

fn apply_spp_channel_blends(layer: &mut Layer, node: &subcraft_spp::SppLayerNode) {
    use subcraft_doc::{BlendMode, ChannelBlend};

    for cb in &node.channel_blends {
        let mode = spp_blend_mode_from_str(&cb.mode);
        let opacity = cb.opacity.clamp(0.0, 1.0);
        if cb.channel == "folder" {
            if node.kind == "folder" {
                for b in layer.blend.values_mut() {
                    b.mode = mode;
                    b.opacity = opacity;
                }
            }
            continue;
        }
        let ch_kind = match cb.channel.as_str() {
            "base_color" => Some(ChannelKind::BaseColor),
            "roughness" => Some(ChannelKind::Roughness),
            "metallic" => Some(ChannelKind::Metallic),
            "height" => Some(ChannelKind::Height),
            "normal" => Some(ChannelKind::Normal),
            _ => None,
        };
        if let Some(kind) = ch_kind {
            let final_mode = if kind == ChannelKind::Normal && mode == BlendMode::Normal {
                BlendMode::NormalMapCombine
            } else {
                mode
            };
            layer.blend.insert(
                kind,
                ChannelBlend {
                    mode: final_mode,
                    opacity,
                },
            );
        }
    }
}

/// Convert a decoded `SppLayerNode` (e.g. from a `.spsm` Smart Material) into a `subcraft_doc::Layer`.
pub fn spp_node_to_layer(node: &subcraft_spp::SppLayerNode, resolution: u32) -> Layer {
    let tc = TextureSet::new(&node.name, 0, resolution);
    spp_node_to_layer_inner(node, &tc, false)
}

fn spp_node_to_layer_inner(
    node: &subcraft_spp::SppLayerNode,
    tc: &TextureSet,
    in_folder: bool,
) -> Layer {
    let mut layer = match node.kind.as_str() {
        "folder" => {
            let mut f = Layer::new_folder(&node.name, tc);
            f.mask = build_spp_mask(node, tc.resolution, false, None);
            if let subcraft_doc::LayerContent::Group(g) = &mut f.content {
                g.expanded = true;
                g.children = node
                    .children
                    .iter()
                    .map(|c| std::sync::Arc::new(spp_node_to_layer_inner(c, tc, true)))
                    .collect();
            }
            f
        }
        "paint" => {
            if node.base_color.is_some()
                || node.roughness.is_some()
                || node.metallic.is_some()
                || node.height.is_some()
                || node.base_color_proc.is_some()
                || node.roughness_proc.is_some()
                || node.height_proc.is_some()
            {
                build_spp_fill_layer(node, tc, in_folder, None)
            } else {
                let mut p = Layer::new_paint(&node.name, tc);
                p.mask = build_spp_mask(node, tc.resolution, in_folder, None);
                p
            }
        }
        _ => build_spp_fill_layer(node, tc, in_folder, None),
    };
    layer.visible = node.visible;
    apply_spp_channel_blends(&mut layer, node);
    layer
}

fn spp_pattern_to_procedural_kind(pattern: &str) -> subcraft_doc::ProceduralKind {
    use subcraft_doc::ProceduralKind;
    match pattern {
        "clouds" | "perlin" => ProceduralKind::Clouds,
        "cells" | "brick" => ProceduralKind::Cells,
        "scratches" => ProceduralKind::Scratches,
        "wood_rings" => ProceduralKind::WoodRings,
        "stripes" => ProceduralKind::Brushed,
        "checker" => ProceduralKind::Checker,
        _ => ProceduralKind::Grunge,
    }
}

fn spp_proc_to_params(proc: &subcraft_spp::SppProceduralChannel) -> subcraft_doc::ProceduralParams {
    let mut p = subcraft_doc::ProceduralParams::new(spp_pattern_to_procedural_kind(&proc.pattern));
    p.seed = proc.seed;
    p.scale = proc.scale;
    p.balance = proc.balance;
    p.contrast = proc.contrast;
    p.invert = proc.invert;
    p
}

fn build_spp_fill_layer(
    node: &subcraft_spp::SppLayerNode,
    tc: &TextureSet,
    in_folder: bool,
    geo: Option<(&SppMaskGeometry<'_>, u32)>,
) -> Layer {
    use subcraft_core::ChannelValue;
    use subcraft_doc::{BlendMode, ChannelBlend, FillSource, LayerContent, ProjectionMode};

    let mut layer = Layer::new_fill(&node.name, tc);
    // In Substance Painter, Height on fill layers defaults to Linear Dodge (Add) so height details
    // accumulate across layers unless overridden by perChannelBlending.
    layer.blend.insert(
        ChannelKind::Height,
        ChannelBlend {
            mode: BlendMode::LinearDodge,
            opacity: 1.0,
        },
    );

    if let LayerContent::Fill(fill) = &mut layer.content {
        if node.triplanar {
            fill.projection.mode = ProjectionMode::Triplanar;
        }
        if let Some(s) = node.uv_scale {
            fill.projection.scale = [s, s];
        }
        for (kind, ch) in fill.channels.iter_mut() {
            match kind {
                ChannelKind::BaseColor => {
                    if let Some(proc) = &node.base_color_proc {
                        ch.enabled = true;
                        ch.source = FillSource::Procedural {
                            params: spp_proc_to_params(proc),
                            low: ChannelValue::rgb(proc.low, proc.low, proc.low),
                            high: ChannelValue::rgb(proc.high, proc.high, proc.high),
                        };
                    } else if let Some([r, g, b]) = node.base_color {
                        ch.enabled = true;
                        ch.source = FillSource::uniform(ChannelValue::rgb(r, g, b));
                    } else {
                        ch.enabled = false;
                    }
                }
                ChannelKind::Roughness => {
                    if let Some(proc) = &node.roughness_proc {
                        ch.enabled = true;
                        ch.source = FillSource::Procedural {
                            params: spp_proc_to_params(proc),
                            low: ChannelValue::scalar(proc.low),
                            high: ChannelValue::scalar(proc.high),
                        };
                    } else if let Some(r) = node.roughness {
                        ch.enabled = true;
                        ch.source = FillSource::uniform(ChannelValue::scalar(r));
                    } else {
                        ch.enabled = false;
                    }
                }
                ChannelKind::Metallic => {
                    if let Some(m) = node.metallic {
                        ch.enabled = true;
                        ch.source = FillSource::uniform(ChannelValue::scalar(m));
                    } else {
                        ch.enabled = false;
                    }
                }
                ChannelKind::Height => {
                    if let Some(proc) = &node.height_proc {
                        ch.enabled = true;
                        ch.source = FillSource::Procedural {
                            params: spp_proc_to_params(proc),
                            low: ChannelValue::scalar(proc.low),
                            high: ChannelValue::scalar(proc.high),
                        };
                    } else if let Some(h) = node.height {
                        ch.enabled = true;
                        ch.source = FillSource::uniform(ChannelValue::scalar(h));
                    } else {
                        ch.enabled = false;
                    }
                }
                _ => {
                    ch.enabled = false;
                }
            }
        }
    }

    for cfx in &node.content_effects {
        layer.content_effects.push(std::sync::Arc::new(build_spp_content_effect(cfx, tc)));
    }

    layer.mask = build_spp_mask(node, tc.resolution, in_folder, geo);

    layer
}

fn build_spp_content_effect(
    cfx: &subcraft_spp::SppContentEffect,
    tc: &TextureSet,
) -> subcraft_doc::ContentEffect {
    use subcraft_core::{ChannelValue, EffectId};
    use subcraft_doc::{
        BlendMode, ChannelBlend, ContentEffect, ContentEffectKind, FillChannel, FillContent,
        FillSource, Projection, ProjectionMode,
    };

    let mut projection = Projection::default();
    if cfx.triplanar {
        projection.mode = ProjectionMode::Triplanar;
    }
    if let Some(s) = cfx.uv_scale {
        projection.scale = [s, s];
    }
    if let Some(rot) = cfx.rotation_deg {
        projection.rotation_deg = rot;
    }

    let mut channels = std::collections::BTreeMap::new();
    for kind in tc.channel_kinds() {
        let ch = match kind {
            ChannelKind::BaseColor => {
                if let Some(proc) = &cfx.base_color_proc {
                    FillChannel::new(FillSource::Procedural {
                        params: spp_proc_to_params(proc),
                        low: ChannelValue::rgb(proc.low, proc.low, proc.low),
                        high: ChannelValue::rgb(proc.high, proc.high, proc.high),
                    })
                } else if let Some([r, g, b]) = cfx.base_color {
                    FillChannel::new(FillSource::uniform(ChannelValue::rgb(r, g, b)))
                } else {
                    FillChannel::disabled(FillSource::uniform(kind.default_paint_value()))
                }
            }
            ChannelKind::Roughness => {
                if let Some(proc) = &cfx.roughness_proc {
                    FillChannel::new(FillSource::Procedural {
                        params: spp_proc_to_params(proc),
                        low: ChannelValue::scalar(proc.low),
                        high: ChannelValue::scalar(proc.high),
                    })
                } else if let Some(r) = cfx.roughness {
                    FillChannel::new(FillSource::uniform(ChannelValue::scalar(r)))
                } else {
                    FillChannel::disabled(FillSource::uniform(kind.default_paint_value()))
                }
            }
            ChannelKind::Metallic => {
                if let Some(m) = cfx.metallic {
                    FillChannel::new(FillSource::uniform(ChannelValue::scalar(m)))
                } else {
                    FillChannel::disabled(FillSource::uniform(kind.default_paint_value()))
                }
            }
            ChannelKind::Height => {
                if let Some(proc) = &cfx.height_proc {
                    FillChannel::new(FillSource::Procedural {
                        params: spp_proc_to_params(proc),
                        low: ChannelValue::scalar(proc.low),
                        high: ChannelValue::scalar(proc.high),
                    })
                } else if let Some(h) = cfx.height {
                    FillChannel::new(FillSource::uniform(ChannelValue::scalar(h)))
                } else {
                    FillChannel::disabled(FillSource::uniform(kind.default_paint_value()))
                }
            }
            _ => FillChannel::disabled(FillSource::uniform(kind.default_paint_value())),
        };
        channels.insert(kind, ch);
    }

    let mut blend: std::collections::BTreeMap<ChannelKind, ChannelBlend> = tc
        .channel_kinds()
        .map(|k| {
            let mode = match k {
                ChannelKind::Height => BlendMode::LinearDodge,
                ChannelKind::Normal => BlendMode::NormalMapCombine,
                _ => BlendMode::Normal,
            };
            (k, ChannelBlend::new(mode))
        })
        .collect();

    for cb in &cfx.channel_blends {
        let ch_kind = match cb.channel.as_str() {
            "base_color" => Some(ChannelKind::BaseColor),
            "roughness" => Some(ChannelKind::Roughness),
            "metallic" => Some(ChannelKind::Metallic),
            "height" => Some(ChannelKind::Height),
            "normal" => Some(ChannelKind::Normal),
            _ => None,
        };
        if let Some(kind) = ch_kind {
            let mode = spp_blend_mode_from_str(&cb.mode);
            let final_mode = if kind == ChannelKind::Normal && mode == BlendMode::Normal {
                BlendMode::NormalMapCombine
            } else {
                mode
            };
            blend.insert(
                kind,
                ChannelBlend {
                    mode: final_mode,
                    opacity: cb.opacity.clamp(0.0, 1.0),
                },
            );
        }
    }

    ContentEffect {
        id: EffectId::new(),
        name: cfx.name.clone(),
        visible: true,
        blend,
        kind: ContentEffectKind::Fill(FillContent {
            projection,
            channels,
        }),
    }
}

/// Convert a slice of `SppMaskEffect` (e.g. from a `.spmsk` Smart Mask) into `subcraft_doc::MaskEffect`s.
pub fn spp_effects_to_mask_effects(
    effects: &[subcraft_spp::SppMaskEffect],
) -> Vec<subcraft_doc::MaskEffect> {
    use subcraft_doc::{
        Generator, GeneratorKind, Levels, MaskEffect, MaskEffectKind,
        ProceduralKind, ProceduralParams, Projection, ProjectionMode,
    };
    use subcraft_spp::SppMaskEffectKind;

    let mut out = Vec::with_capacity(effects.len());
    for fx in effects {
        let blend = spp_blend_mode_from_str(&fx.blend);
        let kind = match &fx.kind {
            SppMaskEffectKind::Generator {
                generator,
                level,
                softness,
                invert,
                noise_amount,
                noise_scale,
            } => {
                let gkind = match generator.as_str() {
                    "cavity" => GeneratorKind::Cavity,
                    "directional" => GeneratorKind::Directional,
                    "position_gradient" => GeneratorKind::PositionGradient,
                    _ => GeneratorKind::Edges,
                };
                let mut g = Generator::new(gkind);
                g.level = *level;
                g.softness = *softness;
                g.invert = *invert;
                g.noise_amount = *noise_amount;
                g.noise.kind = ProceduralKind::Grunge;
                g.noise.scale = *noise_scale;
                MaskEffectKind::Generator(g)
            }
            SppMaskEffectKind::ProceduralFill {
                pattern,
                seed,
                scale,
                balance,
                contrast,
                invert,
                triplanar,
                uv_scale,
                sbsar: _,
            } => {
                let pkind = spp_pattern_to_procedural_kind(pattern.as_str());
                let mut proc_params = ProceduralParams::new(pkind);
                proc_params.seed = *seed;
                proc_params.scale = *scale;
                proc_params.balance = *balance;
                proc_params.contrast = *contrast;
                proc_params.invert = *invert;
                let mut proj = Projection::default();
                if *triplanar {
                    proj.mode = ProjectionMode::Triplanar;
                }
                // Painter UV Transformation "Skalierung / Wiederholen" → Projection tiling.
                if let Some(s) = *uv_scale {
                    proj.scale = [s, s];
                }
                MaskEffectKind::Fill {
                    value: *balance,
                    procedural: Some(proc_params),
                    projection: proj,
                }
            }
            SppMaskEffectKind::UniformFill { value } => MaskEffectKind::Fill {
                value: *value,
                procedural: None,
                projection: Projection::default(),
            },
            SppMaskEffectKind::Levels {
                in_low,
                in_high,
                gamma,
                out_low,
                out_high,
            } => MaskEffectKind::Levels(Levels {
                in_low: *in_low,
                in_high: *in_high,
                gamma: *gamma,
                out_low: *out_low,
                out_high: *out_high,
            }),
            SppMaskEffectKind::Invert => MaskEffectKind::Invert,
        };
        let mut eff = MaskEffect::new(kind);
        eff.name = fx.name.clone();
        eff.blend = blend;
        eff.opacity = fx.opacity.clamp(0.0, 1.0);
        out.push(eff);
    }
    out
}

fn build_spp_mask(
    node: &subcraft_spp::SppLayerNode,
    resolution: u32,
    in_folder: bool,
    geo: Option<(&SppMaskGeometry<'_>, u32)>,
) -> Option<subcraft_doc::Mask> {
    use std::sync::Arc;
    use subcraft_doc::{
        Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams, Projection,
    };

    if !node.has_black_mask
        && node.mask_effects.is_empty()
        && node.mask_decal_hits.is_empty()
        && node.mask_paint_strokes.is_empty()
    {
        return None;
    }

    let mut mask = Mask::new(resolution, !node.has_black_mask);

    if let Some((g, mat_slot)) = geo {
        rasterize_spp_mask_base(&mut mask, node, g, mat_slot);
    }

    for eff in spp_effects_to_mask_effects(&node.mask_effects) {
        mask.effects.push(Arc::new(eff));
    }

    if mask.effects.is_empty()
        && node.mask_decal_hits.is_empty()
        && node.mask_paint_strokes.is_empty()
        && node.has_black_mask
        && in_folder
        && node.kind != "folder"
    {
        let mut proc_params = ProceduralParams::new(ProceduralKind::Grunge);
        proc_params.scale = 6.0;
        proc_params.balance = 0.35;
        proc_params.contrast = 0.25;
        let mut eff = MaskEffect::new(MaskEffectKind::Fill {
            value: 0.25,
            procedural: Some(proc_params),
            projection: Projection::default(),
        });
        eff.opacity = 0.22;
        mask.effects.push(Arc::new(eff));
    }

    Some(mask)
}

struct SppMaskGeometry<'a> {
    mesh: &'a subcraft_mesh::Mesh,
    bvh: &'a subcraft_mesh::Bvh,
    mesh_elements: Vec<u32>,
    uv_islands: Vec<u32>,
    poly_groups: Vec<u32>,
}

impl<'a> SppMaskGeometry<'a> {
    fn new(mesh: &'a subcraft_mesh::Mesh, bvh: &'a subcraft_mesh::Bvh) -> Self {
        let mesh_elements = subcraft_mesh::topology::mesh_elements(mesh);
        let uv_islands = subcraft_mesh::topology::uv_islands(mesh);
        let poly_groups = compute_coplanar_poly_groups(mesh);
        Self {
            mesh,
            bvh,
            mesh_elements,
            uv_islands,
            poly_groups,
        }
    }
}

fn compute_coplanar_poly_groups(mesh: &subcraft_mesh::Mesh) -> Vec<u32> {
    let n = mesh.triangle_count();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            let gp = p[p[x as usize] as usize];
            p[x as usize] = gp;
            x = gp;
        }
        x
    }
    let quant = |v: glam::Vec3| -> [i32; 3] {
        [
            (v.x * 1e4).round() as i32,
            (v.y * 1e4).round() as i32,
            (v.z * 1e4).round() as i32,
        ]
    };
    let mut face_n = Vec::with_capacity(n);
    let mut edge_map: std::collections::HashMap<([i32; 3], [i32; 3]), u32> =
        std::collections::HashMap::with_capacity(n * 3);
    for t in 0..n {
        let [p0, p1, p2] = mesh.tri_positions(t);
        let fn_t = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        face_n.push(fn_t);
        let q = [quant(p0), quant(p1), quant(p2)];
        for e in 0..3 {
            let a = q[e];
            let b = q[(e + 1) % 3];
            let key = if a <= b { (a, b) } else { (b, a) };
            if let Some(&other) = edge_map.get(&key) {
                if mesh.tri_material[other as usize] == mesh.tri_material[t]
                    && face_n[other as usize].dot(fn_t) > 0.995
                {
                    let ra = find(&mut parent, t as u32);
                    let rb = find(&mut parent, other);
                    if ra != rb {
                        parent[ra as usize] = rb;
                    }
                }
            } else {
                edge_map.insert(key, t as u32);
            }
        }
    }
    for t in 0..n as u32 {
        find(&mut parent, t);
    }
    parent
}

fn unproject_pvi(m: &[f32; 16], x: f32, y: f32, z: f32) -> glam::Vec3 {
    let rx = m[0] * x + m[4] * y + m[8] * z + m[12];
    let ry = m[1] * x + m[5] * y + m[9] * z + m[13];
    let rz = m[2] * x + m[6] * y + m[10] * z + m[14];
    let rw = m[3] * x + m[7] * y + m[11] * z + m[15];
    if rw.abs() > 1e-12 {
        glam::Vec3::new(rx / rw, ry / rw, rz / rw)
    } else {
        glam::Vec3::new(rx, ry, rz)
    }
}

fn point_in_uv_tri(uv: [glam::Vec2; 3], u: f32, v: f32) -> bool {
    let e0 = (uv[1].x - uv[0].x) * (v - uv[0].y) - (uv[1].y - uv[0].y) * (u - uv[0].x);
    let e1 = (uv[2].x - uv[1].x) * (v - uv[1].y) - (uv[2].y - uv[1].y) * (u - uv[1].x);
    let e2 = (uv[0].x - uv[2].x) * (v - uv[2].y) - (uv[0].y - uv[2].y) * (u - uv[2].x);
    (e0 >= -1e-6 && e1 >= -1e-6 && e2 >= -1e-6) || (e0 <= 1e-6 && e1 <= 1e-6 && e2 <= 1e-6)
}

fn rasterize_spp_mask_base(
    mask: &mut subcraft_doc::Mask,
    node: &subcraft_spp::SppLayerNode,
    geo: &SppMaskGeometry<'_>,
    material_slot: u32,
) {
    use subcraft_core::IRect;
    use subcraft_image::{TILE_SIZE, TILE_TEXELS};
    use subcraft_mesh::uv_raster::{bary_point, raster_triangle, wrap_to_tile};

    if node.mask_decal_hits.is_empty() && node.mask_paint_strokes.is_empty() {
        return;
    }

    let res = mask.base.width();
    let r_res = res.min(1024);
    let full_rect = IRect::from_size(r_res, r_res);
    let initial_fill = if node.has_black_mask { 0.0f32 } else { 1.0f32 };
    let mut buf_low: Vec<f32> = vec![initial_fill; (r_res * r_res) as usize];
    let mut any_written = false;

    if !node.mask_decal_hits.is_empty() {
        let n_tris = geo.mesh.triangle_count();
        let mut tri_val: Vec<Option<f32>> = vec![None; n_tris];

        for hit in &node.mask_decal_hits {
            let mut seed_tris: Vec<usize> = Vec::new();
            if hit.view_type == 0 {
                if !hit.is_rect {
                    let u = hit.pos[0].rem_euclid(1.0);
                    let v = 1.0 - hit.pos[1].rem_euclid(1.0);
                    for t in geo.mesh.material_triangles(material_slot) {
                        let (uv, _) = wrap_to_tile(geo.mesh.tri_uvs(t));
                        if point_in_uv_tri(uv, u, v) {
                            seed_tris.push(t);
                        }
                    }
                } else {
                    let u0 = hit.rect[0].min(hit.rect[2]).rem_euclid(1.0);
                    let u1 = hit.rect[0].max(hit.rect[2]).rem_euclid(1.0);
                    let (u_min, u_max) = (u0.min(u1), u0.max(u1));
                    let v_a = 1.0 - hit.rect[1].rem_euclid(1.0);
                    let v_b = 1.0 - hit.rect[3].rem_euclid(1.0);
                    let (v_min, v_max) = (v_a.min(v_b), v_a.max(v_b));
                    let uc = 0.5 * (u_min + u_max);
                    let vc = 0.5 * (v_min + v_max);
                    for t in geo.mesh.material_triangles(material_slot) {
                        let (uv, _) = wrap_to_tile(geo.mesh.tri_uvs(t));
                        let c = (uv[0] + uv[1] + uv[2]) / 3.0;
                        let inside_rect = (c.x >= u_min && c.x <= u_max && c.y >= v_min && c.y <= v_max)
                            || uv
                                .iter()
                                .any(|p| p.x >= u_min && p.x <= u_max && p.y >= v_min && p.y <= v_max)
                            || point_in_uv_tri(uv, uc, vc);
                        if inside_rect {
                            seed_tris.push(t);
                        }
                    }
                }
            } else if !hit.is_rect {
                let xn = 2.0 * hit.pos[0] - 1.0;
                let yn = 2.0 * hit.pos[1] - 1.0;
                for z_near in [-1.0f32, 0.0f32] {
                    let p0 = unproject_pvi(&hit.proj_view_inv, xn, yn, z_near);
                    let p1 = unproject_pvi(&hit.proj_view_inv, xn, yn, 1.0);
                    let dir = p1 - p0;
                    if let Some(rh) = geo.bvh.intersect(p0, dir, 0.0, f32::INFINITY) {
                        let t = rh.tri as usize;
                        if geo.mesh.tri_material[t] == material_slot {
                            seed_tris.push(t);
                            break;
                        }
                    }
                }
            } else {
                let x0 = hit.rect[0].min(hit.rect[2]);
                let x1 = hit.rect[0].max(hit.rect[2]);
                let y0 = hit.rect[1].min(hit.rect[3]);
                let y1 = hit.rect[1].max(hit.rect[3]);
                let steps = if (x1 - x0) > 0.008 || (y1 - y0) > 0.008 { 5 } else { 1 };
                for iy in 0..steps {
                    let py = if steps == 1 {
                        0.5 * (y0 + y1)
                    } else {
                        y0 + (y1 - y0) * (iy as f32 / (steps - 1) as f32)
                    };
                    for ix in 0..steps {
                        let px = if steps == 1 {
                            0.5 * (x0 + x1)
                        } else {
                            x0 + (x1 - x0) * (ix as f32 / (steps - 1) as f32)
                        };
                        let xn = 2.0 * px - 1.0;
                        let yn = 2.0 * py - 1.0;
                        let p0 = unproject_pvi(&hit.proj_view_inv, xn, yn, -1.0);
                        let p1 = unproject_pvi(&hit.proj_view_inv, xn, yn, 1.0);
                        if let Some(rh) = geo.bvh.intersect(p0, p1 - p0, 0.0, f32::INFINITY) {
                            let t = rh.tri as usize;
                            if geo.mesh.tri_material[t] == material_slot {
                                seed_tris.push(t);
                            }
                        }
                    }
                }
                if (x1 - x0) > 0.01 || (y1 - y0) > 0.01 {
                    let pvi = glam::Mat4::from_cols_array(&hit.proj_view_inv);
                    if pvi.determinant().abs() > 1e-12 {
                        let vp = pvi.inverse();
                        let cam_dir = (unproject_pvi(&hit.proj_view_inv, 0.0, 0.0, 1.0)
                            - unproject_pvi(&hit.proj_view_inv, 0.0, 0.0, -1.0))
                        .normalize_or_zero();
                        for t in geo.mesh.material_triangles(material_slot) {
                            let [v0, v1, v2] = geo.mesh.tri_positions(t);
                            if hit.backface_culling {
                                let fn_t = (v1 - v0).cross(v2 - v0);
                                if fn_t.dot(cam_dir) > 0.0 {
                                    continue;
                                }
                            }
                            let c = (v0 + v1 + v2) / 3.0;
                            let clip = vp * glam::Vec4::new(c.x, c.y, c.z, 1.0);
                            if clip.w > 1e-6 {
                                let sx = 0.5 * (clip.x / clip.w) + 0.5;
                                let sy = 0.5 * (clip.y / clip.w) + 0.5;
                                if sx >= x0 && sx <= x1 && sy >= y0 && sy <= y1 {
                                    seed_tris.push(t);
                                }
                            }
                        }
                    }
                }
            }

            if seed_tris.is_empty() {
                continue;
            }
            seed_tris.sort_unstable();
            seed_tris.dedup();

            match hit.granularity {
                0 => {
                    for t in seed_tris {
                        tri_val[t] = Some(hit.value);
                    }
                }
                1 => {
                    let mut groups: Vec<u32> = seed_tris.iter().map(|&t| geo.poly_groups[t]).collect();
                    groups.sort_unstable();
                    groups.dedup();
                    for t in geo.mesh.material_triangles(material_slot) {
                        if groups.binary_search(&geo.poly_groups[t]).is_ok() {
                            tri_val[t] = Some(hit.value);
                        }
                    }
                }
                2 => {
                    let mut groups: Vec<u32> = seed_tris.iter().map(|&t| geo.mesh_elements[t]).collect();
                    groups.sort_unstable();
                    groups.dedup();
                    for t in geo.mesh.material_triangles(material_slot) {
                        if groups.binary_search(&geo.mesh_elements[t]).is_ok() {
                            tri_val[t] = Some(hit.value);
                        }
                    }
                }
                _ => {
                    let mut groups: Vec<u32> = seed_tris.iter().map(|&t| geo.uv_islands[t]).collect();
                    groups.sort_unstable();
                    groups.dedup();
                    for t in geo.mesh.material_triangles(material_slot) {
                        if groups.binary_search(&geo.uv_islands[t]).is_ok() {
                            tri_val[t] = Some(hit.value);
                        }
                    }
                }
            }
        }

        // Pass 1: rasterize triangles whose value differs from initial_fill (with 1.5px seam padding)
        for (t, val_opt) in tri_val.iter().enumerate() {
            let Some(val) = *val_opt else { continue };
            if (val - initial_fill).abs() <= 1e-4 {
                continue;
            }
            any_written = true;
            raster_triangle(geo.mesh.tri_uvs(t), r_res, r_res, full_rect, 1.5, |th| {
                let idx = (th.y as u32 * r_res + th.x as u32) as usize;
                buf_low[idx] = val;
            });
        }

        // Pass 2: if any triangle was explicitly erased back to initial_fill, restore its strict interior
        for (t, val_opt) in tri_val.iter().enumerate() {
            let Some(val) = *val_opt else { continue };
            if (val - initial_fill).abs() > 1e-4 {
                continue;
            }
            raster_triangle(geo.mesh.tri_uvs(t), r_res, r_res, full_rect, 0.0, |th| {
                let idx = (th.y as u32 * r_res + th.x as u32) as usize;
                buf_low[idx] = val;
            });
        }
    }

    if !node.mask_paint_strokes.is_empty() {
        let mesh_diag = (geo.mesh.bounds.max - geo.mesh.bounds.min).length().max(0.1);
        for st in &node.mask_paint_strokes {
            if st.points.is_empty() {
                continue;
            }
            let radius = (st.size * 0.008 * mesh_diag).clamp(0.002 * mesh_diag, 0.06 * mesh_diag);
            let r_sq = radius * radius;
            let step = (radius * 0.5).max(1e-4);
            let mut samples: Vec<glam::Vec3> = Vec::new();
            let mut prev = glam::Vec3::from_array(st.points[0]);
            samples.push(prev);
            for pt in &st.points[1..] {
                let cur = glam::Vec3::from_array(*pt);
                let dist = (cur - prev).length();
                if dist > step {
                    let n_sub = (dist / step).ceil() as usize;
                    for i in 1..=n_sub {
                        samples.push(prev.lerp(cur, i as f32 / n_sub as f32));
                    }
                } else {
                    samples.push(cur);
                }
                prev = cur;
            }
            let mut bmin = samples[0];
            let mut bmax = samples[0];
            for &s in &samples {
                bmin = bmin.min(s);
                bmax = bmax.max(s);
            }
            bmin -= glam::Vec3::splat(radius);
            bmax += glam::Vec3::splat(radius);

            for t in geo.mesh.material_triangles(material_slot) {
                let [v0, v1, v2] = geo.mesh.tri_positions(t);
                let tmin = v0.min(v1).min(v2);
                let tmax = v0.max(v1).max(v2);
                if tmax.x < bmin.x
                    || tmin.x > bmax.x
                    || tmax.y < bmin.y
                    || tmin.y > bmax.y
                    || tmax.z < bmin.z
                    || tmin.z > bmax.z
                {
                    continue;
                }
                let near_tri = samples.iter().any(|&s| {
                    let clamped = s.clamp(tmin, tmax);
                    (clamped - s).length_squared() <= r_sq
                });
                if !near_tri {
                    continue;
                }
                any_written = true;
                raster_triangle(geo.mesh.tri_uvs(t), r_res, r_res, full_rect, 1.0, |th| {
                    let p = bary_point(&[v0, v1, v2], th.bary);
                    let mut min_d2 = f32::INFINITY;
                    for &s in &samples {
                        let d2 = (p - s).length_squared();
                        if d2 < min_d2 {
                            min_d2 = d2;
                        }
                    }
                    if min_d2 < r_sq {
                        let d = min_d2.sqrt() / radius;
                        let falloff = (1.0 - d * d).clamp(0.0, 1.0);
                        let idx = (th.y as u32 * r_res + th.x as u32) as usize;
                        buf_low[idx] = buf_low[idx] + (st.value - buf_low[idx]) * falloff;
                    }
                });
            }
        }
    }

    if !any_written {
        return;
    }

    let tiles_x = mask.base.tiles_x();
    let tiles_y = mask.base.tiles_y();
    let scale = (res / r_res).max(1);
    let block_size = (TILE_SIZE / scale).max(1);
    let mut tile_buf = vec![0.0f32; TILE_TEXELS];

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let lx0 = tx * block_size;
            let ly0 = ty * block_size;
            let mut min_v = f32::INFINITY;
            let mut max_v = f32::NEG_INFINITY;
            for by in 0..block_size {
                let row = ((ly0 + by).min(r_res - 1) * r_res) as usize;
                for bx in 0..block_size {
                    let v = buf_low[row + (lx0 + bx).min(r_res - 1) as usize];
                    if v < min_v {
                        min_v = v;
                    }
                    if v > max_v {
                        max_v = v;
                    }
                }
            }
            if (min_v - initial_fill).abs() <= 1e-4 && (max_v - initial_fill).abs() <= 1e-4 {
                continue;
            }
            let tidx = (ty * tiles_x + tx) as usize;
            if (max_v - min_v) <= 1e-4 {
                tile_buf.fill(min_v);
                mask.base.write_tile(tidx, &tile_buf);
                continue;
            }
            if scale == 1 {
                for py in 0..TILE_SIZE {
                    let src_row = ((ly0 + py) * r_res + lx0) as usize;
                    let dst_row = (py * TILE_SIZE) as usize;
                    tile_buf[dst_row..dst_row + TILE_SIZE as usize]
                        .copy_from_slice(&buf_low[src_row..src_row + TILE_SIZE as usize]);
                }
            } else {
                for py in 0..TILE_SIZE {
                    let sy = (ty * TILE_SIZE + py) / scale;
                    let src_row = (sy.min(r_res - 1) * r_res) as usize;
                    let dst_row = (py * TILE_SIZE) as usize;
                    for px in 0..TILE_SIZE {
                        let sx = (tx * TILE_SIZE + px) / scale;
                        tile_buf[dst_row + px as usize] = buf_low[src_row + sx.min(r_res - 1) as usize];
                    }
                }
            }
            mask.base.write_tile(tidx, &tile_buf);
        }
    }
}
