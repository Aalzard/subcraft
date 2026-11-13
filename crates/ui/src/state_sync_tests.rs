//! UI state that must follow the session: caches and pending work that outlived what they
//! belonged to.
use super::*;
use subcraft_mesh::primitives;

fn state() -> UiState {
    UiState::new(Session::new(primitives::sample_crate(), 64), None)
}

/// A Painter import that was still running when another project was opened used to hand its
/// own session over when it finished, discarding everything done in the new project.
#[test]
fn replacing_the_session_cancels_a_running_painter_import() {
    let mut state = state();
    crate::spp_import::start(&mut state, std::path::Path::new("does-not-exist.spp"));
    assert!(state.spp_import.is_some());
    state.set_session(Session::new(primitives::cube(2.0), 64));
    assert!(state.spp_import.is_none(), "the import must not outlive the session it was opening into");
}

#[test]
fn reloading_the_mesh_drops_the_uv_density_cache() {
    let mut state = state();
    state.uv_density.insert(0, 123.0);
    state.mesh_reloaded();
    assert!(state.uv_density.is_empty(), "the 2D-view brush cursor would keep the old mesh's UV density");
}

/// Delete / Duplicate act on the multi-selection's layer ids wherever they live, so ids kept
/// from another texture set edited layers the user was no longer looking at.
#[test]
fn switching_texture_set_clears_the_multi_selection() {
    let mut state = state();
    let sets: Vec<_> = state.session.document().texture_sets.iter().map(|t| t.id).collect();
    assert!(sets.len() >= 2, "sample crate has two material slots");
    state.begin_frame();
    let rows: Vec<_> = state.session.selected_texture_set().layers.iter().map(|l| l.id).collect();
    state.multi.select_all(&rows);
    state.begin_frame();
    assert_eq!(state.multi.ids.len(), rows.len(), "staying on the set keeps the selection");
    let other = sets.iter().copied().find(|id| *id != state.session.selection().texture_set).unwrap();
    state.session.execute(Command::SelectTextureSet(other)).unwrap();
    state.begin_frame();
    assert!(state.multi.ids.is_empty());
}

/// QA 2026-11: the orbit pivot (a point on the model under the cursor) survived opening another
/// model or reloading the mesh, so the next orbit swung around empty space.
#[test]
fn the_orbit_pivot_does_not_outlive_its_model() {
    let mut state = state();
    state.orbit_pivot = Some(glam::Vec3::splat(0.9));
    state.mesh_reloaded();
    assert!(state.orbit_pivot.is_none());
    state.orbit_pivot = Some(glam::Vec3::splat(0.9));
    state.set_session(Session::new(primitives::cube(200.0), 64));
    assert!(state.orbit_pivot.is_none());
}

/// QA 2026-11: after File > Reload mesh the selected imported camera kept its index, so the
/// dropdown showed the new file's camera name while the view was still the old camera's.
#[test]
fn reloading_the_mesh_deselects_the_imported_camera() {
    let mut state = state();
    state.active_camera = Some(0);
    state.mesh_reloaded();
    assert_eq!(state.active_camera, None);
}

/// QA 2026-11: the start of a Shift + click line is a screen position. After the camera moved it
/// was still used, so the line started from whatever was under that pixel now.
#[test]
fn a_straight_line_does_not_start_from_a_point_seen_through_another_view() {
    let mut state = state();
    let view = |z: f32| subcraft_app::Camera::perspective(glam::Vec3::new(0.0, 0.0, z), glam::Vec3::ZERO, glam::Vec3::Y, 40.0, 0.1, 100.0, [640, 480]);
    let p = Vec2::new(100.0, 120.0);
    state.set_line_start(p, &view(5.0));
    assert_eq!(state.line_start(&view(5.0)), Some(p), "same view: the line continues from the stroke's end");
    assert_eq!(state.line_start(&view(6.0)), None, "the camera moved: no stale start");
    assert_eq!(state.line_start(&view(5.0)), None, "and it stays forgotten");

/// QA 2026-11: the mask view (the viewport showing a layer's mask) stayed on after another
/// project was opened, pointing at a layer of the old one.
#[test]
fn replacing_the_session_leaves_the_mask_view() {
    let mut state = state();
    let layer = state.session.selection().layer.expect("the sample has a layer");
    crate::mask_view::toggle(&mut state, layer);
    assert!(state.mask_view.is_some());
    state.set_session(Session::new(primitives::cube(2.0), 64));
    assert!(state.mask_view.is_none());
}
