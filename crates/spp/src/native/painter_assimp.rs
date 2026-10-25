//! Optional Adobe Painter `assimp.dll` export (`--features painter-assimp-host` on `subcraft-spp`).

use std::path::{Path, PathBuf};

pub fn painter_assimp_dll_available() -> bool {
    #[cfg(feature = "painter-assimp-host")]
    {
        return subcraft_assbin_host::dll_available();
    }
    #[cfg(not(feature = "painter-assimp-host"))]
    {
        false
    }
}

pub fn try_assbin_to_glb(assbin: &Path, glb: &Path) -> Option<PathBuf> {
    #[cfg(feature = "painter-assimp-host")]
    {
        if !painter_assimp_dll_available() {
            return None;
        }
        let bytes = std::fs::read(assbin)
            .map_err(|e| {
                log::warn!("painter assimp: read {}: {e}", assbin.display());
                e
            })
            .ok()?;
        let stem = glb.with_extension("");
        match subcraft_assbin_host::export_assbin_for_import(&bytes, &stem) {
            Ok(path) => {
                log::info!("painter assimp: wrote {}", path.display());
                Some(path)
            }
            Err(e) => {
                log::warn!("painter assimp: {e}");
                None
            }
        }
    }
    #[cfg(not(feature = "painter-assimp-host"))]
    {
        let _ = (assbin, glb);
        None
    }
}
