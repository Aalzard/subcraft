//! Opening a Substance 3D Painter project (`.spp`) as a SubCraft session.
//!
//! What comes across: the model (objects, UVs, normals), the texture set with its channels and
//! resolution, the baked mesh maps (normal, world normal, ID/color, AO, curvature, position,
//! thickness) and the flattened painted result of every channel as one paint layer, so the model
//! looks as it did in Painter and can be painted on. What does not: the layer structure itself
//! (masks, blend modes, filters, generators and `.sbsar` procedural materials cannot be re-run
//! without a Substance engine); their names are listed in the import notes.

use std::collections::BTreeMap;
use std::path::Path;

use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_core::color::srgb_to_linear;
use subcraft_doc::{Document, Layer, LayerContent, NormalFormat, ProjectSettings, ResourceStore, TextureSet};
use subcraft_image::TiledImage;
use subcraft_image::dilate::{apply_nearest, dilate_infinite, nearest_covered};
use rayon::prelude::*;
use subcraft_mesh::import_mesh;
use subcraft_mesh::spp::images::{decode_cache_image, decode_resource_image};
use subcraft_mesh::spp::{BakedKind, CacheFormat, CacheImage, LayerInfo, LayerKind, Plane, ProjectInfo, SppFile, TextureSetInfo, channel_bits};

use crate::Session;
use crate::spp_stack::Rebuilder;
use crate::new_project::NewProjectSettings;

/// Largest texture the importer produces (SubCraft's biggest texture-set resolution).
pub const MAX_IMPORT_RESOLUTION: u32 = 4096;

/// Painter channel flag to SubCraft channel; `None` for channels SubCraft has no equivalent for.
pub fn channel_kind(bits: u64) -> Option<ChannelKind> {
    Some(match bits {
        channel_bits::BASE_COLOR => ChannelKind::BaseColor,
        channel_bits::HEIGHT => ChannelKind::Height,
        channel_bits::ROUGHNESS => ChannelKind::Roughness,
        channel_bits::METALLIC => ChannelKind::Metallic,
        channel_bits::NORMAL => ChannelKind::Normal,
        channel_bits::AMBIENT_OCCLUSION => ChannelKind::AmbientOcclusion,
        channel_bits::EMISSIVE => ChannelKind::Emissive,
        channel_bits::OPACITY => ChannelKind::Opacity,
        _ => return None,
    })
}

fn channel_label(bits: u64) -> String {
    match bits {
        0x4 => "Specular edge color".into(),
        0x20 => "Displacement".into(),
        0x40 => "Glossiness".into(),
        b => format!("channel {b:#x}"),
    }
}

fn mesh_map_kind(kind: BakedKind) -> MeshMapKind {
    match kind {
        BakedKind::Normal => MeshMapKind::Normal,
        BakedKind::WorldNormal => MeshMapKind::WorldSpaceNormal,
        BakedKind::ColorId => MeshMapKind::Id,
        BakedKind::AmbientOcclusion => MeshMapKind::AmbientOcclusion,
        BakedKind::Curvature => MeshMapKind::Curvature,
        BakedKind::Position => MeshMapKind::Position,
        BakedKind::Thickness => MeshMapKind::Thickness,
    }
}

fn resolution_of(set: &TextureSetInfo) -> u32 {
    let log2 = set.width_log2.max(set.height_log2).clamp(7, 12) as u32;
    (1u32 << log2).min(MAX_IMPORT_RESOLUTION)
}

fn fit(image: TiledImage, res: u32) -> TiledImage {
    if image.width() == res && image.height() == res { image } else { image.resampled(res, res) }
}

/// A flattened Painter channel as paint-layer content: `components + 1` values per texel, fully opaque.
pub fn channel_image(img: &CacheImage, kind: ChannelKind, flip_green: bool, res: u32) -> TiledImage {
    let p = &img.plane;
    let comps = kind.components() + 1;
    let n = (p.width * p.height) as usize;
    let mut dense = vec![1.0f32; n * comps];
    let width = p.width as usize;
    for i in 0..n {
        // The texture cache holds OpenGL buffers, bottom row first; the baked maps (and SubCraft)
        // are top row first, so the rows are flipped to line up with the baked maps and the UVs.
        let (x, y) = (i % width, i / width);
        let s = ((p.height as usize - 1 - y) * width + x) * p.components;
        let src = &p.data[s..s + p.components];
        let dst = &mut dense[i * comps..(i + 1) * comps];
        match kind {
            ChannelKind::BaseColor | ChannelKind::Emissive => {
                for k in 0..3 {
                    dst[k] = if img.format == CacheFormat::Srgb8Alpha8 { srgb_to_linear(src[k]) } else { src[k] };
                }
            }
            ChannelKind::Normal => {
                dst[0] = src[0];
                dst[1] = if flip_green { 1.0 - src[1] } else { src[1] };
                dst[2] = src[2];
            }
            _ => dst[0] = src[0],
        }
    }
    let mut fill = [0.0f32; 4];
    fill[comps - 1] = 1.0;
    fit(TiledImage::from_dense(p.width, p.height, comps, fill, &dense), res)
}

/// The covered texels of a texture set's baked maps and, for each texel, its nearest covered texel.
pub struct Gutter {
    covered: Vec<bool>,
    nearest: Vec<u32>,
}

impl Gutter {
    /// From the alpha of a four-component baked map.
    pub fn from_coverage(plane: &Plane) -> Self {
        let covered: Vec<bool> = (0..(plane.width * plane.height) as usize).map(|i| plane.data[i * plane.components + 3] > 0.02).collect();
        let nearest = nearest_covered(plane.width as usize, plane.height as usize, &covered);
        Self { covered, nearest }
    }
}

/// A baked mesh map as a SubCraft one: background texels hold the nearest covered value (the
/// gutter other baked maps have), partially covered texels from the size reduction are
/// un-premultiplied. `coverage` is the alpha of a four-component baked map of the same texture set.
pub fn mesh_map_image(kind: MeshMapKind, plane: &Plane, coverage: Option<&Plane>, gutter: Option<&Gutter>, res: u32) -> TiledImage {
    let comps = kind.components();
    let (w, h) = (plane.width as usize, plane.height as usize);
    let cov_plane = coverage.filter(|c| c.width == plane.width && c.height == plane.height);
    let mut dense = vec![0.0f32; w * h * comps];
    let mut covered = vec![true; w * h];
    for i in 0..w * h {
        let alpha = match (cov_plane, plane.components) {
            (Some(c), _) => c.data[i * c.components + 3],
            (None, 4) => plane.data[i * 4 + 3],
            _ => 1.0,
        };
        covered[i] = alpha > 0.02;
        let scale = if alpha > 0.02 && alpha < 1.0 { 1.0 / alpha } else { 1.0 };
        for k in 0..comps {
            let v = plane.data[i * plane.components + k.min(plane.components.min(3) - 1)];
            dense[i * comps + k] = (v * scale).clamp(0.0, 1.0);
        }
    }
    match gutter.filter(|g| cov_plane.is_some() && g.covered.len() == w * h) {
        // All maps of a set share one coverage, so the nearest-texel search is done once.
        Some(g) => apply_nearest(comps, &mut dense, &g.covered, &g.nearest),
        None => dilate_infinite(w, h, comps, &mut dense, &covered),
    }
    let neutral = kind.neutral_value();
    let mut fill = [0.0f32; 4];
    fill[..comps].copy_from_slice(&neutral[..comps]);
    fit(TiledImage::from_dense(plane.width, plane.height, comps, fill, &dense), res)
}

fn describe_layers(layers: &[LayerInfo], depth: usize, lines: &mut Vec<String>, budget: &mut usize) {
    for l in layers.iter().rev() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let kind = match l.kind {
            LayerKind::Fill => "fill",
            LayerKind::Paint => "paint",
            LayerKind::Filter => "filter",
            LayerKind::Group => "group",
            LayerKind::Empty => "empty",
        };
        let name = if l.name.is_empty() { "(unnamed)" } else { l.name.as_str() };
        lines.push(format!("{}{name} ({kind}{}{})", "  ".repeat(depth + 1), if l.has_mask { ", mask" } else { "" }, if l.visible { "" } else { ", hidden" }));
        describe_layers(&l.children, depth + 1, lines, budget);
    }
}

fn count_layers(layers: &[LayerInfo]) -> usize {
    layers.iter().map(|l| 1 + count_layers(&l.children)).sum()
}

fn find_set<'a>(project: &'a ProjectInfo, name: &str) -> Option<&'a TextureSetInfo> {
    let base = name.split('.').next().unwrap_or(name);
    project.texture_sets.iter().find(|s| s.name == name).or_else(|| project.texture_sets.iter().find(|s| s.name == base))
}

impl Session {
    /// Open a Substance 3D Painter project: the model, the baked mesh maps and the flattened
    /// channel textures (see the module docs). Returns the session and import notes.
    pub fn open_spp(path: &Path) -> Result<(Self, Vec<String>), String> {
        let timing = std::env::var_os("SUBCRAFT_SPP_TIMING").is_some();
        let t0 = std::time::Instant::now();
        let mut report = import_mesh(path).map_err(|e| e.to_string())?;
        let t_mesh = t0.elapsed();
        if report.mesh.triangle_count() == 0 {
            return Err("the project's model has no triangles".into());
        }
        // The model carries no material names; with one texture set the model is that set's.
        report.warnings.retain(|w| !w.starts_with("Only the model was read"));
        let mut notes = report.warnings;
        let mut spp = SppFile::open(path).map_err(|e| e.to_string())?;
        let project = match spp.project() {
            Ok(p) if !p.texture_sets.is_empty() => p,
            Ok(_) => {
                notes.push("The Painter project has no texture set; only the model was imported.".into());
                return Ok((Self::new(report.mesh, crate::DEFAULT_RESOLUTION), notes));
            }
            Err(e) => {
                notes.push(format!("The Painter project's layer data could not be read ({e}); only the model was imported."));
                return Ok((Self::new(report.mesh, crate::DEFAULT_RESOLUTION), notes));
            }
        };
        let mut mesh = report.mesh;
        if mesh.materials.len() == 1 {
            mesh.materials[0].name = project.texture_sets[0].name.clone();
            if project.texture_sets.len() > 1 {
                notes.push(format!("The project has {} texture sets but the model file does not say which part belongs to which, so everything was put in '{}'.", project.texture_sets.len(), project.texture_sets[0].name));
            }
        }

        let mut settings = NewProjectSettings::default();
        let mut channels: Vec<ChannelKind> = vec![ChannelKind::BaseColor];
        for set in &project.texture_sets {
            for c in &set.channels {
                match channel_kind(c.bits) {
                    Some(k) if !channels.contains(&k) => channels.push(k),
                    Some(_) => {}
                    None => notes.push(format!("{}: the {} channel has no SubCraft equivalent and was skipped.", set.name, channel_label(c.bits))),
                }
            }
        }
        channels.sort_by_key(|k| ChannelKind::ALL.iter().position(|a| a == k));
        settings.channels = channels;
        settings.resolution = project.texture_sets.iter().map(resolution_of).max().unwrap_or(2048);
        for set in &project.texture_sets {
            settings.resolution_overrides.insert(set.name.clone(), resolution_of(set));
        }
        // `normalMapSyscoord` is 0 in every project seen, which is taken to be Painter's default,
        // DirectX normals. The channel is stored as OpenGL internally and exported in the project's format.
        let project_normal_dx = project.texture_sets.iter().all(|s| s.normal_syscoord == 0);
        settings.normal_format = if project_normal_dx { NormalFormat::DirectX } else { NormalFormat::OpenGl };
        let template_name = settings.template.export_template().to_string();
        let base = Self::from_mesh_with(mesh, &settings)?;
        let mut doc = base.document().clone();
        let mesh = base.scene().mesh.as_ref().clone();
        drop(base);

        let mut maps: Vec<(subcraft_core::TextureSetId, BTreeMap<MeshMapKind, TiledImage>)> = Vec::new();
        let mut resources = ResourceStore::default();
        for ts in &mut doc.texture_sets {
            let Some(info) = find_set(&project, &ts.name) else {
                notes.push(format!("No Painter texture set matches '{}'; it starts empty.", ts.name));
                continue;
            };
            let res = ts.resolution;
            notes.push(format!("Texture set '{}': {}x{}, {} Painter layers.", ts.name, 1u32 << info.width_log2, 1u32 << info.height_log2, count_layers(&info.layers)));
            let t1 = std::time::Instant::now();
            import_channels(&mut spp, info, ts, project_normal_dx, &mut notes);
            let t_channels = t1.elapsed();
            let t2 = std::time::Instant::now();
            let mut found = BTreeMap::new();
            // Read the blobs one after the other (one file), decode all maps at once.
            let mut blobs = Vec::new();
            for kind in spp.baked_map_kinds(&info.name).unwrap_or_default() {
                match spp.baked_map_blob(&info.name, kind) {
                    Ok(Some(blob)) => blobs.push((kind, blob)),
                    Ok(None) => {}
                    Err(e) => notes.push(format!("{}: the baked {kind:?} map could not be read ({e}).", ts.name)),
                }
            }
            let decoded: Vec<_> = blobs.par_iter().map(|(kind, blob)| (*kind, decode_resource_image(blob, res))).collect();
            drop(blobs);
            let mut baked: Vec<(BakedKind, Plane)> = Vec::new();
            for (kind, plane) in decoded {
                match plane {
                    Ok(plane) => baked.push((kind, plane)),
                    Err(e) => notes.push(format!("{}: the baked {kind:?} map could not be read ({e}).", ts.name)),
                }
            }
            let coverage = baked.iter().find(|(_, p)| p.components == 4).map(|(_, p)| p.clone());
            let gutter = coverage.as_ref().map(Gutter::from_coverage);
            let images: Vec<_> = baked.par_iter().map(|(kind, plane)| {
                let k = mesh_map_kind(*kind);
                (k, mesh_map_image(k, plane, coverage.as_ref(), gutter.as_ref(), res))
            }).collect();
            found.extend(images);
            if !found.is_empty() {
                let names: Vec<&str> = found.keys().map(|k| k.display_name()).collect();
                notes.push(format!("  Baked maps: {}.", names.join(", ")));
            }
            let t_maps = t2.elapsed();
            let t3 = std::time::Instant::now();
            rebuild_stack(&mut spp, info, ts, &mesh, &mut resources, &found, coverage.as_ref(), &mut notes);
            if timing {
                notes.push(format!("  timing: mesh {t_mesh:?}, channels {t_channels:?}, baked maps {t_maps:?}, stack {:?}", t3.elapsed()));
            }
            if !found.is_empty() {
                maps.push((ts.id, found));
            }
            let procedural = subcraft_mesh::spp::document::procedural_resources(&info.layers);
            let mut layer_lines = Vec::new();
            let mut budget = 14;
            describe_layers(&info.layers, 0, &mut layer_lines, &mut budget);
            notes.push("  Painter layers (top to bottom):".into());
            notes.extend(layer_lines);
            if !procedural.is_empty() {
                notes.push(format!("  {} procedural Substance graphs are in the stack; only the ones SubCraft has a counterpart for are rebuilt, the rest is part of the flattened layer.", procedural.len()));
            }
        }

        let mut session = Session::from_parts(mesh, Document { texture_sets: std::mem::take(&mut doc.texture_sets) }, resources, "Open Painter project");
        session.set_settings(ProjectSettings {
            normal_format: Some(if project_normal_dx { NormalFormat::DirectX } else { NormalFormat::OpenGl }),
            export_template: template_name,
            ..Default::default()
        });
        for (ts, m) in maps {
            session.set_mesh_maps(ts, m);
        }
        if project_normal_dx {
            notes.push("Normal maps: Painter's default (DirectX, Y down) was assumed; the Normal channel was converted to SubCraft's OpenGL storage. If lighting on the normal detail looks inverted, this assumption is the reason.".into());
        }
        Ok((session, notes))
    }
}

/// Replace the set's layers by the flattened Painter channels plus an empty layer to paint on.
fn import_channels(spp: &mut SppFile, info: &TextureSetInfo, ts: &mut TextureSet, project_normal_dx: bool, notes: &mut Vec<String>) {
    let res = ts.resolution;
    let tc = ts.clone();
    let mut flat = Layer::new_paint("Painter project (flattened)", &tc);
    let mut imported = Vec::new();
    if let LayerContent::Paint(content) = &mut flat.content {
        let mut blobs = Vec::new();
        for c in &info.channels {
            let Some(kind) = channel_kind(c.bits) else { continue };
            if !ts.has_channel(kind) {
                continue;
            }
            match spp.channel_blob(info, c.bits) {
                Ok(Some(blob)) => blobs.push((kind, blob)),
                Ok(None) => {}
                Err(e) => notes.push(format!("{}: the {} texture could not be read ({e}).", ts.name, kind.display_name())),
            }
        }
        let images: Vec<_> = blobs
            .par_iter()
            .map(|(kind, blob)| {
                let image = decode_cache_image(blob, res).map(|img| channel_image(&img, *kind, project_normal_dx && *kind == ChannelKind::Normal, res));
                (*kind, image)
            })
            .collect();
        for (kind, image) in images {
            match image {
                Ok(image) => {
                    content.channels.insert(kind, image);
                    imported.push(kind.display_name());
                }
                Err(e) => notes.push(format!("{}: the {} texture could not be read ({e}).", ts.name, kind.display_name())),
            }
        }
    }
    let missing: Vec<&str> = info.channels.iter().filter_map(|c| channel_kind(c.bits)).filter(|k| !imported.contains(&k.display_name())).map(|k| k.display_name()).collect();
    if imported.is_empty() {
        notes.push("  No painted textures were stored in the project (Painter saves them in its texture cache), so the layer stack starts empty.".into());
    } else {
        notes.push(format!("  Flattened channels: {}.", imported.join(", ")));
        ts.layers.clear();
        ts.insert_above(None, flat);
        ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
    }
    if !missing.is_empty() && !imported.is_empty() {
        notes.push(format!("  Not stored in the project (left at the default): {}.", missing.join(", ")));
    }
}

/// Texels of `a` and `b` (same channel) compared over `covered`: mean absolute difference of the value components.
fn channel_error(a: &TiledImage, kind: ChannelKind, b: &TiledImage, coverage: Option<&Plane>) -> Option<f32> {
    if a.width() != b.width() || a.height() != b.height() {
        return None;
    }
    let comps = kind.components();
    let step = (a.width() / 256).max(1);
    let (mut sum, mut n) = (0.0f64, 0usize);
    let mut y = 0;
    while y < a.height() {
        let mut x = 0;
        while x < a.width() {
            let inside = coverage.is_none_or(|c| {
                let (cx, cy) = (x * c.width / a.width(), y * c.height / a.height());
                c.pixel(cx.min(c.width - 1), cy.min(c.height - 1)).get(3).copied().unwrap_or(1.0) > 0.5
            });
            if inside {
                let (pa, pb) = (a.get(x, y), b.get(x, y));
                for k in 0..comps {
                    sum += (pa[k] - pb[k]).abs() as f64;
                    n += 1;
                }
            }
            x += step;
        }
        y += step;
    }
    (n > 0).then(|| (sum / n as f64) as f32)
}

/// Mean of each component over the covered texels (debug aid for the quality gate).
fn channel_mean(a: &TiledImage, comps: usize, coverage: Option<&Plane>) -> Vec<f32> {
    let step = (a.width() / 256).max(1);
    let (mut sum, mut n) = (vec![0.0f64; comps], 0usize);
    for y in (0..a.height()).step_by(step as usize) {
        for x in (0..a.width()).step_by(step as usize) {
            let inside = coverage.is_none_or(|c| {
                let (cx, cy) = (x * c.width / a.width(), y * c.height / a.height());
                c.pixel(cx.min(c.width - 1), cy.min(c.height - 1)).get(3).copied().unwrap_or(1.0) > 0.5
            });
            if inside {
                let p = a.get(x, y);
                for k in 0..comps {
                    sum[k] += p[k] as f64;
                }
                n += 1;
            }
        }
    }
    sum.iter().map(|s| (*s / n.max(1) as f64 * 1000.0).round() as f32 / 1000.0).collect()
}

/// Rebuild the Painter layer stack as a folder above the flattened layer and decide, by comparing its
/// composite with Painter's flattened textures, whether it can be shown instead of them.
#[allow(clippy::too_many_arguments)]
fn rebuild_stack(spp: &mut SppFile, info: &TextureSetInfo, ts: &mut TextureSet, mesh: &subcraft_mesh::Mesh, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, coverage: Option<&Plane>, notes: &mut Vec<String>) {
    if info.layers.is_empty() || ts.layers.len() != 2 {
        return; // nothing to rebuild, or no flattened result to compare with
    }
    let view = ts.clone();
    let mut rebuilder = Rebuilder::new(spp, &view, resources);
    let layers = rebuilder.rebuild(&info.layers);
    let stats = rebuilder.stats;
    let mut folder = Layer::new_folder("Painter stack (reconstructed)", &view);
    if let LayerContent::Group(g) = &mut folder.content {
        g.children = layers.into_iter().map(std::sync::Arc::new).collect();
        g.expanded = true;
    }
    folder.visible = true;
    let flat = ts.layers[0].clone();
    let flat_content: BTreeMap<ChannelKind, TiledImage> = match &flat.content {
        LayerContent::Paint(p) => p.channels.clone(),
        _ => return,
    };
    // Composite the reconstruction alone and compare it with the flattened channels. The check runs
    // at no more than 512 texels so that it stays quick on 4K sets.
    let gate_res = ts.resolution.min(512);
    let (mut probe_set, probe_folder, probe_maps, flat_content) = if gate_res == ts.resolution {
        (view.clone(), folder.clone(), maps.clone(), flat_content)
    } else {
        let mut small = view.clone();
        small.resolution = gate_res;
        let mut small_stats_resources = resources.clone();
        let mut small_rebuilder = Rebuilder::new(spp, &small, &mut small_stats_resources);
        let small_layers = small_rebuilder.rebuild(&info.layers);
        let mut small_folder = Layer::new_folder("probe", &small);
        if let LayerContent::Group(g) = &mut small_folder.content {
            g.children = small_layers.into_iter().map(std::sync::Arc::new).collect();
        }
        *resources = small_stats_resources;
        let small_maps: BTreeMap<MeshMapKind, TiledImage> = maps.iter().map(|(k, v)| (*k, v.resampled(gate_res, gate_res))).collect();
        let small_flat = flat_content.iter().map(|(k, v)| (*k, v.resampled(gate_res, gate_res))).collect();
        (small, small_folder, small_maps, small_flat)
    };
    probe_set.layers.clear();
    probe_set.insert_above(None, probe_folder);
    let mut probe = Session::from_parts(mesh.clone(), Document { texture_sets: vec![probe_set] }, resources.clone(), "probe");
    let probe_id = probe.document().texture_sets[0].id;
    probe.set_mesh_maps(probe_id, probe_maps.clone());
    let geometry = probe.scene().geometry.get(probe.document().texture_sets[0].material_slot, gate_res);
    let mesh_maps = subcraft_materials::MeshMaps::from_map(probe_maps);
    let composite = subcraft_compositor::composite_full(&probe.document().texture_sets[0], &subcraft_compositor::EvalContext { geometry: &geometry, resources: probe.resources(), mesh_maps: &mesh_maps });
    let mut errors: Vec<(ChannelKind, f32)> = Vec::new();
    for (kind, flat_image) in &flat_content {
        if let Some(c) = composite.channel(*kind) {
            if std::env::var_os("SUBCRAFT_SPP_DEBUG").is_some() {
                notes.push(format!("    debug {}: rebuilt mean {:?}, painter mean {:?}", kind.display_name(), channel_mean(c, kind.components(), coverage), channel_mean(flat_image, kind.components(), coverage)));
            }
            if let Some(e) = channel_error(c, *kind, flat_image, coverage) {
                errors.push((*kind, e));
            }
        }
    }
    let err_of = |k: ChannelKind| errors.iter().find(|(c, _)| *c == k).map(|(_, e)| *e);
    let good = err_of(ChannelKind::BaseColor).is_none_or(|e| e < 0.04) && err_of(ChannelKind::Roughness).is_none_or(|e| e < 0.06) && err_of(ChannelKind::Metallic).is_none_or(|e| e < 0.06);
    let summary: Vec<String> = errors.iter().map(|(k, e)| format!("{} {:.3}", k.display_name(), e)).collect();
    notes.push(format!(
        "  Rebuilt stack: {} layers, {} folders; {} exact and {} approximated effects, {} layers kept as hidden placeholders. Difference to Painter's result (mean, 0..1): {}.",
        stats.layers, stats.folders, stats.exact, stats.approximate, stats.placeholders, if summary.is_empty() { "n/a".into() } else { summary.join(", ") }
    ));
    for (what, n) in &stats.skipped {
        notes.push(format!("    not rebuilt: {what} ({n})"));
    }
    if !stats.blend_ids.is_empty() {
        let ids: Vec<String> = stats.blend_ids.iter().map(|(id, n)| format!("{id} x{n}")).collect();
        notes.push(format!("    Painter blend modes are not mapped yet (ids {}); they were treated as Normal.", ids.join(", ")));
    }
    folder.visible = good;
    let mut flat = (*flat).clone();
    flat.visible = !good;
    let top = ts.layers[1].clone();
    ts.layers.clear();
    ts.layers.push(std::sync::Arc::new(flat));
    ts.layers.push(std::sync::Arc::new(folder));
    ts.layers.push(top);
    notes.push(if good {
        "    The rebuilt stack is close to Painter's result, so it is shown and the flattened layer is hidden.".into()
    } else {
        "    The rebuilt stack differs too much from Painter's result to be trusted, so it is hidden and the flattened layer is shown. Switch them to compare or edit.".into()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::spp::{CacheFormat, CacheImage, Plane};

    /// Painter's texture cache is bottom row first, unlike its baked maps: the first cache row
    /// must end up as the last (bottom) row of the layer, or the texture lands upside down on the
    /// model ("the UVs are messed up").
    #[test]
    fn cache_textures_are_flipped_to_the_top_first_row_order() {
        let img = CacheImage { format: CacheFormat::Rg8, plane: Plane { width: 1, height: 2, components: 2, data: vec![0.2, 1.0, 0.8, 1.0] } };
        let out = channel_image(&img, ChannelKind::Roughness, false, 2);
        assert!((out.get(0, 0)[0] - 0.8).abs() < 1e-3, "{:?}", out.get(0, 0));
        assert!((out.get(0, 1)[0] - 0.2).abs() < 1e-3, "{:?}", out.get(0, 1));
    }
}
