//! Copy `.spp` to local disk before HDF5 random I/O (network shares otherwise hang for hours).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 8 * 1024 * 1024;

pub fn open_log_path() -> PathBuf {
    std::env::temp_dir().join("subcraft-spp-open.log")
}

pub fn log_step(msg: &str) {
    let line = format!("{}\n", msg);
    log::info!("{msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(open_log_path())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn should_stage(path: &Path) -> bool {
    if std::env::var("SUBCRAFT_SPP_NO_STAGE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return false;
    }
    if std::env::var("SUBCRAFT_SPP_STAGE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    // Network / non-C: drives: HDF5 random reads hang; one sequential copy is faster.
    path_is_probably_remote(path)
}

fn path_is_probably_remote(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\") {
        return true;
    }
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let drive = s[..2].to_uppercase();
        // C: is local; other drive letters are often network/USB in studio setups.
        return drive != "C:";
    }
    false
}

/// Return a local path suitable for `Hdf5File::open` (may equal `spp`).
pub fn ensure_local_copy(spp: &Path) -> Result<PathBuf, String> {
    let canonical = spp.canonicalize().map_err(|e| format!("{}: {e}", spp.display()))?;
    if !should_stage(&canonical) {
        log_step(&format!("SPP stage: using source {}", canonical.display()));
        return Ok(canonical);
    }

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project.spp".into());
    let dest = std::env::temp_dir().join("subcraft-spp-cache").join(name);
    if dest.is_file() {
        let src_len = std::fs::metadata(&canonical).map(|m| m.len()).ok();
        let dst_len = std::fs::metadata(&dest).map(|m| m.len()).ok();
        if src_len == dst_len && dst_len.is_some() {
            log_step(&format!("SPP stage: reusing cached {}", dest.display()));
            return Ok(dest);
        }
    }

    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    log_step(&format!(
        "SPP stage: copying {} → {} (one-time; avoids slow network HDF5)",
        canonical.display(),
        dest.display()
    ));

    let mut src = File::open(&canonical).map_err(|e| format!("{}: {e}", canonical.display()))?;
    let total = src.metadata().map(|m| m.len()).unwrap_or(0);
    let mut dst = File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    while let Ok(n) = src.read(&mut buf) {
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if total > 0 && done % (32 * 1024 * 1024) < CHUNK as u64 {
            log_step(&format!("SPP stage: {:.0}% ({done}/{total} bytes)", 100.0 * done as f64 / total as f64));
        }
    }
    log_step("SPP stage: copy complete");
    Ok(dest)
}
