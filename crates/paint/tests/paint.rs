//! Paint-engine acceptance tests (seams, visibility, channel isolation, opacity cap, undo,
//! symmetry, polygon fill).

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Document, History, Layer, LayerContent, Mask, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::{SurfaceTexels, rasterize_surface};
use subcraft_mesh::{Bvh, Mesh, primitives, topology};
use subcraft_paint::{
    Alignment, BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, PolygonFillMode, Stroke, StrokeMode,
    StrokeOptions,
};

struct Scene {
    mesh: Arc<Mesh>,
    bvh: Bvh,
    geo: TextureSetGeometry,
    depth: DepthCache,
    ts: TextureSet,
    layer: LayerId,
}

impl Scene {
    fn new(mesh: Mesh, res: u32) -> Self {
        let mesh = Arc::new(mesh);
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

    fn surface(&self) -> SurfaceTexels {
        rasterize_surface(&self.mesh, 0..self.mesh.triangle_count(), self.ts.resolution, self.ts.resolution, 0.0)
    }

    /// Drag a stroke through object-space points (projected to the screen).
    fn stroke(&mut self, camera: &Camera, points: &[Vec3], settings: BrushSettings, options: StrokeOptions, material: &BrushMaterial) -> Stroke {
        let mut s = Stroke::begin(&self.ts, self.layer, PaintTarget::Content, StrokeMode::Paint, settings, options, material).unwrap();
        let screen: Vec<Vec2> = points.iter().map(|&p| camera.project(p).unwrap().truncate()).collect();
        let mut ts = self.ts.clone();
        for w in screen.windows(2) {
            for k in 0..=20 {
                let p = w[0].lerp(w[1], k as f32 / 20.0);
                s.add_point(&mut ts, &self.ctx(camera), p).unwrap();
            }
        }
        if screen.len() == 1 {
            s.add_point(&mut ts, &self.ctx(camera), screen[0]).unwrap();
        }
        self.ts = ts;
        s
    }
}

fn alpha(img: &TiledImage, i: usize, res: u32) -> f32 {
    let px = img.get(i as u32 % res, i as u32 / res);
    px[img.comps() - 1]
}

fn red() -> BrushMaterial {
    BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))])
}

fn front_camera(dist: f32) -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, dist), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

#[test]
fn stroke_across_a_cube_seam_is_continuous_in_3d() {
    let res = 256;
    let mut sc = Scene::new(primitives::cube(2.0), res);
    let cam = Camera::perspective(Vec3::new(4.0, 0.8, 4.0), Vec3::new(0.0, 0.0, 0.0), Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let settings = BrushSettings { size: 15.0, hardness: 0.3, ..BrushSettings::default() };
    sc.stroke(&cam, &[Vec3::new(0.2, 0.2, 1.0), Vec3::new(1.0, 0.2, 1.0), Vec3::new(1.0, 0.2, 0.2)], settings, StrokeOptions::default(), &red());
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let surf = sc.surface();

    // Texels within two texels of the shared edge (x = 1, z = 1) on each island.
    let texel = 2.0 / ((1.0 / 3.0 - 0.04) * res as f32);
    let mut a_side = Vec::new();
    let mut b_side = Vec::new();
    for i in 0..surf.triangle.len() {
        if !surf.covered(i) {
            continue;
        }
        let p = Vec3::from_slice(&surf.position[i][..3]);
        let n = sc.mesh.geometric_normal(surf.triangle[i] as usize);
        if n.z > 0.9 && p.x > 1.0 - 2.0 * texel {
            a_side.push((p, alpha(&img, i, res)));
        } else if n.x > 0.9 && p.z > 1.0 - 2.0 * texel {
            b_side.push((p, alpha(&img, i, res)));
        }
    }
    let painted: Vec<_> = a_side.iter().filter(|(_, a)| *a > 0.05).collect();
    assert!(painted.len() > 10, "stroke must reach the seam on the +Z island");
    assert!(b_side.iter().filter(|(_, a)| *a > 0.9).count() > 5, "and continue on the +X island");
    let mut worst: f32 = 0.0;
    for (p, a) in &a_side {
        let (_, b) = b_side.iter().min_by(|x, y| (x.0.y - p.y).abs().total_cmp(&(y.0.y - p.y).abs())).unwrap();
        worst = worst.max((a - b).abs());
    }
    assert!(worst < 0.15, "coverage jumps by {worst} across the seam");
}

/// Front-face (+Z) texels of the cube within `band` of its right edge (x = 1): their alphas.
fn front_edge_alphas(sc: &Scene, img: &TiledImage, band: f32) -> Vec<f32> {
    let surf = sc.surface();
    (0..surf.triangle.len())
        .filter(|&i| surf.covered(i))
        .filter(|&i| sc.mesh.geometric_normal(surf.triangle[i] as usize).z > 0.9 && surf.position[i][0] > 1.0 - band)
        .map(|i| alpha(img, i, sc.ts.resolution))
        .collect()
}

#[test]
fn a_dab_whose_center_misses_still_paints_where_its_edge_overlaps() {
    let settings = BrushSettings { size: 30.0, hardness: 1.0, ..BrushSettings::default() };
    let cam = front_camera(6.0);

    // Center just past the right edge of the front face: the brush overlaps the face.
    let mut sc = Scene::new(primitives::cube(2.0), 128);
    sc.stroke(&cam, &[Vec3::new(1.08, 0.0, 1.0)], settings, StrokeOptions::default(), &red());
    let img = sc.image(ChannelKind::BaseColor).expect("the overlapping dab painted");
    let painted = front_edge_alphas(&sc, &img, 0.1).iter().filter(|&&a| a > 0.5).count();
    assert!(painted > 10, "texels along the edge are painted ({painted})");

    // Far from the cube nothing is touched.
    let mut far = Scene::new(primitives::cube(2.0), 128);
    far.stroke(&cam, &[Vec3::new(2.5, 0.0, 1.0)], settings, StrokeOptions::default(), &red());
    let untouched = far.image(ChannelKind::BaseColor).is_none_or(|img| front_edge_alphas(&far, &img, 0.1).iter().all(|&a| a == 0.0));
    assert!(untouched, "a dab that doesn't reach the cube paints nothing");
}

fn slab_back_untouched(settings: BrushSettings) {
    let res = 256;
    let mut sc = Scene::new(primitives::thin_slab(1.0, 1.0, 0.01), res);
    let cam = front_camera(3.0);
    let z = 0.005;
    sc.stroke(&cam, &[Vec3::new(-0.2, 0.0, z), Vec3::new(0.2, 0.05, z)], settings, StrokeOptions::default(), &red());
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let surf = rasterize_surface(&sc.mesh, 0..sc.mesh.triangle_count(), res, res, 1.0);
    let (mut front, mut back, mut back_texels) = (0, 0, 0);
    for i in 0..surf.triangle.len() {
        if !surf.covered(i) {
            continue;
        }
        let n = sc.mesh.geometric_normal(surf.triangle[i] as usize);
        let a = alpha(&img, i, res);
        if n.z > 0.9 && a > 0.5 {
            front += 1;
        }
        if n.z < -0.9 {
            back_texels += 1;
            if a != 0.0 {
                back += 1;
            }
        }
    }
    assert!(front > 500, "front painted ({front} texels)");
    assert!(back_texels > 1000);
    assert_eq!(back, 0, "{back} back-island texels painted through a {:?} slab", settings.alignment);
}

#[test]
fn painting_the_front_of_a_thin_slab_leaves_the_back_island_untouched() {
    let base = BrushSettings { size: 20.0, ..BrushSettings::default() };
    slab_back_untouched(base);
    // The depth map alone must also stop paint-through.
    slab_back_untouched(BrushSettings { backface_culling: false, ..base });
    slab_back_untouched(BrushSettings { alignment: Alignment::Camera, backface_culling: false, ..base });
    slab_back_untouched(BrushSettings { alignment: Alignment::TangentPlanar, ..base });
}

#[test]
fn disabled_channels_are_byte_identical_after_a_stroke() {
    let mut sc = Scene::new(primitives::cube(2.0), 128);
    // Pre-existing roughness paint.
    let res = sc.ts.resolution;
    let LayerContent::Paint(p) = &mut sc.ts.find_mut(sc.layer).unwrap().content else { panic!() };
    p.channel_mut(ChannelKind::Roughness, res).set(30, 30, [0.7, 1.0, 0.0, 0.0]);
    let rough_before = sc.image(ChannelKind::Roughness).unwrap();
    let rough_dense = rough_before.to_dense();

    let mut mat = BrushMaterial::new(&sc.ts);
    mat.set(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 1.0, 0.0));
    mat.disable(ChannelKind::Roughness);
    mat.disable(ChannelKind::Metallic);
    mat.disable(ChannelKind::Height);
    mat.disable(ChannelKind::Normal);
    let cam = front_camera(5.0);
    sc.stroke(&cam, &[Vec3::new(-0.5, 0.0, 1.0), Vec3::new(0.5, 0.0, 1.0)], BrushSettings { size: 30.0, ..BrushSettings::default() }, StrokeOptions::default(), &mat);

    let rough_after = sc.image(ChannelKind::Roughness).unwrap();
    assert!(rough_after.same_storage(&rough_before), "disabled channel untouched (shared storage)");
    assert_eq!(rough_after.to_dense(), rough_dense);
    assert!(sc.image(ChannelKind::Metallic).is_none(), "disabled channel not created");
    assert!(sc.image(ChannelKind::BaseColor).unwrap().allocated_tiles() > 0);
}

#[test]
fn same_radius_gives_identical_footprints() {
    let run = || {
        let mut sc = Scene::new(primitives::plane(2.0, 4), 256);
        let cam = Camera::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, -Vec3::Z, 1.2, 0.1, 20.0, [400, 400]);
        let settings = BrushSettings { size: 10.0, hardness: 0.4, ..BrushSettings::default() };
        sc.stroke(&cam, &[Vec3::new(-0.4, 0.0, 0.3)], settings, StrokeOptions::default(), &red());
        sc.stroke(&cam, &[Vec3::new(0.35, 0.0, -0.45)], settings, StrokeOptions::default(), &red());
        sc.image(ChannelKind::BaseColor).unwrap()
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b, "deterministic");
    // The two dabs (same radius, flat plane, orthographic view) cover the same area.
    let res = 256;
    let count = |left: bool| {
        (0..(res * res) as usize).filter(|&i| ((i as u32 % res) < res / 2) == left && alpha(&a, i, res) > 0.5).count() as f32
    };
    let (l, r) = (count(true), count(false));
    assert!(l > 100.0 && (l - r).abs() / l < 0.03, "footprints differ: {l} vs {r}");
}

#[test]
fn overlapping_stamps_never_exceed_stroke_opacity() {
    let mut sc = Scene::new(primitives::plane(2.0, 4), 256);
    let cam = Camera::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, -Vec3::Z, 1.2, 0.1, 20.0, [400, 400]);
    let settings = BrushSettings { size: 12.0, flow: 0.8, stroke_opacity: 0.4, spacing: 0.02, ..BrushSettings::default() };
    let zigzag = [Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.1), Vec3::new(-0.5, 0.0, 0.05), Vec3::new(0.5, 0.0, -0.05)];
    let stroke = sc.stroke(&cam, &zigzag, settings, StrokeOptions::default(), &red());
    assert!(stroke.stamp_count() > 100);
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let max = (0..256 * 256).map(|i| alpha(&img, i, 256)).fold(0.0f32, f32::max);
    assert!(max <= 0.4 + 1e-3, "alpha {max} exceeds stroke opacity");
    assert!(max > 0.39, "opacity reached ({max})");
}

#[test]
fn base_color_and_roughness_in_one_stroke() {
    let mut sc = Scene::new(primitives::plane(2.0, 4), 128);
    let cam = Camera::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, -Vec3::Z, 1.2, 0.1, 20.0, [400, 400]);
    let mat = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(0.1, 0.2, 0.9)), (ChannelKind::Roughness, ChannelValue::scalar(0.85))]);
    sc.stroke(&cam, &[Vec3::new(0.0, 0.0, 0.0)], BrushSettings { size: 20.0, ..BrushSettings::default() }, StrokeOptions::default(), &mat);
    // Plane UV: u = x/2 + 0.5, v = z/2 + 0.5 → the center texel.
    let c = sc.image(ChannelKind::BaseColor).unwrap().get(64, 64);
    let r = sc.image(ChannelKind::Roughness).unwrap().get(64, 64);
    assert!((c[0] - 0.1).abs() < 2e-3 && (c[1] - 0.2).abs() < 2e-3 && (c[2] - 0.9).abs() < 2e-3 && c[3] == 1.0, "{c:?}");
    assert!((r[0] - 0.85).abs() < 2e-3 && r[1] == 1.0, "{r:?}");
    assert!(sc.image(ChannelKind::Metallic).is_none());
}

#[test]
fn history_undo_and_redo_restore_documents_exactly() {
    let mut sc = Scene::new(primitives::cube(2.0), 128);
    let res = sc.ts.resolution;
    // Existing paint the stroke will overlap (exercises copy-on-write of shared tiles).
    let LayerContent::Paint(p) = &mut sc.ts.find_mut(sc.layer).unwrap().content else { panic!() };
    let img = p.channel_mut(ChannelKind::BaseColor, res);
    for y in 0..res {
        for x in 0..res {
            img.set(x, y, [0.0, 0.0, 1.0, 0.5]);
        }
    }
    let before_dense = sc.image(ChannelKind::BaseColor).unwrap().to_dense();
    let mut history = History::new(Document { texture_sets: vec![sc.ts.clone()] }, "Open");

    let cam = front_camera(5.0);
    sc.stroke(&cam, &[Vec3::new(-0.6, -0.2, 1.0), Vec3::new(0.6, 0.3, 1.0)], BrushSettings { size: 20.0, ..BrushSettings::default() }, StrokeOptions::default(), &red());
    let post = Document { texture_sets: vec![sc.ts.clone()] };
    history.push("Stroke", post.clone());
    assert_ne!(history.entries()[0].doc, post, "stroke changed the document");

    let undone = history.undo().unwrap().clone();
    let LayerContent::Paint(p) = &undone.texture_sets[0].find(sc.layer).unwrap().content else { panic!() };
    assert_eq!(p.channels[&ChannelKind::BaseColor].to_dense(), before_dense, "pre-stroke texels untouched by the stroke");
    assert_eq!(&undone, &history.entries()[0].doc);
    let redone = history.redo().unwrap();
    assert_eq!(redone, &post);
    let LayerContent::Paint(p) = &redone.texture_sets[0].find(sc.layer).unwrap().content else { panic!() };
    assert_ne!(p.channels[&ChannelKind::BaseColor].to_dense(), before_dense);
}

#[test]
fn symmetry_produces_mirrored_coverage() {
    let res = 256;
    let mut sc = Scene::new(primitives::cube(2.0), res);
    let cam = Camera::perspective(Vec3::new(0.0, 0.5, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let options = StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() };
    sc.stroke(&cam, &[Vec3::new(0.3, 0.2, 1.0), Vec3::new(0.65, 0.35, 1.0)], BrushSettings { size: 10.0, ..BrushSettings::default() }, options, &red());
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let surf = sc.surface();
    let (mut wl, mut wr, mut cl, mut cr) = (0.0f32, 0.0f32, Vec3::ZERO, Vec3::ZERO);
    for i in 0..surf.triangle.len() {
        if !surf.covered(i) {
            continue;
        }
        let a = alpha(&img, i, res);
        let p = Vec3::from_slice(&surf.position[i][..3]);
        if p.x > 0.0 {
            wr += a;
            cr += p * a;
        } else {
            wl += a;
            cl += p * a;
        }
    }
    assert!(wr > 50.0, "painted ({wr})");
    assert!((wl - wr).abs() / wr < 0.05, "mirrored amount differs: {wl} vs {wr}");
    let (cl, cr) = (cl / wl, cr / wr);
    let mirrored = Vec3::new(-cr.x, cr.y, cr.z);
    assert!(cl.distance(mirrored) < 0.02, "centroids {cl} vs mirror of {cr}");
}

#[test]
fn polygon_fill_of_one_uv_island_touches_no_other_island() {
    let res = 256;
    let mut sc = Scene::new(primitives::cube(2.0), res);
    let cam = front_camera(5.0);
    let mut s = Stroke::begin(&sc.ts, sc.layer, PaintTarget::Content, StrokeMode::Paint, BrushSettings::default(), StrokeOptions::default(), &red()).unwrap();
    let mut ts = sc.ts.clone();
    let center = cam.project(Vec3::new(0.1, 0.1, 1.0)).unwrap().truncate();
    let dirty = s.polygon_fill(&mut ts, &sc.ctx(&cam), center, PolygonFillMode::UvIsland).unwrap();
    assert!(!dirty.is_empty());
    sc.ts = ts;
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let islands = topology::uv_islands(&sc.mesh);
    let hit = sc.bvh.intersect(cam.ray(center).0, cam.ray(center).1, 0.0, f32::INFINITY).unwrap().tri as usize;
    let surf = sc.surface();
    let (mut inside, mut outside_painted) = (0, 0);
    for i in 0..surf.triangle.len() {
        if !surf.covered(i) {
            continue;
        }
        let a = alpha(&img, i, res);
        if islands[surf.triangle[i] as usize] == islands[hit] {
            assert_eq!(a, 1.0, "island texel {i} not filled");
            inside += 1;
        } else if a != 0.0 {
            outside_painted += 1;
        }
    }
    assert!(inside > 4000);
    assert_eq!(outside_painted, 0);

    // Triangle mode fills strictly less than the island.
    let mut sc2 = Scene::new(primitives::cube(2.0), res);
    let mut s = Stroke::begin(&sc2.ts, sc2.layer, PaintTarget::Content, StrokeMode::Paint, BrushSettings::default(), StrokeOptions::default(), &red()).unwrap();
    let mut ts = sc2.ts.clone();
    s.polygon_fill(&mut ts, &sc2.ctx(&cam), center, PolygonFillMode::Triangle).unwrap();
    sc2.ts = ts;
    let tri_img = sc2.image(ChannelKind::BaseColor).unwrap();
    let n_tri = (0..(res * res) as usize).filter(|&i| alpha(&tri_img, i, res) > 0.0).count();
    assert!(n_tri > 50 && n_tri < inside / 8, "{n_tri}");
}

#[test]
fn mask_painting_and_eraser() {
    let mut sc = Scene::new(primitives::plane(2.0, 4), 128);
    let cam = Camera::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, -Vec3::Z, 1.2, 0.1, 20.0, [400, 400]);
    sc.ts.find_mut(sc.layer).unwrap().mask = Some(Mask::new(128, false));
    let settings = BrushSettings { size: 20.0, ..BrushSettings::default() };
    let center = cam.project(Vec3::ZERO).unwrap().truncate();

    let mut ts = sc.ts.clone();
    let mut s = Stroke::begin(&ts, sc.layer, PaintTarget::MaskBase, StrokeMode::Paint, settings, StrokeOptions::default(), &BrushMaterial::new(&ts)).unwrap();
    s.add_point(&mut ts, &sc.ctx(&cam), center).unwrap();
    let mask = &ts.find(sc.layer).unwrap().mask.as_ref().unwrap().base;
    assert_eq!(mask.get(64, 64)[0], 1.0, "white painted into the mask");
    assert_eq!(mask.get(2, 2)[0], 0.0);
    assert!(sc.image(ChannelKind::BaseColor).is_none(), "mask strokes leave content alone");

    // Paint, then erase half of it back.
    let mut s = Stroke::begin(&ts, sc.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red()).unwrap();
    s.add_point(&mut ts, &sc.ctx(&cam), center).unwrap();
    let erase = BrushSettings { stroke_opacity: 0.5, ..settings };
    let mut e = Stroke::begin(&ts, sc.layer, PaintTarget::Content, StrokeMode::Erase, erase, StrokeOptions::default(), &red()).unwrap();
    e.line_to(&mut ts, &sc.ctx(&cam), center).unwrap();
    e.line_to(&mut ts, &sc.ctx(&cam), center + Vec2::new(5.0, 0.0)).unwrap();
    let LayerContent::Paint(p) = &ts.find(sc.layer).unwrap().content else { panic!() };
    let px = p.channels[&ChannelKind::BaseColor].get(64, 64);
    assert!((px[3] - 0.5).abs() < 1e-3 && px[0] == 1.0, "eraser halves alpha, keeps color: {px:?}");
}

#[test]
fn errors_for_unpaintable_targets() {
    let sc = Scene::new(primitives::plane(2.0, 1), 128);
    let mut ts = sc.ts.clone();
    let tc = ts.clone();
    let fill = ts.insert_above(None, Layer::new_fill("F", &tc));
    let mat = red();
    let begin = |id, target| Stroke::begin(&ts, id, target, StrokeMode::Paint, BrushSettings::default(), StrokeOptions::default(), &mat).err();
    assert_eq!(begin(fill, PaintTarget::Content), Some(subcraft_paint::PaintError::NotAPaintLayer));
    assert_eq!(begin(sc.layer, PaintTarget::MaskBase), Some(subcraft_paint::PaintError::NoMask));
    assert_eq!(begin(LayerId::new(), PaintTarget::Content), Some(subcraft_paint::PaintError::LayerNotFound));
}

#[test]
fn brush_alpha_shapes_the_footprint() {
    // Same stamp with the round tip (hardness 1) and a solid square alpha: the square covers
    // ~4/π more texels; a left-half alpha paints only on one side of the center.
    let count = |alpha: Option<subcraft_image::Bitmap>| {
        let mut sc = Scene::new(primitives::plane(2.0, 4), 256);
        let cam = Camera::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, -Vec3::Z, 1.2, 0.1, 20.0, [400, 400]);
        let settings = BrushSettings { size: 20.0, hardness: 1.0, ..BrushSettings::default() };
        let mut s = Stroke::begin(&sc.ts, sc.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red())
            .unwrap()
            .with_alpha(alpha.map(Arc::new));
        let mut ts = sc.ts.clone();
        s.add_point(&mut ts, &sc.ctx(&cam), cam.project(Vec3::ZERO).unwrap().truncate()).unwrap();
        sc.ts = ts;
        let img = sc.image(ChannelKind::BaseColor).unwrap();
        let painted: Vec<u32> = (0..256 * 256u32).filter(|&i| alpha_at(&img, i) > 0.5).collect();
        let left = painted.iter().filter(|&&i| i % 256 < 128).count();
        (painted.len() as f32, left as f32)
    };
    let (round, _) = count(None);
    let (square, _) = count(Some(subcraft_image::Bitmap::new_f32(4, 4, 1, vec![1.0; 16])));
    let ratio = square / round;
    assert!((ratio - 4.0 / std::f32::consts::PI).abs() < 0.08, "square/round coverage {ratio}");
    let half: Vec<f32> = (0..64 * 64).map(|i| if i % 64 < 32 { 1.0 } else { 0.0 }).collect();
    let (total, left) = count(Some(subcraft_image::Bitmap::new_f32(64, 64, 1, half)));
    assert!(left / total > 0.95 || left / total < 0.05, "half alpha paints one side only ({left}/{total})");
}

fn alpha_at(img: &TiledImage, i: u32) -> f32 {
    img.get(i % 256, i / 256)[img.comps() - 1]
}

#[test]
fn stencil_masks_paint_in_screen_space() {
    use subcraft_image::Bitmap;
    use subcraft_paint::Stencil;

    let res = 256;
    let mut sc = Scene::new(primitives::cube(2.0), res);
    let cam = front_camera(5.0);
    // 2:1 image covering the 512² view: left half of the screen white, right half black. Wide
    // enough that bilinear filtering blurs the edge by only ~4 px.
    let row: Vec<u8> = (0..256).map(|x| if x < 128 { 255 } else { 0 }).collect();
    let img = Bitmap::new_u8(256, 128, 1, row.repeat(128));
    let stencil = Arc::new(Stencil { scale: 1.0, ..Stencil::new(Arc::new(img)) });
    let settings = BrushSettings { size: 20.0, hardness: 0.8, ..BrushSettings::default() };
    let mut s = Stroke::begin(&sc.ts, sc.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red())
        .unwrap()
        .with_stencil(Some(stencil));
    let mut ts = sc.ts.clone();
    let (a, b) = (cam.project(Vec3::new(-0.8, 0.0, 1.0)).unwrap().truncate(), cam.project(Vec3::new(0.8, 0.0, 1.0)).unwrap().truncate());
    for k in 0..=40 {
        s.add_point(&mut ts, &sc.ctx(&cam), a.lerp(b, k as f32 / 40.0)).unwrap();
    }
    sc.ts = ts;
    let img = sc.image(ChannelKind::BaseColor).unwrap();
    let surf = sc.surface();
    let (mut left, mut right) = (0, 0);
    for i in 0..(res * res) as usize {
        if !surf.covered(i) || alpha(&img, i, res) < 0.01 {
            continue;
        }
        let p = surf.position[i];
        let x = cam.project(Vec3::new(p[0], p[1], p[2])).unwrap().x;
        if x < 250.0 {
            left += 1;
        } else if x > 262.0 {
            right += 1;
        }
    }
    assert!(left > 200, "paint goes through the white half ({left} texels)");
    assert_eq!(right, 0, "the black half blocks paint");
}


/// Painting near a hard edge wraps onto the side face as the same coherent tip, not a smear:
/// with a tip striped symmetrically about its center, a side texel at depth d below the edge
/// must match the top texel at distance a + d from the center on the opposite side (the box
/// unfolded flat). The old radial 3D-distance wrap mapped depth d to √(a² + d²) and stretched
/// the stripes down the side. Covers different UV scales and irregular tessellations.
#[test]
fn wrapped_tip_on_the_side_face_matches_the_unfolded_top() {
    for (subdivisions, res) in [(1, 512), (4, 512), (7, 1024)] {
        let mut sc = Scene::new(primitives::atlas_box(Vec3::splat(2.0), subdivisions, "Wrap"), res);
        let cam = Camera::perspective(Vec3::new(0.0, 4.0, 3.2), Vec3::new(0.0, 0.9, 0.7), Vec3::Y, 30.0, 0.1, 100.0, [800, 800]);
        let (a, period) = (0.2f32, 9.0);
        let stripes: Vec<f32> = (0..256 * 256).map(|i| if ((i / 256) as f32 / 256.0 * period).floor() as i32 % 2 == 0 { 1.0 } else { 0.0 }).collect();
        let settings = BrushSettings { size: 30.0, hardness: 1.0, spacing: 10.0, ..BrushSettings::default() };
        let mut s = Stroke::begin(&sc.ts, sc.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red())
            .unwrap().with_alpha(Some(Arc::new(subcraft_image::Bitmap::new_f32(256, 256, 1, stripes))));
        let mut ts = sc.ts.clone();
        s.add_point(&mut ts, &sc.ctx(&cam), cam.project(Vec3::new(0.0, 1.0, 1.0 - a)).unwrap().truncate()).unwrap();
        sc.ts = ts;
        let img = sc.image(ChannelKind::BaseColor).unwrap(); let surface = sc.surface();
        // Coverage profiles along x≈0, binned by unfolded distance from the stamp center.
        let bins = 120; let bin = |s: f32| (s / 0.6 * bins as f32) as usize;
        let (mut top, mut side) = (vec![(0.0f32, 0u32); bins], vec![(0.0f32, 0u32); bins]);
        for (i, p) in surface.position.iter().enumerate() {
            if surface.triangle[i] == SurfaceTexels::NO_TRIANGLE || p[0].abs() > 0.04 { continue; }
            let value = alpha(&img, i, res);
            let (target, s) = if (p[1] - 1.0).abs() < 1e-4 && p[2] < 1.0 - a { (&mut top, 1.0 - a - p[2]) }
                else if (p[2] - 1.0).abs() < 1e-4 && p[1] < 1.0 { (&mut side, a + 1.0 - p[1]) } else { continue };
            if s < 0.6 { let b = &mut target[bin(s)]; b.0 += value; b.1 += 1; }
        }
        let (mut agree, mut compared, mut painted_side) = (0, 0, 0);
        for (t, sd) in top.iter().zip(&side) {
            if t.1 == 0 || sd.1 == 0 { continue; }
            let (t, sd) = (t.0 / t.1 as f32, sd.0 / sd.1 as f32);
            if sd > 0.5 { painted_side += 1; }
            // Ignore bins straddling a stripe transition.
            if (0.2..0.8).contains(&t) || (0.2..0.8).contains(&sd) { continue; }
            compared += 1; if (t > 0.5) == (sd > 0.5) { agree += 1; }
        }
        assert!(painted_side > 10 && compared > 30, "subdivisions {subdivisions}: side {painted_side}, compared {compared}");
        assert!(agree as f32 >= compared as f32 * 0.95, "subdivisions {subdivisions}: stripes on the side are stretched ({agree}/{compared})");
    }
}
