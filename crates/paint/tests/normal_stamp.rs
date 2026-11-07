//! Normal-map / height stamp brush: numeric checks of what lands in the layer's Normal and
//! Height channels. Scene: a plane facing +Y seen from above with screen-right = +X and
//! screen-up = -Z, whose UV tangent frame is T = +X, B = -Z, so an image-frame slope (x right,
//! y up) appears unchanged in tangent space for an unrotated stamp.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, normal};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::{
    BrushMaterial, BrushSettings, Camera, DepthCache, ImageStamp, PaintContext, PaintTarget, StampKind, Stroke, StrokeMode, StrokeOptions,
};

const RES: u32 = 128;

struct Scene {
    mesh: Arc<Mesh>,
    bvh: Bvh,
    geo: TextureSetGeometry,
    depth: DepthCache,
    ts: TextureSet,
    layer: LayerId,
    camera: Camera,
}

impl Scene {
    fn new() -> Self {
        let mesh = Arc::new(primitives::plane(4.0, 4));
        let mut ts = TextureSet::new("T", 0, RES);
        let tc = ts.clone();
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
        let camera = Camera::perspective(Vec3::new(0.0, 6.0, 0.0), Vec3::ZERO, -Vec3::Z, 40.0, 0.1, 100.0, [512, 512]);
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, RES), depth: DepthCache::new(), mesh, ts, layer, camera }
    }

    /// One stamp at `at` (plane XZ), through the full stroke pipeline.
    fn stamp(&mut self, at: Vec2, stamp: &ImageStamp, image: &Bitmap, settings: BrushSettings) {
        self.stroke(&[at], stamp, image, settings);
    }

    fn stroke(&mut self, points: &[Vec2], stamp: &ImageStamp, image: &Bitmap, settings: BrushSettings) {
        let material = BrushMaterial::with(&[(stamp.kind.channel(), ChannelValue::rgb(0.5, 0.5, 1.0))]);
        let mut s = Stroke::begin(&self.ts, self.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &material)
            .unwrap()
            .with_stamp(Some(stamp.clone()), Some(Arc::new(image.clone())));
        let mut ts = self.ts.clone();
        let screen: Vec<Vec2> = points.iter().map(|p| self.camera.project(Vec3::new(p.x, 0.0, p.y)).unwrap().truncate()).collect();
        let ctx = PaintContext { mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera: &self.camera, depth: &self.depth };
        for w in screen.windows(2) {
            for k in 0..=20 {
                s.add_point(&mut ts, &ctx, w[0].lerp(w[1], k as f32 / 20.0)).unwrap();
            }
        }
        if screen.len() == 1 {
            s.add_point(&mut ts, &ctx, screen[0]).unwrap();
        }
        s.finish(&mut ts, &ctx).unwrap();
        s.flush_deferred(&mut ts, &ctx).unwrap();
        self.ts = ts;
    }

    fn image(&self, kind: ChannelKind) -> Option<TiledImage> {
        let LayerContent::Paint(p) = &self.ts.find(self.layer).unwrap().content else { panic!() };
        p.channels.get(&kind).cloned()
    }

    /// `(normal, alpha)` of the texel under plane point `at`.
    fn texel(&self, kind: ChannelKind, at: Vec2) -> ([f32; 3], f32) {
        let img = self.image(kind).expect("channel painted");
        let (x, y) = (((at.x / 4.0 + 0.5) * RES as f32) as u32, ((at.y / 4.0 + 0.5) * RES as f32) as u32);
        let px = img.get(x, y);
        ([px[0], px[1], px[2]], px[img.comps() - 1])
    }
}

fn solid(rgb: [u8; 3]) -> Bitmap {
    Bitmap::new_u8(4, 4, 3, rgb.repeat(16))
}

/// Normal (0.6, 0, 0.8) = leaning toward +X.
fn tilted() -> Bitmap {
    solid([204, 128, 230])
}

fn brush() -> BrushSettings {
    BrushSettings { size: 30.0, hardness: 1.0, ..BrushSettings::default() }
}

fn dec(n: [f32; 3]) -> Vec3 {
    normal::decode(n)
}

#[test]
fn flat_stamp_leaves_flat_normals() {
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &solid([128, 128, 255]), brush());
    let (n, a) = sc.texel(ChannelKind::Normal, Vec2::ZERO);
    assert!(a > 0.99, "alpha {a}");
    assert!((dec(n) - Vec3::Z).length() < 0.01, "{:?}", dec(n));
}

#[test]
fn tilted_stamp_over_empty_layer_writes_the_stamp_normal() {
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &tilted(), brush());
    let (n, _) = sc.texel(ChannelKind::Normal, Vec2::ZERO);
    let d = dec(n);
    assert!((d - Vec3::new(0.6, 0.0, 0.8)).length() < 0.02, "{d:?}");
    assert!((d.length() - 1.0).abs() < 1e-4);
}

#[test]
fn rotation_rotates_the_normal_xy_with_the_stamp() {
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &tilted(), BrushSettings { angle_deg: 90.0, ..brush() });
    let d = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    // +90° on screen (y down): the +X lean now leans toward screen-down = +Z world = -V = tangent -Y.
    assert!((d - Vec3::new(0.0, -0.6, 0.8)).length() < 0.02, "{d:?}");
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &tilted(), BrushSettings { angle_deg: 180.0, ..brush() });
    let d = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    assert!((d - Vec3::new(-0.6, 0.0, 0.8)).length() < 0.02, "{d:?}");
}

#[test]
fn strength_zero_changes_nothing_and_half_halves_the_slope() {
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp { strength: 0.0, ..Default::default() }, &tilted(), brush());
    assert!((dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0) - Vec3::Z).length() < 0.01);
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp { strength: 0.5, ..Default::default() }, &tilted(), brush());
    let d = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    let slope = d.x / d.z;
    assert!((slope - 0.375).abs() < 0.01, "slope {slope}");
}

#[test]
fn flip_green_mirrors_the_y_deviation() {
    let leaning_y = solid([128, 204, 230]); // (0, 0.6, 0.8)
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &leaning_y, brush());
    let a = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp { flip_green: true, ..Default::default() }, &leaning_y, brush());
    let b = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    assert!((a.y - 0.6).abs() < 0.02 && (b.y + 0.6).abs() < 0.02, "{a:?} {b:?}");
}

#[test]
fn second_stamp_blends_over_the_normals_already_in_the_layer_with_whiteout() {
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &solid([128, 204, 230]), brush()); // (0, 0.6, 0.8)
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &tilted(), brush()); // (0.6, 0, 0.8)
    let d = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    let expect = normal::whiteout(Vec3::new(0.0, 0.6, 0.8), Vec3::new(0.6, 0.0, 0.8));
    assert!((d - expect).length() < 0.02, "{d:?} vs {expect:?}");
    assert!((d.length() - 1.0).abs() < 2e-3);
}

#[test]
fn every_painted_texel_is_unit_length_and_the_edge_fades_softly() {
    let mut sc = Scene::new();
    let settings = BrushSettings { size: 40.0, hardness: 0.3, ..BrushSettings::default() };
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &tilted(), settings);
    let img = sc.image(ChannelKind::Normal).unwrap();
    let mut painted = 0;
    for y in 0..RES {
        for x in 0..RES {
            let px = img.get(x, y);
            if px[3] > 0.0 {
                painted += 1;
                assert!((dec([px[0], px[1], px[2]]).length() - 1.0).abs() < 1e-3);
            }
        }
    }
    assert!(painted > 100);
    // Along +X from the center the deviation never jumps: neighbours differ by little.
    let mut prev: Option<f32> = None;
    let mut worst: f32 = 0.0;
    for k in 0..40 {
        let at = Vec2::new(k as f32 * 0.05, 0.0);
        let img_px = img.get(((at.x / 4.0 + 0.5) * RES as f32) as u32, RES / 2);
        let slope = dec([img_px[0], img_px[1], img_px[2]]).x * img_px[3];
        if let Some(p) = prev { worst = worst.max((slope - p).abs()); }
        prev = Some(slope);
    }
    assert!(worst < 0.2, "hard border: step {worst}");
    // And far outside the stamp nothing was written.
    assert!(sc.texel(ChannelKind::Normal, Vec2::new(1.9, 1.9)).1 == 0.0);
}

#[test]
fn flat_areas_of_the_map_do_not_disturb_existing_normals() {
    // Left half flat, right half tilted: painting over a pre-tilted layer keeps the left half.
    let mut data = Vec::new();
    for _y in 0..4 { for x in 0..4 { data.extend(if x < 2 { [128u8, 128, 255] } else { [204, 128, 230] }); } }
    let half = Bitmap::new_u8(4, 4, 3, data);
    let mut sc = Scene::new();
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &solid([128, 204, 230]), BrushSettings { size: 60.0, hardness: 1.0, ..brush() });
    let before = dec(sc.texel(ChannelKind::Normal, Vec2::new(-0.5, 0.0)).0);
    sc.stamp(Vec2::ZERO, &ImageStamp::default(), &half, BrushSettings { size: 60.0, hardness: 1.0, ..brush() });
    let after = dec(sc.texel(ChannelKind::Normal, Vec2::new(-0.5, 0.0)).0);
    assert!((before - after).length() < 0.03, "{before:?} -> {after:?}");
    let right = dec(sc.texel(ChannelKind::Normal, Vec2::new(0.5, 0.0)).0);
    assert!(right.x > 0.3, "{right:?}");
}

#[test]
fn tiled_mode_repeats_the_image_over_the_stroke() {
    // Checker normal: alternate left/right lean per image pixel, 2x2 image repeated.
    let checker = Bitmap::new_u8(2, 2, 3, [[204u8, 128, 230], [52, 128, 230], [52, 128, 230], [204, 128, 230]].concat());
    let mut sc = Scene::new();
    let stamp = ImageStamp { tiled: true, ..Default::default() };
    sc.stroke(&[Vec2::new(-0.6, 0.0), Vec2::new(0.6, 0.0)], &stamp, &checker, BrushSettings { size: 25.0, hardness: 1.0, ..BrushSettings::default() });
    let img = sc.image(ChannelKind::Normal).unwrap();
    let (mut left, mut right) = (0, 0);
    for y in 0..RES {
        for x in 0..RES {
            let px = img.get(x, y);
            if px[3] > 0.9 {
                if px[0] > 0.6 { right += 1 } else if px[0] < 0.4 { left += 1 }
            }
        }
    }
    assert!(left > 50 && right > 50, "pattern not repeated: left {left}, right {right}");
}

#[test]
fn height_stamp_writes_the_height_channel_only() {
    let mut sc = Scene::new();
    let stamp = ImageStamp { kind: StampKind::Height, ..Default::default() };
    let white = Bitmap::new_u8(4, 4, 1, vec![255; 16]);
    sc.stamp(Vec2::ZERO, &stamp, &white, brush());
    let img = sc.image(ChannelKind::Height).expect("height painted");
    let (x, y) = (RES / 2, RES / 2);
    let px = img.get(x, y);
    assert!(px[img.comps() - 1] > 0.9 && px[0] > 0.9, "{px:?}");
    assert!(sc.image(ChannelKind::Normal).is_none());
}

#[test]
fn overlapping_dense_stamps_do_not_stack_the_strength() {
    // Spacing 0.1 lays ~10 stamps per diameter; the weighted average keeps the slope at 0.75.
    let mut sc = Scene::new();
    let settings = BrushSettings { spacing: 0.1, ..brush() };
    sc.stroke(&[Vec2::new(-0.3, 0.0), Vec2::new(0.3, 0.0)], &ImageStamp::default(), &tilted(), settings);
    let d = dec(sc.texel(ChannelKind::Normal, Vec2::ZERO).0);
    assert!((d.x / d.z - 0.75).abs() < 0.05, "{d:?}");
}
