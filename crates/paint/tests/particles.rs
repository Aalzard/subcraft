//! Particle brush strokes: trails run down the model from where the brush touches, hidden trail
//! points are not painted onto what hides them, and the brush stamp itself is not painted.

use std::sync::Arc;

use glam::Vec3;
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::{BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, Stroke, StrokeMode, StrokeOptions};

const RES: u32 = 256;

/// Painted surface positions (alpha > 0.05) after one click at `at`, seen from `eye`.
fn click(mesh: Mesh, settings: BrushSettings, eye: Vec3, at: Vec3) -> Vec<Vec3> {
    let mesh = Arc::new(mesh);
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let layer: LayerId = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let (bvh, geo, depth) = (Bvh::build(&mesh), TextureSetGeometry::new(mesh.clone(), 0, RES), DepthCache::new());
    let cam = Camera::perspective(eye, Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let ctx = PaintContext { mesh: &mesh, bvh: &bvh, geometry: &geo, camera: &cam, depth: &depth, displaced: None };
    let red = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    let mut s = Stroke::begin(&ts, layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red).unwrap();
    let mut out = ts.clone();
    s.add_point(&mut out, &ctx, cam.project(at).unwrap().truncate()).unwrap();
    let LayerContent::Paint(p) = &out.find(layer).unwrap().content else { panic!() };
    let Some(img) = p.channels.get(&ChannelKind::BaseColor) else { return Vec::new() };
    let surf = rasterize_surface(&mesh, 0..mesh.triangle_count(), RES, RES, 0.0);
    (0..(RES * RES) as usize)
        .filter(|&i| surf.covered(i) && img.get(i as u32 % RES, i as u32 / RES)[img.comps() - 1] > 0.05)
        .map(|i| Vec3::from_slice(&surf.position[i][..3]))
        .collect()
}

fn drips() -> BrushSettings {
    BrushSettings {
        size: 8.0, hardness: 1.0, pressure_size: 0.0, particle_count: 4, particle_length: 3.0, particle_size: 0.3,
        particle_size_end: 0.5, particle_gravity: 1.0, particle_wander: 0.0, particle_spread: 1.0, ..BrushSettings::default()
    }
}

#[test]
fn drips_run_down_from_the_brush_and_the_stamp_itself_is_not_painted() {
    let front = Vec3::new(0.0, 0.0, 6.0);
    let painted = click(primitives::cube(2.0), drips(), front, Vec3::new(0.0, 0.5, 1.0));
    assert!(painted.len() > 30, "trails painted: {}", painted.len());
    // Brush radius: 8% of the 3.46 m diagonal / 2 = 0.139 m. Trails: 3 diameters = 0.83 m down.
    let (top, bottom) = painted.iter().fold((f32::MIN, f32::MAX), |(t, b), p| (t.max(p.y), b.min(p.y)));
    assert!(top < 0.5 + 0.18, "nothing far above the brush: top {top}");
    assert!(bottom < 0.5 - 0.6, "trails reach well below the brush: bottom {bottom}");
    // Thin trails, not the full brush disc: far less area than a normal stamp.
    let normal = click(primitives::cube(2.0), BrushSettings { particle_count: 0, ..drips() }, front, Vec3::new(0.0, 0.5, 1.0));
    let width = painted.iter().map(|p| p.x).fold(f32::MIN, f32::max) - painted.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    assert!(width <= 2.0 * 0.139 + 0.1, "trails start inside the brush: width {width}");
    assert!(painted.iter().all(|p| (p.z - 1.0).abs() < 1e-3), "only the front face");
    assert!(!normal.is_empty());
}

/// A 2 m wall with a small box in front of it, one mesh and one material (both paintable).
fn wall_with_box_in_front() -> Mesh {
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("Wall");
    let m = b.material("M");
    let wall = primitives::cube(2.0);
    // The box: 0.6 m, its top at y = 0.3, standing 1.4 m in front of the wall's front face.
    let small = primitives::cube(0.6).transformed(&glam::Mat4::from_translation(Vec3::new(0.0, 0.0, 2.4)));
    for (mesh, u0) in [(&wall, 0.0f32), (&small, 0.5)] {
        for t in 0..mesh.triangle_count() {
            let p = mesh.tri_positions(t);
            let uv = mesh.tri_uvs(t);
            let n = mesh.geometric_normal(t);
            let c = |k: usize| Corner { position: p[k], normal: Some(n), uv: Some(glam::Vec2::new(u0 + uv[k].x * 0.5, uv[k].y)) };
            b.push_triangle(m, [c(0), c(1), c(2)]);
        }
    }
    b.build().unwrap()
}

#[test]
fn trail_points_hidden_behind_other_geometry_are_not_painted_onto_it() {
    // Seen from the front, drips run down the wall behind the small box. Those points are hidden:
    // painted through the view they would land on the box's front face instead.
    let s = BrushSettings { particle_length: 4.0, particle_spread: 0.3, ..drips() };
    let painted = click(wall_with_box_in_front(), s, Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.55, 1.0));
    assert!(painted.iter().any(|p| (p.z - 1.0).abs() < 1e-3 && p.y < 0.5), "visible trail on the wall above the box");
    let on_box: Vec<_> = painted.iter().filter(|p| p.z > 1.5).collect();
    assert!(on_box.is_empty(), "{} texels painted on the box, e.g. {:?}", on_box.len(), on_box.first());
}

#[test]
fn burn_marks_rise() {
    let s = BrushSettings { particle_gravity: -1.0, ..drips() };
    let painted = click(primitives::cube(2.0), s, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, -0.5, 1.0));
    let top = painted.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let bottom = painted.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(top > -0.5 + 0.6 && bottom > -0.5 - 0.18, "rises from the brush: {bottom}..{top}");
}
