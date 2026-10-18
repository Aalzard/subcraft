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

pub mod fragment;
pub mod raster;
pub mod schema;

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_core::{EffectId, LayerId, ResourceId, TextureSetId};
use subcraft_doc::{
    ChannelBlend, ChannelDef, ContentEffect, ContentEffectKind, Document, FillChannel, FillContent, GroupContent, Layer,
    LayerContent, Mask, MaskEffect,
    MaskEffectKind, PaintContent, PartMask, Projection, Resource, ResourceStore, TextureSet,
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

/// Baked mesh maps per texture set.
pub type MeshMapSet = BTreeMap<TextureSetId, BTreeMap<subcraft_core::MeshMapKind, TiledImage>>;

/// Everything a project file holds.
#[derive(Debug)]
pub struct ProjectData {
    pub mesh: Mesh,
    pub document: Document,
    pub resources: ResourceStore,
    pub mesh_maps: MeshMapSet,
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

    fn mask_effect(&mut self, e: &MaskEffect) -> MaskEffectV1 {
        MaskEffectV1 {
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
                MaskEffectKind::Image { resource, projection, invert } => {
                    MaskEffectKindV1::Image { resource: resource.0, projection: *projection, invert: *invert }
                }
            },
        }
    }

    fn paint_channels(&mut self, channels: &BTreeMap<subcraft_core::ChannelKind, TiledImage>) -> Vec<PaintChannelV1> {
        channels.iter().map(|(&channel, img)| PaintChannelV1 { channel, raster: self.raster(img) }).collect()
    }

    fn content_effect(&mut self, e: &ContentEffect) -> ContentEffectV5 {
        ContentEffectV5 {
            id: e.id.0,
            name: e.name.clone(),
            visible: e.visible,
            blend: blends(&e.blend),
            kind: match &e.kind {
                ContentEffectKind::Paint { channels } => ContentEffectKindV5::Paint { channels: self.paint_channels(channels) },
                ContentEffectKind::Fill(f) => ContentEffectKindV5::Fill { projection: f.projection, channels: fill_channels(f) },
                ContentEffectKind::Levels { channel, levels } => ContentEffectKindV5::Levels { channel: *channel, levels: *levels },
            },
        }
    }

    fn layer(&mut self, l: &Layer) -> LayerV1 {
        let blend = blends(&l.blend);
        let mask = l.mask.as_ref().map(|m| MaskV1 {
            enabled: m.enabled,
            base: self.raster(&m.base),
            effects: m.effects.iter().map(|e| self.mask_effect(e)).collect(),
        });
        let content = match &l.content {
            LayerContent::Paint(p) => ContentV1::Paint {
                channels: self.paint_channels(&p.channels),
                path: p.path.as_ref().map(|path| PenPathV4 {
                    anchors: path.anchors.iter().map(|a| PenAnchorV4 { id: a.id.0, position: a.position.to_array(), incoming: a.incoming.to_array(), outgoing: a.outgoing.to_array() }).collect(),
                    closed: path.closed, radius: path.radius, hardness: path.hardness, opacity: path.opacity,
                    values: path.values.iter().map(|(&channel, value)| PenChannelV4 { channel, value: value.0 }).collect(),
                }),
            },
            LayerContent::Fill(f) => ContentV1::Fill { projection: f.projection, channels: fill_channels(f) },
            LayerContent::Group(g) => ContentV1::Folder { expanded: g.expanded, children: g.children.iter().map(|c| self.layer(c)).collect() },
        };
        let parts = l.parts.as_ref().map(|p| PartsV1 { enabled: p.enabled, kind: p.kind, parts: p.parts.iter().copied().collect() });
        let content_effects = l.content_effects.iter().map(|e| self.content_effect(e)).collect();
        LayerV1 { id: l.id.0, name: l.name.clone(), visible: l.visible, blend, mask, parts, content, content_effects }
    }
}

fn blends(blend: &BTreeMap<subcraft_core::ChannelKind, ChannelBlend>) -> Vec<BlendV1> {
    blend.iter().map(|(&channel, b)| BlendV1 { channel, mode: b.mode, opacity: b.opacity }).collect()
}

fn fill_channels(f: &FillContent) -> Vec<FillChannelV1> {
    f.channels.iter().map(|(&channel, c)| FillChannelV1 { channel, enabled: c.enabled, source: c.source.clone(), strength: c.strength }).collect()
}

fn read_fill(projection: Projection, channels: &[FillChannelV1]) -> FillContent {
    FillContent {
        projection,
        channels: channels.iter().map(|c| (c.channel, FillChannel { enabled: c.enabled, source: c.source.clone(), strength: c.strength })).collect(),
    }
}

fn referenced_resources(doc: &Document) -> Vec<ResourceId> {
    let mut out = Vec::new();
    for ts in &doc.texture_sets {
        for l in &ts.layers {
            l.collect_resources(&mut out);
        }
    }
    out
}

/// Write a project container to any seekable writer (files, memory buffers in tests).
pub fn write_project<W: Write + Seek>(
    w: W,
    mesh: &Mesh,
    doc: &Document,
    resources: &ResourceStore,
    mesh_maps: &MeshMapSet,
) -> Result<W, ProjectError> {
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
            mesh_maps: mesh_maps
                .get(&ts.id)
                .map(|m| m.iter().map(|(&kind, img)| MeshMapV1 { kind, raster: writer.raster(img) }).collect())
                .unwrap_or_default(),
        })
        .collect();
    let (res_entries, res_files) = pack_resources(&referenced_resources(doc), resources);
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
    write_payload(&mut zip, &writer.rasters, &res_files)?;
    Ok(zip.finish()?)
}

/// Resource table entries + file payloads for `ids` (resources without original bytes are skipped).
fn pack_resources(ids: &[ResourceId], resources: &ResourceStore) -> (Vec<ResourceV1>, Vec<(String, Arc<Vec<u8>>)>) {
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for &id in ids {
        let Some(r) = resources.get(id) else { continue };
        let Some(bytes) = &r.encoded else {
            log::warn!("resource {} has no original file bytes; not saved", r.name);
            continue;
        };
        // By content, so re-saving a loaded project keeps the extension (TGA falls back to the name).
        let hint = r.source_path.as_deref().and_then(|p| p.to_str()).unwrap_or(&r.name);
        let ext = subcraft_image::guess_extension(bytes, Some(hint));
        let file = format!("resources/{}.{ext}", id.0);
        entries.push(ResourceV1 { id: id.0, name: r.name.clone(), kind: r.kind, file: file.clone() });
        files.push((file, bytes.clone()));
    }
    (entries, files)
}

/// Raster blobs (deflated) and resource files (stored: already compressed image formats).
fn write_payload<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    rasters: &[(String, Vec<u8>)],
    resources: &[(String, Arc<Vec<u8>>)],
) -> Result<(), ProjectError> {
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in rasters {
        zip.start_file(name.as_str(), deflate)?;
        zip.write_all(bytes)?;
    }
    for (name, bytes) in resources {
        zip.start_file(name.as_str(), SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored))?;
        zip.write_all(bytes)?;
    }
    Ok(())
}

fn unpack_resources<R: Read + Seek>(loader: &mut Loader<'_, R>, entries: &[ResourceV1]) -> Result<ResourceStore, ProjectError> {
    let mut resources = ResourceStore::default();
    for r in entries {
        let bytes = loader.bytes(&r.file)?;
        let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some(&r.file))
            .map_err(|e| ProjectError::Invalid(format!("resource {}: {e}", r.name)))?;
        resources.insert(Resource {
            id: ResourceId(r.id),
            name: r.name.clone(),
            kind: r.kind,
            bitmap: Arc::new(bitmap),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        });
    }
    Ok(resources)
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).expect("schema types serialize")
}

/// Save atomically to `path` (temp file + rename).
#[cfg(not(target_arch = "wasm32"))]
pub fn save_project(path: &Path, mesh: &Mesh, doc: &Document, resources: &ResourceStore, mesh_maps: &MeshMapSet) -> Result<(), ProjectError> {
    let tmp = temp_path(path);
    let result = (|| -> Result<(), ProjectError> {
        let file = std::fs::File::create(&tmp)?;
        let buf = write_project(std::io::BufWriter::new(file), mesh, doc, resources, mesh_maps)?;
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

/// Web build: encode in memory and hand the file to `vfs` (which offers it as a download).
#[cfg(target_arch = "wasm32")]
pub fn save_project(path: &Path, mesh: &Mesh, doc: &Document, resources: &ResourceStore, mesh_maps: &MeshMapSet) -> Result<(), ProjectError> {
    let buf = write_project(std::io::Cursor::new(Vec::new()), mesh, doc, resources, mesh_maps)?;
    subcraft_core::vfs::write(path, &buf.into_inner())?;
    Ok(())
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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

    fn mask_effect(&mut self, e: &MaskEffectV1, res: u32) -> Result<MaskEffect, ProjectError> {
        let kind = match &e.kind {
            MaskEffectKindV1::Paint { raster } => MaskEffectKind::Paint { image: self.raster(raster, res)? },
            MaskEffectKindV1::Fill { value, procedural, projection } => {
                MaskEffectKind::Fill { value: *value, procedural: *procedural, projection: *projection }
            }
            MaskEffectKindV1::Generator { generator } => MaskEffectKind::Generator(*generator),
            MaskEffectKindV1::Levels { levels } => MaskEffectKind::Levels(*levels),
            MaskEffectKindV1::Invert => MaskEffectKind::Invert,
            MaskEffectKindV1::Image { resource, projection, invert } => {
                MaskEffectKind::Image { resource: ResourceId(*resource), projection: *projection, invert: *invert }
            }
        };
        Ok(MaskEffect { id: EffectId(e.id), name: e.name.clone(), visible: e.visible, blend: e.blend, opacity: e.opacity, kind })
    }

    fn paint_channels(&mut self, channels: &[PaintChannelV1], res: u32) -> Result<BTreeMap<subcraft_core::ChannelKind, TiledImage>, ProjectError> {
        let mut out = BTreeMap::new();
        for c in channels {
            let img = self.raster(&c.raster, res)?;
            if img.comps() != c.channel.components() + 1 {
                return Err(ProjectError::Invalid(format!("{}: wrong component count for {:?}", c.raster, c.channel)));
            }
            out.insert(c.channel, img);
        }
        Ok(out)
    }

    fn content_effect(&mut self, e: &ContentEffectV5, res: u32) -> Result<ContentEffect, ProjectError> {
        let kind = match &e.kind {
            ContentEffectKindV5::Paint { channels } => ContentEffectKind::Paint { channels: self.paint_channels(channels, res)? },
            ContentEffectKindV5::Fill { projection, channels } => ContentEffectKind::Fill(read_fill(*projection, channels)),
            ContentEffectKindV5::Levels { channel, levels } => {
                if *channel == subcraft_core::ChannelKind::Normal {
                    return Err(ProjectError::Invalid("levels effect on the normal channel".into()));
                }
                ContentEffectKind::Levels { channel: *channel, levels: *levels }
            }
        };
        let blend = e.blend.iter().map(|b| (b.channel, ChannelBlend { mode: b.mode, opacity: b.opacity })).collect();
        Ok(ContentEffect { id: EffectId(e.id), name: e.name.clone(), visible: e.visible, blend, kind })
    }

    fn layer(&mut self, l: &LayerV1, res: u32) -> Result<Layer, ProjectError> {
        let mask = match &l.mask {
            None => None,
            Some(m) => {
                let base = self.raster(&m.base, res)?;
                if base.comps() != 1 {
                    return Err(ProjectError::Invalid("mask base must have 1 component".into()));
                }
                let mut effects = Vec::new();
                for e in &m.effects {
                    effects.push(Arc::new(self.mask_effect(e, res)?));
                }
                Some(Mask { enabled: m.enabled, base, effects })
            }
        };
        let content = match &l.content {
            ContentV1::Paint { channels, path } => {
                let mut p = PaintContent::default();
                if let Some(path) = path {
                    let source = subcraft_doc::PenPath {
                        anchors: path.anchors.iter().map(|a| subcraft_doc::PenAnchor { id: subcraft_core::AnchorId(a.id), position: a.position.into(), incoming: a.incoming.into(), outgoing: a.outgoing.into() }).collect(),
                        closed: path.closed, radius: path.radius, hardness: path.hardness, opacity: path.opacity,
                        values: path.values.iter().map(|v| (v.channel, subcraft_core::ChannelValue(v.value))).collect(),
                    };
                    source.validate().map_err(|e| ProjectError::Invalid(e.into()))?;
                    if source.values.len() != path.values.len() { return Err(ProjectError::Invalid("duplicate pen channels".into())); }
                    p.path = Some(source);
                }
                p.channels = self.paint_channels(channels, res)?;
                LayerContent::Paint(p)
            }
            ContentV1::Fill { projection, channels } => LayerContent::Fill(read_fill(*projection, channels)),
            ContentV1::Folder { expanded, children } => LayerContent::Group(GroupContent {
                expanded: *expanded,
                children: children.iter().map(|c| self.layer(c, res).map(Arc::new)).collect::<Result<_, _>>()?,
            }),
        };
        if matches!(content, LayerContent::Group(_)) && !l.content_effects.is_empty() {
            return Err(ProjectError::Invalid("content effects on a folder".into()));
        }
        let content_effects = l.content_effects.iter().map(|e| self.content_effect(e, res).map(Arc::new)).collect::<Result<_, _>>()?;
        Ok(Layer {
            id: LayerId(l.id),
            name: l.name.clone(),
            visible: l.visible,
            blend: l.blend.iter().map(|b| (b.channel, ChannelBlend { mode: b.mode, opacity: b.opacity })).collect(),
            mask,
            parts: l.parts.as_ref().map(|p| PartMask { enabled: p.enabled, kind: p.kind, parts: p.parts.iter().copied().collect() }),
            content,
            content_effects,
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
    let mut mesh_maps = MeshMapSet::new();
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
        let layers = t.layers.iter().map(|l| loader.layer(l, ts.resolution).map(Arc::new)).collect::<Result<Vec<_>, _>>()?;
        ts.layers = layers;
        let mut maps = BTreeMap::new();
        for m in &t.mesh_maps {
            let img = raster::decode(&loader.bytes(&m.raster)?).map_err(|e| ProjectError::Invalid(format!("{}: {e}", m.raster)))?;
            if img.width() != img.height() || img.comps() != m.kind.components() {
                return Err(ProjectError::Invalid(format!("{}: unexpected layout for {:?}", m.raster, m.kind)));
            }
            maps.insert(m.kind, img);
        }
        if !maps.is_empty() {
            mesh_maps.insert(ts.id, maps);
        }
        texture_sets.push(ts);
    }

    let resources = unpack_resources(&mut loader, &doc.resources)?;
    Ok(ProjectData { mesh, document: Document { texture_sets }, resources, mesh_maps })
}

pub fn load_project(path: &Path) -> Result<ProjectData, ProjectError> {
    #[cfg(not(target_arch = "wasm32"))]
    return read_project(std::io::BufReader::new(std::fs::File::open(path)?));
    #[cfg(target_arch = "wasm32")]
    return read_project(std::io::Cursor::new(subcraft_core::vfs::read(path)?));
}

#[cfg(test)]
mod tests;
