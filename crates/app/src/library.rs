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
    Ok(SmartMask {
        id: AssetId::from_name(&format!("smart_mask/user/{}", path.display())),
        name: display_name(&path, &f.name),
        resources: resources_of(&f),
        effects: f.mask_effects.into_iter().map(|e| e.kind).collect(),
        user_file: Some(path),
    })
}

/// Shelf name: the file name (renaming the file in Explorer renames the asset).
fn display_name(path: &Path, fallback: &str) -> String {
    path.file_stem().and_then(|s| s.to_str()).map(str::to_string).unwrap_or_else(|| fallback.to_string())
}

/// All smart materials in the library; unreadable files are reported, not fatal.
pub fn load_smart_materials(lib: &UserLibrary) -> (Vec<SmartMaterial>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for p in lib.list(UserAssetKind::SmartMaterial) {
        match load_fragment(&p).map_err(|e| format!("{}: {e}", p.display())).and_then(|f| smart_material_from(f, p.clone())) {
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
        match load_fragment(&p).map_err(|e| format!("{}: {e}", p.display())).and_then(|f| smart_mask_from(f, p.clone())) {
            Ok(m) => ok.push(m),
            Err(e) => errors.push(e),
        }
    }
    (ok, errors)
}
