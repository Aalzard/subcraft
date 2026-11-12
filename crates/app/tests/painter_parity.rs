//! Painter-parity commands behind the key map: isolate a texture set (Alt+Q) and invert the
//! mask brush (X).
use glam::{Vec2, Vec3};
use subcraft_app::{Action, Command, Session};
use subcraft_mesh::{Corner, Mesh, MeshBuilder};

/// Three quads, one material each, so the project has three texture sets.
fn three_materials() -> Mesh {
    let mut b = MeshBuilder::new("Parts");
    for (i, name) in ["Body", "Glass", "Trim"].into_iter().enumerate() {
        let m = b.material(name);
        let origin = Vec3::new(2.0 * i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

#[test]
fn alt_q_isolates_the_selected_texture_set_and_toggles_back() {
    let mut s = Session::new(three_materials(), 64);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    assert_eq!(sets.len(), 3);
    let steps = s.history().entries().len();
    let hidden = |s: &Session| sets.iter().map(|&id| s.is_set_hidden(id)).collect::<Vec<_>>();

    s.execute(Command::SelectTextureSet(sets[1])).unwrap();
    assert!(s.perform(Action::IsolateTextureSet).unwrap());
    assert_eq!(hidden(&s), [true, false, true]);
    // Isolating another set switches to it, even from a partly hidden state.
    s.execute(Command::IsolateTextureSet(sets[2])).unwrap();
    assert_eq!(hidden(&s), [true, true, false]);
    // The same set again shows everything.
    s.execute(Command::IsolateTextureSet(sets[2])).unwrap();
    assert_eq!(hidden(&s), [false, false, false]);
    // A set hidden by hand is replaced by the isolation, not added to it.
    s.execute(Command::SetTextureSetHidden { texture_set: sets[0], hidden: true }).unwrap();
    s.execute(Command::IsolateTextureSet(sets[0])).unwrap();
    assert_eq!(hidden(&s), [false, true, true]);
    assert_eq!(s.history().entries().len(), steps, "view state only: no undo steps");
}

#[test]
fn x_inverts_the_mask_brush_value() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    assert_eq!(s.brush.material.mask_value, 1.0);
    assert!(s.perform(Action::InvertMaskGray).unwrap());
    assert_eq!(s.brush.material.mask_value, 0.0);
    s.brush.material.mask_value = 0.25;
    s.perform(Action::InvertMaskGray).unwrap();
    assert_eq!(s.brush.material.mask_value, 0.75);
    // UI-side actions are left to the front end.
    for a in [Action::HideUi, Action::View3dAnd2d, Action::SwapViews, Action::NextMeshMap, Action::PaintingMode, Action::CloseProject] {
        assert!(!s.perform(a).unwrap(), "{a:?}");
    }
}
