//! Polygon loop fill selection (Painter's Polygon loop fill / Partial polygon loop fill): the loop
//! follows the drag direction, partial loops stop under the release point, and the fill paints it.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::select::{loop_edge, loop_triangles};
use subcraft_paint::{BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, PolygonFillMode, Stroke, StrokeMode, StrokeOptions};

struct Scene {
    mesh: Arc<Mesh>,
    bvh: Bvh,
    geo: TextureSetGeometry,
    depth: DepthCache,
}

impl Scene {
    fn new(mesh: Mesh, res: u32) -> Self {
        let mesh = Arc::new(mesh);
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, res), depth: DepthCache::new(), mesh }
    }
    fn ctx<'a>(&'a self, camera: &'a Camera) -> PaintContext<'a> {
        PaintContext { mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera, depth: &self.depth }
    }
}

fn front_camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

/// Screen position of a point on the cube.
fn screen(cam: &Camera, p: Vec3) -> Vec2 {
    cam.project(p).unwrap().truncate()
}

/// Triangles whose centroid satisfies `f`.
fn where_centroid(mesh: &Mesh, f: impl Fn(Vec3) -> bool) -> Vec<u32> {
    (0..mesh.triangle_count() as u32)
        .filter(|&t| {
            let p = mesh.tri_positions(t as usize);
            f((p[0] + p[1] + p[2]) / 3.0)
        })
        .collect()
}

#[test]
fn a_horizontal_drag_picks_the_loop_round_the_cube() {
    // cube(2): 4 x 4 cells of 0.5 per face. A drag to the right on the front face, in the row
    // between y = 0 and 0.5, picks that row on the front, right, back and left faces.
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    let a = screen(&cam, Vec3::new(-0.25, 0.25, 1.0));
    let b = screen(&cam, Vec3::new(0.6, 0.25, 1.0));
    let ring = loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, a, b, false, false);
    let row = where_centroid(&sc.mesh, |c| c.y > 0.0 && c.y < 0.5 && (c.x.abs() > 0.99 || c.z.abs() > 0.99));
    assert_eq!(row.len(), 32, "16 quads round the cube");
    assert_eq!(ring, row);
    // A vertical drag picks the column instead: front, top, back and bottom.
    let up = screen(&cam, Vec3::new(-0.25, 0.9, 1.0));
    let column = loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, a, up, false, false);
    let expected = where_centroid(&sc.mesh, |c| c.x > -0.5 && c.x < 0.0 && (c.y.abs() > 0.99 || c.z.abs() > 0.99));
    assert_eq!(column, expected);
    // Missing the model picks nothing.
    assert!(loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, Vec2::new(2.0, 2.0), Vec2::new(40.0, 2.0), false, false).is_empty());
}

#[test]
fn a_partial_loop_ends_under_the_release_point_and_alt_mirrors_it() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    // Front row y in 0..0.5, cells centred at x = -0.75, -0.25, 0.25, 0.75. From -0.25 to 0.75.
    let a = screen(&cam, Vec3::new(-0.25, 0.25, 1.0));
    let b = screen(&cam, Vec3::new(0.75, 0.25, 1.0));
    let span = loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, a, b, true, false);
    let expected = where_centroid(&sc.mesh, |c| c.z > 0.99 && c.y > 0.0 && c.y < 0.5 && c.x > -0.5);
    assert_eq!(span, expected, "3 cells: start, middle, end");
    // Alt: 2 more cells on the other side of the start (the front cell at -0.75 and the first
    // cell of the left face).
    let both = loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, a, b, true, true);
    assert_eq!(both.len(), 10);
    assert!(expected.iter().all(|t| both.contains(t)));
    let left = where_centroid(&sc.mesh, |c| c.x < -0.99 && c.y > 0.0 && c.y < 0.5 && c.z > 0.5);
    assert!(left.iter().all(|t| both.contains(t)), "the loop goes on round the corner");
}

#[test]
fn a_click_crosses_the_nearest_edge() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    let topology = sc.geo.polygon_topology();
    // Near the right edge of a cell: the loop crosses that edge (a horizontal row).
    let at = screen(&cam, Vec3::new(-0.04, 0.25, 1.0));
    let (o, d) = cam.ray(at);
    let tri = sc.bvh.intersect(o, d, 0.0, f32::INFINITY).unwrap().tri;
    let p = topology.polygon_of[tri as usize];
    let e = loop_edge(topology, &cam, p, at, at).unwrap();
    let (a, b) = (topology.positions[e.0 as usize], topology.positions[e.1 as usize]);
    assert!(a.x.abs() < 1e-6 && b.x.abs() < 1e-6, "the vertical edge at x = 0: {a} {b}");
    let row = loop_triangles(&sc.mesh, &sc.bvh, &sc.geo, &cam, at, at, false, false);
    assert_eq!(row.len(), 32);
}

#[test]
fn the_loop_fill_paints_only_the_loop() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    let mut ts = TextureSet::new("T", 0, 128);
    let tc = ts.clone();
    let id = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let red = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    let mut s = Stroke::begin(&ts, id, PaintTarget::Content, StrokeMode::Paint, BrushSettings::default(), StrokeOptions::default(), &red).unwrap();
    let a = screen(&cam, Vec3::new(-0.25, 0.25, 1.0));
    let b = screen(&cam, Vec3::new(0.6, 0.25, 1.0));
    let dirty = s.polygon_fill_loop(&mut ts, &sc.ctx(&cam), a, b, PolygonFillMode::Triangle, false, false).unwrap();
    assert!(!dirty.is_empty());
    let LayerContent::Paint(p) = &ts.find(id).unwrap().content else { panic!() };
    let img = p.channels.get(&ChannelKind::BaseColor).unwrap();
    // The centre of a loop cell is painted, the centre of a cell off the loop is not.
    let uv_of = |t: u32| {
        let uv = sc.mesh.tri_uvs(t as usize);
        (uv[0] + uv[1] + uv[2]) / 3.0
    };
    let texel = |uv: Vec2| img.get((uv.x * 128.0) as u32, (uv.y * 128.0) as u32)[3];
    let on = where_centroid(&sc.mesh, |c| c.z > 0.99 && c.y > 0.0 && c.y < 0.5);
    let off = where_centroid(&sc.mesh, |c| c.z > 0.99 && c.y < -0.5);
    assert!(on.iter().all(|&t| texel(uv_of(t)) > 0.99), "loop cells painted");
    assert!(off.iter().all(|&t| texel(uv_of(t)) == 0.0), "other cells untouched");
}
