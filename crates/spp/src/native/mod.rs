//! In-process `.spp` (HDF5) reader — no Python.

mod assimp;
mod container;
mod mesh_maps;
mod painter_assimp;
mod sbsar;
mod stage;
mod texture_cache;

pub use assimp::assimp_cli_available;
pub use painter_assimp::painter_assimp_dll_available;

pub fn try_assbin_to_glb(assbin: &Path, glb: &Path) -> Option<std::path::PathBuf> {
    if use_painter_assimp() {
        if let Some(p) = painter_assimp::try_assbin_to_glb(assbin, glb) {
            return Some(p);
        }
    }
    assimp::try_assbin_to_glb(assbin, glb)
}

/// Native Rust assbin 1.7 decode is always available; optional Assimp CLI / Painter DLL are extras.
pub fn mesh_export_available() -> bool {
    true
}

fn use_painter_assimp() -> bool {
    std::env::var("SUBCRAFT_USE_PAINTER_ASSIMP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

use std::path::Path;
pub use container::{read_container, read_container_layers_only, read_magic_only, resolve_spp_path, SppContainer};
pub use sbsar::copy_referenced_sbsar;
pub use stage::{log_step, open_log_path};
pub use mesh_maps::{decode_painter_mesh_maps, CachedMeshMap, PainterMeshMapKind};
pub use texture_cache::{
    decode_texture_cache, probe_texture_cache, texture_cache_decodable, CachedMaterialChannels,
    TextureCacheProbe,
};
