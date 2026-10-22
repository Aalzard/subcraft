//! Texture export: composites → files through a template (packing, color encoding, normal
//! format, height-to-normal, UV padding).
//!
//! A map is written for a texture set when at least one of its channel sources exists in that
//! set (missing packed channels read their default, e.g. occlusion = 1); maps whose channels are
//! all absent are skipped. Color channels are sRGB-encoded for integer formats and stay linear in
//! EXR. Data channels are never color-transformed.

pub mod selection;
pub mod template;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_core::color::linear_to_srgb;
use subcraft_image::dilate::{dilate_infinite, dilate_width};
use subcraft_image::{FileFormat, TiledImage};
use subcraft_mesh::Mesh;
use subcraft_mesh::uv_raster::rasterize_surface;

pub use selection::{MapKey, MapSelection};
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
    /// Maps switched off per texture set (default: all on).
    pub selection: MapSelection,
    /// Force DirectX (`true`) or OpenGL (`false`) normal maps whatever the template says.
    pub normal_directx: Option<bool>,
    /// UDIM sets go in one sub-folder per tile (`1001/`...) instead of a `.1001` file-name suffix.
    pub udim_folders: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { template: ExportTemplate::pbr_metal_rough(), padding: Padding::Infinite, height_to_normal: 0.02, format_override: None, selection: MapSelection::default(), normal_directx: None, udim_folders: false }
    }
}

/// One texture set's composite channel images (`subcraft_compositor::Composite::channels`).
pub struct ExportTextureSet<'a> {
    pub name: &'a str,
    pub material_slot: u32,
    /// UDIM number (1001...) when this set is one tile of a material.
    pub udim: Option<u32>,
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
    #[error("could not create texture archive: {0}")]
    Archive(String),
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

/// One output file of the template for one texture set, whether or not it is switched on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedFile {
    pub set: String,
    pub suffix: String,
    /// Path relative to the export folder (`/`-separated; UDIM folders add a leading `1001/`).
    pub path: String,
    pub enabled: bool,
}

/// The set name without its `.1001` tile suffix.
fn base_name<'a>(set: &'a ExportTextureSet) -> &'a str {
    match set.udim {
        Some(n) => set.name.strip_suffix(&format!(".{n}")).unwrap_or(set.name),
        None => set.name,
    }
}

fn output_path(mesh_name: &str, set: &ExportTextureSet, suffix: &str, ext: &str, settings: &ExportSettings) -> String {
    let (mesh, base) = (sanitize(mesh_name), sanitize(base_name(set)));
    match set.udim {
        None => format!("{mesh}_{}_{suffix}.{ext}", sanitize(set.name)),
        Some(n) if settings.udim_folders => format!("{n}/{mesh}_{base}_{suffix}.{ext}"),
        Some(n) => format!("{mesh}_{base}_{suffix}.{n}.{ext}"),
    }
}

/// Every file the template could write, with its checkbox state.
pub fn plan_all(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Vec<PlannedFile> {
    let mut out = Vec::new();
    for set in sets {
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set)) {
            let fmt = settings.format_override.unwrap_or(map.format);
            out.push(PlannedFile {
                set: set.name.to_string(),
                suffix: map.suffix.clone(),
                path: output_path(mesh_name, set, &map.suffix, fmt.extension(), settings),
                enabled: settings.selection.is_on(set.name, &map.suffix),
            });
        }
    }
    out
}

/// Files an export would write (only the checked maps): `(texture set, relative path)`.
pub fn plan(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Vec<(String, String)> {
    plan_all(mesh_name, sets, settings).into_iter().filter(|f| f.enabled).map(|f| (f.set, f.path)).collect()
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
                let directx = settings.normal_directx.unwrap_or(directx);
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
    subcraft_core::vfs::create_dir_all(dir).map_err(|e| ExportError::Folder(dir.to_path_buf(), e))?;
    let mut written = Vec::new();
    render_maps(mesh,mesh_name,sets,settings,|name,bytes| {
        let path=dir.join(name);
        if let Some(parent)=path.parent() {subcraft_core::vfs::create_dir_all(parent).map_err(|e|ExportError::Folder(parent.to_path_buf(),e))?;}
        subcraft_core::vfs::write(&path,bytes).map_err(|e|ExportError::Write(path.clone(),e.to_string()))?;
        written.push(path);Ok(())
    })?;
    Ok(written)
}

/// All rendered maps in one file.
pub struct TextureArchive {pub name: String,pub files: usize,pub bytes: Vec<u8>}

pub fn archive(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Result<TextureArchive,ExportError> {
    use std::io::{Cursor,Write};
    let mut zip=zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut names=std::collections::HashSet::new();
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let files=render_maps(mesh,mesh_name,sets,settings,|name,bytes| {
        if name.contains('\\') || name.starts_with('/') || name.split('/').any(|p| p == "..") || !names.insert(name.to_string()) {return Err(ExportError::Archive(format!("invalid or duplicate output name: {name}")));}
        zip.start_file(name,options).map_err(|e|ExportError::Archive(e.to_string()))?;
        zip.write_all(bytes).map_err(|e|ExportError::Archive(e.to_string()))
    })?;
    let bytes=zip.finish().map_err(|e|ExportError::Archive(e.to_string()))?.into_inner();
    Ok(TextureArchive {name:format!("{}-textures.zip",sanitize(mesh_name)),files,bytes})
}

/// Directory and archive exports share rendering, encoding, templates and padding.
fn render_maps(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, mut write: impl FnMut(&str,&[u8])->Result<(),ExportError>) -> Result<usize,ExportError> {
    let mut count=0;
    for set in sets {
        let Some(size) = set.channels.values().next().map(|i| i.width()) else { continue };
        let covered = if settings.padding == Padding::None { Vec::new() } else { coverage(mesh, set.material_slot, size) };
        let mut dense = Dense { set, size, cache: HashMap::new(), normal: None };
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set) && settings.selection.is_on(set.name, &m.suffix)) {
            if map.suffix.contains(['/', '\\']) || map.suffix.contains("..") {
                return Err(ExportError::Archive(format!("invalid map suffix: {}", map.suffix)));
            }
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
            let name = output_path(mesh_name, set, &map.suffix, format.extension(), settings);
            let bytes=subcraft_image::io::encode_image(size,size,comps,&data,format).map_err(|e|ExportError::Write(PathBuf::from(&name),e.to_string()))?;
            write(&name,&bytes)?;count+=1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
