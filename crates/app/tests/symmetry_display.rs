//! Symmetry display data the viewport draws: guides at the moved mirror plane, flipped radial
//! copies, and the mirrored cursor points, all from the options the stroke paints with.

use glam::Vec3;
use subcraft_app::{Command, Session};
use subcraft_mesh::primitives;
use subcraft_paint::{RadialSymmetry, StrokeOptions};

fn session_with(options: StrokeOptions) -> Session {
    let mut s = Session::new(primitives::cube(2.0), 64);
    s.execute(Command::SetStrokeOptions(options)).unwrap();
    s
}

#[test]
fn guides_and_cursor_copies_follow_the_mirror_plane_position() {
    let s = session_with(StrokeOptions { symmetry: [true, false, false], mirror_offset: [0.5, 0.0, 0.0], ..StrokeOptions::default() });
    let g = s.symmetry_guides();
    assert_eq!(g.planes.len(), 1);
    assert!(g.planes[0].iter().all(|p| (p.x - 0.5).abs() < 1e-6));
    assert!(!g.intersections.is_empty() && g.intersections.iter().flatten().all(|p| (p.x - 0.5).abs() < 1e-6));
    assert!(g.axis.is_empty());
    // Mirrored across x = 0.5.
    let copies = s.symmetry_copies(Vec3::new(0.8, 0.2, 1.0));
    assert_eq!(copies.len(), 1);
    assert!(copies[0].distance(Vec3::new(0.2, 0.2, 1.0)) < 1e-6, "{copies:?}");
    // The chosen guide kinds: 4 outline edges per plane.
    assert_eq!(g.segments(false, true, false).len(), 4);
    assert_eq!(g.segments(true, true, true).len(), g.intersections.len() + 4);
    assert_eq!(s.symmetry_guide_segments().len(), g.intersections.len() + 4);

    // Moving the plane again refreshes the cached guides.
    let mut s = s;
    s.execute(Command::SetStrokeOptions(StrokeOptions { mirror_offset: [-0.25, 0.0, 0.0], ..s.brush.options })).unwrap();
    assert!(s.symmetry_guides().planes[0].iter().all(|p| (p.x + 0.25).abs() < 1e-6));
}

#[test]
fn radial_cursor_copies_alternate_when_flipped() {
    let radial = RadialSymmetry { enabled: true, axis: 1, count: 4, center_offset: [0.0; 3], flip: true };
    let s = session_with(StrokeOptions { radial, ..StrokeOptions::default() });
    let copies = s.symmetry_copies(Vec3::new(0.0, 0.3, 1.0));
    assert_eq!(copies.len(), 3);
    // Copy 2 is a plain half turn; copies 1 and 3 are mirrored, so they sit on the X axis.
    assert!(copies[1].distance(Vec3::new(0.0, 0.3, -1.0)) < 1e-5, "{copies:?}");
    assert!(copies[0].z.abs() < 1e-5 && copies[2].z.abs() < 1e-5);
    assert_eq!(s.symmetry_guides().axis.len(), 5, "axis line and four spokes");
    assert!(session_with(StrokeOptions::default()).symmetry_copies(Vec3::ONE).is_empty());
}
