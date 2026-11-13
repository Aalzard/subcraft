//! Painter's mask view: Alt+click on a mask thumbnail shows that layer's mask alone on the model
//! (gray), Alt+click again (or Esc in the viewport) goes back. Also the mask menu's export.

use subcraft_core::LayerId;

use crate::state::UiState;

/// The mask shown in the 3D view, and what was last uploaded for it.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskView {
    pub layer: LayerId,
    /// Document revision and time of the last evaluation (None: not uploaded yet).
    uploaded: Option<(u64, web_time::Instant)>,
}

/// Re-evaluate at most this often while the document keeps changing (painting into the mask).
const REFRESH_SECONDS: f64 = 0.25;

/// Request ids the mask menu uses (it has no access to the UI state).
pub fn toggle_request_id() -> egui::Id {
    egui::Id::new("mask_view_toggle")
}

pub fn export_request_id() -> egui::Id {
    egui::Id::new("mask_export")
}

/// Show `layer`'s mask in the viewport, or go back to the material when it is already shown.
pub fn toggle(state: &mut UiState, layer: LayerId) {
    state.mask_view = match &state.mask_view {
        Some(v) if v.layer == layer => None,
        _ => Some(MaskView { layer, uploaded: None }),
    };
}

/// Whether the viewport shows a mask (for the render parameters).
pub fn active(state: &UiState) -> bool {
    state.mask_view.is_some()
}

/// How long until the shown mask should be re-evaluated, when it is older than the document: a
/// change within [`REFRESH_SECONDS`] of the last upload is skipped by [`sync`], and nothing else
/// may repaint to pick it up (QA 2026-11: the last dab painted into a shown mask appeared only
/// once something else repainted).
pub fn refresh_due(state: &UiState) -> Option<std::time::Duration> {
    let (revision, at) = state.mask_view.as_ref()?.uploaded?;
    (revision != state.session.revision()).then(|| std::time::Duration::from_secs_f64((REFRESH_SECONDS - at.elapsed().as_secs_f64()).max(0.0)))
}

/// Text of the viewport label while a mask is shown.
pub fn label(state: &UiState) -> Option<String> {
    let v = state.mask_view.as_ref()?;
    let name = state.session.selected_texture_set().find(v.layer).map(|l| l.name.clone()).unwrap_or_default();
    Some(format!("Mask of \"{name}\" · Alt+click the mask again or Esc: back to the material"))
}

/// Handle the mask menu's requests (call once per frame).
pub fn handle_requests(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if let Some(layer) = ctx.data_mut(|d| d.remove_temp::<LayerId>(toggle_request_id())) {
        toggle(state, layer);
    }
    if let Some(layer) = ctx.data_mut(|d| d.remove_temp::<LayerId>(export_request_id())) {
        export(state, layer, now);
    }
}

/// Painter's "Export mask to file".
pub fn export(state: &mut UiState, layer: LayerId, now: f64) {
    let name = state.session.selected_texture_set().find(layer).map(|l| subcraft_export::sanitize(&format!("{}_mask", l.name))).unwrap_or_else(|| "mask".into());
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .add_filter("TGA image", &["tga"])
        .add_filter("EXR image", &["exr"])
        .add_filter("TIFF image", &["tif", "tiff"])
        .set_file_name(format!("{name}.png"))
        .save_file()
    else {
        return;
    };
    let format = format_for(&path);
    match state.session.export_mask(layer, &path, format) {
        Ok(()) => state.set_status(format!("Exported the mask to {}", path.display()), now),
        Err(e) => state.set_status(format!("Export mask: {e}"), now),
    }
}

/// File format from the extension: 8-bit PNG / TGA / TIFF, 32-bit float EXR.
pub fn format_for(path: &std::path::Path) -> subcraft_image::FileFormat {
    use subcraft_image::FileFormat;
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("tga") => FileFormat::Tga8,
        Some("exr") => FileFormat::Exr32,
        Some("tif" | "tiff") => FileFormat::Tiff8,
        _ => FileFormat::Png8,
    }
}

/// Keep the uploaded mask current (called from the GPU sync). Leaves the mask view when the
/// layer or its mask is gone.
pub fn sync(state: &mut UiState) {
    let (Some(rs), Some(renderer)) = (&state.render_state, &mut state.renderer) else { return };
    let Some(view) = state.mask_view.as_mut() else {
        renderer.clear_mask_views();
        return;
    };
    let revision = state.session.revision();
    let fresh = view.uploaded.is_some_and(|(r, t)| r == revision || t.elapsed().as_secs_f64() < REFRESH_SECONDS);
    if fresh {
        return;
    }
    match state.session.mask_image(view.layer) {
        Ok(m) => {
            renderer.clear_mask_views();
            renderer.upload_mask_view(&rs.device, &rs.queue, m.material_slot, Some(&m.image));
            view.uploaded = Some((revision, web_time::Instant::now()));
            // A throttled upload lands after the revision changed: TAA must see it as new data.
            state.gpu_changed = true;
        }
        Err(_) => {
            renderer.clear_mask_views();
            state.mask_view = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::{Command, Session};

    #[test]
    fn alt_click_toggles_the_mask_view_and_a_layer_without_mask_ends_it() {
        let mut s = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let layer = s.session.selection().layer.unwrap();
        s.session.execute(Command::AddMask { layer, white: false }).unwrap();
        toggle(&mut s, layer);
        assert!(active(&s));
        assert!(label(&s).unwrap().contains("Mask of"));
        toggle(&mut s, layer);
        assert!(!active(&s), "Alt+click again goes back");
        // Another layer's mask replaces the shown one.
        toggle(&mut s, layer);
        let other = LayerId::new();
        toggle(&mut s, other);
        assert_eq!(s.mask_view.as_ref().map(|v| v.layer), Some(other));
        assert_eq!(format_for(std::path::Path::new("a.EXR")).extension(), "exr");
        // A change after the last upload asks for a repaint once the refresh interval is over.
        s.mask_view = Some(MaskView { layer, uploaded: Some((s.session.revision(), web_time::Instant::now())) });
        assert_eq!(refresh_due(&s), None, "up to date");
        s.session.execute(Command::SetMaskEnabled { layer, enabled: false }).unwrap();
        let due = refresh_due(&s).expect("the shown mask is older than the document");
        assert!(due.as_secs_f64() <= REFRESH_SECONDS, "{due:?}");
        assert_eq!(format_for(std::path::Path::new("a")).extension(), "png");
    }
}
