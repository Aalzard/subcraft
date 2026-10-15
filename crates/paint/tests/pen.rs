use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{PenAnchor, PenPath};
use subcraft_paint::pen::PenRasterizer;

fn path() -> PenPath {
    PenPath { anchors: vec![PenAnchor::new(Vec2::new(0.2, 0.5)), PenAnchor::new(Vec2::new(0.8, 0.5))], closed: false, radius: 0.05, opacity: 0.5, hardness: 1.0, values: [(ChannelKind::BaseColor, ChannelValue::rgb(0.25, 0.5, 1.0)), (ChannelKind::Height, ChannelValue::scalar(-0.2))].into() }
}

fn render(path: &PenPath, budget: usize) -> std::collections::BTreeMap<ChannelKind, subcraft_image::TiledImage> {
    let mut raster = PenRasterizer::new(path, 128).unwrap();
    for _ in 0..100000 { if raster.is_done() { break; } raster.step(budget); }
    raster.finish().expect("bounded job must finish")
}

#[test]
fn round_caps_and_linear_channel_values_have_numeric_coverage() {
    let images = render(&path(), 16384); let color = &images[&ChannelKind::BaseColor];
    assert_eq!(color.get(64, 64), [0.25, 0.5, 1.0, 0.5]);
    assert_eq!(color.get(21, 64)[3], 0.5, "round start cap"); assert_eq!(color.get(15, 64)[3], 0.0);
    assert_eq!(color.get(64, 80)[3], 0.0, "outside width");
    let height = images[&ChannelKind::Height].get(64, 64);
    assert!((height[0] + 0.2).abs() < 0.001); assert_eq!(height[1], 0.5);
}

#[test]
fn segment_overlap_does_not_accumulate_opacity_and_budgets_match() {
    let mut path = path(); path.closed = true;
    let fine = render(&path, 7); let full = render(&path, usize::MAX);
    assert_eq!(fine, full); assert_eq!(full[&ChannelKind::BaseColor].get(64, 64)[3], 0.5);
}

#[test]
fn moving_bezier_handles_rerenders_the_curve_and_leaves_old_texels_clear() {
    let mut path = path(); path.anchors[0].outgoing.y = -0.3; path.anchors[1].incoming.y = -0.3;
    let images = render(&path, 1000); let color = &images[&ChannelKind::BaseColor];
    assert_eq!(color.get(64, 35)[3], 0.5); assert_eq!(color.get(64, 64)[3], 0.0);
}

#[test]
fn off_texture_paths_and_empty_paths_allocate_no_tiles() {
    let mut path = path(); for a in &mut path.anchors { a.position += Vec2::splat(2.0); }
    let images = render(&path, 1); assert!(images.values().all(|i| (0..i.tile_count()).all(|t| i.tile(t).is_none())));
    path.anchors.clear(); assert_eq!(render(&path, 1), images);
}

#[test]
fn subpixel_strokes_have_antialiased_coverage() {
    let mut path = path(); path.radius = 0.001;
    let images = render(&path, 1000); let color = &images[&ChannelKind::BaseColor];
    let a = color.get(64, 63)[3]; let b = color.get(64, 64)[3];
    assert!(a > 0.0 && a < 0.1); assert!((a + b - 0.128).abs() < 0.001);
}
