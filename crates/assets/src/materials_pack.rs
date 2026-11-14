//! "SubCraft Materials" packs: a folder of tileable PBR materials made for SubCraft, read from
//! disk (never embedded; the pack is a separate download). Each material is a sub-folder with
//! its maps (base colour, normal, height, roughness, AO, optionally metallic and emissive),
//! `material.json` and `preview.png`:
//!
//! ```json
//! { "name": "wood_ebony", "display_name": "Wood Ebony", "category": "Wood", "tags": ["ebony"],
//!   "maps": { "basecolor": "wood_ebony_basecolor.png", "normal_gl": "...", "height": "...",
//!             "roughness": "...", "ao": "..." },
//!   "preview": "preview.png" }
//! ```
//!
//! Folders whose name starts with `_` or `.` (tools, caches) and folders without a readable
//! `material.json` are skipped. Map files are read only when a material is applied.

use std::path::{Path, PathBuf};

use subcraft_core::AssetId;

/// One material of a pack.
#[derive(Clone, Debug, PartialEq)]
pub struct PackMaterial {
    /// Stable across runs: from the folder name.
    pub id: AssetId,
    /// Shown name (`display_name`, else `name`, else the folder name).
    pub name: String,
    /// Folder name inside the pack.
    pub key: String,
    pub category: String,
    pub tags: Vec<String>,
    /// Map files that exist (any order). Their file names say the channel
    /// (`texture_maps::classify`): basecolor, normal_gl / normal_dx, height, roughness, ao,
    /// metallic, emissive.
    pub maps: Vec<PathBuf>,
    pub preview: Option<PathBuf>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct MaterialJson {
    name: String,
    display_name: String,
    category: String,
    tags: Vec<String>,
    /// Map key → file name.
    maps: serde_json::Map<String, serde_json::Value>,
    preview: String,
}

/// Read one material folder, or `None` when it has no usable `material.json` or no maps.
pub fn read_material(folder: &Path) -> Option<PackMaterial> {
    let key = folder.file_name()?.to_str()?.to_string();
    let text = std::fs::read_to_string(folder.join("material.json")).ok()?;
    let json: MaterialJson = serde_json::from_str(&text).ok()?;
    let maps: Vec<PathBuf> = json.maps.values().filter_map(|v| v.as_str()).map(|f| folder.join(f)).filter(|p| p.is_file()).collect();
    if maps.is_empty() {
        return None;
    }
    let name = [json.display_name.trim(), json.name.trim(), key.as_str()].into_iter().find(|n| !n.is_empty()).unwrap_or_default().to_string();
    let preview = [json.preview.as_str(), "preview.png"].into_iter().filter(|f| !f.is_empty()).map(|f| folder.join(f)).find(|p| p.is_file());
    Some(PackMaterial {
        id: AssetId::from_name(&format!("materials-pack/{key}")),
        name,
        key,
        category: if json.category.trim().is_empty() { "Materials".into() } else { json.category.trim().to_string() },
        tags: json.tags,
        maps,
        preview,
    })
}

/// Every material of the pack at `root`, sorted by category then name. Empty when the folder
/// is missing.
pub fn load_pack(root: &Path) -> Vec<PackMaterial> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<PackMaterial> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('_') && !n.starts_with('.')))
        .filter_map(|p| read_material(&p))
        .collect();
    out.sort_by(|a, b| (a.category.to_lowercase(), a.name.to_lowercase()).cmp(&(b.category.to_lowercase(), b.name.to_lowercase())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-pack-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_pack_lists_its_materials_with_categories_maps_and_previews() {
        let root = temp("list");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/materials_pack");
        for entry in std::fs::read_dir(&fixtures).unwrap() {
            let src = entry.unwrap().path();
            let dst = root.join(src.file_name().unwrap());
            std::fs::create_dir_all(&dst).unwrap();
            for f in std::fs::read_dir(&src).unwrap() {
                let f = f.unwrap().path();
                std::fs::copy(&f, dst.join(f.file_name().unwrap())).unwrap();
            }
        }
        // Tools and broken folders are left out.
        std::fs::create_dir_all(root.join("_tools")).unwrap();
        std::fs::write(root.join("_tools").join("material.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("broken")).unwrap();
        std::fs::write(root.join("broken").join("material.json"), "not json").unwrap();
        let pack = load_pack(&root);
        let names: Vec<&str> = pack.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Metal Test Plate", "Wood Test Planks"], "sorted by category (Metal, Wood) then name");
        let wood = &pack[1];
        assert_eq!((wood.key.as_str(), wood.category.as_str()), ("wood_test", "Wood"));
        assert_eq!(wood.tags, ["oak", "planks"]);
        let mut files: Vec<String> = wood.maps.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        files.sort();
        assert_eq!(files, ["wood_test_ao.png", "wood_test_basecolor.png", "wood_test_height.png", "wood_test_normal_gl.png", "wood_test_roughness.png"], "every map that exists");
        assert!(wood.preview.as_ref().is_some_and(|p| p.ends_with("preview.png")));
        assert_eq!(wood.id, AssetId::from_name("materials-pack/wood_test"), "stable ids");
        // The metal one names a metallic map and a missing emissive file: only existing maps.
        assert!(pack[0].maps.iter().any(|p| p.to_string_lossy().contains("metallic")));
        assert!(!pack[0].maps.iter().any(|p| p.to_string_lossy().contains("emissive")));
        assert!(load_pack(&root.join("missing")).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
