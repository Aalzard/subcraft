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

    /// One baked mesh map of a texture set, reduced to at most `max_size`.
    pub fn baked_map(&mut self, set_name: &str, kind: BakedKind, max_size: u32) -> Result<Option<Plane>, SppError> {
        let toc = images::read_toc(&self.bytes("resources.toc")?);
        let Some(key) = toc.iter().find_map(|(n, k)| images::classify_baked(n).filter(|(bk, m)| *bk == kind && *m == set_name).map(|_| k.clone())) else {
            return Ok(None);
        };
        let blob = self.bytes(&format!("resources/{key}"))?;
        Ok(Some(images::decode_resource_image(&blob, max_size)?))
    }

    /// The flattened texture of one channel (`bits` = [`channel_bits`] flag) of a texture set, if the
    /// project's texture cache holds it.
    pub fn channel_texture(&mut self, set: &TextureSetInfo, bits: u64, max_size: u32) -> Result<Option<CacheImage>, SppError> {
        let dir = format!("texture-cache/{}", set.uid);
        let Ok(subs) = self.h5.list(&dir) else { return Ok(None) };
        for sub in subs {
            let path = format!("{dir}/{sub}/{bits}");
            if let Ok(info) = self.h5.dataset(&path) {
                let blob = self.h5.read(&info)?;
                return Ok(Some(images::decode_cache_image(&blob, max_size)?));
            }
        }
        Ok(None)
    }
}
