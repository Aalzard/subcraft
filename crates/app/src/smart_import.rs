//! Importing and exporting smart materials.
//!
//! Three inputs end up as `.scsmart` files in the user library (never in the bundled assets):
//!
//! * SubCraft smart-material files (`.scsmart`), validated and copied in;
//! * folders that contain such files (searched recursively);
//! * PBR texture sets (ambientCG style: `Bricks059_2K-JPG_Color.jpg`, `..._NormalGL.jpg`, ...)
//!   which become one fill-layer smart material per material, maps assigned by file name.
//!
//! Nothing here panics on bad input: every problem is a line in [`SmartImport::errors`].
//! Archive entries are only looked up by name, never extracted to disk, so there is no path to
//! traverse; library file names come from a sanitized stem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_assets::texture_maps::{self, MapConversion};
use subcraft_assets::{SmartMaterial, UserAssetKind, UserLibrary};
use subcraft_core::{ChannelKind, ResourceId};
use subcraft_doc::{ChannelDef, FillChannel, FillContent, FillSource, Layer, LayerContent, Resource, ResourceKind, ResourceStore, TextureSet};
use subcraft_project::fragment::{Fragment, FragmentKind, load_fragment, save_fragment};

use crate::library::smart_material_from;

/// Largest `.scsmart` file read (its images are decoded in memory).
pub const MAX_SMART_FILE_BYTES: u64 = 1 << 30;
/// Largest single texture file of a PBR set.
pub const MAX_MAP_FILE_BYTES: u64 = 256 << 20;
/// Largest side of a texture of a PBR set.
pub const MAX_MAP_SIDE: u32 = 8192;
/// Materials created by one import (a pack beyond this is cut off and says so).
pub const MAX_MATERIALS: usize = 500;
/// Files looked at while walking folders.
pub const MAX_FILES_SCANNED: usize = 20_000;
/// Folder nesting followed below a dropped folder.
pub const MAX_DEPTH: usize = 8;
/// Resolution field of a generated material; fill layers have no rasters so any valid value works.
const FRAGMENT_RESOLUTION: u32 = 1024;
/// Error / note lines kept in a report.
const MAX_REPORT_LINES: usize = 50;

/// Channels an imported material may bring that the texture set might not have yet.
const ADDED_ON_APPLY: [ChannelKind; 3] = [ChannelKind::AmbientOcclusion, ChannelKind::Emissive, ChannelKind::Opacity];

/// What an import did.
#[derive(Default)]
pub struct SmartImport {
    /// New shelf entries (already saved in the library).
    pub imported: Vec<SmartMaterial>,
    /// Names of materials that were already in the library unchanged.
    pub already_present: Vec<String>,
    /// Files / folders that could not be imported, with the reason.
    pub errors: Vec<String>,
    /// Informational lines (renamed on collision, ignored files, which map went where).
    pub notes: Vec<String>,
}

impl SmartImport {
    /// One status-line sentence.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match self.imported.as_slice() {
            [] => {}
            [one] => parts.push(format!("Imported smart material \"{}\"", one.name)),
            many => parts.push(format!("Imported {} smart materials", many.len())),
        }
        if !self.already_present.is_empty() {
            parts.push(format!("{} already in the library", self.already_present.len()));
        }
        if !self.errors.is_empty() {
            parts.push(format!("{} problem(s), see the log", self.errors.len()));
        }
        if parts.is_empty() {
            "Nothing to import: use .scsmart files, or folders with PBR texture sets (Color, NormalGL, Roughness…)".into()
        } else {
            parts.join("; ")
        }
    }

    /// Every error and note, for a dialog or the log.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.errors.iter().map(|e| format!("error: {e}")).collect();
        out.extend(self.notes.iter().cloned());
        out
    }

    fn error(&mut self, msg: String) {
        if self.errors.len() < MAX_REPORT_LINES {
            self.errors.push(msg);
        } else if self.errors.len() == MAX_REPORT_LINES {
            self.errors.push("…more problems not listed".into());
        }
    }

    fn note(&mut self, msg: String) {
        if self.notes.len() < MAX_REPORT_LINES {
            self.notes.push(msg);
        }
    }
}

/// Files found under the dropped / picked paths.
#[derive(Default)]
struct Found {
    smart_files: Vec<PathBuf>,
    /// Image files by containing folder.
    images: BTreeMap<PathBuf, Vec<PathBuf>>,
    scanned: usize,
}

/// Import everything recognizable under `paths` into `lib`. `existing` are the smart materials the
/// shelf already has (identical imports are not added twice).
pub fn import_smart_materials(paths: &[PathBuf], lib: &UserLibrary, existing: &[SmartMaterial]) -> SmartImport {
    let mut report = SmartImport::default();
    let mut found = Found::default();
    for p in paths {
        let meta = match std::fs::metadata(p) {
            Ok(m) => m,
            Err(e) => {
                report.error(format!("{}: {e}", p.display()));
                continue;
            }
        };
        if meta.is_dir() {
            walk(p, 0, &mut found, &mut report);
        } else {
            classify_file(p, &mut found, &mut report, true);
        }
    }
    let mut known: Vec<SmartMaterial> = existing.to_vec();
    for file in std::mem::take(&mut found.smart_files) {
        if report.imported.len() >= MAX_MATERIALS {
            report.error(format!("more than {MAX_MATERIALS} materials in one import; stopped at {}", file.display()));
            break;
        }
        match import_smart_file(&file, lib, &known) {
            Ok(Added::New(m, note)) => {
                report.notes.extend(note);
                known.push(m.clone());
                report.imported.push(m);
            }
            Ok(Added::Present(name)) => report.already_present.push(name),
            Err(e) => report.error(e),
        }
    }
    for (dir, files) in std::mem::take(&mut found.images) {
        import_texture_folder(&dir, &files, lib, &mut known, &mut report);
    }
    report
}

fn walk(dir: &Path, depth: usize, found: &mut Found, report: &mut SmartImport) {
    if depth > MAX_DEPTH {
        report.note(format!("{}: folders nested deeper than {MAX_DEPTH} levels are not searched", dir.display()));
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            report.error(format!("{}: {e}", dir.display()));
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if found.scanned >= MAX_FILES_SCANNED {
            report.note(format!("stopped after scanning {MAX_FILES_SCANNED} files"));
            return;
        }
        found.scanned += 1;
        // Links inside a dropped folder are not followed (they could point out of it or loop).
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(&p, depth + 1, found, report);
        } else if meta.is_file() {
            classify_file(&p, found, report, false);
        }
    }
}

fn classify_file(p: &Path, found: &mut Found, report: &mut SmartImport, explicit: bool) {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if ext == "scsmart" {
        found.smart_files.push(p.to_path_buf());
    } else if subcraft_image::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
        found.images.entry(dir).or_default().push(p.to_path_buf());
    } else if explicit {
        let hint = if ext == "scmask" { " (that is a smart mask; import it from the Smart Masks tab)" } else { "" };
        report.error(format!("{}: not a smart material or an image{hint}", p.display()));
    }
}

enum Added {
    New(SmartMaterial, Option<String>),
    Present(String),
}

/// Validate a `.scsmart` file and copy it into the library.
fn import_smart_file(src: &Path, lib: &UserLibrary, existing: &[SmartMaterial]) -> Result<Added, String> {
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let size = std::fs::metadata(src).map_err(|e| format!("{name}: {e}"))?.len();
    if size > MAX_SMART_FILE_BYTES {
        return Err(format!("{name}: {} MB is larger than the {} MB limit", size >> 20, MAX_SMART_FILE_BYTES >> 20));
    }
    let fragment = load_fragment(src).map_err(|e| format!("{name}: {e}"))?;
    if fragment.kind != FragmentKind::SmartMaterial {
        return Err(format!("{name}: this is a smart mask, not a smart material"));
    }
    // Validates the shape (one layer) before anything is written to the library.
    smart_material_from(fragment.clone(), src.to_path_buf()).map_err(|_| format!("{name}: not a smart material (it must hold exactly one layer)"))?;
    let dest = lib.add_file(UserAssetKind::SmartMaterial, src)?;
    if let Some(e) = existing.iter().find(|m| m.user_file.as_deref() == Some(dest.as_path())) {
        return Ok(Added::Present(e.name.clone()));
    }
    let stem = |p: &Path| p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let note = (stem(&dest) != stem(src)).then(|| format!("{name}: a different file of that name exists, saved as {}", stem(&dest)));
    let material = smart_material_from(fragment, dest).map_err(|e| format!("{name}: {e}"))?;
    Ok(Added::New(material, note))
}

/// Words that name a resolution or file format rather than the material.
fn is_noise_word(w: &str) -> bool {
    let w = w.to_ascii_lowercase();
    matches!(w.as_str(), "jpg" | "jpeg" | "png" | "exr" | "tga" | "tif" | "tiff" | "bmp" | "hdr")
        || (w.len() > 1 && w.ends_with('k') && w[..w.len() - 1].bytes().all(|b| b.is_ascii_digit()))
}

/// Material name from the text before the map-type words (`Bricks059_2K-JPG_` -> `Bricks059`).
fn material_name(prefix: &str) -> String {
    prefix
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !is_noise_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One folder of images: group by name prefix (several materials may share a folder) and make a
/// smart material of each group that has recognized maps.
fn import_texture_folder(dir: &Path, files: &[PathBuf], lib: &UserLibrary, known: &mut Vec<SmartMaterial>, report: &mut SmartImport) {
    let mut groups: BTreeMap<String, (String, Vec<PathBuf>)> = BTreeMap::new();
    let mut unrecognized = 0usize;
    for f in files {
        let file_name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        match texture_maps::split_map_name(file_name) {
            Some((prefix, _)) => {
                let key = prefix.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("_").to_ascii_lowercase();
                groups.entry(key).or_insert_with(|| (material_name(&prefix), Vec::new())).1.push(f.clone());
            }
            None => unrecognized += 1,
        }
    }
    if unrecognized > 0 && groups.is_empty() {
        report.note(format!("{}: {unrecognized} image(s) ignored, no map names recognized (Color, NormalGL, Roughness…)", dir.display()));
    }
    for (_, (mut name, maps)) in groups {
        if report.imported.len() >= MAX_MATERIALS {
            report.error(format!("more than {MAX_MATERIALS} materials in one import; the rest of {} was skipped", dir.display()));
            return;
        }
        if name.is_empty() {
            name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("Imported material").to_string();
        }
        match build_texture_material(&name, &maps, report) {
            Ok(fragment) => match store_material(fragment, lib, known) {
                Ok(Added::New(m, note)) => {
                    report.notes.extend(note);
                    known.push(m.clone());
                    report.imported.push(m);
                }
                Ok(Added::Present(n)) => report.already_present.push(n),
                Err(e) => report.error(format!("{name}: {e}")),
            },
            Err(e) if e.is_empty() => {}
            Err(e) => report.error(format!("{name}: {e}")),
        }
    }
}

/// Decode, validate and convert one map; returns the resource to embed.
fn load_map(path: &Path, conversion: MapConversion) -> Result<Resource, String> {
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
    let size = std::fs::metadata(path).map_err(|e| format!("{file}: {e}"))?.len();
    if size > MAX_MAP_FILE_BYTES {
        return Err(format!("{file}: {} MB is larger than the {} MB limit", size >> 20, MAX_MAP_FILE_BYTES >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{file}: {e}"))?;
    let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some(&file)).map_err(|e| format!("{file}: {e}"))?;
    if bitmap.width > MAX_MAP_SIDE || bitmap.height > MAX_MAP_SIDE {
        return Err(format!("{file}: {}x{} is larger than {MAX_MAP_SIDE} pixels", bitmap.width, bitmap.height));
    }
    let (bitmap, bytes, name) = match conversion {
        MapConversion::None => (bitmap, bytes, file),
        _ => {
            let converted = if conversion == MapConversion::Invert {
                bitmap.map_color(|_, v| 1.0 - v)
            } else {
                bitmap.map_color(|c, v| if c == 1 { 1.0 - v } else { v })
            };
            let (encoded, ext) = subcraft_image::encode_bitmap(&converted).map_err(|e| format!("{file}: {e}"))?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
            let suffix = if conversion == MapConversion::Invert { "roughness" } else { "opengl" };
            (converted, encoded, format!("{stem} ({suffix}).{ext}"))
        }
    };
    Ok(Resource { id: ResourceId::new(), name, kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: Some(Arc::new(bytes)) })
}

/// The smart material fragment for one material's map files.
fn build_texture_material(name: &str, files: &[PathBuf], report: &mut SmartImport) -> Result<Fragment, String> {
    let plan = texture_maps::plan(files);
    for (p, why) in &plan.skipped {
        report.note(format!("{name}: {} skipped: {why}", p.file_name().and_then(|n| n.to_str()).unwrap_or_default()));
    }
    if plan.maps.is_empty() {
        return Err("no usable texture maps".into());
    }
    let mut ts = TextureSet::new("import", 0, FRAGMENT_RESOLUTION);
    for (_, m) in &plan.maps {
        if !ts.has_channel(m.channel) && ADDED_ON_APPLY.contains(&m.channel) {
            ts.channels.push(ChannelDef::default_for(m.channel));
        }
    }
    let mut layer = Layer::new_fill(name, &ts);
    let LayerContent::Fill(FillContent { channels, .. }) = &mut layer.content else { unreachable!("new_fill makes a fill layer") };
    for c in channels.values_mut() {
        c.enabled = false;
    }
    let mut resources = ResourceStore::default();
    let mut used = Vec::new();
    for (path, m) in &plan.maps {
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if !ts.has_channel(m.channel) {
            report.note(format!("{name}: {file} skipped: {} maps are not part of a smart material", m.channel.display_name()));
            continue;
        }
        let resource = match load_map(path, m.conversion) {
            Ok(r) => r,
            Err(e) => {
                report.error(format!("{name}: {e}"));
                continue;
            }
        };
        let id = resources.insert(resource);
        let strength = if m.channel == ChannelKind::Emissive { 3.0 } else { 1.0 };
        channels.insert(m.channel, FillChannel { strength, ..FillChannel::new(FillSource::Bitmap { resource: id }) });
        used.push(format!("{file} → {}", m.channel.display_name()));
    }
    if used.is_empty() {
        // Every file already reported its own problem.
        return Err(String::new());
    }
    report.note(format!("{name}: {}", used.join(", ")));
    Ok(Fragment {
        kind: FragmentKind::SmartMaterial,
        name: name.to_string(),
        resolution: FRAGMENT_RESOLUTION,
        layers: vec![layer],
        mask_effects: Vec::new(),
        resources,
    })
}

/// Same images (byte for byte) as a library smart material: re-importing a folder adds nothing.
fn same_images(fragment: &Fragment, m: &SmartMaterial) -> bool {
    let mut mine: Vec<&[u8]> = fragment
        .referenced_resources()
        .into_iter()
        .filter_map(|id| fragment.resources.get(id))
        .filter_map(|r| r.encoded.as_deref().map(|v| v.as_slice()))
        .collect();
    let mut theirs: Vec<&[u8]> = m.resources().iter().filter_map(|r| r.encoded.as_deref().map(|v| v.as_slice())).collect();
    mine.sort();
    theirs.sort();
    !mine.is_empty() && mine == theirs
}

fn store_material(fragment: Fragment, lib: &UserLibrary, known: &[SmartMaterial]) -> Result<Added, String> {
    if let Some(m) = known.iter().find(|m| m.user_file.is_some() && same_images(&fragment, m)) {
        return Ok(Added::Present(m.name.clone()));
    }
    let dir = lib.folder(UserAssetKind::SmartMaterial);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = lib.new_file_path(UserAssetKind::SmartMaterial, &fragment.name);
    save_fragment(&path, &fragment).map_err(|e| format!("{}: {e}", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let note = (stem != fragment.name).then(|| format!("a smart material named \"{}\" exists, saved as \"{stem}\"", fragment.name));
    let material = smart_material_from(fragment, path).map_err(|e| e.to_string())?;
    Ok(Added::New(material, note))
}

/// Write a library smart material to `dest` (a shareable `.scsmart`). The library file is re-read
/// first so a damaged one is reported instead of copied; the write is atomic.
pub fn export_smart_material(m: &SmartMaterial, dest: &Path) -> Result<(), String> {
    let src = m.user_file.as_ref().ok_or("built-in smart materials can't be exported; save a layer to the library first")?;
    export_smart_file(src, &m.name, dest)
}

/// [`export_smart_material`] for a library file.
pub fn export_smart_file(src: &Path, name: &str, dest: &Path) -> Result<(), String> {
    let mut fragment = load_fragment(src).map_err(|e| format!("{}: {e}", src.display()))?;
    fragment.name = name.to_string();
    let dest = if dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("scsmart")) { dest.to_path_buf() } else { dest.with_extension("scsmart") };
    save_fragment(&dest, &fragment).map_err(|e| format!("{}: {e}", dest.display()))
}

/// Fill-layer channels in `layer` (and its children) that a texture set may have to gain.
pub fn channels_to_add(layer: &Layer) -> Vec<ChannelKind> {
    let mut out = Vec::new();
    collect_channels(layer, &mut out);
    out
}

fn collect_channels(layer: &Layer, out: &mut Vec<ChannelKind>) {
    match &layer.content {
        LayerContent::Fill(f) => {
            for (k, c) in &f.channels {
                if c.enabled && ADDED_ON_APPLY.contains(k) && !out.contains(k) {
                    out.push(*k);
                }
            }
        }
        LayerContent::Group(g) => {
            for c in &g.children {
                collect_channels(c, out);
            }
        }
        _ => {}
    }
}
