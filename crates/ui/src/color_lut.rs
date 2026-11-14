//! The colour profile's LUT in the UI: which project resource the 3D view uses, its pixels for
//! the renderer, its name for the view's corner, and loading one from a file (Painter's
//! "colorlut" resources; saved with the project, identified by UUID).

use subcraft_core::ResourceId;
use subcraft_doc::{ResourceKind, ResourceStore};

use crate::state::UiState;
use crate::viewport_bar::ViewportPrefs;

/// The LUT the renderer should hold: the display settings' LUT while the profile is on and the
/// project has it as a colour LUT, otherwise none.
pub fn wanted(prefs: &ViewportPrefs, resources: &ResourceStore) -> Option<ResourceId> {
    prefs.color_lut.filter(|_| prefs.color_profile).filter(|id| resources.get(*id).is_some_and(|r| r.kind == ResourceKind::ColorLut))
}

/// The LUT image as RGBA float entries, row by row (data values: never colour-converted).
pub fn pixels(bitmap: &subcraft_image::Bitmap) -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity((bitmap.width * bitmap.height) as usize);
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            out.push(bitmap.texel(x, y, false));
        }
    }
    out
}

/// The project's colour LUTs, by id and name.
pub fn names(resources: &ResourceStore) -> Vec<(ResourceId, String)> {
    resources.iter().filter(|r| r.kind == ResourceKind::ColorLut).map(|r| (r.id, r.name.clone())).collect()
}

pub fn name_in(names: &[(ResourceId, String)], id: ResourceId) -> Option<String> {
    names.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone())
}

/// What the 3D view's corner says while the profile is on: the LUT's name, or that the view is
/// linear raw.
pub fn profile_name(prefs: &ViewportPrefs, resources: &ResourceStore) -> String {
    match wanted(prefs, resources).and_then(|id| resources.get(id)) {
        Some(r) => r.name.clone(),
        None => crate::i18n::tr("Linear raw").to_string(),
    }
}

/// Load > pick a LUT image and use it: added to the project and set as the profile's LUT.
pub fn load_with_dialog(state: &mut UiState, now: f64) {
    let Some(path) = rfd::FileDialog::new().add_filter("LUT image", &["exr", "hdr", "tif", "tiff", "png"]).pick_file() else { return };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "LUT".into());
    let result = std::fs::read(&path).map_err(|e| format!("{name}: {e}")).and_then(|bytes| state.session.add_color_lut(&name, bytes));
    match result {
        Ok(id) => {
            use_lut(&mut state.recovery.prefs.viewport, id);
            state.set_status(format!("Color profile: {name}"), now);
        }
        Err(e) => state.set_status(format!("Color LUT: {e}"), now),
    }
}

/// Make `id` the profile's LUT and turn the profile on.
pub fn use_lut(prefs: &mut ViewportPrefs, id: ResourceId) {
    prefs.color_lut = Some(id);
    prefs.color_profile = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Session;

    /// A 4-cube identity LUT (16 x 4) as an EXR file's bytes.
    fn lut_file() -> Vec<u8> {
        let n = 4u32;
        let data: Vec<f32> = (0..n * n * n).flat_map(|i| [(i % n) as f32 / 3.0, ((i / n) % n) as f32 / 3.0, (i / n / n) as f32 / 3.0, 1.0]).collect();
        let bitmap = subcraft_image::Bitmap::new_f32(16, 4, 4, data);
        subcraft_image::encode_bitmap(&bitmap).expect("float images encode losslessly").0
    }

    #[test]
    fn a_loaded_lut_is_a_project_resource_used_by_the_profile_and_saved_with_it() {
        let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
        let id = session.add_color_lut("look.exr", lut_file()).unwrap();
        assert!(session.add_color_lut("photo.png", subcraft_image::encode_bitmap(&subcraft_image::Bitmap::new_u8(10, 10, 3, vec![0; 300])).unwrap().0).is_err(), "a 10 x 10 image is not a cube");
        let mut prefs = ViewportPrefs::default();
        assert_eq!(wanted(&prefs, session.resources()), None);
        use_lut(&mut prefs, id);
        assert_eq!(wanted(&prefs, session.resources()), Some(id));
        assert_eq!(profile_name(&prefs, session.resources()), "look.exr");
        let px = pixels(&session.resources().get(id).unwrap().bitmap);
        assert_eq!(px.len(), 64);
        assert!((px[1][0] - 1.0 / 3.0).abs() < 1e-3 && px[1][1].abs() < 1e-6, "data values, red fastest: {:?}", px[1]);
        // Off, or a LUT the project does not have: none.
        prefs.color_profile = false;
        assert_eq!(wanted(&prefs, session.resources()), None);
        prefs.color_profile = true;
        let other = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
        assert_eq!(wanted(&prefs, other.resources()), None);
        assert_eq!(profile_name(&prefs, other.resources()), crate::i18n::tr("Linear raw"));
        // Saved with the project through the display settings, and back on open.
        let mut settings = session.settings().clone();
        settings.display = Some(prefs.display.clone());
        session.set_settings(settings);
        let path = std::env::temp_dir().join(format!("subcraft-color-lut-{}.subcraft", std::process::id()));
        session.save(Some(&path)).unwrap();
        let reopened = Session::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let r = reopened.resources().get(id).expect("the LUT is saved with the project");
        assert_eq!((r.kind, r.name.as_str(), r.bitmap.width, r.bitmap.height), (ResourceKind::ColorLut, "look.exr", 16, 4));
        assert_eq!(reopened.settings().display.as_ref().and_then(|d| d.color_lut), Some(id));
        let back = pixels(&r.bitmap);
        assert!(back.iter().zip(&px).all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)), "the same values after the round trip");
    }
}
