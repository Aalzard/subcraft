//! Runtime host for Painter's `assimp.dll` (assbin format 1.7).
//!
//! Adobe binaries are **not** linked; the DLL is loaded from the Painter install
//! or `SUBCRAFT_PAINTER_ASSIMP`. Pure-Rust decode lives in `subcraft-mesh::assbin`.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};

use thiserror::Error;

type AiImportFileFromMemory =
    unsafe extern "C" fn(*const c_void, u32, u32, *const c_char) -> *const c_void;
type AiExportScene =
    unsafe extern "C" fn(*const c_void, *const c_char, *const c_char, u32) -> i32;
type AiReleaseImport = unsafe extern "C" fn(*const c_void);

const AI_SUCCESS: i32 = 0;

#[derive(Debug, Error)]
pub enum AssbinHostError {
    #[error("Painter assimp.dll not found (install Painter or set SUBCRAFT_PAINTER_ASSIMP)")]
    DllMissing,
    #[error("assbin too small")]
    TooSmall,
    #[error("{0}")]
    Io(String),
    #[error("aiImportFileFromMemory failed")]
    ImportFailed,
    #[error("aiExportScene returned {0}")]
    ExportFailed(i32),
    #[error("export produced empty file")]
    EmptyOutput,
    #[error("{0}")]
    Ffi(String),
}

pub fn dll_available() -> bool {
    resolve_dll().is_some()
}

/// Whether `subcraft-mesh::import_mesh` can load this export (glTF 2 GLB or OBJ).
pub fn mesh_export_importable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if path.extension().and_then(|e| e.to_str()) == Some("obj") {
        return path.metadata().map(|m| m.len() > 0).unwrap_or(false);
    }
    let mut head = [0u8; 8];
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    head[0..4] == *b"glTF" && head[4..8] == [2, 0, 0, 0]
}

pub fn resolve_dll() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("SUBCRAFT_PAINTER_ASSIMP") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Some(p);
        }
    }
    let default = PathBuf::from(r"C:\Program Files\Adobe\Adobe Substance 3D Painter\assimp.dll");
    if default.is_file() {
        return Some(default);
    }
    None
}

/// Write `assbin` bytes to `glb` using Painter's Assimp.
///
/// Tries glTF 2.0 (`glb2`) first so `subcraft-mesh` can load the file; falls back to legacy `glb` (glTF 1).
pub fn export_assbin_to_glb(assbin: &[u8], glb: &Path) -> Result<(), AssbinHostError> {
    if assbin.len() < 512 {
        return Err(AssbinHostError::TooSmall);
    }
    let dll = resolve_dll().ok_or(AssbinHostError::DllMissing)?;
    if let Some(parent) = glb.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AssbinHostError::Io(e.to_string()))?;
    }
    if export_with_dll(&dll, assbin, "glb2", glb).is_ok() {
        return Ok(());
    }
    export_with_dll(&dll, assbin, "glb", glb)?;
    Ok(())
}

/// Export to a mesh file SubCraft can import (glTF 2 GLB or OBJ).
pub fn export_assbin_for_import(assbin: &[u8], stem: &Path) -> Result<PathBuf, AssbinHostError> {
    if assbin.len() < 512 {
        return Err(AssbinHostError::TooSmall);
    }
    let dll = resolve_dll().ok_or(AssbinHostError::DllMissing)?;
    if let Some(parent) = stem.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AssbinHostError::Io(e.to_string()))?;
    }
    let attempts: [(&str, &str); 3] = [("glb2", "glb"), ("obj", "obj"), ("glb", "glb")];
    for (fmt, ext) in attempts {
        let out = stem.with_extension(ext);
        if export_with_dll(&dll, assbin, fmt, &out).is_ok() {
            return Ok(out);
        }
    }
    Err(AssbinHostError::ExportFailed(-1))
}

fn export_with_dll(dll: &Path, bytes: &[u8], export_fmt: &str, out: &Path) -> Result<(), AssbinHostError> {
    if let Some(dir) = dll.parent() {
        let _ = std::env::set_current_dir(dir);
    }
    let lib = unsafe { libloading::Library::new(dll) }
        .map_err(|e| AssbinHostError::Ffi(format!("load {}: {e}", dll.display())))?;
    let import: libloading::Symbol<AiImportFileFromMemory> =
        unsafe { lib.get(b"aiImportFileFromMemory") }
            .map_err(|e| AssbinHostError::Ffi(e.to_string()))?;
    let export_scene: libloading::Symbol<AiExportScene> =
        unsafe { lib.get(b"aiExportScene") }.map_err(|e| AssbinHostError::Ffi(e.to_string()))?;
    let release: libloading::Symbol<AiReleaseImport> =
        unsafe { lib.get(b"aiReleaseImport") }.map_err(|e| AssbinHostError::Ffi(e.to_string()))?;

    let hint = CString::new("assbin").map_err(|_| AssbinHostError::Ffi("hint".into()))?;
    let scene = unsafe {
        import(
            bytes.as_ptr() as *const c_void,
            bytes.len() as u32,
            0,
            hint.as_ptr(),
        )
    };
    if scene.is_null() {
        return Err(AssbinHostError::ImportFailed);
    }

    let fmt = CString::new(export_fmt).map_err(|_| AssbinHostError::Ffi("fmt".into()))?;
    let out_c = CString::new(out.to_string_lossy().into_owned())
        .map_err(|_| AssbinHostError::Ffi("path".into()))?;
    let status = unsafe { export_scene(scene, fmt.as_ptr(), out_c.as_ptr(), 0) };
    unsafe { release(scene) };

    if status != AI_SUCCESS {
        return Err(AssbinHostError::ExportFailed(status));
    }
    if out.is_file() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        Ok(())
    } else {
        Err(AssbinHostError::EmptyOutput)
    }
}
