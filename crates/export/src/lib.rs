//! Texture export: composites → files through a template (packing, color encoding, normal
//! format, height-to-normal, UV padding).
//!
//! A map is written for a texture set when at least one of its channel sources exists in that
//! set (missing packed channels read their default, e.g. occlusion = 1); maps whose channels are
//! all absent are skipped. Color channels are sRGB-encoded for integer formats and stay linear in
//! EXR. Data channels are never color-transformed.

pub mod preset;
pub mod psd;
pub mod scene;
pub mod selection;
pub mod template;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_core::color::linear_to_srgb;
use subcraft_image::dilate::{diffuse_fill, dilate_infinite, dilate_width};
use subcraft_image::{FileFormat, TiledImage};
use subcraft_mesh::Mesh;
use subcraft_mesh::uv_raster::rasterize_surface;

pub use selection::{MapKey, MapSelection};
pub use template::{ExportTemplate, OutputMap, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Padding {
    /// No padding: gutter texels keep their composite values.
    None,
    /// Grow islands by this many texels; the rest of the gutter keeps its composite values.
    Texels(u32),
    /// Fill the whole gutter with the nearest island texel (best for mipmaps).
    Infinite,
    /// Painter's "Dilation + default background color": grow islands by this many texels, then
    /// fill the rest of the gutter with each output's default (the value an empty texture set
    /// exports: flat normal, white occlusion, black base color...).
    Background(u32),
    /// Painter's "Dilation + diffusion": grow islands by this many texels, then fill the rest of
    /// the gutter with a smooth blend of the surrounding islands.
    Diffusion(u32),
}

impl Padding {
    /// Dilation distance of the modes that have one.
    pub fn distance(self) -> Option<u32> {
        match self {
            Padding::Texels(n) | Padding::Background(n) | Padding::Diffusion(n) => Some(n),
            Padding::None | Padding::Infinite => None,
        }
    }

    /// The same mode with another dilation distance (modes without one are unchanged).
    pub fn with_distance(self, n: u32) -> Padding {
        match self {
            Padding::Texels(_) => Padding::Texels(n),
            Padding::Background(_) => Padding::Background(n),
            Padding::Diffusion(_) => Padding::Diffusion(n),
            other => other,
        }
    }
}

/// Sizes offered by the export window's Size menu (Painter's list).
pub const EXPORT_SIZES: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

/// What the 2D view shows: a composite channel or a baked mesh map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View2d {
    Channel(ChannelKind),
    MeshMap(MeshMapKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    /// Source of `Source::View2d` components (the 2D panel's current view). Maps using it are
    /// skipped when `None` or when a texture set lacks that source.
    pub view_2d: Option<View2d>,
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
    /// Output size (square) for maps whose template sets none; `None` = each texture set's size.
    pub size: Option<u32>,
    /// Dither 8-bit outputs ("8 bits + dithering"), overriding the maps' own choice.
    pub dither: Option<bool>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { view_2d: None, template: ExportTemplate::pbr_metal_rough(), padding: Padding::Infinite, height_to_normal: 0.02, format_override: None, selection: MapSelection::default(), normal_directx: None, udim_folders: false, size: None, dither: None }
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
    fn view_source(&self, view: View2d) -> Option<&TiledImage> {
        match view {
            View2d::Channel(k) => self.source(k),
            View2d::MeshMap(k) => self.mesh_maps.and_then(|m| m.get(&k)),
        }
    }

    /// Whether the Base Color stack starts transparent (a transparent texture set: the composite
    /// is filled with alpha 0 where nothing was evaluated).
    fn has_coverage(&self) -> bool {
        self.channels.get(&ChannelKind::BaseColor).is_some_and(|img| img.comps() == 4 && img.fill()[3] < 1.0)
    }

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
    #[error("the export preset cannot be used: {0}")]
    Template(String),
}

/// File-system friendly name part.
pub fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if t.is_empty() { "untitled".into() } else { t }
}

fn map_applies(map: &OutputMap, set: &ExportTextureSet, settings: &ExportSettings) -> bool {
    if map.components.iter().any(|s| matches!(s, Source::View2d { .. })) {
        return settings.view_2d.is_some_and(|v| set.view_source(v).is_some());
    }
    // A transparent set has alpha without an Opacity channel: its Base Color stack starts empty.
    map.components.iter().any(|s| matches!(s, Source::Alpha)) && set.has_coverage()
        || map.components.iter().filter_map(|s| s.channel()).any(|k| set.source(k).is_some())
        || map.components.iter().any(|s| matches!(s, Source::Normal { .. })) && set.channels.contains_key(&ChannelKind::Height)
        || map.components.iter().any(|s| matches!(s, Source::MeshMap { kind, .. } if set.mesh_maps.is_some_and(|m| m.contains_key(kind))))
}

/// Expand a file name pattern (no extension). Tokens: `$mesh`, `$textureSet` (without its UDIM
/// tile), `$udim` (tile number; empty outside UDIM sets) and `$suffix` (the map's suffix).
/// Token values are sanitized like the classic names; other characters outside letters,
/// digits, `-`, `_` and `.` become `_`. Separators left dangling by an empty `$udim` are trimmed.
pub fn expand_pattern(pattern: &str, mesh: &str, texture_set: &str, udim: Option<u32>, suffix: &str) -> String {
    const TOKENS: [&str; 4] = ["$textureSet", "$suffix", "$mesh", "$udim"];
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(tok) = TOKENS.iter().find(|t| rest.starts_with(**t)) {
            match *tok {
                "$mesh" => out.push_str(&sanitize(mesh)),
                "$textureSet" => out.push_str(&sanitize(texture_set)),
                "$udim" => match udim {
                    Some(n) => out.push_str(&n.to_string()),
                    // No tile: drop the separator written for it ("_$udim", ".$udim").
                    None => {
                        if out.ends_with(['_', '.', '-']) {
                            out.pop();
                        }
                    }
                },
                _ => out.push_str(&sanitize(suffix)),
            }
            rest = &rest[tok.len()..];
            continue;
        }
        let c = rest.chars().next().expect("rest is not empty");
        out.push(if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' });
        rest = &rest[c.len_utf8()..];
    }
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    out.trim_matches(|c| c == '.' || c == '_' || c == '-').to_string()
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

pub(crate) fn output_path(mesh_name: &str, set: &ExportTextureSet, suffix: &str, ext: &str, settings: &ExportSettings) -> String {
    let pattern = settings.template.maps.iter().find(|m| m.suffix == suffix).and_then(|m| m.pattern.as_deref());
    if let Some(pattern) = pattern {
        let mut name = expand_pattern(pattern, mesh_name, base_name(set), set.udim, suffix);
        if name.is_empty() {
            name = format!("{}_{}_{suffix}", sanitize(mesh_name), sanitize(base_name(set)));
        }
        return match set.udim {
            None => format!("{name}.{ext}"),
            Some(n) if settings.udim_folders => format!("{n}/{name}.{ext}"),
            Some(_) if pattern.contains("$udim") => format!("{name}.{ext}"),
            Some(n) => format!("{name}.{n}.{ext}"),
        };
    }
    let (mesh, base) = (sanitize(mesh_name), sanitize(base_name(set)));
    match set.udim {
        None => format!("{mesh}_{}_{suffix}.{ext}", sanitize(set.name)),
        Some(n) if settings.udim_folders => format!("{n}/{mesh}_{base}_{suffix}.{ext}"),
        Some(n) => format!("{mesh}_{base}_{suffix}.{n}.{ext}"),
    }
}

/// The file format a map is written in: the export-wide override, except that a four-component
/// map keeps its own format when the override is JPEG. JPEG has no alpha channel, so the override
/// used to drop the fourth component (Unity's smoothness in a mask map) without a word.
pub(crate) fn output_format(map: &OutputMap, settings: &ExportSettings) -> FileFormat {
    match settings.format_override {
        // Radiance HDR stores RGB only as well.
        Some(f @ (FileFormat::Jpeg | FileFormat::Hdr32)) if map.components.len() == 4 && map.format != f => map.format,
        Some(format) => format,
        None => map.format,
    }
}

/// Output size of `map` for a texture set of `set_size` texels: the map's own size, else the
/// template's, else the export window's, else the texture set's.
pub fn output_size(map: &OutputMap, settings: &ExportSettings, set_size: u32) -> u32 {
    map.size.or(settings.template.size).or(settings.size).unwrap_or(set_size).max(1)
}

/// Two checked maps that would be written to the same file (texture sets whose names differ only
/// in characters a file name cannot hold, or only in letter case): `(path, first set, second set)`.
pub fn colliding_outputs(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Option<(String, String, String)> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for (set, path) in plan(mesh_name, sets, settings) {
        // Windows and macOS file names ignore case.
        if let Some(first) = seen.insert(path.to_lowercase(), set.clone()) {
            return Some((path, first, set));
        }
    }
    None
}

fn check_outputs(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Result<(), ExportError> {
    match colliding_outputs(mesh_name, sets, settings) {
        Some((path, a, b)) => Err(ExportError::Write(PathBuf::from(&path), format!("texture sets \"{a}\" and \"{b}\" would both be written to this file; rename one of them"))),
        None => Ok(()),
    }
}

/// Every file the template could write, with its checkbox state.
pub fn plan_all(mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings) -> Vec<PlannedFile> {
    let mut out = Vec::new();
    for set in sets {
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set, settings)) {
            let fmt = output_format(map, settings);
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
    view: Option<(usize, Vec<f32>)>,
    mesh_maps: HashMap<MeshMapKind, (usize, Vec<f32>)>,
    alpha: Option<Vec<f32>>,
}

/// Nearest-neighbour copy of `img` at `size` × `size` (the image itself when it already fits).
fn dense_at(img: &TiledImage, size: u32) -> (usize, Vec<f32>) {
    let c = img.comps();
    if img.width() == size {
        return (c, img.to_dense());
    }
    let mut out = Vec::with_capacity((size * size) as usize * c);
    for y in 0..size {
        for x in 0..size {
            out.extend_from_slice(&img.get(x * img.width() / size, y * img.height() / size)[..c]);
        }
    }
    (c, out)
}

impl<'a> Dense<'a> {
    /// The 2D view's source at export size (nearest resample when baked at another size).
    fn view(&mut self, v: View2d) -> &(usize, Vec<f32>) {
        let (size, set) = (self.size, self.set);
        self.view.get_or_insert_with(|| {
            let img = set.view_source(v).expect("map_applies checked the source");
            let c = img.comps();
            if img.width() == size {
                return (c, img.to_dense());
            }
            let mut out = Vec::with_capacity((size * size) as usize * c);
            for y in 0..size {
                for x in 0..size {
                    out.extend_from_slice(&img.get(x * img.width() / size, y * img.height() / size)[..c]);
                }
            }
            (c, out)
        })
    }

    /// A baked mesh map at export size; the map's default value when it is not baked.
    fn mesh_map(&mut self, kind: MeshMapKind) -> &(usize, Vec<f32>) {
        let (size, set) = (self.size, self.set);
        self.mesh_maps.entry(kind).or_insert_with(|| match set.mesh_maps.and_then(|m| m.get(&kind)) {
            Some(img) => dense_at(img, size),
            None => {
                let d = kind.neutral_value();
                let n = (size * size) as usize;
                (3, (0..n).flat_map(|_| d).collect())
            }
        })
    }

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

    /// Effective opacity per texel: the Opacity channel (1 when absent) times the Base Color
    /// coverage (its composite alpha; 1 when absent or opaque).
    fn alpha(&mut self) -> &Vec<f32> {
        if self.alpha.is_none() {
            let n = (self.size * self.size) as usize;
            let mut out = vec![1.0f32; n];
            if self.set.source(ChannelKind::Opacity).is_some() {
                let (oc, o) = self.channel(ChannelKind::Opacity);
                let oc = *oc;
                out.iter_mut().enumerate().for_each(|(i, a)| *a = o[i * oc].clamp(0.0, 1.0));
            }
            if self.set.source(ChannelKind::BaseColor).is_some_and(|img| img.comps() == 4) {
                let (bc, b) = self.channel(ChannelKind::BaseColor);
                let bc = *bc;
                out.iter_mut().enumerate().for_each(|(i, a)| *a *= b[i * bc + 3].clamp(0.0, 1.0));
            }
            self.alpha = Some(out);
        }
        self.alpha.as_ref().expect("computed above")
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

/// Normal convention of `map`: the export window's override, else the map's, else the
/// template's, else the source's own.
fn normal_directx(map: &OutputMap, settings: &ExportSettings, source: bool) -> bool {
    settings.normal_directx.or(map.normal_directx).or(settings.template.normal_directx).unwrap_or(source)
}

/// Rec. 709 luminance weights (linear RGB).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

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
                // Component 3 of a colour channel is its stack's coverage; a channel stored
                // without one (absent: the default) is fully covered.
                if component == 3 && *cc < 4 {
                    (0..n).for_each(|i| out[i * comps + ci] = if invert { 0.0 } else { 1.0 });
                    continue;
                }
                let c = (component as usize).min(cc - 1);
                for i in 0..n {
                    let mut v = data[i * cc + c];
                    if invert {
                        v = 1.0 - v;
                    }
                    // Component 3 is coverage, not colour: it is never sRGB-encoded (alpha 0.5 was
                    // written as 188 instead of 128).
                    out[i * comps + ci] = if encode && component != 3 { linear_to_srgb(v) } else { v };
                }
            }
            Source::Normal { component, directx } => {
                let directx = normal_directx(map, settings, directx);
                let nm = dense.normal(settings.height_to_normal);
                let c = component.min(2) as usize;
                for i in 0..n {
                    let v = nm[i * 3 + c];
                    out[i * comps + ci] = if directx && c == 1 { 1.0 - v } else { v };
                }
            }
            Source::View2d { component } => {
                let Some(view) = settings.view_2d else { continue };
                let encode = map.srgb && matches!(view, View2d::Channel(k) if k.is_color()) && !format.is_float();
                let (cc, data) = dense.view(view);
                // Same as the panel: one component shows as gray, otherwise RGB; display clamps to 0..1.
                let c = if *cc == 1 { 0 } else { (component as usize).min(*cc - 1).min(2) };
                for i in 0..n {
                    let v = data[i * cc + c];
                    let v = if format.is_float() { v } else { v.clamp(0.0, 1.0) };
                    out[i * comps + ci] = if encode { linear_to_srgb(v.clamp(0.0, 1.0)) } else { v };
                }
            }
            Source::Height => {
                let (cc, data) = dense.channel(ChannelKind::Height);
                for i in 0..n {
                    let h = data[i * cc];
                    // Radiance HDR is a float format without negative values: signed height is
                    // remapped like the integer formats (raw, everything below 0 was written as 0).
                    out[i * comps + ci] = if format.is_float() && format != FileFormat::Hdr32 { h } else { h * 0.5 + 0.5 };
                }
            }
            Source::Gray { kind, invert } => {
                let encode = map.srgb && kind.is_color() && !format.is_float();
                let (cc, data) = dense.channel(kind);
                let cc = *cc;
                for i in 0..n {
                    let mut v = if cc >= 3 { (0..3).map(|c| data[i * cc + c] * LUMA[c]).sum::<f32>() } else { data[i * cc] };
                    if invert {
                        v = 1.0 - v;
                    }
                    out[i * comps + ci] = if encode { linear_to_srgb(v) } else { v };
                }
            }
            Source::Alpha => {
                let a = dense.alpha();
                (0..n).for_each(|i| out[i * comps + ci] = a[i]);
            }
            Source::MeshMap { kind, component, invert } => {
                let (cc, data) = dense.mesh_map(kind);
                let c = (component as usize).min(kind.components() - 1).min(*cc - 1);
                for i in 0..n {
                    let v = data[i * cc + c];
                    out[i * comps + ci] = if invert { 1.0 - v } else { v };
                }
            }
        }
    }
    out
}

/// What an empty texture set exports for `map` (one texel): channel defaults, a flat normal,
/// mid height... The fill colour of [`Padding::Background`].
fn background_value(map: &OutputMap, settings: &ExportSettings, format: FileFormat) -> Vec<f32> {
    if map.components.iter().any(|s| matches!(s, Source::View2d { .. })) {
        return vec![0.0; map.components.len()];
    }
    let empty = BTreeMap::new();
    let set = ExportTextureSet { name: "", material_slot: 0, udim: None, channels: &empty, mesh_maps: None };
    let mut dense = Dense { set: &set, size: 1, cache: HashMap::new(), normal: None, view: None, mesh_maps: HashMap::new(), alpha: None };
    evaluate_map(map, &mut dense, settings, format)
}

/// Pad `data` (`size`² texels, `comps` interleaved) outside `covered` with `padding`.
fn pad(data: &mut [f32], size: u32, comps: usize, covered: &[bool], padding: Padding, background: impl FnOnce() -> Vec<f32>) {
    let s = size as usize;
    match padding {
        Padding::None => {}
        Padding::Infinite => dilate_infinite(s, s, comps, data, covered),
        Padding::Texels(t) => {
            dilate_width(s, s, comps, data, covered, t);
        }
        Padding::Background(t) => {
            let grown = dilate_width(s, s, comps, data, covered, t);
            let bg = background();
            for (px, _) in data.chunks_exact_mut(comps).zip(&grown).filter(|(_, g)| !**g) {
                px.copy_from_slice(&bg);
            }
        }
        Padding::Diffusion(t) => {
            let grown = dilate_width(s, s, comps, data, covered, t);
            diffuse_fill(s, s, comps, data, &grown);
        }
    }
}

/// Whether a map's fourth component is an alpha (effective opacity or a colour stack's coverage)
/// over colour in the first three.
fn has_color_alpha(map: &OutputMap) -> bool {
    map.components.len() == 4
        && matches!(map.components[3], Source::Alpha | Source::Channel { component: 3, invert: false, .. })
        && map.components[..3].iter().all(|s| matches!(s, Source::Channel { kind, .. } if kind.is_color()))
}

/// Edge bleed of an RGBA colour map: texels with alpha 0 take the colour of the nearest texel
/// with alpha above 0, keeping their alpha. Without it, filtering and mipmaps blend the hidden
/// colour under transparent texels (the channel default grey) into the visible edges, which
/// shows as a grey fringe. Alpha and every visible texel are untouched (issue #145).
pub fn bleed_under_alpha(map: &OutputMap, size: usize, data: &mut [f32]) {
    if !has_color_alpha(map) {
        return;
    }
    let visible: Vec<bool> = data.chunks_exact(4).map(|p| p[3] > 0.0).collect();
    if visible.iter().all(|&v| v) || !visible.iter().any(|&v| v) {
        return;
    }
    let mut rgb: Vec<f32> = data.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    dilate_infinite(size, size, 3, &mut rgb, &visible);
    for (p, c) in data.chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
        p[..3].copy_from_slice(c);
    }
}

/// UV coverage of a texture set at `size` (inside + 1-texel rim).
pub fn coverage(mesh: &Mesh, material_slot: u32, size: u32) -> Vec<bool> {
    let s = rasterize_surface(mesh, mesh.material_triangles(material_slot), size, size, 1.0);
    (0..s.triangle.len()).map(|i| s.covered(i)).collect()
}

/// Write every applicable map of every texture set into `dir`. Returns the written files.
pub fn export(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    // An empty folder is the process's working directory, wherever that happens to be.
    if dir.as_os_str().is_empty() {
        return Err(ExportError::Folder(dir.to_path_buf(), std::io::Error::new(std::io::ErrorKind::InvalidInput, "choose an export folder")));
    }
    // Before anything is written: the second file would silently replace the first.
    check_outputs(mesh_name, sets, settings)?;
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

/// Resample a square image (`comps` interleaved) from `src` to `dst` texels per side: box
/// average when shrinking by a whole factor, bilinear (texel centres, clamped) otherwise.
pub fn resample(data: &[f32], comps: usize, src: u32, dst: u32) -> Vec<f32> {
    if src == dst {
        return data.to_vec();
    }
    let (s, d) = (src as usize, dst as usize);
    let mut out = vec![0.0f32; d * d * comps];
    if d < s && s % d == 0 {
        let f = s / d;
        let inv = 1.0 / (f * f) as f32;
        out.par_chunks_mut(d * comps).enumerate().for_each(|(y, row)| {
            for x in 0..d {
                for c in 0..comps {
                    let mut acc = 0.0;
                    for sy in y * f..(y + 1) * f {
                        for sx in x * f..(x + 1) * f {
                            acc += data[(sy * s + sx) * comps + c];
                        }
                    }
                    row[x * comps + c] = acc * inv;
                }
            }
        });
        return out;
    }
    let scale = s as f32 / d as f32;
    out.par_chunks_mut(d * comps).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) * scale - 0.5).clamp(0.0, (s - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(s - 1);
        for x in 0..d {
            let fx = ((x as f32 + 0.5) * scale - 0.5).clamp(0.0, (s - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(s - 1);
            for c in 0..comps {
                let at = |xx: usize, yy: usize| data[(yy * s + xx) * comps + c];
                let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                row[x * comps + c] = top + (bottom - top) * ty;
            }
        }
    });
    out
}

/// Directory and archive exports share rendering, encoding, templates and padding.
pub(crate) fn render_maps(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, mut write: impl FnMut(&str,&[u8])->Result<(),ExportError>) -> Result<usize,ExportError> {
    // Presets read from a file or imported were never checked (only the editor checks them): a
    // suffix such as `Base:Color` named an NTFS stream or aborted the export halfway, and a size
    // of 100000 tried to allocate 120 GB. Nothing is written when the preset has a problem.
    if let Some(problem) = settings.template.problems().into_iter().next() {
        return Err(ExportError::Template(problem));
    }
    let mut count=0;
    for set in sets {
        let Some(size) = set.channels.values().next().map(|i| i.width()) else { continue };
        let mut covered: Option<Vec<bool>> = None;
        let mut dense = Dense { set, size, cache: HashMap::new(), normal: None, view: None, mesh_maps: HashMap::new(), alpha: None };
        for map in settings.template.maps.iter().filter(|m| map_applies(m, set, settings) && settings.selection.is_on(set.name, &m.suffix)) {
            if map.suffix.contains(['/', '\\']) || map.suffix.contains("..") {
                return Err(ExportError::Archive(format!("invalid map suffix: {}", map.suffix)));
            }
            let format = output_format(map, settings);
            let comps = map.components.len();
            let mut data = evaluate_map(map, &mut dense, settings, format);
            bleed_under_alpha(map, size as usize, &mut data);
            let padding = map.padding.or(settings.template.padding).unwrap_or(settings.padding);
            if padding != Padding::None {
                let covered = covered.get_or_insert_with(|| coverage(mesh, set.material_slot, size));
                pad(&mut data, size, comps, covered, padding, || background_value(map, settings, format));
            }
            // Size override after padding, so shrinking never pulls unpadded gutter into islands.
            let out_size = output_size(map, settings, size);
            if out_size != size {
                data = resample(&data, comps, size, out_size);
            }
            let name = output_path(mesh_name, set, &map.suffix, format.extension(), settings);
            let dither = settings.dither.or(map.dither).unwrap_or(false);
            let bytes=subcraft_image::io::encode_image_dithered(out_size,out_size,comps,&data,format,dither).map_err(|e|ExportError::Write(PathBuf::from(&name),e.to_string()))?;
            write(&name,&bytes)?;count+=1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
