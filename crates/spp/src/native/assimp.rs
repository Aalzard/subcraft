//! Convert Painter `scene.assbin` to GLB via bundled Assimp CLI (until `assimp-sys` lands).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const ASSIMP_TIMEOUT: Duration = Duration::from_secs(25);

pub fn try_assbin_to_glb(assbin: &Path, glb: &Path) -> Option<PathBuf> {
    let Some(cmd_base) = resolve_assimp_export_cmd() else {
        return None;
    };
    if let Some(parent) = glb.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut cmd = Command::new(&cmd_base[0]);
    for arg in &cmd_base[1..] {
        cmd.arg(arg);
    }
    cmd.arg(assbin).arg(glb);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("assimp spawn failed: {e}");
            return None;
        }
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() && glb.is_file() && glb.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    return Some(glb.to_path_buf());
                }
                log::warn!("assimp export failed ({status})");
                return None;
            }
            Ok(None) => {
                if start.elapsed() > ASSIMP_TIMEOUT {
                    let _ = child.kill();
                    log::warn!("assimp export timed out after {}s", ASSIMP_TIMEOUT.as_secs());
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                log::warn!("assimp wait failed: {e}");
                return None;
            }
        }
    }
}

/// `true` when `bin/assimp/assimp.exe` or `SUBCRAFT_ASSIMP` is set (public Assimp — not Painter 1.7 assbin).
pub fn assimp_cli_available() -> bool {
    resolve_assimp_export_cmd().is_some()
}

fn resolve_assimp_export_cmd() -> Option<Vec<String>> {
    if let Ok(env) = std::env::var("SUBCRAFT_ASSIMP") {
        if Path::new(&env).is_file() {
            return Some(vec![env, "export".into()]);
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.join("../..");
    for rel in [
        "bin/assimp/assimp.exe",
        "bin/assimp/Assimp.exe",
        "bin/assimp/bin/assimp.exe",
        "bin/assimp/bin/x64/assimp.exe",
    ] {
        let exe = repo.join(rel);
        if exe.is_file() {
            return Some(vec![exe.display().to_string(), "export".into()]);
        }
    }
    None
}
