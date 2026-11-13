//! The 3D view's camera and the 2D view's pan and zoom, saved with the project (issue #321):
//! captured every frame into the project settings (which Save and the autosave write; moving
//! around never marks the project as modified) and put back once when a project opens. A
//! project without a saved view, or with a broken one, opens with the default framing.

use subcraft_doc::ViewState;

use crate::state::UiState;

/// The current view.
pub fn capture(state: &UiState) -> ViewState {
    let c = &state.camera;
    ViewState {
        target: c.target.to_array(),
        yaw: c.yaw,
        pitch: c.pitch,
        roll: c.roll,
        distance: c.distance,
        fov_y_deg: c.fov_y_deg,
        orthographic: c.orthographic,
        pivot: state.orbit_pivot.map(|p| p.to_array()),
        uv_zoom: state.uv.zoom,
        uv_offset: [state.uv.offset.x, state.uv.offset.y],
    }
}

/// Put a saved view back (the scene scale for the clip planes stays the open model's).
pub fn apply(state: &mut UiState, v: &ViewState) {
    let c = &mut state.camera;
    c.target = glam::Vec3::from_array(v.target);
    c.yaw = v.yaw;
    c.pitch = v.pitch;
    c.roll = v.roll;
    c.distance = v.distance;
    c.fov_y_deg = v.fov_y_deg;
    c.orthographic = v.orthographic;
    state.recovery.prefs.viewport.orthographic = v.orthographic;
    state.orbit_pivot = v.pivot.map(glam::Vec3::from_array);
    state.uv.zoom = v.uv_zoom.clamp(0.2, 20.0);
    state.uv.offset = egui::vec2(v.uv_offset[0], v.uv_offset[1]);
}

/// Once per opened project: restore its saved view. Every frame: keep the project's copy of the
/// view current. Call after the display settings sync, so a saved lens wins over theirs.
pub fn sync(state: &mut UiState) {
    if !std::mem::replace(&mut state.view_loaded, true) {
        // A view given on the command line (scripted screenshots) wins over the saved one.
        if state.camera_view.is_none()
            && let Some(saved) = state.session.settings().view.filter(ViewState::is_valid)
        {
            apply(state, &saved);
        }
    }
    let now = capture(state);
    if state.session.settings().view != Some(now) {
        state.session.set_view_state(Some(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Session;

    fn state() -> UiState {
        UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
    }

    #[test]
    fn the_view_is_saved_with_the_project_without_modifying_it_and_restored_on_open() {
        let mut s = state();
        let path = std::env::temp_dir().join(format!("subcraft-view-state-{}.subcraft", std::process::id()));
        s.session.save(Some(&path)).unwrap();
        sync(&mut s);
        assert!(!s.session.is_modified(), "keeping the view current is not an edit");
        // Move around in both views.
        s.camera.orbit(120.0, -40.0);
        s.camera.zoom(0.6);
        s.camera.target = glam::Vec3::new(0.3, -0.2, 0.1);
        s.camera.fov_y_deg = 28.0;
        s.orbit_pivot = Some(glam::Vec3::new(0.5, 0.5, 0.5));
        s.uv.zoom = 3.0;
        s.uv.offset = egui::vec2(-60.0, 25.0);
        sync(&mut s);
        let moved = capture(&s);
        assert_eq!(s.session.settings().view, Some(moved));
        assert!(!s.session.is_modified(), "moving the camera is not an edit");
        s.session.save(Some(&path)).unwrap();

        // Reopen: the saved view comes back.
        let mut reopened = state();
        reopened.set_session(Session::open_project(&path).unwrap());
        sync(&mut reopened);
        assert_eq!(capture(&reopened), moved);
        assert_eq!(reopened.camera.view(), s.camera.view(), "the same 3D camera");
        assert!(!reopened.session.is_modified());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn projects_without_a_view_or_with_a_broken_one_keep_the_default_framing() {
        let mut s = state();
        let framing = s.camera;
        sync(&mut s);
        assert_eq!(s.camera, framing, "no saved view: today's framing");
        let mut broken = state();
        broken.session.set_view_state(Some(ViewState { distance: f32::NAN, ..capture(&broken) }));
        broken.view_loaded = false;
        let framing = broken.camera;
        sync(&mut broken);
        assert_eq!(broken.camera, framing, "a broken view is ignored");
        assert!(broken.session.settings().view.is_some_and(|v| v.is_valid()), "and replaced by the current one");
    }
}
