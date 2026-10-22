//! Reading Substance 3D Painter projects (`.spp`).
//!
//! An `.spp` is an HDF5 file (see [`hdf5`]). The model is the `paint/scene.assbin` dataset
//! ([`assbin`]). Layers, painted pixels and baked maps are not read yet; see `docs/spp-research.md`.

pub mod assbin;
pub mod hdf5;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub use assbin::DumpMesh;

#[derive(Debug, thiserror::Error)]
pub enum SppError {
    #[error(transparent)]
    Hdf5(#[from] hdf5::Hdf5Error),
    #[error(transparent)]
    Mesh(#[from] assbin::AssbinError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The meshes stored in a Painter project.
pub fn read_meshes(path: &Path) -> Result<Vec<DumpMesh>, SppError> {
    let mut f = hdf5::Hdf5File::open(BufReader::new(File::open(path)?))?;
    let info = f.dataset("paint/scene.assbin")?;
    let bytes = f.read(&info)?;
    Ok(assbin::parse(&bytes)?)
}
