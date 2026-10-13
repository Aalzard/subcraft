//! Texture export: composites → files through a template (packing, color encoding, normal
//! format, height-to-normal, UV padding).
//!
//! A map is written for a texture set when at least one of its channel sources exists in that
//! set (missing packed channels read their default, e.g. occlusion = 1); maps whose channels are
//! all absent are skipped. Color channels are sRGB-encoded for integer formats and stay linear in
//! EXR. Data channels are never color-transformed.

pub mod template;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_core::color::linear_to_srgb;
use subcraft_image::dilate::{dilate_infinite, dilate_width};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::Mesh;
use subcraft_mesh::uv_raster::rasterize_surface;

pub use template::{ExportTemplate, OutputMap, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Padding {
    /// No padding: gutter texels keep their composite values.
    None,
    /// Grow islands by this many texels.
    Texels(u32),
    /// Fill the whole gutter with the nearest island texel (best for mipmaps).
    Infinite,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub template: ExportTemplate,
    pub padding: Padding,
    /// Height-to-normal strength baked into exported normals (same units as the viewport's
    /// `height_scale`; 0 = normal channel only).
    pub height_to_normal: f32,
    /// Force every map to this file format.
    pub format_override: Option<FileFormat>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { template: ExportTemplate::pbr_metal_rough(), padding: Padding::Infinite, height_to_normal: 0.02, format_override: None }
    }
}

/// One texture set's composite channel images (`subcraft_compositor::Composite::channels`).
pub struct ExportTextureSet<'a> {
    pub name: &'a str,
    pub material_slot: u32,
    pub channels: &'a BTreeMap<ChannelKind, TiledImage>,
    /// Baked mesh maps; baked AO stands in for a missing Ambient Occlusion channel.
    pub mesh_maps: Option<&'a BTreeMap<MeshMapKind, TiledImage>>,
}

impl ExportTextureSet<'_> {
    /// Image providing `kind`: the channel composite, or the baked AO for the AO channel.
    fn source(&self, kind: ChannelKind) -> Option<&TiledImage> {
        self.channels.get(&kind).or_else(|| match kind {
            ChannelKind::AmbientOcclusion => self.mesh_maps.and_then(|m| m.get(&MeshMapKind::AmbientOcclusion)),
            _ => None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("could not create output folder {0}: {1}")]
    Folder(PathBuf, std::io::Error),
    #[error("could not write {0}: {1}")]
    Write(PathBuf, String),
}

/// File-system friendly name part.
pub fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if t.is_empty() { "untitled".into() } else { t }
}

fn map_applies(map: &OutputMap, set: &ExportTextureSet) -> bool {
    map.components.iter().filter_map(|s| s.channel()).any(|k| set.source(k).is_some())
        || map.components.iter().any(|s| matches!(s, Source::Normal { .. })) && set.channels.contains_key(&ChannelKind::Height)
}

/// Files an export would write: `(texture set, file name)`.
pub fn plan(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for set in sets {
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set)) {
            let fmt = settings.format_override.unwrap_or(map.format);
            out.push((set.name.to_string(), format!("{}_{}_{}.{}", sanitize(mesh_name), sanitize(set.name), map.suffix, fmt.extension())));
        }
    }
    out
}

/// Dense channel data of one texture set (lazily converted, shared by all maps).
struct Dense<'a> {
    set: &'a ExportTextureSet<'a>,
    size: u32,
    cache: HashMap<ChannelKind, (usize, Vec<f32>)>,
    normal: Option<Vec<f32>>,
}

impl<'a> Dense<'a> {
    fn channel(&mut self, kind: ChannelKind) -> &(usize, Vec<f32>) {
        let size = self.size;
        let set = self.set;
        self.cache.entry(kind).or_insert_with(|| match set.source(kind) {
            Some(img) if img.width() == size => (img.comps(), img.to_dense()),
            Some(img) => {
                // Baked at another resolution: nearest-neighbour resample.
                let c = img.comps();
                let mut out = Vec::with_capacity((size * size) as usize * c);
                for y in 0..size {
                    for x in 0..size {
                        let px = img.get(x * img.width() / size, y * img.height() / size);
                        out.extend_from_slice(&px[..c]);
                    }
                }
                (c, out)
            }
            None => {
                let d = kind.default_value().0;
                let n = (size * size) as usize;
                (3, (0..n).flat_map(|_| d).collect())
            }
        })
    }

    /// Normal channel with height bumps folded in (whiteout), encoded 0..1, 3 comps.
    fn normal(&mut self, height_scale: f32) -> &Vec<f32> {
        if self.normal.is_none() {
            let n = (self.size * self.size) as usize;
            let (nc, nmap) = self.channel(ChannelKind::Normal).clone();
            let mut out: Vec<f32> = (0..n).flat_map(|i| [nmap[i * nc], nmap[i * nc + 1], nmap[i * nc + 2]]).collect();
            if height_scale > 0.0 && self.set.channels.contains_key(&ChannelKind::Height) {
                let (hc, h) = self.channel(ChannelKind::Height).clone();
                let s = self.size as i64;
                let at = |x: i64, y: i64| h[((y.clamp(0, s - 1) * s + x.clamp(0, s - 1)) as usize) * hc];
                out.par_chunks_mut(3).enumerate().for_each(|(i, px)| {
                    let (x, y) = ((i as i64) % s, (i as i64) / s);
                    // Same convention as the viewport shader (UV v down, bitangent up).
                    let dhdu = (at(x + 1, y) - at(x - 1, y)) * 0.5 * s as f32;
                    let dhdv = (at(x, y + 1) - at(x, y - 1)) * 0.5 * s as f32;
                    let bump = Vec3::new(-dhdu * height_scale, dhdv * height_scale, 1.0).normalize();
                    let base = Vec3::new(px[0] * 2.0 - 1.0, px[1] * 2.0 - 1.0, (px[2] * 2.0 - 1.0).max(1e-3));
                    let r = Vec3::new(base.x + bump.x, base.y + bump.y, base.z * bump.z).normalize();
                    px.copy_from_slice(&[r.x * 0.5 + 0.5, r.y * 0.5 + 0.5, r.z * 0.5 + 0.5]);
                });
            }
            self.normal = Some(out);
        }
        self.normal.as_ref().expect("computed above")
    }
}

/// Compute one output map's pixels (row-major, `components.len()` interleaved), unpadded.
fn evaluate_map(map: &OutputMap, dense: &mut Dense, settings: &ExportSettings, format: FileFormat) -> Vec<f32> {
    let n = (dense.size * dense.size) as usize;
    let comps = map.components.len();
    let mut out = vec![0.0f32; n * comps];
    for (ci, src) in map.components.iter().enumerate() {
        match *src {
            Source::Constant { value } => (0..n).for_each(|i| out[i * comps + ci] = value),
            Source::Channel { kind, component, invert } => {
                let encode = map.srgb && kind.is_color() && !format.is_float();
                let (cc, data) = dense.channel(kind);
                let c = (component as usize).min(cc - 1);
                for i in 0..n {
                    let mut v = data[i * cc + c];
                    if invert {
                        v = 1.0 - v;
                    }
                    out[i * comps + ci] = if encode { linear_to_srgb(v) } else { v };
                }
            }
            Source::Normal { component, directx } => {
                let nm = dense.normal(settings.height_to_normal);
                let c = component.min(2) as usize;
                for i in 0..n {
                    let v = nm[i * 3 + c];
                    out[i * comps + ci] = if directx && c == 1 { 1.0 - v } else { v };
                }
            }
            Source::Height => {
                let (cc, data) = dense.channel(ChannelKind::Height);
                for i in 0..n {
                    let h = data[i * cc];
                    out[i * comps + ci] = if format.is_float() { h } else { h * 0.5 + 0.5 };
                }
            }
        }
    }
    out
}

/// UV coverage of a texture set at `size` (inside + 1-texel rim).
pub fn coverage(mesh: &Mesh, material_slot: u32, size: u32) -> Vec<bool> {
    let s = rasterize_surface(mesh, mesh.material_triangles(material_slot), size, size, 1.0);
    (0..s.triangle.len()).map(|i| s.covered(i)).collect()
}

/// Write every applicable map of every texture set into `dir`. Returns the written files.
pub fn export(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    std::fs::create_dir_all(dir).map_err(|e| ExportError::Folder(dir.to_path_buf(), e))?;
    let mut written = Vec::new();
    for set in sets {
        let Some(size) = set.channels.values().next().map(|i| i.width()) else { continue };
        let covered = if settings.padding == Padding::None { Vec::new() } else { coverage(mesh, set.material_slot, size) };
        let mut dense = Dense { set, size, cache: HashMap::new(), normal: None };
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set)) {
            let format = settings.format_override.unwrap_or(map.format);
            let comps = map.components.len();
            let mut data = evaluate_map(map, &mut dense, settings, format);
            match settings.padding {
                Padding::None => {}
                Padding::Infinite => dilate_infinite(size as usize, size as usize, comps, &mut data, &covered),
                Padding::Texels(t) => {
                    dilate_width(size as usize, size as usize, comps, &mut data, &covered, t);
                }
            }
            let name = format!("{}_{}_{}.{}", sanitize(mesh_name), sanitize(set.name), map.suffix, format.extension());
            let path = dir.join(name);
            save_image(&path, size, size, comps, &data, format).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
