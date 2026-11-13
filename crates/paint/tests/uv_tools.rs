//! 2D-view tools beyond the plain brush: smudge, clone, stencil and symmetry in UV space give
//! the same kind of result as in the 3D view (chunk 32 of the Painter parity list).

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::wrap_to_tile;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::stroke::uv_symmetry_copies;
use subcraft_paint::{BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintError, PaintTarget, Stencil, Stroke, StrokeMode, StrokeOptions, UvView};

struct Scene {
    mesh: Arc<Mesh>,
    bvh: Bvh,
    geo: TextureSetGeometry,
    depth: DepthCache,
    ts: TextureSet,
    layer: LayerId,
}

const RES: u32 = 256;

impl Scene {
    fn new() -> Self {
        let mesh = Arc::new(primitives::cube(2.0));
        let mut ts = TextureSet::new("T", 0, RES);
        let tc = ts.clone();
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, RES), depth: DepthCache::new(), mesh, ts, layer }
    }

    fn image(&self) -> TiledImage {
        let LayerContent::Paint(p) = &self.ts.find(self.layer).unwrap().content else { panic!() };
        p.channels.get(&ChannelKind::BaseColor).cloned().expect("base colour painted")
    }

    /// One 2D-view stroke through `points` (UV), built by `setup`.
    fn stroke(&mut self, mode: StrokeMode, material: &BrushMaterial, settings: BrushSettings, options: StrokeOptions, points: &[Vec2], setup: impl FnOnce(Stroke) -> Stroke) -> Result<Stroke, PaintError> {
        let mut s = setup(Stroke::begin(&self.ts, self.layer, PaintTarget::Content, mode, settings, options, material)?);
        let cam = uv_camera();
        let mut ts = self.ts.clone();
        let ctx = PaintContext { mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera: &cam, depth: &self.depth };
        for &p in points {
            s.add_uv_point(&mut ts, &ctx, p, false, 1.0)?;
        }
        self.ts = ts;
        Ok(s)
    }

    /// UV centroid of the first triangle facing `normal`.
    fn face_uv(&self, normal: Vec3) -> Vec2 {
        let t = (0..self.mesh.triangle_count()).find(|&t| self.mesh.geometric_normal(t).dot(normal) > 0.99).unwrap();
        let (q, _) = wrap_to_tile(self.mesh.tri_uvs(t));
        (q[0] + q[1] + q[2]) / 3.0
    }
}

fn uv_camera() -> Camera {
    Camera { view: glam::Mat4::IDENTITY, proj: glam::Mat4::IDENTITY, viewport: [1, 1] }
}

fn color(rgb: [f32; 3]) -> BrushMaterial {
    BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(rgb[0], rgb[1], rgb[2]))])
}

fn texel(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let p = (uv * RES as f32).as_uvec2();
    let px = img.get(p.x, p.y);
    [px[0], px[1], px[2], px[img.comps() - 1]]
}

fn small() -> BrushSettings {
    BrushSettings { size: 4.0, hardness: 1.0, ..BrushSettings::default() }
}

/// Radius of `small()` in UV units on this cube.
fn small_radius_uv(sc: &Scene) -> f32 {
    small().radius(sc.mesh.bounds.diagonal()) * subcraft_paint::stroke::uv_per_unit(&sc.mesh, 0)
}

#[test]
fn smudge_in_uv_drags_the_paint_along_instead_of_painting_the_brush_colour() {
    let mut sc = Scene::new();
    let center = sc.face_uv(Vec3::Z);
    let r = small_radius_uv(&sc);
    sc.stroke(StrokeMode::Paint, &color([1.0, 0.0, 0.0]), small(), StrokeOptions::default(), &[center], |s| s).unwrap();
    // From the middle of the red dab to beyond its edge, with a green brush colour.
    let end = center + Vec2::new(3.0 * r, 0.0);
    let path: Vec<Vec2> = (0..=24).map(|i| center.lerp(end, i as f32 / 24.0)).collect();
    sc.stroke(StrokeMode::Smudge, &color([0.0, 1.0, 0.0]), small(), StrokeOptions::default(), &path, |s| s).unwrap();
    let img = sc.image();
    let beyond = texel(&img, center + Vec2::new(2.0 * r, 0.0));
    assert!(beyond[3] > 0.05, "red was dragged past the dab's edge: {beyond:?}");
    assert!(beyond[0] > 0.5 && beyond[1] < 0.05, "smudge carries the red, it does not paint the green brush colour: {beyond:?}");
}

#[test]
fn clone_in_uv_copies_from_the_source_texture_position() {
    let mut sc = Scene::new();
    let source = sc.face_uv(Vec3::Z);
    let target = sc.face_uv(-Vec3::Z);
    sc.stroke(StrokeMode::Paint, &color([0.0, 0.0, 1.0]), small(), StrokeOptions::default(), &[source], |s| s).unwrap();
    let s = sc.stroke(StrokeMode::Clone, &color([1.0, 0.0, 0.0]), small(), StrokeOptions::default(), &[target], |s| s.with_uv_clone(source, None)).unwrap();
    assert!((s.uv_clone_offset().unwrap() - (source - target)).length() < 1e-4, "first dab fixes the UV offset");
    let img = sc.image();
    let copied = texel(&img, target);
    assert!(copied[3] > 0.9 && copied[2] > 0.9 && copied[0] < 0.05, "the blue dab is copied, not the red brush colour: {copied:?}");
}

#[test]
fn clone_in_uv_without_a_source_is_refused() {
    let mut sc = Scene::new();
    let at = sc.face_uv(Vec3::Z);
    let err = sc.stroke(StrokeMode::Clone, &color([1.0, 0.0, 0.0]), small(), StrokeOptions::default(), &[at], |s| s).err();
    assert!(matches!(err, Some(PaintError::NoCloneSource)), "{err:?}");
}

#[test]
fn a_stencil_masks_uv_strokes_where_it_lies_over_the_2d_view() {
    let mut sc = Scene::new();
    let center = sc.face_uv(Vec3::Z);
    let r = small_radius_uv(&sc);
    // Texture drawn 512 px large at the view origin; a stencil covering the whole view, white on
    // its left half, black on its right half, so the split runs through the dab's centre.
    let view = UvView { origin: Vec2::ZERO, side: 512.0, viewport: [512, 512] };
    let split = view.to_view(center).x / 512.0;
    let n = 512usize;
    let mut data = vec![0u8; n * n];
    for (i, v) in data.iter_mut().enumerate() {
        *v = if ((i % n) as f32 + 0.5) / n as f32 <= split - 0.5 / n as f32 { 255 } else { 0 };
    }
    let bitmap = Arc::new(Bitmap::new_u8(n as u32, n as u32, 1, data));
    let stencil = Arc::new(Stencil { scale: 1.0, center: Vec2::splat(0.5), ..Stencil::new(bitmap) });
    sc.stroke(StrokeMode::Paint, &color([1.0, 0.0, 0.0]), small(), StrokeOptions::default(), &[center], |s| s.with_stencil(Some(stencil)).with_uv_view(Some(view))).unwrap();
    let img = sc.image();
    let left = texel(&img, center - Vec2::new(0.6 * r, 0.0));
    let right = texel(&img, center + Vec2::new(0.6 * r, 0.0));
    assert!(left[3] > 0.9, "white half paints: {left:?}");
    assert_eq!(right[3], 0.0, "black half blocks: {right:?}");
}

#[test]
fn without_a_uv_view_the_stencil_does_not_touch_uv_strokes() {
    let mut sc = Scene::new();
    let center = sc.face_uv(Vec3::Z);
    let black = Arc::new(Bitmap::new_u8(4, 4, 1, vec![0; 16]));
    sc.stroke(StrokeMode::Paint, &color([1.0, 0.0, 0.0]), small(), StrokeOptions::default(), &[center], |s| s.with_stencil(Some(Arc::new(Stencil::new(black))))).unwrap();
    assert!(texel(&sc.image(), center)[3] > 0.9);
}

#[test]
fn mirror_symmetry_in_uv_paints_the_mirrored_face_too() {
    let mut sc = Scene::new();
    let right = sc.face_uv(Vec3::X);
    let left = sc.face_uv(-Vec3::X);
    let options = StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() };
    let cam = uv_camera();
    let ctx = PaintContext { mesh: &sc.mesh, bvh: &sc.bvh, geometry: &sc.geo, camera: &cam, depth: &sc.depth };
    let copies = uv_symmetry_copies(&ctx, &options, right);
    assert_eq!(copies.len(), 1, "{copies:?}");
    // The copy is the surface point mirrored across X: same Y and Z, opposite X.
    let at = |uv: Vec2| {
        let t = subcraft_paint::stroke::triangle_at_uv(&sc.mesh, &sc.geo, uv).expect("on the model");
        let (q, _) = wrap_to_tile(sc.mesh.tri_uvs(t as usize));
        let (e0, e1, w) = (q[1] - q[0], q[2] - q[0], uv - q[0]);
        let d = e0.perp_dot(e1);
        sc.mesh.interpolate(t as usize, w.perp_dot(e1) / d, e0.perp_dot(w) / d).position
    };
    let (p, m) = (at(right), at(copies[0]));
    assert!((m - Vec3::new(-p.x, p.y, p.z)).length() < 0.02, "mirror of {p} should be at {m}");
    assert!(sc.mesh.geometric_normal(subcraft_paint::stroke::triangle_at_uv(&sc.mesh, &sc.geo, copies[0]).unwrap() as usize).dot(-Vec3::X) > 0.99, "on the -X face");
    let _ = left;
    sc.stroke(StrokeMode::Paint, &color([1.0, 0.0, 0.0]), small(), options, &[right], |s| s).unwrap();
    let img = sc.image();
    assert!(texel(&img, right)[3] > 0.9);
    assert!(texel(&img, copies[0])[3] > 0.9, "the mirrored dab lands on the -X face");
    // The +Y face is on neither side: untouched.
    assert_eq!(texel(&img, sc.face_uv(Vec3::Y))[3], 0.0);
}

/// QA 2026-11: 2D smudge with symmetry shared one carried colour between the stroke and its
/// mirror copy, so paint picked up on one side was laid down on the other.
#[test]
fn uv_smudge_with_symmetry_carries_each_sides_own_paint() {
    let mut sc = Scene::new();
    // Everything red, the -X face blue.
    {
        let (mesh, geo) = (sc.mesh.clone(), &sc.geo);
        let blue_face = |uv: Vec2| subcraft_paint::stroke::triangle_at_uv(&mesh, geo, uv).is_some_and(|t| mesh.geometric_normal(t as usize).dot(-Vec3::X) > 0.99);
        let mut colors = Vec::with_capacity((RES * RES) as usize);
        for y in 0..RES {
            for x in 0..RES {
                let uv = (Vec2::new(x as f32, y as f32) + 0.5) / RES as f32;
                colors.push(if blue_face(uv) { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 0.0, 0.0, 1.0] });
            }
        }
        let LayerContent::Paint(p) = &mut sc.ts.find_mut(sc.layer).unwrap().content else { panic!() };
        let img = p.channel_mut(ChannelKind::BaseColor, RES);
        for (i, c) in colors.into_iter().enumerate() {
            img.set(i as u32 % RES, i as u32 / RES, c);
        }
    }
    let right = sc.face_uv(Vec3::X);
    let r = small_radius_uv(&sc);
    let options = StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() };
    let cam = uv_camera();
    let mirrored = {
        let ctx = PaintContext { mesh: &sc.mesh, bvh: &sc.bvh, geometry: &sc.geo, camera: &cam, depth: &sc.depth };
        uv_symmetry_copies(&ctx, &options, right)[0]
    };
    let path: Vec<Vec2> = (0..=8).map(|i| right + Vec2::new(0.0, r * i as f32 / 8.0)).collect();
    sc.stroke(StrokeMode::Smudge, &color([0.0, 1.0, 0.0]), small(), options, &path, |s| s).unwrap();
    let img = sc.image();
    let there = texel(&img, mirrored);
    assert!(there[0] < 0.05 && there[2] > 0.9, "the mirror side keeps its own blue: {there:?}");
    let here = texel(&img, right);
    assert!(here[0] > 0.9 && here[2] < 0.05, "and this side its red: {here:?}");
}

/// QA 2026-11: the smudge's carried colours were sized by the copies that landed in each
/// segment, so a copy dropped on the mirror plane reset the carried paint mid-stroke. The count
/// is the number of symmetry copies, whatever lands.
#[test]
fn uv_smudge_carries_paint_for_every_symmetry_copy_even_when_one_is_dropped() {
    let sc = Scene::new();
    let options = StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() };
    assert_eq!(subcraft_paint::stroke::uv_copy_count(&options), 2);
    assert_eq!(subcraft_paint::stroke::uv_copy_count(&StrokeOptions::default()), 1);
    let cam = uv_camera();
    let ctx = PaintContext { mesh: &sc.mesh, bvh: &sc.bvh, geometry: &sc.geo, camera: &cam, depth: &sc.depth };
    // Outside every island the copy is dropped; the count stays.
    assert!(uv_symmetry_copies(&ctx, &options, Vec2::new(-1.0, -1.0)).is_empty());
    assert_eq!(subcraft_paint::stroke::uv_copy_count(&options), 2);
}

#[test]
fn without_symmetry_there_are_no_uv_copies() {
    let sc = Scene::new();
    let cam = uv_camera();
    let ctx = PaintContext { mesh: &sc.mesh, bvh: &sc.bvh, geometry: &sc.geo, camera: &cam, depth: &sc.depth };
    assert!(uv_symmetry_copies(&ctx, &StrokeOptions::default(), sc.face_uv(Vec3::X)).is_empty());
    // Outside every UV island: nothing to mirror.
    let options = StrokeOptions { symmetry: [true, true, true], ..StrokeOptions::default() };
    assert!(uv_symmetry_copies(&ctx, &options, Vec2::new(-1.0, -1.0)).is_empty());
}
