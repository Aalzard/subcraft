//! Project files (`.subcraft`): a zip container with a versioned logical schema.
//!
//! ```text
//! manifest.json        format name + schema version (read first, gates everything else)
//! mesh.bin             mesh::serialize blob
//! document.json        schema v1: texture sets, layer tree, masks, raster references
//! rasters/NNNNN.tiles  one sparse TiledImage per paint channel / mask base / mask paint effect
//! resources/<id>.<ext> original bytes of imported bitmaps referenced by fills
//! ```
//!
//! Saving is atomic: the container is written to a temporary file next to the target, synced,
//! then renamed over it, so a crash never leaves a half-written project.

pub mod raster;
pub mod schema;

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_core::{EffectId, LayerId, ResourceId, TextureSetId};
use subcraft_doc::{
    ChannelBlend, ChannelDef, Document, FillChannel, FillContent, FillSource, GroupContent, Layer, LayerContent, Mask, MaskEffect,
    MaskEffectKind, PaintContent, Resource, ResourceStore, TextureSet,
};
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;
use zip::write::SimpleFileOptions;

use schema::*;

pub const EXTENSION: &str = "subcraft";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid project container: {0}")]
    Container(String),
    #[error("not a SubCraft project")]
    NotAProject,
    #[error("project was saved by a newer SubCraft (schema {0}, this version reads up to {SCHEMA_VERSION})")]
    UnsupportedVersion(u32),
    #[error("project data is invalid: {0}")]
    Invalid(String),
}

impl From<zip::result::ZipError> for ProjectError {
    fn from(e: zip::result::ZipError) -> Self {
        ProjectError::Container(e.to_string())
    }
}

/// Everything a project file holds.
#[derive(Debug)]
pub struct ProjectData {
    pub mesh: Mesh,
    pub document: Document,
    pub resources: ResourceStore,
}

// ----- save ----------------------------------------------------------------------------------

struct Writer {
    rasters: Vec<(String, Vec<u8>)>,
}

impl Writer {
    fn raster(&mut self, img: &TiledImage) -> String {
        let name = format!("rasters/{:05}.tiles", self.rasters.len());
        self.rasters.push((name.clone(), raster::encode(img)));
        name
    }

    fn layer(&mut self, l: &Layer) -> LayerV1 {
        let blend = l.blend.iter().map(|(&channel, b)| BlendV1 { channel, mode: b.mode, opacity: b.opacity }).collect();
        let mask = l.mask.as_ref().map(|m| MaskV1 {
            enabled: m.enabled,
            base: self.raster(&m.base),
            effects: m
                .effects
                .iter()
                .map(|e| MaskEffectV1 {
                    id: e.id.0,
                    name: e.name.clone(),
                    visible: e.visible,
                    blend: e.blend,
                    opacity: e.opacity,
                    kind: match &e.kind {
                        MaskEffectKind::Paint { image } => MaskEffectKindV1::Paint { raster: self.raster(image) },
                        MaskEffectKind::Fill { value, procedural, projection } => {
                            MaskEffectKindV1::Fill { value: *value, procedural: *procedural, projection: *projection }
                        }
                        MaskEffectKind::Generator(g) => MaskEffectKindV1::Generator { generator: *g },
                        MaskEffectKind::Levels(l) => MaskEffectKindV1::Levels { levels: *l },
                        MaskEffectKind::Invert => MaskEffectKindV1::Invert,
                    },
                })
                .collect(),
        });
        let content = match &l.content {
            LayerContent::Paint(p) => ContentV1::Paint {
                channels: p.channels.iter().map(|(&channel, img)| PaintChannelV1 { channel, raster: self.raster(img) }).collect(),
            },
            LayerContent::Fill(f) => ContentV1::Fill {
                projection: f.projection,
                channels: f
                    .channels
                    .iter()
                    .map(|(&channel, c)| FillChannelV1 { channel, enabled: c.enabled, source: c.source.clone() })
                    .collect(),
            },
            LayerContent::Group(g) => ContentV1::Folder { expanded: g.expanded, children: g.children.iter().map(|c| self.layer(c)).collect() },
        };
        LayerV1 { id: l.id.0, name: l.name.clone(), visible: l.visible, blend, mask, content }
    }
}

fn referenced_resources(doc: &Document) -> Vec<ResourceId> {
    let mut out = Vec::new();
    for ts in &doc.texture_sets {
        ts.walk(|l, _| {
            if let LayerContent::Fill(f) = &l.content {
                for c in f.channels.values() {
                    if let FillSource::Bitmap { resource } = c.source
                        && !out.contains(&resource)
                    {
                        out.push(resource);
                    }
                }
            }
        });
    }
    out
}

/// Write a project container to any seekable writer (files, memory buffers in tests).
pub fn write_project<W: Write + Seek>(w: W, mesh: &Mesh, doc: &Document, resources: &ResourceStore) -> Result<W, ProjectError> {
    let mut writer = Writer { rasters: Vec::new() };
    let texture_sets = doc
        .texture_sets
        .iter()
        .map(|ts| TextureSetV1 {
            id: ts.id.0,
            name: ts.name.clone(),
            material_slot: ts.material_slot,
            resolution: ts.resolution,
            channels: ts.channels.iter().map(|c| ChannelDefV1 { kind: c.kind, format: c.format }).collect(),
            layers: ts.layers.iter().map(|l| writer.layer(l)).collect(),
        })
        .collect();
    let mut res_files = Vec::new();
    let mut res_entries = Vec::new();
    for id in referenced_resources(doc) {
        let Some(r) = resources.get(id) else { continue };
        let Some(bytes) = &r.encoded else {
            log::warn!("resource {} has no original file bytes; not saved", r.name);
            continue;
        };
        let ext = r.source_path.as_ref().and_then(|p| p.extension()).and_then(|e| e.to_str()).unwrap_or("bin").to_ascii_lowercase();
        let file = format!("resources/{}.{ext}", id.0);
        res_entries.push(ResourceV1 { id: id.0, name: r.name.clone(), kind: r.kind, file: file.clone() });
        res_files.push((file, bytes.clone()));
    }
    let document = DocumentV1 { texture_sets, resources: res_entries };
    let manifest = Manifest {
        format: FORMAT_NAME.into(),
        schema_version: SCHEMA_VERSION,
        writer: format!("SubCraft {}", env!("CARGO_PKG_VERSION")),
        mesh: "mesh.bin".into(),
        document: "document.json".into(),
    };

    let mut zip = zip::ZipWriter::new(w);
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", deflate)?;
    zip.write_all(to_json(&manifest).as_bytes())?;
    zip.start_file("document.json", deflate)?;
    zip.write_all(to_json(&document).as_bytes())?;
    zip.start_file("mesh.bin", deflate)?;
    zip.write_all(&subcraft_mesh::serialize::encode(mesh))?;
    for (name, bytes) in &writer.rasters {
        zip.start_file(name.as_str(), deflate)?;
        zip.write_all(bytes)?;
    }
    for (name, bytes) in &res_files {
        // Already compressed image formats: store.
        zip.start_file(name.as_str(), SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored))?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?)
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).expect("schema types serialize")
}

/// Save atomically to `path` (temp file + rename).
pub fn save_project(path: &Path, mesh: &Mesh, doc: &Document, resources: &ResourceStore) -> Result<(), ProjectError> {
    let tmp = temp_path(path);
    let result = (|| -> Result<(), ProjectError> {
        let file = std::fs::File::create(&tmp)?;
        let buf = write_project(std::io::BufWriter::new(file), mesh, doc, resources)?;
        let file = buf.into_inner().map_err(|e| ProjectError::Io(e.into_error()))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".saving");
    path.with_file_name(name)
}

// ----- load ----------------------------------------------------------------------------------

struct Loader<'a, R: Read + Seek> {
    zip: &'a mut zip::ZipArchive<R>,
    cache: BTreeMap<String, TiledImage>,
}

impl<R: Read + Seek> Loader<'_, R> {
    fn bytes(&mut self, name: &str) -> Result<Vec<u8>, ProjectError> {
        let mut f = self.zip.by_name(name).map_err(|_| ProjectError::Invalid(format!("missing entry {name}")))?;
        let mut v = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut v)?;
        Ok(v)
    }

    fn raster(&mut self, name: &str, resolution: u32) -> Result<TiledImage, ProjectError> {
        if let Some(img) = self.cache.get(name) {
            return Ok(img.clone());
        }
        let img = raster::decode(&self.bytes(name)?).map_err(|e| ProjectError::Invalid(format!("{name}: {e}")))?;
        if img.width() != resolution || img.height() != resolution {
            return Err(ProjectError::Invalid(format!("{name}: {}x{} does not match resolution {resolution}", img.width(), img.height())));
        }
        self.cache.insert(name.to_string(), img.clone());
        Ok(img)
    }

    fn layer(&mut self, l: &LayerV1, ts: &TextureSet) -> Result<Layer, ProjectError> {
        let res = ts.resolution;
        let mask = match &l.mask {
            None => None,
            Some(m) => {
                let base = self.raster(&m.base, res)?;
                if base.comps() != 1 {
                    return Err(ProjectError::Invalid("mask base must have 1 component".into()));
                }
                let mut effects = Vec::new();
                for e in &m.effects {
                    let kind = match &e.kind {
                        MaskEffectKindV1::Paint { raster } => MaskEffectKind::Paint { image: self.raster(raster, res)? },
                        MaskEffectKindV1::Fill { value, procedural, projection } => {
                            MaskEffectKind::Fill { value: *value, procedural: *procedural, projection: *projection }
                        }
                        MaskEffectKindV1::Generator { generator } => MaskEffectKind::Generator(*generator),
                        MaskEffectKindV1::Levels { levels } => MaskEffectKind::Levels(*levels),
                        MaskEffectKindV1::Invert => MaskEffectKind::Invert,
                    };
                    effects.push(Arc::new(MaskEffect {
                        id: EffectId(e.id),
                        name: e.name.clone(),
                        visible: e.visible,
                        blend: e.blend,
                        opacity: e.opacity,
                        kind,
                    }));
                }
                Some(Mask { enabled: m.enabled, base, effects })
            }
        };
        let content = match &l.content {
            ContentV1::Paint { channels } => {
                let mut p = PaintContent::default();
                for c in channels {
                    let img = self.raster(&c.raster, res)?;
                    if img.comps() != c.channel.components() + 1 {
                        return Err(ProjectError::Invalid(format!("{}: wrong component count for {:?}", c.raster, c.channel)));
                    }
                    p.channels.insert(c.channel, img);
                }
                LayerContent::Paint(p)
            }
            ContentV1::Fill { projection, channels } => LayerContent::Fill(FillContent {
                projection: *projection,
                channels: channels.iter().map(|c| (c.channel, FillChannel { enabled: c.enabled, source: c.source.clone() })).collect(),
            }),
            ContentV1::Folder { expanded, children } => LayerContent::Group(GroupContent {
                expanded: *expanded,
                children: children.iter().map(|c| self.layer(c, ts).map(Arc::new)).collect::<Result<_, _>>()?,
            }),
        };
        Ok(Layer {
            id: LayerId(l.id),
            name: l.name.clone(),
            visible: l.visible,
            blend: l.blend.iter().map(|b| (b.channel, ChannelBlend { mode: b.mode, opacity: b.opacity })).collect(),
            mask,
            content,
        })
    }
}

/// Read a project container.
pub fn read_project<R: Read + Seek>(r: R) -> Result<ProjectData, ProjectError> {
    let mut zip = zip::ZipArchive::new(r)?;
    let manifest: Manifest = {
        let mut f = zip.by_name("manifest.json").map_err(|_| ProjectError::NotAProject)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        serde_json::from_str(&s).map_err(|_| ProjectError::NotAProject)?
    };
    if manifest.format != FORMAT_NAME {
        return Err(ProjectError::NotAProject);
    }
    if manifest.schema_version > SCHEMA_VERSION || manifest.schema_version == 0 {
        return Err(ProjectError::UnsupportedVersion(manifest.schema_version));
    }
    let mut loader = Loader { zip: &mut zip, cache: BTreeMap::new() };
    let mesh = subcraft_mesh::serialize::decode(&loader.bytes(&manifest.mesh)?).map_err(|e| ProjectError::Invalid(format!("mesh: {e}")))?;
    let doc: DocumentV1 = serde_json::from_slice(&loader.bytes(&manifest.document)?).map_err(|e| ProjectError::Invalid(format!("document: {e}")))?;

    let mut texture_sets = Vec::new();
    for t in &doc.texture_sets {
        if !subcraft_doc::RESOLUTIONS.contains(&t.resolution) {
            return Err(ProjectError::Invalid(format!("texture set {}: unsupported resolution {}", t.name, t.resolution)));
        }
        let mut ts = TextureSet {
            id: TextureSetId(t.id),
            name: t.name.clone(),
            material_slot: t.material_slot,
            resolution: t.resolution,
            channels: t.channels.iter().map(|c| ChannelDef { kind: c.kind, format: c.format }).collect(),
            layers: Vec::new(),
        };
        let layers = t.layers.iter().map(|l| loader.layer(l, &ts).map(Arc::new)).collect::<Result<Vec<_>, _>>()?;
        ts.layers = layers;
        texture_sets.push(ts);
    }

    let mut resources = ResourceStore::default();
    for r in &doc.resources {
        let bytes = loader.bytes(&r.file)?;
        let bitmap = subcraft_image::io::decode_bitmap(&bytes).map_err(|e| ProjectError::Invalid(format!("resource {}: {e}", r.name)))?;
        resources.insert(Resource {
            id: ResourceId(r.id),
            name: r.name.clone(),
            kind: r.kind,
            bitmap: Arc::new(bitmap),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        });
    }
    Ok(ProjectData { mesh, document: Document { texture_sets }, resources })
}

pub fn load_project(path: &Path) -> Result<ProjectData, ProjectError> {
    let file = std::fs::File::open(path)?;
    read_project(std::io::BufReader::new(file))
}

#[cfg(test)]
mod tests;
