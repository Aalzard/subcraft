//! Cache of rebuilt Painter stacks. Rebuilding a big project's stack takes minutes; the result
//! only depends on the project file and the importer, so it is stored under SubCraft's local data
//! folder (never next to the project), keyed by the SHA-256 of the file and the importer version,
//! and read back on the next open. A changed file hashes differently; a changed importer bumps
//! [`IMPORTER_VERSION`]. The stack is stored with the project writer, so it comes back exactly.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subcraft_core::ResourceId;
use subcraft_doc::{Document, ProjectSettings, Resource, ResourceKind, ResourceStore, TextureSet};
use subcraft_image::{Bitmap, BitmapData};

use crate::spp_job::{SppStack, SppStackUpdate};

/// Bump whenever the rebuild (or the quality check) can produce a different stack.
pub const IMPORTER_VERSION: u32 = 8;

/// Entries kept; the least recently written ones go first.
const KEEP: usize = 3;

#[derive(Serialize, Deserialize)]
struct Meta {
    importer: u32,
    /// Per stored texture set, in order: its name and whether the flattened layer is shown.
    sets: Vec<(String, bool)>,
    notes: Vec<String>,
}

/// SHA-256 of the file, as hex.
pub fn file_key(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Where stacks are cached and from which rebuild time on (small projects are not worth a file).
#[derive(Clone, Debug)]
pub struct SppCache {
    pub dir: PathBuf,
    pub min_rebuild: std::time::Duration,
}

impl SppCache {
    /// The per-user cache; developer switches (`SUBCRAFT_SPP_*`) change the result, so they turn it off.
    pub fn default_location() -> Option<Self> {
        if std::env::vars_os().any(|(k, _)| k.to_string_lossy().starts_with("SUBCRAFT_SPP_") && k != "SUBCRAFT_SPP_TIMING" && k != "SUBCRAFT_SPP_MEMLOG") {
            return None;
        }
        crate::recovery::cache_dir().map(|d| Self { dir: d.join("spp-cache"), min_rebuild: std::time::Duration::from_secs(10) })
    }
}

fn paths(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (dir.join(format!("{key}-v{IMPORTER_VERSION}.subcraft")), dir.join(format!("{key}-v{IMPORTER_VERSION}.json")))
}

fn resources_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}-v{IMPORTER_VERSION}.res"))
}

#[derive(Serialize, Deserialize)]
struct ResourceMeta {
    name: String,
    kind: ResourceKind,
    width: u32,
    height: u32,
    channels: u8,
    /// `u8`, `u16` or `f32`.
    format: String,
    has_encoded: bool,
}

/// The project writer keeps only resources that still have their original file bytes; the
/// rebuilt stack also uses textures made by the importer (solved textures, leveled grunge), so
/// every resource goes into this archive as raw samples, exactly.
fn write_resources(path: &Path, resources: &ResourceStore) -> Result<(), String> {
    use std::io::Write;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for r in resources.iter() {
        let (format, data): (&str, Vec<u8>) = match &r.bitmap.data {
            BitmapData::U8(d) => ("u8", d.clone()),
            BitmapData::U16(d) => ("u16", d.iter().flat_map(|v| v.to_le_bytes()).collect()),
            BitmapData::F32(d) => ("f32", d.iter().flat_map(|v| v.to_le_bytes()).collect()),
        };
        let meta = ResourceMeta { name: r.name.clone(), kind: r.kind, width: r.bitmap.width, height: r.bitmap.height, channels: r.bitmap.channels, format: format.into(), has_encoded: r.encoded.is_some() };
        let id = r.id.0;
        zip.start_file(format!("{id}.json"), options).map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec(&meta).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        zip.start_file(format!("{id}.bin"), options).map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
        if let Some(bytes) = &r.encoded {
            zip.start_file(format!("{id}.enc"), options).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn read_resources(path: &Path) -> Result<ResourceStore, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("no resources ({e})"))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).map(|n| n.to_string()).collect();
    let mut store = ResourceStore::default();
    for name in names.iter().filter(|n| n.ends_with(".json")) {
        let id = name.trim_end_matches(".json");
        let uuid = uuid::Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let mut read = |entry: String| -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            zip.by_name(&entry).map_err(|e| e.to_string())?.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            Ok(bytes)
        };
        let meta: ResourceMeta = serde_json::from_slice(&read(name.clone())?).map_err(|e| e.to_string())?;
        let raw = read(format!("{id}.bin"))?;
        let samples = meta.width as usize * meta.height as usize * meta.channels as usize;
        let data = match meta.format.as_str() {
            "u8" if raw.len() == samples => BitmapData::U8(raw),
            "u16" if raw.len() == samples * 2 => BitmapData::U16(raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()),
            "f32" if raw.len() == samples * 4 => BitmapData::F32(raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()),
            other => return Err(format!("resource {}: unexpected data ({other}, {} bytes)", meta.name, raw.len())),
        };
        let encoded = if meta.has_encoded { Some(Arc::new(read(format!("{id}.enc"))?)) } else { None };
        store.insert(Resource { id: ResourceId(uuid), name: meta.name, kind: meta.kind, bitmap: Arc::new(Bitmap { width: meta.width, height: meta.height, channels: meta.channels, data }), source_path: None, encoded });
    }
    Ok(store)
}

/// The stack stored for `key`, applied to `sets` (the opened texture sets, matched by name).
/// `Err`: nothing usable is stored (with the reason).
pub fn load(dir: &Path, key: &str, sets: &[TextureSet]) -> Result<SppStack, String> {
    let (data, meta) = paths(dir, key);
    let meta: Meta = serde_json::from_slice(&std::fs::read(meta).map_err(|e| format!("no entry ({e})"))?).map_err(|e| format!("unreadable description ({e})"))?;
    if meta.importer != IMPORTER_VERSION {
        return Err("written by another importer version".into());
    }
    let project = subcraft_project::load_project_trusted(&data).map_err(|e| format!("unreadable stack ({e})"))?;
    if project.document.texture_sets.len() != meta.sets.len() {
        return Err("texture set count differs".into());
    }
    let mut updates = Vec::new();
    for (stored, (name, flat_visible)) in project.document.texture_sets.iter().zip(&meta.sets) {
        let ts = sets.iter().find(|s| &s.name == name).ok_or("texture set not found")?;
        if ts.resolution != stored.resolution || ts.layers.len() != 2 {
            return Err("texture set differs".into());
        }
        let mut flat = (*ts.layers[0]).clone();
        flat.visible = *flat_visible;
        updates.push(SppStackUpdate { texture_set: ts.id, flat: Arc::new(flat), folder: stored.layers.first().ok_or("empty stack")?.clone() });
    }
    let resources = read_resources(&resources_path(dir, key))?;
    let mut notes = meta.notes;
    notes.push("    The rebuilt stack was read from the import cache.".into());
    Ok(SppStack { updates, resources, notes })
}

/// Store `stack` (whose updates belong to `sets`) under `key`. Best effort: a failure only means
/// the next open rebuilds again.
pub fn store(dir: &Path, key: &str, stack: &SppStack, sets: &[TextureSet]) {
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let (data, meta) = paths(dir, key);
        let mut document = Document::default();
        let mut stored = Vec::new();
        for u in &stack.updates {
            let Some(ts) = sets.iter().find(|s| s.id == u.texture_set) else { continue };
            let mut ts = ts.clone();
            ts.layers = vec![u.folder.clone()];
            document.texture_sets.push(ts.clone());
            stored.push((ts.name.clone(), u.flat.visible));
        }
        let meta_value = Meta { importer: IMPORTER_VERSION, sets: stored, notes: stack.notes.clone() };
        let tmp = data.with_extension("tmp");
        write_resources(&resources_path(dir, key), &stack.resources)?;
        subcraft_project::save_project_with(&tmp, &subcraft_mesh::primitives::cube(1.0), &document, &ResourceStore::default(), &Default::default(), &ProjectSettings::default()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &data).map_err(|e| e.to_string())?;
        std::fs::write(&meta, serde_json::to_vec(&meta_value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        prune(dir);
        Ok(())
    })();
    if let Err(e) = result {
        log::warn!("Painter import cache: could not store the rebuilt stack: {e}");
    }
}

/// Keep the newest [`KEEP`] entries.
fn prune(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "subcraft"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in entries.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old.with_extension("json"));
        let _ = std::fs::remove_file(old.with_extension("res"));
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_doc::{Filter, Layer, Mask, MaskEffect, MaskEffectKind};

    /// What the importer makes and the project writer would drop or reject: textures without
    /// original file bytes, and filter parameters outside the editable range.
    #[test]
    fn a_stored_stack_comes_back_exactly_with_its_generated_textures() {
        let dir = std::env::temp_dir().join(format!("subcraft-spp-cache-unit-{}", std::process::id()));
        let mut ts = TextureSet::new("T", 0, 128);
        let tc = ts.clone();
        ts.insert_above(None, Layer::new_paint("flat", &tc));
        ts.insert_above(None, Layer::new_paint("top", &tc));
        let mut folder = Layer::new_folder("stack", &tc);
        let mut mask = Mask::new(128, true);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Blur { radius: 90.0 }))));
        folder.mask = Some(mask);
        let mut resources = ResourceStore::default();
        let solved = resources.insert(Resource { id: ResourceId::new(), name: "solved".into(), kind: ResourceKind::Texture, bitmap: Arc::new(Bitmap::new_f32(3, 2, 1, vec![0.0, 0.1, 0.25, 1e-8, 0.999, 1.5])), source_path: None, encoded: None });
        let file = resources.insert(Resource { id: ResourceId::new(), name: "file".into(), kind: ResourceKind::Alpha, bitmap: Arc::new(Bitmap::new_u8(2, 1, 3, vec![1, 2, 3, 250, 251, 252])), source_path: None, encoded: Some(Arc::new(vec![9, 8, 7])) });
        let mut flat = (*ts.layers[0]).clone();
        flat.visible = false;
        let stack = SppStack { updates: vec![SppStackUpdate { texture_set: ts.id, flat: Arc::new(flat), folder: Arc::new(folder) }], resources, notes: vec!["note".into()] };
        store(&dir, "abc", &stack, std::slice::from_ref(&ts));
        let back = load(&dir, "abc", std::slice::from_ref(&ts)).expect("stored stack is read");
        assert!(back.updates[0].folder == stack.updates[0].folder, "the folder is unchanged");
        assert!(!back.updates[0].flat.visible && back.updates[0].flat.id == ts.layers[0].id);
        for id in [solved, file] {
            let (a, b) = (stack.resources.get(id).unwrap(), back.resources.get(id).unwrap());
            assert_eq!((&a.name, a.kind, &a.bitmap, &a.encoded), (&b.name, b.kind, &b.bitmap, &b.encoded));
        }
        assert_eq!(back.notes[0], "note");
        assert!(load(&dir, "other", std::slice::from_ref(&ts)).is_err(), "a different key finds nothing");
        std::fs::remove_dir_all(&dir).ok();
    }
}
