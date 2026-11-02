//! 2D-view painting: strokes and polygon fills placed by texture UV instead of a camera ray.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::wrap_to_tile;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::stroke::{triangle_at_uv, uv_per_unit};
use subcraft_paint::{BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, PolygonFillMode, Stroke, StrokeMode, StrokeOptions};

struct Scene {
    mesh: Arc<Mesh>,
    bvh: Bvh,
    geo: TextureSetGeometry,
    depth: DepthCache,
    ts: TextureSet,
    layer: LayerId,
}

impl Scene {
    fn new(res: u32) -> Self {
        let mesh = Arc::new(primitives::cube(2.0));
        let mut ts = TextureSet::new("T", 0, res);
        let tc = ts.clone();
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, res), depth: DepthCache::new(), mesh, ts, layer }
    }

    fn ctx<'a>(&'a self, camera: &'a Camera) -> PaintContext<'a> {
        PaintContext { mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera, depth: &self.depth }
    }

    fn image(&self, kind: ChannelKind) -> Option<TiledImage> {
        let LayerContent::Paint(p) = &self.ts.find(self.layer).unwrap().content else { panic!() };
        p.channels.get(&kind).cloned()
    }

    fn begin(&self, settings: BrushSettings, material: &BrushMaterial) -> Stroke {
        Stroke::begin(&self.ts, self.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), material).unwrap()
    }

    /// UV centroid of the first triangle facing `normal` (a cube face, hidden from `camera()`).
    fn face_uv(&self, normal: Vec3) -> (u32, Vec2) {
        let t = (0..self.mesh.triangle_count()).find(|&t| self.mesh.geometric_normal(t).dot(normal) > 0.99).unwrap();
        let (q, _) = wrap_to_tile(self.mesh.tri_uvs(t));
        (t as u32, (q[0] + q[1] + q[2]) / 3.0)
    }
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

fn alpha(img: &TiledImage, x: u32, y: u32) -> f32 {
    let px = img.get(x, y);
    px[img.comps() - 1]
}

fn red() -> BrushMaterial {
    BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))])
}

#[test]
fn a_uv_dab_paints_a_disc_around_the_cursor_even_on_a_face_hidden_from_the_3d_camera() {
    let res = 256;
    let mut sc = Scene::new(res);
    let (_, uv) = sc.face_uv(-Vec3::Z); // back face: unreachable from the 3D camera
    let settings = BrushSettings { size: 10.0, hardness: 1.0, ..BrushSettings::default() };
    let radius_texels = settings.radius(sc.mesh.bounds.diagonal()) * uv_per_unit(&sc.mesh, 0) * res as f32;
    let cam = camera();
    let mut s = sc.begin(settings, &red());
    let mut ts = sc.ts.clone();
    let dirty = s.add_uv_point(&mut ts, &sc.ctx(&cam), uv, false, 1.0).unwrap();
    assert!(!dirty.is_empty());
    sc.ts = ts;
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let center = uv * res as f32;
    assert_eq!(alpha(&img, center.x as u32, center.y as u32), 1.0, "cursor texel fully painted");
    let (mut painted, mut outside) = (0, 0);
    for y in 0..res {
        for x in 0..res {
            if alpha(&img, x, y) > 0.0 {
                painted += 1;
                if Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(center) > radius_texels + 1.0 {
                    outside += 1;
                }
            }
        }
    }
    // A hard round tip inside one face: about πr² texels, none beyond the radius.
    let disc = std::f32::consts::PI * radius_texels * radius_texels;
    assert!(radius_texels > 4.0, "{radius_texels}");
    assert!((painted as f32 - disc).abs() < disc * 0.15, "painted {painted} vs disc {disc}");
    assert_eq!(outside, 0);
}

#[test]
fn uv_strokes_write_only_the_enabled_channels() {
    let mut sc = Scene::new(128);
    let mut material = BrushMaterial::new(&sc.ts);
    material.set(ChannelKind::Roughness, ChannelValue::scalar(0.1));
    material.solo(ChannelKind::Roughness, &sc.ts.channel_kinds().collect::<Vec<_>>());
    let (_, uv) = sc.face_uv(Vec3::Z);
    let cam = camera();
    let mut s = sc.begin(BrushSettings { size: 20.0, ..BrushSettings::default() }, &material);
    let mut ts = sc.ts.clone();
    s.add_uv_point(&mut ts, &sc.ctx(&cam), uv, false, 1.0).unwrap();
    sc.ts = ts;
    assert!(sc.image(ChannelKind::Roughness).unwrap().allocated_tiles() > 0);
    for k in [ChannelKind::BaseColor, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::Normal] {
        assert!(sc.image(k).is_none(), "{k:?} must stay unpainted");
    }
}

#[test]
fn uv_line_covers_the_segment_between_its_ends() {
    let res = 256;
    let mut sc = Scene::new(res);
    let (t, _) = sc.face_uv(Vec3::Z);
    let (q, _) = wrap_to_tile(sc.mesh.tri_uvs(t as usize));
    // Two points well inside the same triangle.
    let c = (q[0] + q[1] + q[2]) / 3.0;
    let (a, b) = (c.lerp(q[0], 0.5), c.lerp(q[1], 0.5));
    let cam = camera();
    let mut s = sc.begin(BrushSettings { size: 2.0, hardness: 1.0, ..BrushSettings::default() }, &red());
    let mut ts = sc.ts.clone();
    s.add_uv_point(&mut ts, &sc.ctx(&cam), a, true, 1.0).unwrap();
    s.add_uv_point(&mut ts, &sc.ctx(&cam), b, true, 1.0).unwrap();
    sc.ts = ts;
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    for k in 0..=10 {
        let p = a.lerp(b, k as f32 / 10.0) * res as f32;
        assert_eq!(alpha(&img, p.x as u32, p.y as u32), 1.0, "gap at step {k}");
    }
}

#[test]
fn uv_polygon_fill_matches_the_3d_fill_of_the_same_triangle() {
    let res = 128;
    let cam = camera();
    let screen = Vec2::new(290.0, 240.0); // off the face diagonal: one unambiguous triangle
    let (o, d) = cam.ray(screen);
    let a = Scene::new(res);
    let hit = a.bvh.intersect(o, d, 0.0, f32::INFINITY).unwrap();
    let sp = a.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
    let (q, off) = wrap_to_tile(a.mesh.tri_uvs(hit.tri as usize));
    let uv = sp.uv - Vec2::new(off[0] as f32, off[1] as f32);
    assert!(triangle_at_uv(&a.mesh, &a.geo, uv).is_some(), "uv {uv} in {q:?}");

    for mode in [PolygonFillMode::Triangle, PolygonFillMode::UvIsland] {
        let mut a = Scene::new(res);
        let mut s = a.begin(BrushSettings::default(), &red());
        let mut ts = a.ts.clone();
        s.polygon_fill(&mut ts, &a.ctx(&cam), screen, mode).unwrap();
        a.ts = ts;
        let mut b = Scene::new(res);
        let mut s = b.begin(BrushSettings::default(), &red());
        let mut ts = b.ts.clone();
        s.polygon_fill_uv(&mut ts, &b.ctx(&cam), uv, mode).unwrap();
        b.ts = ts;
        let (ia, ib) = (a.image(ChannelKind::BaseColor).unwrap(), b.image(ChannelKind::BaseColor).unwrap());
        assert!(ia.allocated_tiles() > 0);
        assert_eq!(ia.to_dense(), ib.to_dense(), "{mode:?}");
    }
}

#[test]
fn clicks_outside_every_island_pick_no_triangle() {
    let sc = Scene::new(128);
    assert_eq!(triangle_at_uv(&sc.mesh, &sc.geo, Vec2::new(-0.1, 0.5)), None);
    assert_eq!(triangle_at_uv(&sc.mesh, &sc.geo, Vec2::new(0.5, 1.0)), None);
    // Every triangle's UV centroid finds that triangle.
    for t in 0..sc.mesh.triangle_count() {
        let (q, _) = wrap_to_tile(sc.mesh.tri_uvs(t));
        assert_eq!(triangle_at_uv(&sc.mesh, &sc.geo, (q[0] + q[1] + q[2]) / 3.0), Some(t as u32));
    }
}
