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
use subcraft_mesh::spp::images::{decode_cache_image, decode_image_resource, decode_resource_image};
use subcraft_mesh::spp::{BakedKind, CacheFormat, CacheImage, LayerInfo, LayerKind, Plane, ProjectInfo, SppFile, TextureSetInfo, channel_bits};

use crate::Session;
use crate::spp_job::{ImportControl, SppEvent, SppStack, SppStackUpdate};
use crate::spp_stack::Rebuilder;
use std::sync::Arc;
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

/// Baked map kind of a texture set's `additionalMaps` usage id (the same in every sample project:
/// 0 ID, 1 ambient occlusion, 2 curvature, 3 world-space normals, 4 position, 5 thickness).
fn usage_kind(usage: i32) -> Option<BakedKind> {
    Some(match usage {
        0 => BakedKind::ColorId,
        1 => BakedKind::AmbientOcclusion,
        2 => BakedKind::Curvature,
        3 => BakedKind::WorldNormal,
        4 => BakedKind::Position,
        5 => BakedKind::Thickness,
        _ => return None,
    })
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
    (1u32 << log2).min(max_import_resolution())
}

/// [`MAX_IMPORT_RESOLUTION`], or less when `SUBCRAFT_SPP_MAX_RESOLUTION` asks for it (developer aid:
/// quick experiments on 4K projects).
fn max_import_resolution() -> u32 {
    let cap = std::env::var("SUBCRAFT_SPP_MAX_RESOLUTION").ok().and_then(|v| v.parse::<u32>().ok()).filter(|v| v.is_power_of_two() && *v >= 128);
    cap.map_or(MAX_IMPORT_RESOLUTION, |c| c.min(MAX_IMPORT_RESOLUTION))
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
            // Texels no layer wrote a normal to are stored as (0, 0, 0): no perturbation, a flat normal.
            ChannelKind::Normal if src[..3].iter().all(|v| v.abs() < 1e-3) => dst[..3].copy_from_slice(&[0.5, 0.5, 1.0]),
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
/// A fill showing the set's normal mesh map as the base orientation, with the layers' normals
/// beneath it as detail (as `Session::show_baked_normal` adds after a high-poly bake).
pub fn baked_normal_layer(ts: &TextureSet) -> Layer {
    use subcraft_doc::{BlendMode, ChannelBlend, FillChannel, FillSource};
    let mut layer = Layer::new_fill("Baked normal (Painter's normal map)", ts);
    if let LayerContent::Fill(fill) = &mut layer.content {
        fill.channels.clear();
        fill.channels.insert(ChannelKind::Normal, FillChannel::new(FillSource::MeshMap { map: MeshMapKind::Normal }));
    }
    layer.blend.insert(ChannelKind::Normal, ChannelBlend::new(BlendMode::NormalMapInverseDetail));
    layer
}

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
        Self::open_spp_cached(path, crate::spp_cache::SppCache::default_location())
    }

    /// [`Session::open_spp`] with the import cache in `cache` (none: no caching).
    pub fn open_spp_cached(path: &Path, cache: Option<crate::spp_cache::SppCache>) -> Result<(Self, Vec<String>), String> {
        let control = ImportControl::new();
        let mut opened: Option<(Session, Vec<String>)> = None;
        import_spp_cached(path, &control, cache, &mut |event| match event {
            SppEvent::Opened(session, notes) => opened = Some((*session, notes)),
            SppEvent::Stack(stack) => {
                if let Some((session, notes)) = &mut opened {
                    notes.extend(session.apply_spp_stack(stack));
                }
            }
        })?;
        opened.ok_or_else(|| "the Painter import produced no project".to_string())
    }
}

/// Import a Painter project in two phases, handing each to `emit`: [`SppEvent::Opened`] as soon as
/// the model, the baked maps and the flattened channels are read, then [`SppEvent::Stack`] with the
/// rebuilt layer stack (judged against the flattened result; the quality check decides which of the
/// two is shown). `control` reports progress and can cancel (the result is then `Err`).
pub fn import_spp(path: &Path, control: &Arc<ImportControl>, emit: &mut dyn FnMut(SppEvent)) -> Result<(), String> {
    import_spp_cached(path, control, crate::spp_cache::SppCache::default_location(), emit)
}

/// [`import_spp`] with the import cache in `cache_dir` (none: no caching).
pub fn import_spp_cached(path: &Path, control: &Arc<ImportControl>, cache: Option<crate::spp_cache::SppCache>, emit: &mut dyn FnMut(SppEvent)) -> Result<(), String> {
    let cancelled = || if control.is_cancelled() { Err("cancelled".to_string()) } else { Ok(()) };
    {
        let timing = std::env::var_os("SUBCRAFT_SPP_TIMING").is_some();
        let t0 = std::time::Instant::now();
        control.set_stage("Reading the model");
        // The file's hash (the cache key) is computed alongside the import.
        let key_thread = cache.as_ref().map(|_| {
            let p = path.to_path_buf();
            std::thread::spawn(move || crate::spp_cache::file_key(&p).ok())
        });
        let mut report = import_mesh(path).map_err(|e| e.to_string())?;
        cancelled()?;
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
                emit(SppEvent::Opened(Box::new(Session::new(report.mesh, crate::DEFAULT_RESOLUTION)), notes));
                return Ok(());
            }
            Err(e) => {
                notes.push(format!("The Painter project's layer data could not be read ({e}); only the model was imported."));
                emit(SppEvent::Opened(Box::new(Session::new(report.mesh, crate::DEFAULT_RESOLUTION)), notes));
                return Ok(());
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
        let base = Session::from_mesh_with(mesh, &settings)?;
        let mut doc = base.document().clone();
        let mesh = base.scene().mesh.as_ref().clone();
        drop(base);

        let mut maps: Vec<(subcraft_core::TextureSetId, BTreeMap<MeshMapKind, TiledImage>)> = Vec::new();
        let resources = ResourceStore::default();
        // Per set with Painter layers: (index in `project.texture_sets`, the set as flattened, its baked maps).
        let mut pending: Vec<(usize, TextureSet, BTreeMap<MeshMapKind, TiledImage>)> = Vec::new();
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
            control.set_stage(format!("Reading the textures of '{}'", ts.name));
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
            // Maps baked outside Painter and imported (`.image` resources), by their usage id; only
            // for kinds the project has no baked map of.
            let mut imported = Vec::new();
            for (usage, url) in &info.mesh_maps {
                let Some(kind) = usage_kind(*usage) else { continue };
                if blobs.iter().any(|(k, _)| *k == kind) {
                    continue;
                }
                match spp.image_resource_blob(url) {
                    Ok(Some(blob)) => imported.push((kind, blob)),
                    Ok(None) => {}
                    Err(e) => notes.push(format!("{}: the imported {kind:?} map could not be read ({e}).", ts.name)),
                }
            }
            // The set's own normal map (Painter's "Normal" in the texture set settings): the base
            // orientation under the layers' normals. DBAL-A2's carries its high-poly detail (the
            // knurling of a wheel, among others), which the layers do not. Read last, so the
            // coverage still comes from a baked map.
            if let Some(url) = info.normal_map.as_ref().filter(|_| !blobs.iter().any(|(k, _)| *k == BakedKind::Normal)) {
                match spp.image_resource_blob(url) {
                    Ok(Some(blob)) => imported.push((BakedKind::Normal, blob)),
                    Ok(None) => {}
                    Err(e) => notes.push(format!("{}: the normal map could not be read ({e}).", ts.name)),
                }
            }
            let mut decoded: Vec<_> = blobs.par_iter().map(|(kind, blob)| (*kind, decode_resource_image(blob, res))).collect();
            decoded.par_extend(imported.par_iter().map(|(kind, blob)| (*kind, decode_image_resource(blob, res))));
            drop(blobs);
            let mut baked: Vec<(BakedKind, Plane)> = Vec::new();
            for (kind, plane) in decoded {
                match plane {
                    // Tangent-space normals are stored as OpenGL, like the Normal channel.
                    Ok(mut plane) if kind == BakedKind::Normal && project_normal_dx => {
                        let c = plane.components;
                        plane.data.chunks_mut(c).for_each(|px| px[1] = 1.0 - px[1]);
                        baked.push((kind, plane));
                    }
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
            if timing {
                notes.push(format!("  timing: mesh {t_mesh:?}, channels {t_channels:?}, baked maps {t_maps:?}"));
            }
            if let Some(index) = project.texture_sets.iter().position(|s| std::ptr::eq(s, info)) {
                pending.push((index, ts.clone(), found.clone()));
            }
            // Painter shades with the set's normal map under the layers' normals: the same top layer
            // a high-poly bake adds (after the rebuild's copy, which expects the two imported layers).
            if found.contains_key(&MeshMapKind::Normal) {
                ts.layers.push(std::sync::Arc::new(baked_normal_layer(ts)));
                notes.push("  The set's normal map is applied by the top layer \"Baked normal (Painter's normal map)\".".into());
            }
            cancelled()?;
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

        let mesh_arc = std::sync::Arc::new(mesh);
        let mut session = Session::from_parts(mesh_arc.as_ref().clone(), Document { texture_sets: std::mem::take(&mut doc.texture_sets) }, resources, "Open Painter project");
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
        emit(SppEvent::Opened(Box::new(session), notes));

        // Second phase: rebuild the Painter stacks and judge them against the flattened result.
        let key = key_thread.and_then(|t| t.join().ok().flatten());
        let cache = cache.zip(key);
        let sets: Vec<TextureSet> = pending.iter().map(|p| p.1.clone()).collect();
        if let Some((cache, key)) = &cache {
            control.set_stage("Reading the cached Painter stack");
            match crate::spp_cache::load(&cache.dir, key, &sets) {
                Ok(stack) => {
                    emit(SppEvent::Stack(stack));
                    return Ok(());
                }
                Err(why) => {
                    log::info!("Painter import cache: not used ({why})");
                    if timing {
                        eprintln!("Painter import cache: not used ({why})");
                    }
                }
            }
        }
        let rebuild_started = std::time::Instant::now();
        let mut stack = SppStack { updates: Vec::new(), resources: ResourceStore::default(), notes: Vec::new() };
        for (index, mut ts, found) in pending {
            let info = &project.texture_sets[index];
            let t3 = std::time::Instant::now();
            rebuild_stack(&mut spp, info, &mut ts, &mesh_arc, &mut stack.resources, &found, &mut stack.notes, Some(control));
            cancelled()?;
            if ts.layers.len() == 3 {
                stack.updates.push(SppStackUpdate { texture_set: ts.id, flat: ts.layers[0].clone(), folder: ts.layers[1].clone() });
            }
            if timing {
                stack.notes.push(format!("  timing: stack {:?}", t3.elapsed()));
            }
        }
        let rebuild_took = rebuild_started.elapsed();
        let to_store = cache.as_ref().is_some_and(|(c, _)| rebuild_took >= c.min_rebuild).then(|| stack.clone());
        emit(SppEvent::Stack(stack));
        if let (Some((cache, key)), Some(stack)) = (&cache, &to_store) {
            control.set_stage("Storing the rebuilt stack in the import cache");
            crate::spp_cache::store(&cache.dir, key, stack, &sets);
        }
        Ok(())
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

/// Fit the `fitted` layers of `set` (holding the rebuilt folder; at the resolution of `maps` and
/// `flat`) against Painter's `flat` channels, leave the values in `set` and return them with the
/// composite of the fitted set.
pub fn fit_and_composite(set: &mut TextureSet, fitted: &[crate::spp_fit::Fitted], mesh: &subcraft_mesh::Mesh, resources: &ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, flat: &BTreeMap<ChannelKind, TiledImage>, coverage: Option<&Plane>) -> (Vec<Vec<(ChannelKind, subcraft_core::ChannelValue)>>, subcraft_compositor::Composite) {
    let mut res = resources.clone();
    let (values, _, composite) = fit_solve_composite(set, fitted, mesh, &mut res, maps, flat, coverage, false);
    (values, composite)
}

/// Channel textures solved for a fitted layer: (layer, channel, resource holding the texture).
pub type Solved = Vec<(subcraft_core::LayerId, ChannelKind, subcraft_core::ResourceId)>;

/// [`fit_and_composite`]; with `detail`, the fitted layers whose sources are all uniform also get
/// their content solved per texel ([`crate::spp_fit::fit_with_detail`]) as textures at the set's
/// resolution, added to `resources` and used by the layers in `set`.
#[allow(clippy::too_many_arguments)]
pub fn fit_solve_composite(set: &mut TextureSet, fitted: &[crate::spp_fit::Fitted], mesh: &subcraft_mesh::Mesh, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, flat: &BTreeMap<ChannelKind, TiledImage>, coverage: Option<&Plane>, detail: bool) -> (Vec<Vec<(ChannelKind, subcraft_core::ChannelValue)>>, Solved, subcraft_compositor::Composite) {
    let geometry = subcraft_materials::TextureSetGeometry::new(std::sync::Arc::new(mesh.clone()), set.material_slot, set.resolution);
    let mesh_maps = subcraft_materials::MeshMaps::from_map(maps.clone());
    let res = set.resolution;
    // Detail needs every covered texel; the uniform fit alone is fine with a thinned grid.
    let at = covered_texels(res, coverage, if detail { usize::MAX } else { 160_000 });
    let timing = std::env::var_os("SUBCRAFT_SPP_TIMING").is_some();
    let t0 = std::time::Instant::now();
    let regions = fit_regions(set, fitted, &at, &geometry, resources, &mesh_maps);
    if timing {
        eprintln!("fit regions: {:?}", t0.elapsed());
    }
    let t0 = std::time::Instant::now();
    let result = {
        let ctx = subcraft_compositor::EvalContext { geometry: &geometry, resources, mesh_maps: &mesh_maps };
        crate::spp_fit::fit_with_detail(set, fitted, flat, &at, Some(&regions), detail, &|s| subcraft_compositor::composite_full(s, &ctx).channels)
    };
    if timing {
        eprintln!("fit and detail: {:?}", t0.elapsed());
    }
    let mut solved = Solved::new();
    // Checked before the solved textures replace the uniform sources below.
    let normal_users: Vec<usize> = if detail { (0..fitted.len()).filter(|&k| crate::spp_fit::has_uniform_normal(&set.layers, fitted[k].0)).collect() } else { Vec::new() };
    for (k, channels) in result.detail.iter().enumerate() {
        let (id, _) = fitted[k];
        if !crate::spp_fit::only_uniform(&set.layers, id) {
            continue;
        }
        let name = find_name(&set.layers, id).unwrap_or_default();
        for (kind, samples) in channels {
            let comps = kind.components();
            let uniform = result.values[k].iter().find(|(c, _)| c == kind).map(|(_, v)| *v);
            let base: Vec<f32> = (0..comps).map(|c| uniform.map_or(0.5, |v| v.0[c])).collect();
            let mut data: Vec<f32> = (0..(res * res) as usize * comps).map(|i| base[i % comps]).collect();
            for (t, &(x, y)) in at.iter().enumerate() {
                let o = (y as usize * res as usize + x as usize) * comps;
                data[o..o + comps].copy_from_slice(&samples[t * comps..(t + 1) * comps]);
            }
            let bitmap = subcraft_image::Bitmap::new_f32(res, res, comps as u8, data);
            let id_res = resources.insert(subcraft_doc::Resource { id: subcraft_core::ResourceId::new(), name: format!("{name} {} (solved from Painter's result)", kind.display_name()), kind: subcraft_doc::ResourceKind::Texture, bitmap: std::sync::Arc::new(bitmap), source_path: None, encoded: None });
            crate::spp_fit::set_bitmap(&mut set.layers, id, *kind, id_res);
            solved.push((id, *kind, id_res));
        }
    }
    if std::env::var_os("SUBCRAFT_SPP_DEBUG").is_some() {
        eprintln!("normal solve: {} users of {} fitted", normal_users.len(), fitted.len());
    }
    if let Some(goal) = flat.get(&ChannelKind::Normal).filter(|_| !normal_users.is_empty()) {
        let t = std::time::Instant::now();
        solve_normals(set, fitted, &normal_users, &regions, &at, goal, &geometry, &mesh_maps, resources, &mut solved);
        if std::env::var_os("SUBCRAFT_SPP_TIMING").is_some() {
            eprintln!("normal solve: {:?} for {} layers", t.elapsed(), normal_users.len());
        }
    }
    let ctx = subcraft_compositor::EvalContext { geometry: &geometry, resources, mesh_maps: &mesh_maps };
    let composite = subcraft_compositor::composite_full(set, &ctx);
    (result.values, solved, composite)
}

/// Painter's normal detail, solved into the Normal of the material layers it came from: each
/// sample goes to the fitted layer with a (flat) Normal that shows most there (measured by tilting
/// that layer's normal alone; at least 0.2), as a texture "... Normal (solved from Painter's
/// result)". The rebuilt normal is flat elsewhere, so Painter's value is the start; two passes
/// against the composite then correct for the layers' normal opacity and partial coverage.
#[allow(clippy::too_many_arguments)]
fn solve_normals(set: &mut TextureSet, fitted: &[crate::spp_fit::Fitted], users: &[usize], regions: &crate::spp_fit::Regions, at: &[(u32, u32)], goal: &TiledImage, geometry: &subcraft_materials::TextureSetGeometry, mesh_maps: &subcraft_materials::MeshMaps, resources: &mut ResourceStore, solved: &mut Solved) {
    let res = set.resolution as usize;
    // The probes and corrections composite the Normal channel only.
    let mut nset = set.clone();
    nset.channels.retain(|c| c.kind == ChannelKind::Normal);
    let normal_of = |nset: &TextureSet, resources: &ResourceStore| {
        let ctx = subcraft_compositor::EvalContext { geometry, resources, mesh_maps };
        subcraft_compositor::composite_full(nset, &ctx).channels.remove(&ChannelKind::Normal)
    };
    // How much each candidate's normal shows: tilt it and read the composite's response. Layers of
    // different top-level folders (which rarely overlap) are measured in the same composite, each
    // over its own folder's samples, as the fit does; two layers of a folder share a composite,
    // one tilted along x and read on x, the other along y.
    let tilt = 0.4f32;
    let z = 0.5 + (1.0 - (2.0 * tilt).powi(2)).sqrt() * 0.5;
    let tilted = [subcraft_core::ChannelValue::rgb(0.5 + tilt, 0.5, z), subcraft_core::ChannelValue::rgb(0.5, 0.5 + tilt, z)];
    let mut rounds: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    for (u, &k) in users.iter().enumerate() {
        let i = seen.entry(regions.group[k]).or_default();
        if rounds.len() <= *i / 2 {
            rounds.push(Vec::new());
        }
        rounds[*i / 2].push((u, *i % 2));
        *i += 1;
    }
    let mut shows: Vec<Vec<f32>> = vec![Vec::new(); users.len()];
    for round in &rounds {
        for &(u, axis) in round {
            crate::spp_fit::set_uniform(&mut nset.layers, fitted[users[u]].0, ChannelKind::Normal, tilted[axis]);
        }
        let normal = normal_of(&nset, resources);
        let shared = round.iter().map(|&(u, _)| regions.group[users[u]]).collect::<std::collections::BTreeSet<_>>().len() > 1;
        for &(u, axis) in round {
            let k = users[u];
            crate::spp_fit::set_uniform(&mut nset.layers, fitted[k].0, ChannelKind::Normal, subcraft_core::ChannelValue::rgb(0.5, 0.5, 1.0));
            let mine = |t: usize| !shared || regions.owner[t] == Some(regions.group[k]);
            shows[u] = normal.as_ref().map_or_else(|| vec![0.0; at.len()], |n| at.iter().enumerate().map(|(t, &(x, y))| if mine(t) { ((n.get(x, y)[axis] - 0.5) / tilt).clamp(0.0, 1.0) } else { 0.0 }).collect());
        }
    }
    let owner: Vec<Option<usize>> = (0..at.len())
        .map(|t| users.iter().enumerate().fold((0.2f32, None), |best, (u, &k)| if shows[u][t] >= best.0 { (shows[u][t], Some(k)) } else { best }).1)
        .collect();
    let flat_normal = [0.5f32, 0.5, 1.0];
    let mut maps: BTreeMap<usize, (subcraft_core::ResourceId, Vec<f32>)> = BTreeMap::new();
    for (t, &(x, y)) in at.iter().enumerate() {
        let Some(k) = owner[t] else { continue };
        let entry = maps.entry(k).or_insert_with(|| (subcraft_core::ResourceId::new(), (0..res * res * 3).map(|i| flat_normal[i % 3]).collect()));
        let o = (y as usize * res + x as usize) * 3;
        entry.1[o..o + 3].copy_from_slice(&goal.get(x, y)[..3]);
    }
    let publish = |set: &mut TextureSet, resources: &mut ResourceStore, maps: &BTreeMap<usize, (subcraft_core::ResourceId, Vec<f32>)>| {
        for (&k, (rid, data)) in maps {
            let (id, _) = fitted[k];
            let name = find_name(&set.layers, id).unwrap_or_default();
            let bitmap = subcraft_image::Bitmap::new_f32(res as u32, res as u32, 3, data.clone());
            resources.insert(subcraft_doc::Resource { id: *rid, name: format!("{name} Normal (solved from Painter's result)"), kind: subcraft_doc::ResourceKind::Texture, bitmap: std::sync::Arc::new(bitmap), source_path: None, encoded: None });
            crate::spp_fit::set_bitmap(&mut set.layers, id, ChannelKind::Normal, *rid);
        }
    };
    if std::env::var_os("SUBCRAFT_SPP_DEBUG").is_some() {
        eprintln!("normal solve: {} of {} samples owned by {} layers", owner.iter().filter(|o| o.is_some()).count(), at.len(), maps.len());
    }
    publish(&mut nset, resources, &maps);
    for _ in 0..2 {
        let current = normal_of(&nset, resources);
        let Some(current) = current else { break };
        for (t, &(x, y)) in at.iter().enumerate() {
            let Some(k) = owner[t] else { continue };
            let (g, c) = (goal.get(x, y), current.get(x, y));
            let data = &mut maps.get_mut(&k).unwrap().1;
            let o = (y as usize * res + x as usize) * 3;
            let mut n = glam::Vec3::new(data[o], data[o + 1], data[o + 2]) * 2.0 - glam::Vec3::ONE;
            n += (glam::Vec3::new(g[0], g[1], g[2]) - glam::Vec3::new(c[0], c[1], c[2])) * 2.0;
            let n = if n.z > 0.05 { n.normalize() } else { glam::Vec3::Z };
            data[o..o + 3].copy_from_slice(&(n * 0.5 + glam::Vec3::splat(0.5)).to_array());
        }
        publish(&mut nset, resources, &maps);
    }
    publish(set, resources, &maps);
    for (&k, (rid, _)) in &maps {
        solved.push((fitted[k].0, ChannelKind::Normal, *rid));
    }
}

fn find_name(layers: &[std::sync::Arc<Layer>], id: subcraft_core::LayerId) -> Option<String> {
    layers.iter().find_map(|l| {
        if l.id == id {
            return Some(l.name.clone());
        }
        match &l.content {
            LayerContent::Group(g) => find_name(&g.children, id),
            _ => None,
        }
    })
}

/// Fit regions from the rebuilt folder's top-level entries: a sample belongs to the only entry
/// whose mask (or, without a mask, which) covers it. Polygon-filled part folders rarely overlap, so
/// the layers of different parts are measured with the same composites.
fn fit_regions(set: &TextureSet, fitted: &[crate::spp_fit::Fitted], at: &[(u32, u32)], geometry: &subcraft_materials::TextureSetGeometry, resources: &ResourceStore, maps: &subcraft_materials::MeshMaps) -> crate::spp_fit::Regions {
    let children: Vec<std::sync::Arc<Layer>> = match set.layers.first().map(|f| &f.content) {
        Some(LayerContent::Group(g)) => g.children.clone(),
        _ => Vec::new(),
    };
    let contains = |l: &Layer, id: subcraft_core::LayerId| {
        fn walk(l: &Layer, id: subcraft_core::LayerId) -> bool {
            l.id == id || matches!(&l.content, LayerContent::Group(g) if g.children.iter().any(|c| walk(c, id)))
        }
        walk(l, id)
    };
    let group: Vec<usize> = fitted.iter().map(|(id, _)| children.iter().position(|c| contains(c, *id)).unwrap_or(usize::MAX)).collect();
    // Coverage of each top-level entry that holds a fitted layer (everything for an entry without a mask).
    let mut cover: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
    for &g in group.iter().filter(|g| **g != usize::MAX) {
        if cover.contains_key(&g) {
            continue;
        }
        let c = &children[g];
        let v = match c.mask.as_ref().filter(|m| m.enabled && c.visible) {
            Some(m) => {
                let img = crate::spp_stack::eval_mask_image(m, set, geometry, resources, maps);
                at.iter().map(|&(x, y)| img.get(x, y)[0] > 1e-3).collect()
            }
            None => vec![c.visible; at.len()],
        };
        cover.insert(g, v);
    }
    let mut owner = vec![None; at.len()];
    for (t, o) in owner.iter_mut().enumerate() {
        let mut found = cover.iter().filter(|(_, v)| v[t]).map(|(g, _)| *g);
        if let (Some(g), None) = (found.next(), found.next()) {
            *o = Some(g);
        }
    }
    crate::spp_fit::Regions { group, owner }
}

/// A 4-component plane whose alpha is 1 on texels inside the set's UV triangles.
fn geometry_coverage(mesh: &subcraft_mesh::Mesh, slot: u32, res: u32) -> Plane {
    let geometry = subcraft_materials::TextureSetGeometry::new(std::sync::Arc::new(mesh.clone()), slot, res);
    let mut data = vec![0.0f32; (res * res) as usize * 4];
    for t in 0..geometry.tile_count() {
        let tile = geometry.tile(t);
        if tile.is_empty() {
            continue;
        }
        for i in 0..subcraft_image::TILE_TEXELS {
            let (x, y) = tile.texel_xy(i);
            if x < res && y < res && tile.coverage(i) > 0.5 {
                data[(y * res + x) as usize * 4 + 3] = 1.0;
            }
        }
    }
    Plane { width: res, height: res, components: 4, data }
}

/// Texels (x, y) at resolution `res` that the gate compares: covered by the baked maps (all of
/// them without coverage), on a regular grid thinned to at most about `limit`.
fn covered_texels(res: u32, coverage: Option<&Plane>, limit: usize) -> Vec<(u32, u32)> {
    let step = ((res as usize * res as usize / limit.max(1)) as f64).sqrt().ceil().max(1.0) as u32;
    let mut out = Vec::new();
    for y in (0..res).step_by(step as usize) {
        for x in (0..res).step_by(step as usize) {
            let inside = coverage.is_none_or(|c| {
                let (cx, cy) = (x * c.width / res, y * c.height / res);
                c.pixel(cx.min(c.width - 1), cy.min(c.height - 1)).get(3).copied().unwrap_or(1.0) > 0.5
            });
            if inside {
                out.push((x, y));
            }
        }
    }
    out
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

/// The Painter stack of `info` rebuilt as one SubCraft folder for the set `view` (its resolution
/// decides the rasters'). `blend_map` overrides the Painter blending id table (experiments).
pub fn rebuild_folder(spp: &mut SppFile, info: &TextureSetInfo, view: &TextureSet, picker: &crate::spp_select::Picker, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, blend_map: Option<&BTreeMap<i32, subcraft_doc::BlendMode>>) -> (Layer, crate::spp_stack::StackStats, Vec<crate::spp_fit::Fitted>) {
    rebuild_folder_cached(spp, info, view, picker, resources, maps, blend_map, None)
}

/// [`rebuild_folder`] reusing the mask rasters of an earlier call with the same `cache`.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_folder_cached(spp: &mut SppFile, info: &TextureSetInfo, view: &TextureSet, picker: &crate::spp_select::Picker, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, blend_map: Option<&BTreeMap<i32, subcraft_doc::BlendMode>>, cache: Option<std::sync::Arc<std::sync::Mutex<Vec<TiledImage>>>>) -> (Layer, crate::spp_stack::StackStats, Vec<crate::spp_fit::Fitted>) {
    rebuild_folder_tuned(spp, info, view, picker, resources, maps, blend_map, cache, None)
}

/// [`rebuild_folder_cached`] with generator mappings other than the calibrated ones (experiments).
#[allow(clippy::too_many_arguments)]
pub fn rebuild_folder_tuned(spp: &mut SppFile, info: &TextureSetInfo, view: &TextureSet, picker: &crate::spp_select::Picker, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, blend_map: Option<&BTreeMap<i32, subcraft_doc::BlendMode>>, cache: Option<std::sync::Arc<std::sync::Mutex<Vec<TiledImage>>>>, tuning: Option<crate::spp_stack::Tuning>) -> (Layer, crate::spp_stack::StackStats, Vec<crate::spp_fit::Fitted>) {
    rebuild_folder_controlled(spp, info, view, picker, resources, maps, blend_map, cache, tuning, None)
}

/// [`rebuild_folder_tuned`] reporting progress to (and stopping on) a background import's `control`.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_folder_controlled(spp: &mut SppFile, info: &TextureSetInfo, view: &TextureSet, picker: &crate::spp_select::Picker, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, blend_map: Option<&BTreeMap<i32, subcraft_doc::BlendMode>>, cache: Option<std::sync::Arc<std::sync::Mutex<Vec<TiledImage>>>>, tuning: Option<crate::spp_stack::Tuning>, control: Option<&Arc<ImportControl>>) -> (Layer, crate::spp_stack::StackStats, Vec<crate::spp_fit::Fitted>) {
    let mut rebuilder = Rebuilder::new(spp, view, resources);
    rebuilder.control = control.cloned();
    rebuilder.raster_cache = cache;
    if let Some(t) = tuning {
        rebuilder.tuning = t;
    }
    rebuilder.mesh_maps = Some(subcraft_materials::MeshMaps::from_map(maps.iter().map(|(k, v)| (*k, if v.width() == view.resolution { v.clone() } else { v.resampled(view.resolution, view.resolution) })).collect()));
    rebuilder.picker = Some(picker);
    rebuilder.geometry = Some(std::sync::Arc::new(subcraft_materials::TextureSetGeometry::new(picker.mesh.clone(), view.material_slot, view.resolution)));
    if let Some(map) = blend_map {
        rebuilder.blend_map = map.clone();
    }
    let layers = rebuilder.rebuild(&info.layers);
    let fitted = std::mem::take(&mut rebuilder.fitted);
    let stats = rebuilder.stats;
    let mut folder = Layer::new_folder("Painter stack (reconstructed)", view);
    if let LayerContent::Group(g) = &mut folder.content {
        g.children = layers.into_iter().map(std::sync::Arc::new).collect();
        g.expanded = true;
    }
    (folder, stats, fitted)
}

/// Rebuild the Painter layer stack as a folder above the flattened layer and decide, by comparing its
/// composite with Painter's flattened textures, whether it can be shown instead of them.
#[allow(clippy::too_many_arguments)]
fn rebuild_stack(spp: &mut SppFile, info: &TextureSetInfo, ts: &mut TextureSet, mesh: &subcraft_mesh::Mesh, resources: &mut ResourceStore, maps: &BTreeMap<MeshMapKind, TiledImage>, notes: &mut Vec<String>, control: Option<&Arc<ImportControl>>) {
    if info.layers.is_empty() || ts.layers.len() != 2 {
        return; // nothing to rebuild, or no flattened result to compare with
    }
    let view = ts.clone();
    let picker = crate::spp_select::Picker::new(std::sync::Arc::new(mesh.clone()));
    let layer_count = count_layers(&info.layers);
    if let Some(c) = control {
        c.begin_layers(format!("Rebuilding the Painter stack of '{}'", ts.name), layer_count);
    }
    let (mut folder, stats, fitted_full) = rebuild_folder_controlled(spp, info, &view, &picker, resources, maps, None, None, None, control);
    folder.visible = true;
    let flat = ts.layers[0].clone();
    let flat_content: BTreeMap<ChannelKind, TiledImage> = match &flat.content {
        LayerContent::Paint(p) => p.channels.clone(),
        _ => return,
    };
    // Composite the reconstruction alone and compare it with the flattened channels. The check runs
    // at no more than 512 texels so that it stays quick on 4K sets.
    let gate_res = ts.resolution.min(512);
    let (mut probe_set, probe_folder, probe_maps, flat_content, fitted) = if gate_res == ts.resolution {
        (view.clone(), folder.clone(), maps.clone(), flat_content, fitted_full.clone())
    } else {
        let mut small = view.clone();
        small.resolution = gate_res;
        let mut small_stats_resources = resources.clone();
        if let Some(c) = control {
            c.begin_layers("Checking the rebuilt stack against Painter's result", layer_count);
        }
        let (small_folder, _, fitted_small) = rebuild_folder_controlled(spp, info, &small, &picker, &mut small_stats_resources, maps, None, None, None, control);
        *resources = small_stats_resources;
        let small_maps: BTreeMap<MeshMapKind, TiledImage> = maps.iter().map(|(k, v)| (*k, v.resampled(gate_res, gate_res))).collect();
        let small_flat = flat_content.iter().map(|(k, v)| (*k, v.resampled(gate_res, gate_res))).collect();
        (small, small_folder, small_maps, small_flat, fitted_small)
    };
    probe_set.layers.clear();
    probe_set.insert_above(None, probe_folder);
    // Compared where the texture set has surface (inside its UV triangles): outside them the stack
    // holds nothing while Painter's texture holds padding. (The baked maps' alpha counted part of
    // that padding as covered, which made a matching stack look 4x worse.)
    let surface = geometry_coverage(mesh, probe_set.material_slot, probe_set.resolution);
    let coverage = Some(&surface);
    // Uniform values for the fills that could not be rebuilt, fitted at the gate's resolution and
    // applied to the full-resolution stack (both rebuilds list the same layers in the same order).
    if let Some(c) = control {
        c.set_stage("Fitting the rebuilt layers to Painter's result");
    }
    let t_fit = std::time::Instant::now();
    // SUBCRAFT_SPP_NO_FIT leaves the provisional values (developer aid for faster experiments).
    let fitted = if std::env::var_os("SUBCRAFT_SPP_NO_FIT").is_some() { Vec::new() } else { fitted };
    // Fitted layers whose sources are uniform get their content solved per texel from Painter's
    // result (SUBCRAFT_SPP_NO_DETAIL keeps the plain fitted values, for comparisons).
    let detail = std::env::var_os("SUBCRAFT_SPP_NO_DETAIL").is_none();
    let (values, solved, composite) = fit_solve_composite(&mut probe_set, &fitted, mesh, resources, &probe_maps, &flat_content, coverage, detail);
    if fitted.len() == fitted_full.len() && !fitted.is_empty() {
        let mut top = vec![std::sync::Arc::new(folder)];
        for ((id, _), vals) in fitted_full.iter().zip(&values) {
            for (kind, v) in vals {
                crate::spp_fit::set_uniform(&mut top, *id, *kind, *v);
            }
        }
        // Solved textures: the same layer in the full-resolution stack (same rebuild order).
        for (small_id, kind, resource) in &solved {
            if let Some(k) = fitted.iter().position(|(id, _)| id == small_id) {
                crate::spp_fit::set_bitmap(&mut top, fitted_full[k].0, *kind, *resource);
            }
        }
        folder = std::sync::Arc::try_unwrap(top.pop().unwrap()).unwrap_or_else(|a| (*a).clone());
    }
    let t_fit = t_fit.elapsed();
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
    if std::env::var_os("SUBCRAFT_SPP_TIMING").is_some() {
        notes.push(format!("    rebuild timing: {:?}, fit of {} layers {t_fit:?}, {} solved textures", stats.timings, fitted.len(), solved.len()));
        for (label, n, ms) in subcraft_compositor::profile::take().iter().take(10) {
            notes.push(format!("    composite profile {label}: {n} evaluations, {ms:.0} ms"));
        }
    }
    for (what, n) in &stats.skipped {
        notes.push(format!("    not rebuilt: {what} ({n})"));
    }
    if !stats.blend_ids.is_empty() {
        let ids: Vec<String> = stats.blend_ids.iter().map(|(id, n)| format!("{id} x{n}")).collect();
        notes.push(format!("    Painter blend modes are not mapped yet (ids {}); they were treated as Normal.", ids.join(", ")));
    }
    // SUBCRAFT_SPP_SHOW=rebuilt|flattened overrides the choice (developer aid for comparisons).
    let show_rebuilt = match std::env::var("SUBCRAFT_SPP_SHOW").as_deref() {
        Ok("rebuilt") => true,
        Ok("flattened") => false,
        _ => good,
    };
    folder.visible = show_rebuilt;
    let mut flat = (*flat).clone();
    flat.visible = !show_rebuilt;
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

    /// Painter stores (0, 0, 0) where no layer wrote a normal; that is a flat normal, not (0, 1, 0)
    /// after the DirectX flip (which shows as a green blob and wrong lighting).
    #[test]
    fn unwritten_cache_normals_are_flat() {
        let img = CacheImage { format: CacheFormat::Rgba16f, plane: Plane { width: 2, height: 1, components: 4, data: vec![0.0, 0.0, 0.0, 0.0, 0.6, 0.3, 0.9, 1.0] } };
        let out = channel_image(&img, ChannelKind::Normal, true, 2);
        let (a, b) = (out.get(0, 0), out.get(1, 0));
        assert_eq!(&a[..3], &[0.5, 0.5, 1.0]);
        assert!((b[1] - 0.7).abs() < 1e-3 && (b[0] - 0.6).abs() < 1e-3, "{b:?}");
    }
}
