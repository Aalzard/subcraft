//! Reading Substance 3D Painter projects (`.spp`).
//!
//! An `.spp` is an HDF5 file (see [`hdf5`]). Inside it:
//! * `paint/scene.assbin`: the model ([`assbin`]).
//! * `paint/document.bin`: texture sets, channels and the layer stack ([`document`]).
//! * `resources.toc` + `resources/<sha1>`: baked mesh maps as JPEG 2000 tiles ([`jp2`], [`images`]).
//! * `texture-cache/<texture set uid>/<n>/<channel flag>`: the flattened channel textures as of the
//!   last save, LZ4-compressed ([`lz4`], [`images`]).
//!
//! Not recovered: procedural (`.sbsar`) layers, filters and generators cannot be re-evaluated (there
//! is no Substance engine); only their flattened result in the texture cache is available.
//! See `docs/spp-research.md`.

pub mod assbin;
pub mod document;
pub mod hdf5;
pub mod images;
pub mod jp2;
pub mod lz4;
pub mod qt;
pub mod sevenz;
pub mod sheets;

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

pub use assbin::DumpMesh;
pub use document::{ChannelInfo, LayerInfo, LayerKind, ProjectInfo, SourceInfo, TextureSetInfo, channel_bits};
pub use images::{BakedKind, CacheFormat, CacheImage, Plane};

#[derive(Debug, thiserror::Error)]
pub enum SppError {
    #[error(transparent)]
    Hdf5(#[from] hdf5::Hdf5Error),
    #[error(transparent)]
    Mesh(#[from] assbin::AssbinError),
    #[error(transparent)]
    Document(#[from] document::DocumentError),
    #[error(transparent)]
    Image(#[from] images::ImageError),
    #[error(transparent)]
    SevenZip(#[from] sevenz::SevenZipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The meshes stored in a Painter project.
pub fn read_meshes(path: &Path) -> Result<Vec<DumpMesh>, SppError> {
    SppFile::open(path)?.meshes()
}

/// An open Painter project.
pub struct SppFile<R: Read + Seek = BufReader<File>> {
    h5: hdf5::Hdf5File<R>,
}

impl SppFile<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self, SppError> {
        Self::from_reader(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> SppFile<R> {
    pub fn from_reader(r: R) -> Result<Self, SppError> {
        Ok(Self { h5: hdf5::Hdf5File::open(r)? })
    }

    fn bytes(&mut self, path: &str) -> Result<Vec<u8>, SppError> {
        let info = self.h5.dataset(path)?;
        Ok(self.h5.read(&info)?)
    }

    pub fn meshes(&mut self) -> Result<Vec<DumpMesh>, SppError> {
        Ok(assbin::parse(&self.bytes("paint/scene.assbin")?)?)
    }

    /// Texture sets, channels and layer stack.
    pub fn project(&mut self) -> Result<ProjectInfo, SppError> {
        let root = document::parse(&self.bytes("paint/document.bin")?)?;
        Ok(document::project_info(&root))
    }

    /// Names of the baked mesh maps stored for the texture set called `set_name`.
    pub fn baked_map_kinds(&mut self, set_name: &str) -> Result<Vec<BakedKind>, SppError> {
        let toc = images::read_toc(&self.bytes("resources.toc")?);
        let mut kinds: Vec<BakedKind> = toc.keys().filter_map(|n| images::classify_baked(n)).filter(|(_, m)| *m == set_name).map(|(k, _)| k).collect();
        kinds.sort();
        Ok(kinds)
    }

    /// The stored (still encoded) blob of one baked mesh map; decode it with
    /// [`images::decode_resource_image`]. Reading is sequential, decoding is not: callers with
    /// several maps read them all first and decode them on all cores.
    pub fn baked_map_blob(&mut self, set_name: &str, kind: BakedKind) -> Result<Option<Vec<u8>>, SppError> {
        let toc = images::read_toc(&self.bytes("resources.toc")?);
        let Some(key) = toc.iter().find_map(|(n, k)| images::classify_baked(n).filter(|(bk, m)| *bk == kind && *m == set_name).map(|_| k.clone())) else {
            return Ok(None);
        };
        Ok(Some(self.bytes(&format!("resources/{key}"))?))
    }

    /// One baked mesh map of a texture set, reduced to at most `max_size`.
    pub fn baked_map(&mut self, set_name: &str, kind: BakedKind, max_size: u32) -> Result<Option<Plane>, SppError> {
        let Some(blob) = self.baked_map_blob(set_name, kind)? else { return Ok(None) };
        Ok(Some(images::decode_resource_image(&blob, max_size)?))
    }

    /// The stored blob of an imported image resource (`...?version=<sha1>.image` URL, as in bitmap
    /// sources and baked map references); decode it with [`images::decode_image_resource`].
    pub fn image_resource_blob(&mut self, url: &str) -> Result<Option<Vec<u8>>, SppError> {
        let Some((_, key)) = url.split_once("?version=") else { return Ok(None) };
        if !key.ends_with(".image") {
            return Ok(None);
        }
        let Ok(info) = self.h5.dataset(&format!("resources/{key}")) else { return Ok(None) };
        Ok(Some(self.h5.read(&info)?))
    }

    /// The stored (still encoded) flattened texture of one channel; see [`SppFile::channel_texture`].
    pub fn channel_blob(&mut self, set: &TextureSetInfo, bits: u64) -> Result<Option<Vec<u8>>, SppError> {
        let dir = format!("texture-cache/{}", set.uid);
        let Ok(subs) = self.h5.list(&dir) else { return Ok(None) };
        for sub in subs {
            let path = format!("{dir}/{sub}/{bits}");
            if let Ok(info) = self.h5.dataset(&path) {
                return Ok(Some(self.h5.read(&info)?));
            }
        }
        Ok(None)
    }

    /// The flattened texture of one channel (`bits` = [`channel_bits`] flag) of a texture set, if the
    /// project's texture cache holds it.
    pub fn channel_texture(&mut self, set: &TextureSetInfo, bits: u64, max_size: u32) -> Result<Option<CacheImage>, SppError> {
        let Some(blob) = self.channel_blob(set, bits)? else { return Ok(None) };
        Ok(Some(images::decode_cache_image(&blob, max_size)?))
    }

    /// The files of a Substance archive (`.sbsar`, a 7z) stored in the project, found by the
    /// `version=` key of a resource URL such as `resource://starter_assets/x/x?version=<sha1>.sbsar`.
    pub fn sbsar_files(&mut self, url: &str) -> Result<Option<Vec<sevenz::Entry>>, SppError> {
        let Some(key) = sbsar_key(url) else { return Ok(None) };
        let path = format!("resources/{key}");
        let Ok(info) = self.h5.dataset(&path) else { return Ok(None) };
        let bytes = self.h5.read(&info)?;
        Ok(Some(sevenz::read_all(&bytes)?))
    }

    /// The raw alpha sheets of a stencil-pack archive (see [`sheets`]), one per picker value, each
    /// reduced to at most `max_size` texels a side. `None` when the archive holds none or its
    /// layout cannot be recovered.
    pub fn stencil_sheets(&mut self, url: &str, max_size: u32) -> Result<Option<Vec<sheets::Sheet>>, SppError> {
        let Some(files) = self.sbsar_files(url)? else { return Ok(None) };
        let count = files.iter().find(|f| f.name.ends_with(".xml")).and_then(|f| sheets::sheet_count(&String::from_utf8_lossy(&f.data)));
        let (Some(count), Some(asm)) = (count, files.iter().find(|f| f.name.ends_with(".sbsasm"))) else { return Ok(None) };
        Ok(sheets::read_sheets(&asm.data, count).map(|all| all.iter().map(|s| sheets::reduced(s, max_size)).collect()))
    }

    /// The JPEG images embedded in a Substance archive's compiled graph (grunge maps, bitmap
    /// materials, brush tips), as encoded bytes, in file order. Empty for purely procedural graphs.
    pub fn embedded_jpegs(&mut self, url: &str) -> Result<Vec<Vec<u8>>, SppError> {
        let Some(files) = self.sbsar_files(url)? else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for f in files.iter().filter(|f| f.name.ends_with(".sbsasm")) {
            out.extend(find_jpegs(&f.data));
        }
        Ok(out)
    }
}

/// `<sha1>.sbsar` from a resource URL.
pub fn sbsar_key(url: &str) -> Option<&str> {
    let (_, key) = url.split_once("?version=")?;
    key.ends_with(".sbsar").then_some(key)
}

/// Encoded JPEG images inside a blob: each starts at an SOI marker followed by a known segment
/// marker and runs to the next one (or the end).
pub fn find_jpegs(data: &[u8]) -> Vec<Vec<u8>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        if data[i] == 0xff && data[i + 1] == 0xd8 && data[i + 2] == 0xff && matches!(data[i + 3], 0xe0 | 0xe1 | 0xdb | 0xc0 | 0xc2 | 0xc4 | 0xee) {
            starts.push(i);
            i += 4;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for (k, s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(data.len());
        // Trim to the last end-of-image marker so trailing data of the graph does not follow the picture.
        let slice = &data[*s..end];
        let cut = slice.windows(2).rposition(|w| w == [0xff, 0xd9]).map(|p| p + 2).unwrap_or(slice.len());
        out.push(slice[..cut].to_vec());
    }
    out
}
