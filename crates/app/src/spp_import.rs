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
use subcraft_image::dilate::dilate_infinite;
use subcraft_mesh::import_mesh;
use subcraft_mesh::spp::{BakedKind, CacheFormat, CacheImage, LayerInfo, LayerKind, Plane, ProjectInfo, SppFile, TextureSetInfo, channel_bits};

use crate::Session;
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
    for i in 0..n {
        let src = &p.data[i * p.components..(i + 1) * p.components];
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

/// A baked mesh map as a SubCraft one: background texels hold the nearest covered value (the
/// gutter other baked maps have), partially covered texels from the size reduction are
/// un-premultiplied. `coverage` is the alpha of a four-component baked map of the same texture set.
pub fn mesh_map_image(kind: MeshMapKind, plane: &Plane, coverage: Option<&Plane>, res: u32) -> TiledImage {
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
    dilate_infinite(w, h, comps, &mut dense, &covered);
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
        let mut report = import_mesh(path).map_err(|e| e.to_string())?;
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
        for ts in &mut doc.texture_sets {
            let Some(info) = find_set(&project, &ts.name) else {
                notes.push(format!("No Painter texture set matches '{}'; it starts empty.", ts.name));
                continue;
            };
            let res = ts.resolution;
            notes.push(format!("Texture set '{}': {}x{}, {} Painter layers.", ts.name, 1u32 << info.width_log2, 1u32 << info.height_log2, count_layers(&info.layers)));
            import_channels(&mut spp, info, ts, project_normal_dx, &mut notes);
            let mut found = BTreeMap::new();
            let mut baked: Vec<(BakedKind, Plane)> = Vec::new();
            for kind in spp.baked_map_kinds(&info.name).unwrap_or_default() {
                match spp.baked_map(&info.name, kind, res) {
                    Ok(Some(plane)) => baked.push((kind, plane)),
                    Ok(None) => {}
                    Err(e) => notes.push(format!("{}: the baked {kind:?} map could not be read ({e}).", ts.name)),
                }
            }
            let coverage = baked.iter().find(|(_, p)| p.components == 4).map(|(_, p)| p.clone());
            for (kind, plane) in &baked {
                let k = mesh_map_kind(*kind);
                found.insert(k, mesh_map_image(k, plane, coverage.as_ref(), res));
            }
            if !found.is_empty() {
                let names: Vec<&str> = found.keys().map(|k| k.display_name()).collect();
                notes.push(format!("  Baked maps: {}.", names.join(", ")));
                maps.push((ts.id, found));
            }
            let procedural = subcraft_mesh::spp::document::procedural_resources(&info.layers);
            let mut layer_lines = Vec::new();
            let mut budget = 14;
            describe_layers(&info.layers, 0, &mut layer_lines, &mut budget);
            notes.push("  Painter layers (top to bottom; the stack itself is not recreated, only its flattened result):".into());
            notes.extend(layer_lines);
            if !procedural.is_empty() {
                notes.push(format!("  {} procedural Substance materials / filters in the stack cannot be re-evaluated; their look is part of the flattened layer.", procedural.len()));
            }
        }

        let mut session = Session::from_parts(mesh, Document { texture_sets: std::mem::take(&mut doc.texture_sets) }, ResourceStore::default(), "Open Painter project");
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
        for c in &info.channels {
            let Some(kind) = channel_kind(c.bits) else { continue };
            if !ts.has_channel(kind) {
                continue;
            }
            match spp.channel_texture(info, c.bits, res) {
                Ok(Some(img)) => {
                    content.channels.insert(kind, channel_image(&img, kind, project_normal_dx && kind == ChannelKind::Normal, res));
                    imported.push(kind.display_name());
                }
                Ok(None) => {}
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
