//! Lazy `.sbsar` extraction from `resources/` using references found in HBO blobs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use hdf5_reader::Hdf5File;

use super::container::resolve_spp_path;

static SBSAR_REF: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn sbsar_re() -> &'static regex::Regex {
    SBSAR_REF.get_or_init(|| regex::Regex::new(r"[\w./\\-]+\.sbsar").expect("sbsar ref regex"))
}

pub fn sbsar_references(blobs: &[&[u8]]) -> HashSet<String> {
    let mut refs = HashSet::new();
    for blob in blobs {
        let latin = String::from_utf8_lossy(blob);
        for m in sbsar_re().find_iter(&latin) {
            refs.insert(m.as_str().to_string());
        }
        for s in utf16le_strings(blob, 8) {
            if s.to_ascii_lowercase().contains(".sbsar") {
                refs.insert(s);
            }
        }
    }
    refs
}

fn utf16le_strings(blob: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 < blob.len() {
        if blob[i] == 0 && blob[i + 1] >= 32 && blob[i + 1] < 127 {
            let mut j = i;
            let mut chars = Vec::new();
            while j + 1 < blob.len() {
                let lo = blob[j];
                let hi = blob[j + 1];
                if hi != 0 || !(32..127).contains(&lo) && !matches!(lo, 9 | 10 | 13) {
                    break;
                }
                chars.push(lo);
                j += 2;
            }
            let s = String::from_utf8_lossy(&chars).trim().to_string();
            if s.len() >= min_len {
                out.push(s);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

pub fn copy_referenced_sbsar(spp: &Path, document_and_settings: &[&[u8]], out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let refs = sbsar_references(document_and_settings);
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    let spp_path = resolve_spp_path(spp);
    let file = Hdf5File::open(&spp_path).map_err(|e| format!("HDF5 open {}: {e}", spp_path.display()))?;
    let sbsar_dir = out_dir.join("embedded_sbsar");
    std::fs::create_dir_all(&sbsar_dir).map_err(|e| format!("{}: {e}", sbsar_dir.display()))?;

    let mut written = Vec::new();
    // Direct dataset reads — never walk all of `resources/` (hundreds of MB in large .spp).
    for ref_path in &refs {
        let base = Path::new(ref_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if base.is_empty() || !base.to_ascii_lowercase().ends_with(".sbsar") {
            continue;
        }
        let dest = sbsar_dir.join(&base);
        if dest.is_file() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            written.push(dest);
            continue;
        }
        let dataset_paths = [
            format!("resources/{base}"),
            format!("resources/{ref_path}"),
            ref_path.replace('\\', "/"),
        ];
        let mut copied = false;
        for ds_path in dataset_paths {
            match file.dataset(&ds_path) {
                Ok(ds) => {
                    let bytes = ds.read_raw_bytes().map_err(|e| format!("read {ds_path}: {e}"))?;
                    std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
                    written.push(dest.clone());
                    copied = true;
                    break;
                }
                Err(_) => continue,
            }
        }
        if !copied {
            log::debug!("SBSAR reference not found in HDF5: {ref_path}");
        }
    }
    Ok(written)
}
