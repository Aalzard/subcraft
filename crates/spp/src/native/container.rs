//! Read the small set of Painter datasets SubCraft needs on open (not the full `resources/` tree).

use std::path::Path;

use hdf5_reader::Hdf5File;

const MAGIC: &[u8; 8] = b"\x89HDF\r\n\x1a\n";

/// Bytes pulled from a `.spp` without extracting the whole archive to disk.
#[derive(Debug, Clone)]
pub struct SppContainer {
    pub settings_ini: Vec<u8>,
    pub scene_assbin: Vec<u8>,
    pub document_bin: Vec<u8>,
}

/// Prefer a local dev copy when opening Drehgestell from a network share.
pub fn resolve_spp_path(spp: &Path) -> std::path::PathBuf {
    if let Some(local) = local_scratch_copy(spp) {
        return local;
    }
    super::stage::ensure_local_copy(spp).unwrap_or_else(|e| {
        log::warn!("SPP local staging failed ({e}); opening in place");
        spp.to_path_buf()
    })
}

fn local_scratch_copy(spp: &Path) -> Option<std::path::PathBuf> {
    if !spp
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains("drehgestell"))
    {
        return None;
    }
    let local = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scratch/project.spp")
        .canonicalize()
        .ok()?;
    local.is_file().then_some(local)
}

pub fn read_magic_only(spp: &Path) -> Result<(), String> {
    read_magic(&resolve_spp_path(spp))
}

fn read_magic(path: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if buf != *MAGIC {
        return Err(format!("{}: not HDF5 (missing \\x89HDF magic)", path.display()));
    }
    Ok(())
}

/// Fast path for layer import: **no** `scene.assbin` (saves ~2 MB HDF5 I/O on open).
pub fn read_container_layers_only(spp: &Path) -> Result<SppContainer, String> {
    read_container_inner(spp, false)
}

pub fn read_container(spp: &Path) -> Result<SppContainer, String> {
    read_container_inner(spp, true)
}

fn read_container_inner(spp: &Path, include_scene: bool) -> Result<SppContainer, String> {
    super::stage::log_step(&format!("SPP: resolve path for {}", spp.display()));
    let path = resolve_spp_path(spp);
    read_magic(&path)?;
    super::stage::log_step(&format!("SPP: HDF5 open {}", path.display()));

    let file = Hdf5File::open(&path).map_err(|e| format!("HDF5 open {}: {e}", path.display()))?;

    super::stage::log_step("SPP: read projectsettings.ini");
    let settings_ini = read_dataset(&file, "projectsettings.ini")?;
    super::stage::log_step("SPP: read paint/document.bin");
    let document_bin = read_dataset(&file, "paint/document.bin")?;
    let scene_assbin = if include_scene {
        super::stage::log_step("SPP: read paint/scene.assbin");
        read_dataset(&file, "paint/scene.assbin")?
    } else {
        Vec::new()
    };

    Ok(SppContainer {
        settings_ini,
        scene_assbin,
        document_bin,
    })
}

fn read_dataset(file: &Hdf5File, path: &str) -> Result<Vec<u8>, String> {
    file.dataset(path)
        .map_err(|e| format!("dataset '{path}': {e}"))?
        .read_raw_bytes()
        .map_err(|e| format!("dataset '{path}' read: {e}"))
}
