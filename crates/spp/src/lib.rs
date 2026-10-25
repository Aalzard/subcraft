//! Experimental `.spp` import via `tools/spp-import/extract.py`.

pub mod hbo;
mod inspect;
mod native;
mod stack;

use std::path::{Path, PathBuf};

pub use hbo::{
    build_stack_from_document, decode_spmsk, decode_spmsk_from_preset, decode_spsm,
    decode_spsm_from_preset,
};
pub use hbo::{count_hbo_features, generators_decodable, HboFeatureCounts};
pub use inspect::{
    DecodeStatus, HboGeneratorSummary, MaterialSummary, SppInspectReport, TextureCacheSummary,
    inspect,
};
pub use native::{
    decode_painter_mesh_maps, decode_texture_cache, probe_texture_cache, texture_cache_decodable,
    CachedMaterialChannels, CachedMeshMap, PainterMeshMapKind, TextureCacheProbe,
};
pub use native::{
    assimp_cli_available, log_step, mesh_export_available, open_log_path, painter_assimp_dll_available,
    read_container, try_assbin_to_glb, SppContainer,
};
pub use stack::{
    load_stack, SppChannelBlend, SppContentEffect, SppDecalHit, SppLayerNode, SppMaskEffect,
    SppMaskEffectKind, SppMaterialStack, SppPaintStroke, SppProceduralChannel, SppSbsarRef,
    SppStackFile, SppTextureSetStack,
};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SppExtractManifest {
    pub format: String,
    pub source: String,
    pub painter_version: PainterVersion,
    pub mesh_assbin: PathBuf,
    #[serde(default)]
    pub mesh_glb: Option<PathBuf>,
    pub mesh_note: String,
    pub embedded_sbsar: Vec<PathBuf>,
    pub layer_stack: PathBuf,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PainterVersion {
    pub major: u32,
    pub minor: u32,
}

pub fn resolve_extract_script() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUBCRAFT_SPP_EXTRACT") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in [
                "tools/spp-import/extract.py",
                "../tools/spp-import/extract.py",
                "../../tools/spp-import/extract.py",
            ] {
                let candidate = dir.join(rel);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn python_bin() -> String {
    std::env::var("SUBCRAFT_PYTHON").unwrap_or_else(|_| "python".into())
}

fn use_python_extract() -> bool {
    std::env::var("SUBCRAFT_SPP_PYTHON")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Read `.spp` in-process (Painter-style: HDF5 datasets only).
pub fn open_container(spp: &Path) -> Result<SppContainer, String> {
    read_container(spp)
}

/// Materialize import artifacts under `out_dir` and return the manifest.
pub fn extract_spp(spp: &Path, out_dir: &Path) -> Result<SppExtractManifest, String> {
    if !use_python_extract() {
        if let Ok(m) = extract_spp_native(spp, out_dir) {
            return Ok(m);
        }
        log::warn!("native SPP extract failed; set SUBCRAFT_SPP_PYTHON=1 to force Python fallback");
    }
    extract_spp_python(spp, out_dir)
}

/// Open `.spp`: layer stack always; mesh → GLB when Assimp CLI is available (see `scripts/fetch-assimp.ps1`).
pub fn open_pack(spp: &Path, out_dir: &Path) -> Result<(SppStackFile, SppExtractManifest), String> {
    native::log_step(&format!("open_pack: {}", spp.display()));
    if use_python_extract() {
        let manifest = extract_spp_python(spp, out_dir)?;
        let stack = load_stack(&manifest.layer_stack)?;
        return Ok((stack, manifest));
    }

    let want_mesh = should_export_mesh_on_open();
    let container = if want_mesh {
        native::log_step("open_pack: read paint/scene.assbin for Assimp");
        read_container(spp)?
    } else {
        native::read_container_layers_only(spp)?
    };

    native::log_step("open_pack: HBO decode");
    let (stack, _, _) = build_stack_from_document(&container.document_bin)?;
    let _ = export_stack_native(&container.document_bin, out_dir)?;
    native::log_step("open_pack: building manifest");
    let manifest = if want_mesh {
        manifest_with_mesh(spp, &container, out_dir)?
    } else {
        manifest_layers_only(spp, &container, out_dir)
    };
    native::log_step("open_pack: done");
    Ok((stack, manifest))
}

fn should_export_mesh_on_open() -> bool {
    !env_flag("SUBCRAFT_SPP_NO_MESH", false)
}

fn manifest_layers_only(spp: &Path, container: &SppContainer, out_dir: &Path) -> SppExtractManifest {
    let _ = std::fs::create_dir_all(out_dir);
    let ver = painter_version_from_settings(&container.settings_ini);
    SppExtractManifest {
        format: "subcraft-spp-open-v1".into(),
        source: spp.display().to_string(),
        painter_version: ver,
        mesh_assbin: out_dir.join("scene.assbin"),
        mesh_glb: None,
        mesh_note: "Mesh skipped (SUBCRAFT_SPP_NO_MESH).".into(),
        embedded_sbsar: Vec::new(),
        layer_stack: out_dir.join("subcraft-spp-stack.json"),
        limitations: vec![
            "Layer tree imported; texture-cache not replayed into TiledImage yet.".into(),
        ],
    }
}

fn manifest_with_mesh(
    spp: &Path,
    container: &SppContainer,
    out_dir: &Path,
) -> Result<SppExtractManifest, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let scene_path = out_dir.join("scene.assbin");
    std::fs::write(&scene_path, &container.scene_assbin)
        .map_err(|e| format!("{}: {e}", scene_path.display()))?;
    native::log_step("open_pack: scene.assbin (native Rust mesh decoder)");
    let mesh_glb = None;
    let ver = painter_version_from_settings(&container.settings_ini);
    let mesh_note = "Mesh in scene.assbin — decoded in-process (no Substance Painter install).".into();
    Ok(SppExtractManifest {
        format: "subcraft-spp-open-v1".into(),
        source: spp.display().to_string(),
        painter_version: ver,
        mesh_assbin: scene_path,
        mesh_glb,
        mesh_note,
        embedded_sbsar: Vec::new(),
        layer_stack: out_dir.join("subcraft-spp-stack.json"),
        limitations: vec![
            "Layer tree imported; painted pixels from texture-cache not in viewport yet.".into(),
        ],
    })
}

fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn extract_spp_native(spp: &Path, out_dir: &Path) -> Result<SppExtractManifest, String> {
    log::info!("SPP native: reading HDF5 (paint/* only) from {}", spp.display());
    let container = read_container(spp)?;
    extract_spp_native_from_container(spp, &container, out_dir)
}

fn extract_spp_native_from_container(
    spp: &Path,
    container: &SppContainer,
    out_dir: &Path,
) -> Result<SppExtractManifest, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let scene_path = out_dir.join("scene.assbin");
    let stack_path = export_stack_native(&container.document_bin, out_dir)?;

    let mesh_glb = if should_export_mesh_on_open() {
        std::fs::write(&scene_path, &container.scene_assbin)
            .map_err(|e| format!("{}: {e}", scene_path.display()))?;
        log::info!("SPP native: scene.assbin for in-process mesh decode");
        None
    } else {
        None
    };

    let embedded_sbsar = if env_flag("SUBCRAFT_SPP_SBSAR", false) {
        log::info!("SPP native: extracting referenced .sbsar from resources/");
        native::copy_referenced_sbsar(spp, &[&container.document_bin, &container.settings_ini], out_dir)
            .unwrap_or_else(|e| {
                log::warn!("lazy SBSAR extract: {e}");
                Vec::new()
            })
    } else {
        Vec::new()
    };
    let mesh_note = if mesh_glb.as_ref().is_some_and(|p: &PathBuf| p.is_file()) {
        "Mesh exported to scene.glb (optional Assimp CLI path)."
    } else if should_export_mesh_on_open() {
        "Mesh in scene.assbin — decoded in-process (no Substance Painter install)."
    } else {
        "Mesh skipped (SUBCRAFT_SPP_NO_MESH)."
    }
    .into();

    let ver = painter_version_from_settings(&container.settings_ini);
    let manifest = SppExtractManifest {
        format: "subcraft-spp-extract-v1-native".into(),
        source: spp.display().to_string(),
        painter_version: ver,
        mesh_assbin: scene_path,
        mesh_glb,
        mesh_note,
        embedded_sbsar,
        layer_stack: stack_path,
        limitations: vec![
            "Native import: layer tree from HBO; SBSAR copied only when referenced in document.".into(),
            "texture-cache SVT readable (see inspect); painted pixels are not restored in the viewport yet.".into(),
        ],
    };
    let manifest_path = out_dir.join("subcraft-spp-manifest.json");
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    Ok(manifest)
}

fn painter_version_from_settings(settings: &[u8]) -> PainterVersion {
    let major = inspect::painter_major_from_settings(settings);
    PainterVersion { major, minor: 0 }
}

fn export_stack_native(document_bin: &[u8], out_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let stack_path = out_dir.join("subcraft-spp-stack.json");
    hbo::write_stack_json(document_bin, &stack_path)?;
    Ok(stack_path)
}

fn extract_spp_python(spp: &Path, out_dir: &Path) -> Result<SppExtractManifest, String> {
    let script = resolve_extract_script().ok_or(
        "SPP extract script not found. Set SUBCRAFT_SPP_EXTRACT to tools/spp-import/extract.py.",
    )?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    let mut cmd = Command::new(python_bin());
    cmd.arg(&script).arg(spp).arg("-o").arg(out_dir);
    if std::env::var("SUBCRAFT_SPP_FAST")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
    {
        cmd.arg("--fast");
    }
    let output = cmd.output().map_err(|e| format!("failed to run extract.py: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("SPP extract failed:\n{stderr}{stdout}"));
    }

    let manifest_path = out_dir.join("subcraft-spp-manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("manifest: {e}"))
}
