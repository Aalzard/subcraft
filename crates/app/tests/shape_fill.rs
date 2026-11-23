//! Shape fill: a rectangle or ellipse dragged in the 3D or 2D view lands on the dragged box,
//! painted with the brush material in one undo step.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_paint::shape::{Shape, ShapeKind};
use subcraft_paint::{BrushMaterial, UvView};

const RES: u32 = 256;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.0, 0.0, 1.0))]);
    s
}
/// Looking straight down at the 2 × 2 plane (x right, z down the screen).
fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 {
    cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate()
}
fn raster(s: &Session) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels.get(&K::BaseColor).cloned().unwrap_or_else(|| TiledImage::new(RES, RES, 4, [0.0; 4]))
}
/// Alpha at plane position (x, z).
fn alpha(img: &TiledImage, x: f32, z: f32) -> f32 {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)[3]
}
/// Painted extent along the row at `z`: (leftmost, rightmost) x with alpha > 0.5.
fn row_extent(img: &TiledImage, z: f32) -> (f32, f32) {
    let y = ((z / 2.0 + 0.5) * RES as f32) as u32;
    let xs: Vec<u32> = (0..RES).filter(|&x| img.get(x, y)[3] > 0.5).collect();
    let to_x = |t: u32| (t as f32 + 0.5) / RES as f32 * 2.0 - 1.0;
    (to_x(xs[0]), to_x(*xs.last().unwrap()))
}
fn painted_fraction(img: &TiledImage) -> f32 {
    let mut sum = 0.0;
    for y in 0..RES {
        for x in 0..RES {
            sum += img.get(x, y)[3];
        }
    }
    sum / (RES * RES) as f32
}

#[test]
fn a_rectangle_covers_the_dragged_box_in_the_3d_view() {
    let mut s = session();
    let cam = camera();
    let brush_before = s.brush.settings.clone();
    s.fill_shape_projected(&cam, screen(&cam, -0.6, -0.2), screen(&cam, 0.4, 0.3), &Shape::default()).unwrap();
    let img = raster(&s);
    let (l, r) = row_extent(&img, 0.0);
    let texel = 2.0 / RES as f32;
    assert!((l + 0.6).abs() < 2.0 * texel && (r - 0.4).abs() < 2.0 * texel, "row spans {l}..{r}");
    // Corners are in (a rectangle), the outside is clear, the colour is the brush's.
    assert!(alpha(&img, -0.55, -0.15) > 0.99 && alpha(&img, 0.35, 0.25) > 0.99);
    assert_eq!(alpha(&img, 0.0, 0.5), 0.0);
    assert_eq!(alpha(&img, 0.6, 0.0), 0.0);
    let px = img.get(RES / 2, RES / 2);
    assert!(px[2] > 0.99 && px[0] < 0.01, "{px:?}");
    // About 1.0 × 0.5 of the 2 × 2 plane.
    assert!((painted_fraction(&img) - 0.125).abs() < 0.01, "{}", painted_fraction(&img));
    assert_eq!(s.brush.settings, brush_before, "the brush is put back");
}

#[test]
fn an_ellipse_leaves_the_corners_and_a_tall_box_turns_the_shape() {
    let mut s = session();
    let cam = camera();
    let ellipse = Shape { kind: ShapeKind::Ellipse, ..Shape::default() };
    // Tall: 0.4 wide, 1.2 high.
    s.fill_shape_projected(&cam, screen(&cam, -0.2, -0.6), screen(&cam, 0.2, 0.6), &ellipse).unwrap();
    let img = raster(&s);
    assert!(alpha(&img, 0.0, 0.0) > 0.99 && alpha(&img, 0.0, 0.55) > 0.5 && alpha(&img, 0.0, -0.55) > 0.5, "the long axis is vertical");
    assert_eq!(alpha(&img, 0.18, 0.55), 0.0, "corners are left out");
    let (l, r) = row_extent(&img, 0.0);
    assert!((l + 0.2).abs() < 0.03 && (r - 0.2).abs() < 0.03, "{l}..{r}");
    let expected = std::f32::consts::FRAC_PI_4 * 0.4 * 1.2 / 4.0;
    assert!((painted_fraction(&img) - expected).abs() < 0.006, "{} vs {expected}", painted_fraction(&img));
}

#[test]
fn a_feathered_shape_is_soft_inside_its_edge_and_one_undo_removes_it() {
    let mut s = session();
    let cam = camera();
    let before = raster(&s);
    s.fill_shape_projected(&cam, screen(&cam, -0.5, -0.5), screen(&cam, 0.5, 0.5), &Shape { feather: 0.6, ..Shape::default() }).unwrap();
    let img = raster(&s);
    assert!(alpha(&img, 0.0, 0.0) > 0.99);
    let near_edge = alpha(&img, 0.0, 0.45);
    assert!((0.02..0.6).contains(&near_edge), "soft near the edge: {near_edge}");
    assert_eq!(alpha(&img, 0.0, 0.55), 0.0, "nothing outside the box");
    s.execute(Command::Undo).unwrap();
    assert_eq!(raster(&s), before);
}

#[test]
fn a_shape_off_the_model_or_without_area_paints_nothing() {
    let mut s = session();
    let cam = camera();
    assert!(s.fill_shape_projected(&cam, screen(&cam, 1.5, 1.5), screen(&cam, 2.5, 2.5), &Shape::default()).is_err());
    assert!(s.fill_shape_projected(&cam, screen(&cam, 0.0, 0.0), screen(&cam, 0.0, 0.0), &Shape::default()).is_err());
    assert_eq!(painted_fraction(&raster(&s)), 0.0);
    assert!(!s.is_stroking());
}

#[test]
fn a_shape_in_the_2d_view_covers_the_dragged_box_of_texels() {
    let mut s = session();
    // The texture square drawn 400 px wide at (50, 50) in a 500 × 500 view.
    let view = UvView { origin: Vec2::splat(50.0), side: 400.0, viewport: [500, 500], rotation: 0.0 };
    let (a, b) = (view.to_view(Vec2::new(0.25, 0.25)), view.to_view(Vec2::new(0.75, 0.5)));
    s.fill_shape_uv(view, view.to_uv((a + b) * 0.5), b - a, &Shape::default()).unwrap();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    let img = &p.channels[&K::BaseColor];
    let r = RES as f32;
    assert!(img.get((0.3 * r) as u32, (0.3 * r) as u32)[3] > 0.99);
    assert!(img.get((0.7 * r) as u32, (0.45 * r) as u32)[3] > 0.99);
    assert_eq!(img.get((0.5 * r) as u32, (0.6 * r) as u32)[3], 0.0);
    assert_eq!(img.get((0.2 * r) as u32, (0.4 * r) as u32)[3], 0.0);
    assert!((painted_fraction(img) - 0.125).abs() < 0.01, "{}", painted_fraction(img));
}

#[test]
fn a_shape_on_a_face_nearer_than_the_models_centre_keeps_the_dragged_size() {
    // The cube's top face is 1 unit nearer the camera than its centre.
    let mut s = Session::new(primitives::cube(2.0), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.0, 0.0, 1.0))]);
    let cam = Camera::perspective(Vec3::new(0.0, 4.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    let on_top = |x: f32, z: f32| cam.project(Vec3::new(x, 1.0, z)).unwrap().truncate();
    s.fill_shape_projected(&cam, on_top(-0.5, -0.25), on_top(0.5, 0.25), &Shape::default()).unwrap();
    let img = raster(&s);
    let alpha_at = |x: f32, z: f32| {
        let uv = s.pick(&cam, on_top(x, z)).unwrap().uv;
        img.get((uv.x * RES as f32) as u32, (uv.y * RES as f32) as u32)[3]
    };
    for (x, z) in [(0.45, 0.0), (-0.45, 0.0), (0.0, 0.22), (0.0, -0.22), (0.45, 0.22)] {
        assert!(alpha_at(x, z) > 0.99, "inside the box at ({x}, {z})");
    }
    for (x, z) in [(0.56, 0.0), (-0.56, 0.0), (0.0, 0.31), (0.0, -0.31)] {
        assert_eq!(alpha_at(x, z), 0.0, "outside the box at ({x}, {z})");
    }
}
