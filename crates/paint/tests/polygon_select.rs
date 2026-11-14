//! Polygon fill selection: box selection (screen and UV), visible-only, and the fill modes.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::select::{grow, triangle_overlaps_rect, triangles_in_screen_box, triangles_in_uv_box};
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

/// Triangles of the cube facing `normal`.
fn facing(mesh: &Mesh, normal: Vec3) -> Vec<u32> {
    (0..mesh.triangle_count() as u32).filter(|&t| mesh.geometric_normal(t as usize).dot(normal) > 0.99).collect()
}

fn red() -> BrushMaterial {
    BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))])
}

fn painted_texels(ts: &TextureSet, layer: subcraft_core::LayerId) -> usize {
    let LayerContent::Paint(p) = &ts.find(layer).unwrap().content else { panic!() };
    let Some(img) = p.channels.get(&ChannelKind::BaseColor) else { return 0 };
    let res = img.width();
    (0..res * res).filter(|&i| img.get(i % res, i / res)[3] > 0.0).count()
}

#[test]
fn rect_overlap_is_exact_for_separated_touching_and_enclosing_cases() {
    let tri = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)];
    assert!(triangle_overlaps_rect(tri, Vec2::new(1.0, 1.0), Vec2::new(1.5, 1.5)), "box inside the triangle");
    assert!(triangle_overlaps_rect(tri, Vec2::new(-10.0, -10.0), Vec2::new(10.0, 10.0)), "triangle inside the box");
    assert!(triangle_overlaps_rect(tri, Vec2::new(3.0, -1.0), Vec2::new(5.0, 0.5)), "box over a corner");
    // Beyond the hypotenuse, inside the bounding box of the triangle: separated by the edge axis.
    assert!(!triangle_overlaps_rect(tri, Vec2::new(3.0, 3.0), Vec2::new(3.9, 3.9)));
    assert!(!triangle_overlaps_rect(tri, Vec2::new(5.0, 0.0), Vec2::new(6.0, 1.0)));
}

#[test]
fn a_box_over_the_whole_cube_selects_only_the_front_face_when_visible_only() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(512.0, 512.0));
    let visible = triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, true);
    assert_eq!(visible, facing(&sc.mesh, Vec3::Z), "only the +Z face is seen from the front");
    let all = triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, false);
    assert!(all.len() > visible.len() * 4, "x-ray selection reaches the back faces too: {}", all.len());
    assert!(facing(&sc.mesh, -Vec3::Z).iter().all(|t| all.contains(t)));
}

#[test]
fn a_small_box_selects_the_cells_it_touches_and_nothing_behind_them() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    // A box inside one quad cell of the front face (4 x 4 cells over [-1, 1]).
    let c = cam.project(Vec3::new(-0.25, -0.25, 1.0)).unwrap().truncate();
    let (a, b) = (c - Vec2::splat(3.0), c + Vec2::splat(3.0));
    let picked = triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, true);
    assert!(!picked.is_empty() && picked.len() <= 2, "{picked:?}");
    assert!(picked.iter().all(|&t| sc.mesh.geometric_normal(t as usize).z > 0.99));
    let quad = grow(&sc.mesh, &sc.geo, &picked, PolygonFillMode::Polygon);
    assert_eq!(quad.len(), 2, "polygon mode grows to the whole quad cell");
    let xray = triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, false);
    assert!(xray.iter().any(|&t| sc.mesh.geometric_normal(t as usize).z < -0.99), "without visible-only the back cell is picked too");
}

#[test]
fn modes_grow_a_pick_to_polygon_mesh_island_and_object() {
    let sc = Scene::new(primitives::sample_crate(), 128);
    let mesh = &sc.mesh;
    let seed = [mesh.material_triangles(0).start as u32];
    let tri = grow(mesh, &sc.geo, &seed, PolygonFillMode::Triangle);
    let poly = grow(mesh, &sc.geo, &seed, PolygonFillMode::Polygon);
    let element = grow(mesh, &sc.geo, &seed, PolygonFillMode::MeshElement);
    let object = grow(mesh, &sc.geo, &seed, PolygonFillMode::Object);
    assert_eq!(tri, seed.to_vec());
    assert!(poly.contains(&seed[0]) && poly.iter().all(|t| element.contains(t)), "a polygon lies within its mesh element");
    assert!(element.iter().all(|t| object.contains(t)), "an element lies within its object");
    assert_eq!(object.len(), mesh.material_triangles(0).len(), "no object data: the whole texture set");
}

#[test]
fn uv_box_selects_triangles_by_their_uv_footprint() {
    let sc = Scene::new(primitives::cube(2.0), 128);
    let all = triangles_in_uv_box(&sc.mesh, 0, Vec2::ZERO, Vec2::ONE);
    assert_eq!(all.len(), sc.mesh.triangle_count());
    let none = triangles_in_uv_box(&sc.mesh, 0, Vec2::new(5.0, 5.0), Vec2::new(6.0, 6.0));
    assert!(none.is_empty());
    // A tiny box around one triangle's UV centroid picks that triangle (plus neighbours at most).
    let (q, _) = subcraft_mesh::uv_raster::wrap_to_tile(sc.mesh.tri_uvs(7));
    let c = (q[0] + q[1] + q[2]) / 3.0;
    let few = triangles_in_uv_box(&sc.mesh, 0, c - Vec2::splat(1e-4), c + Vec2::splat(1e-4));
    assert_eq!(few, vec![7]);
}

#[test]
fn box_fill_paints_exactly_the_front_face_and_matches_clicking_its_cells() {
    let res = 128;
    let sc = Scene::new(primitives::cube(2.0), res);
    let cam = front_camera();
    let mut ts = TextureSet::new("T", 0, res);
    let tc = ts.clone();
    let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let begin = |ts: &TextureSet| Stroke::begin(ts, layer, PaintTarget::Content, StrokeMode::Paint, BrushSettings::default(), StrokeOptions::default(), &red()).unwrap();

    let mut boxed = ts.clone();
    let mut s = begin(&boxed);
    let dirty = s.polygon_fill_box(&mut boxed, &sc.ctx(&cam), Vec2::ZERO, Vec2::splat(512.0), PolygonFillMode::Triangle, true).unwrap();
    assert!(!dirty.is_empty());

    // The same texels as filling every front triangle's island (the front face is one UV island).
    let mut clicked = ts.clone();
    let mut s = begin(&clicked);
    let center = cam.project(Vec3::new(0.1, 0.1, 1.0)).unwrap().truncate();
    s.polygon_fill(&mut clicked, &sc.ctx(&cam), center, PolygonFillMode::UvIsland).unwrap();
    let (nb, nc) = (painted_texels(&boxed, layer), painted_texels(&clicked, layer));
    assert!(nb > 1000);
    assert_eq!(nb, nc, "box over the front face = its UV island");

    // An empty box (off the model) fills nothing.
    let mut empty = ts.clone();
    let mut s = begin(&empty);
    assert!(s.polygon_fill_box(&mut empty, &sc.ctx(&cam), Vec2::ZERO, Vec2::splat(4.0), PolygonFillMode::Object, true).unwrap().is_empty());
    assert_eq!(painted_texels(&empty, layer), 0);
}

/// QA 2026-11: hidden parts stay in the mesh as triangles collapsed onto one vertex. Such a
/// triangle "overlapped" any box containing that vertex, so a box selection picked hidden parts.
#[test]
fn a_collapsed_triangle_overlaps_no_box() {
    let p = Vec2::new(1.0, 1.0);
    assert!(!triangle_overlaps_rect([p, p, p], Vec2::ZERO, Vec2::splat(2.0)), "a triangle collapsed onto a point");
    assert!(!triangle_overlaps_rect([p, p, Vec2::new(1.5, 1.5)], Vec2::ZERO, Vec2::splat(2.0)), "a triangle collapsed onto a line");
    assert!(triangle_overlaps_rect([p, Vec2::new(1.5, 1.0), Vec2::new(1.0, 1.5)], Vec2::ZERO, Vec2::splat(2.0)), "a real triangle still overlaps");
}

#[test]
fn contained_center_keeps_only_the_elements_centred_in_the_box() {
    use subcraft_paint::select::{keep_contained, screen_centre};
    let sc = Scene::new(primitives::cube(2.0), 128);
    let cam = front_camera();
    // Front face cells are 0.5 wide, centred at +-0.25 and +-0.75. A box from x -0.3 to 0.2 and
    // y -0.3 to 0.3 touches 4 cells but holds only the centres at x = -0.25.
    let a = cam.project(Vec3::new(-0.3, 0.3, 1.0)).unwrap().truncate();
    let b = cam.project(Vec3::new(0.2, -0.3, 1.0)).unwrap().truncate();
    let touched = grow(&sc.mesh, &sc.geo, &triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, true), PolygonFillMode::Polygon);
    assert_eq!(touched.len(), 8, "4 quads touched");
    let kept = keep_contained(&sc.mesh, &sc.geo, &touched, PolygonFillMode::Polygon, a, b, |m| screen_centre(&sc.mesh, &cam, m));
    assert_eq!(kept.len(), 4, "the 2 quads centred at x = -0.25");
    assert!(kept.iter().all(|&t| sc.mesh.tri_positions(t as usize).iter().all(|p| p.x <= 1e-6)));
    // Per triangle: a box around the vertex at the face centre holds no triangle centroid.
    let c = cam.project(Vec3::new(0.0, 0.0, 1.0)).unwrap().truncate();
    let (a, b) = (c - Vec2::splat(2.0), c + Vec2::splat(2.0));
    let around = triangles_in_screen_box(&sc.mesh, &sc.bvh, &cam, 0, a, b, true);
    assert!(!around.is_empty());
    assert!(keep_contained(&sc.mesh, &sc.geo, &around, PolygonFillMode::Triangle, a, b, |m| screen_centre(&sc.mesh, &cam, m)).is_empty());
    // A UV island centred in a UV box is kept whole.
    let island = grow(&sc.mesh, &sc.geo, &around[..1], PolygonFillMode::UvIsland);
    let centre = subcraft_paint::select::uv_centre(&sc.mesh, &island).unwrap();
    let kept = keep_contained(&sc.mesh, &sc.geo, &island, PolygonFillMode::UvIsland, centre - Vec2::splat(0.01), centre + Vec2::splat(0.01), |m| subcraft_paint::select::uv_centre(&sc.mesh, m));
    assert_eq!(kept, island);
}
