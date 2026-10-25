//! Bake `.sbsar` archives via the optional `subcraft-sbsar-bake` host (Substance Materials SDK).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Default bake resolution when applying an SBSAR to the project.
pub const DEFAULT_BAKE_SIZE: u32 = 2048;

pub const MIN_BAKE_SIZE: u32 = 256;
pub const MAX_BAKE_SIZE: u32 = 8192;

/// Clamp user/UI bake size to what the host accepts (power of two, 256–8192).
pub fn clamp_bake_size(size_px: u32) -> u32 {
    size_px.clamp(MIN_BAKE_SIZE, MAX_BAKE_SIZE).next_power_of_two().min(MAX_BAKE_SIZE)
}

#[derive(Clone, Debug, Deserialize)]
pub struct BakeManifest {
    pub graph: String,
    pub graph_index: usize,
    pub size: u32,
    pub graphs: Vec<GraphEntry>,
    #[serde(default)]
    pub inputs: Vec<SbsarInput>,
    pub outputs: Vec<BakedOutput>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SbsarEnumValue {
    pub value: i32,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SbsarInput {
    pub identifier: String,
    pub label: String,
    #[serde(default)]
    pub group: String,
    pub kind: String,
    pub widget: String,
    pub value: String,
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub enum_values: Vec<SbsarEnumValue>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GraphEntry {
    pub index: usize,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BakedOutput {
    pub file: String,
    pub identifier: String,
}

/// Locate `subcraft-sbsar-bake.exe` (or bare name on PATH).
pub fn resolve_host() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUBCRAFT_SBSAR_HOST") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["subcraft-sbsar-bake.exe", "subcraft-sbsar-bake"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
            let nested = dir.join("sbsar-bake").join("subcraft-sbsar-bake.exe");
            if nested.is_file() {
                return Some(nested);
            }
            for extra in [
                dir.join("..").join("..").join("..").join("bin").join("sbsar-bake").join("subcraft-sbsar-bake.exe"),
                dir.join("..").join("..").join("bin").join("sbsar-bake").join("subcraft-sbsar-bake.exe"),
                dir.join("..").join("bin").join("sbsar-bake").join("subcraft-sbsar-bake.exe"),
            ] {
                if extra.is_file() {
                    return Some(extra);
                }
            }
        }
    }
    which_on_path("subcraft-sbsar-bake.exe").or_else(|| which_on_path("subcraft-sbsar-bake"))
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Directory that contains Substance engine DLLs (`substance_sse2_blend.dll`, …).
pub fn substance_bin_dir(host: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUBSTANCE_SDK_BIN") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    if let Some(parent) = host.parent() {
        if parent.join("substance_sse2_blend.dll").is_file() {
            return Some(parent.to_path_buf());
        }
        let sdk_release = parent
            .join("_sdk_extract")
            .join("substance-win-x64-v9.6.2-bf39d392")
            .join("bin")
            .join("release");
        if sdk_release.is_dir() {
            return Some(sdk_release);
        }
        let repo_sdk = parent.join("..").join("..").join("_sdk_extract");
        if let Ok(entries) = std::fs::read_dir(&repo_sdk) {
            for entry in entries.flatten() {
                let bin = entry.path().join("bin").join("release");
                if bin.is_dir() {
                    return Some(bin);
                }
            }
        }
    }
    None
}

/// Bake one graph from an `.sbsar` into `out_dir` (created if missing). Writes `manifest.json` and TGA maps.
pub fn bake_sbsar(
    sbsar: &Path,
    out_dir: &Path,
    size_px: u32,
    graph_index: usize,
) -> Result<BakeManifest, String> {
    bake_sbsar_with_overrides(sbsar, out_dir, size_px, graph_index, &[])
}

/// Bake one graph from an `.sbsar` with `--set key=value` input overrides.
pub fn bake_sbsar_with_overrides(
    sbsar: &Path,
    out_dir: &Path,
    size_px: u32,
    graph_index: usize,
    overrides: &[(String, String)],
) -> Result<BakeManifest, String> {
    let host = resolve_host().ok_or(
        "SBSAR host not found. Build tools/sbsar-bake (see docs/sbsar-integration.md) or set SUBCRAFT_SBSAR_HOST.",
    )?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    let mut cmd = Command::new(&host);
    cmd.arg(sbsar).arg(out_dir).arg(size_px.to_string()).arg(graph_index.to_string());
    for (k, v) in overrides {
        cmd.arg("--set").arg(format!("{k}={v}"));
    }
    if let Some(bin) = substance_bin_dir(&host) {
        if let Ok(path) = std::env::join_paths([bin]) {
            if let Ok(existing) = std::env::var("PATH") {
                let merged = format!("{};{}", path.to_string_lossy(), existing);
                cmd.env("PATH", merged);
            } else {
                cmd.env("PATH", path);
            }
        }
    }

    let output = cmd.output().map_err(|e| format!("failed to run {}: {e}", host.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "SBSAR bake failed (exit {:?})\n{stderr}{stdout}",
            output.status.code()
        ));
    }

    let manifest_path = out_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: BakeManifest = serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    Ok(manifest)
}

/// Paths to baked map files, in manifest order (preferring PBR metal/rough outputs when both
/// metal/rough and spec/gloss outputs are present in the graph).
pub fn baked_texture_paths(out_dir: &Path, manifest: &BakeManifest) -> Vec<PathBuf> {
    let has_roughness = manifest
        .outputs
        .iter()
        .any(|o| o.identifier.eq_ignore_ascii_case("roughness"));
    let has_basecolor = manifest
        .outputs
        .iter()
        .any(|o| o.identifier.eq_ignore_ascii_case("basecolor"));

    manifest
        .outputs
        .iter()
        .filter(|o| {
            if has_roughness
                && (o.identifier.eq_ignore_ascii_case("glossiness")
                    || o.identifier.eq_ignore_ascii_case("specular"))
            {
                return false;
            }
            if has_basecolor && o.identifier.eq_ignore_ascii_case("diffuse") {
                return false;
            }
            true
        })
        .map(|o| out_dir.join(&o.file))
        .collect()
}
