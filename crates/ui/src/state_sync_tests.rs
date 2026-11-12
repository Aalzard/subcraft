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
