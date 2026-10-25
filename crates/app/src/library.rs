//! Saving layers / masks to the user library as smart materials / smart masks, and loading them
//! back for the shelf.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_assets::{SmartMask, SmartMaterial, UserAssetKind, UserLibrary};
use subcraft_core::{AssetId, LayerId};
use subcraft_doc::ResourceStore;
use subcraft_project::fragment::{Fragment, FragmentKind, load_fragment, save_fragment};

use crate::session::Session;

impl Session {
    /// Save a layer (usually a folder) of the selected texture set as a smart material named
    /// after it. Returns the shelf entry for the new file.
    pub fn save_smart_material(&self, layer: LayerId, lib: &UserLibrary) -> Result<SmartMaterial, String> {
        let ts = self.selected_texture_set();
        let l = ts.find(layer).ok_or("select a layer to save")?.clone();
        let mut fragment = Fragment {
            kind: FragmentKind::SmartMaterial,
            name: l.name.clone(),
            resolution: ts.resolution,
            layers: vec![l],
            mask_effects: Vec::new(),
            resources: ResourceStore::default(),
        };
        fragment.resources = self.resources_subset(&fragment.referenced_resources());
        let path = write(lib, UserAssetKind::SmartMaterial, &fragment)?;
        smart_material_from(fragment, path)
    }

    /// Save the mask effects of a layer of the selected texture set as a smart mask named after
    /// the layer.
    pub fn save_smart_mask(&self, layer: LayerId, lib: &UserLibrary) -> Result<SmartMask, String> {
        let ts = self.selected_texture_set();
        let l = ts.find(layer).ok_or("select a layer to save")?;
        let mask = l.mask.as_ref().ok_or("the layer has no mask")?;
        if mask.effects.is_empty() {
            return Err("the mask has no effects to save (add generators, fills, images…)".into());
        }
        let mut fragment = Fragment {
            kind: FragmentKind::SmartMask,
            name: format!("{} mask", l.name),
            resolution: ts.resolution,
            layers: Vec::new(),
            mask_effects: mask.effects.iter().map(|e| (**e).clone()).collect(),
            resources: ResourceStore::default(),
        };
        fragment.resources = self.resources_subset(&fragment.referenced_resources());
        let path = write(lib, UserAssetKind::SmartMask, &fragment)?;
        smart_mask_from(fragment, path)
    }

    fn resources_subset(&self, ids: &[subcraft_core::ResourceId]) -> ResourceStore {
        let mut out = ResourceStore::default();
        for &id in ids {
            if let Some(r) = self.resources().get(id) {
                out.insert_shared(r.clone());
            }
        }
        out
    }
}

fn write(lib: &UserLibrary, kind: UserAssetKind, f: &Fragment) -> Result<PathBuf, String> {
    let path = lib.new_file_path(kind, &f.name);
    std::fs::create_dir_all(lib.folder(kind)).map_err(|e| format!("{}: {e}", lib.folder(kind).display()))?;
    save_fragment(&path, f).map_err(|e| format!("{}: {e}", path.display()))?;
    log::info!("saved {}", path.display());
    Ok(path)
}

fn resources_of(f: &Fragment) -> Vec<Arc<subcraft_doc::Resource>> {
    f.referenced_resources().into_iter().filter_map(|id| f.resources.get(id).cloned()).collect()
}

fn smart_material_from(f: Fragment, path: PathBuf) -> Result<SmartMaterial, String> {
    if f.kind != FragmentKind::SmartMaterial || f.layers.len() != 1 {
        return Err(format!("{}: not a smart material", path.display()));
    }
    let resources = resources_of(&f);
    let name = display_name(&path, &f.name);
    let layer = f.layers.into_iter().next().expect("checked: one layer");
    Ok(SmartMaterial::saved(name, layer, f.resolution, resources, Some(path)))
}

fn smart_mask_from(f: Fragment, path: PathBuf) -> Result<SmartMask, String> {
    if f.kind != FragmentKind::SmartMask {
        return Err(format!("{}: not a smart mask", path.display()));
    }
    let full_effects = f.mask_effects.clone();
    Ok(SmartMask {
        id: AssetId::from_name(&format!("smart_mask/user/{}", path.display())),
        name: display_name(&path, &f.name),
        resources: resources_of(&f),
        effects: f.mask_effects.into_iter().map(|e| e.kind).collect(),
        full_effects,
        user_file: Some(path),
        source_spmsk: None,
    })
}

/// Shelf name: the file name (renaming the file in Explorer renames the asset).
fn display_name(path: &Path, fallback: &str) -> String {
    path.file_stem().and_then(|s| s.to_str()).map(str::to_string).unwrap_or_else(|| fallback.to_string())
}

/// Load a single smart material file (`.scsmart` or Substance Painter `.spsm`).
pub fn load_smart_material_file(path: &Path) -> Result<SmartMaterial, String> {
    let is_spsm = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("spsm"));
    if is_spsm {
        let node = subcraft_spp::decode_spsm(path)?;
        let resolution = 1024;
        let layer = crate::spp_convert::spp_node_to_layer(&node, resolution);
        let name = display_name(path, &layer.name);
        Ok(SmartMaterial::saved(
            name,
            layer,
            resolution,
            Vec::new(),
            Some(path.to_path_buf()),
        )
        .with_source_spsm(path.to_path_buf()))
    } else {
        load_fragment(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|f| smart_material_from(f, path.to_path_buf()))
    }
}

/// Load a single smart mask file (`.scmask` or Substance Painter `.spmsk`).
pub fn load_smart_mask_file(path: &Path) -> Result<SmartMask, String> {
    let is_spmsk = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("spmsk"));
    if is_spmsk {
        let spp_fx = subcraft_spp::decode_spmsk(path)?;
        let full_effects = crate::spp_convert::spp_effects_to_mask_effects(&spp_fx);
        if full_effects.is_empty() {
            return Err(format!("{}: no decodable mask effects", path.display()));
        }
        let effects = full_effects.iter().map(|e| e.kind.clone()).collect();
        let name = display_name(path, "Smart Mask");
        Ok(SmartMask {
            id: AssetId::from_name(&format!("smart_mask/user/{}", path.display())),
            name,
            effects,
            full_effects,
            resources: Vec::new(),
            user_file: Some(path.to_path_buf()),
            source_spmsk: Some(path.to_path_buf()),
        })
    } else {
        load_fragment(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|f| smart_mask_from(f, path.to_path_buf()))
    }
}

/// All smart materials in the library; unreadable files are reported, not fatal.
pub fn load_smart_materials(lib: &UserLibrary) -> (Vec<SmartMaterial>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for p in lib.list(UserAssetKind::SmartMaterial) {
        match load_smart_material_file(&p) {
            Ok(m) => ok.push(m),
            Err(e) => errors.push(e),
        }
    }
    (ok, errors)
}

/// All smart masks in the library; unreadable files are reported, not fatal.
pub fn load_smart_masks(lib: &UserLibrary) -> (Vec<SmartMask>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for p in lib.list(UserAssetKind::SmartMask) {
        match load_smart_mask_file(&p) {
            Ok(m) => ok.push(m),
            Err(e) => errors.push(e),
        }
    }
    (ok, errors)
}

/// Default Substance 3D Painter starter `smart-materials` folder if installed on disk.
pub fn painter_starter_smart_materials_dir() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-materials",
        r"C:\Program Files\Allegorithmic\Substance Painter\resources\shelf\allegorithmic\smart-materials",
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

/// Default Substance 3D Painter starter `smart-masks` folder if installed on disk.
pub fn painter_starter_smart_masks_dir() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-masks",
        r"C:\Program Files\Allegorithmic\Substance Painter\resources\shelf\allegorithmic\smart-masks",
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

fn collect_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_files_recursive(&p, ext, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        {
            out.push(p);
        }
    }
}

/// Load all `.spsm` files from a folder tree (such as Painter's starter assets).
pub fn load_spsm_directory(dir: &Path) -> Vec<SmartMaterial> {
    let mut files = Vec::new();
    collect_files_recursive(dir, "spsm", &mut files);
    let mut out = Vec::new();
    for f in files {
        if let Ok(mut sm) = load_smart_material_file(&f) {
            // Not inside UserLibrary, so don't mark as removable user_file unless copied
            sm.user_file = None;
            out.push(sm);
        }
    }
    out
}

/// Load all `.spmsk` files from a folder tree (such as Painter's starter assets).
pub fn load_spmsk_directory(dir: &Path) -> Vec<SmartMask> {
    let mut files = Vec::new();
    collect_files_recursive(dir, "spmsk", &mut files);
    let mut out = Vec::new();
    for f in files {
        if let Ok(mut m) = load_smart_mask_file(&f) {
            // Keep `source_spmsk` for bake-on-apply; only clear removable-library marking.
            m.user_file = None;
            out.push(m);
        }
    }
    out
}
