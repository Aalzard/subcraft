//! Pen pressure dynamics: pressure drives size / flow / opacity / hardness per stamp, and a brush
//! with every pressure curve off paints exactly what it painted before pressure existed.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::{
    BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, PressureCurve, Stroke, StrokeMode, StrokeOptions,
};

const RES: u32 = 256;

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
        let mesh = Arc::new(primitives::cube(2.0));
        let mut ts = TextureSet::new("T", 0, RES);
        let tc = ts.clone();
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
        let camera = Camera::perspective(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, RES), depth: DepthCache::new(), mesh, ts, layer, camera }
    }

    fn screen(&self, p: Vec3) -> Vec2 {
        self.camera.project(p).unwrap().truncate()
    }

    /// Paint through `points` (screen) with per-point pressure; `None` uses the mouse entry point.
    fn paint(&self, settings: BrushSettings, points: &[(Vec2, Option<f32>)]) -> TiledImage {
        let material = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
        let mut s = Stroke::begin(&self.ts, self.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &material)
            .unwrap();
        let mut ts = self.ts.clone();
        let ctx = PaintContext { mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera: &self.camera, depth: &self.depth };
        for &(p, pressure) in points {
            match pressure {
                Some(pr) => s.add_point_pressure(&mut ts, &ctx, p, pr).unwrap(),
                None => s.add_point(&mut ts, &ctx, p).unwrap(),
            };
        }
        let LayerContent::Paint(p) = &ts.find(self.layer).unwrap().content else { panic!() };
        p.channels[&ChannelKind::BaseColor].clone()
    }

    /// A horizontal drag across the front face (x from -0.8 to 0.8) with pressure `f(t)`.
    fn drag(&self, f: impl Fn(f32) -> Option<f32>) -> Vec<(Vec2, Option<f32>)> {
        (0..=60)
            .map(|k| {
                let t = k as f32 / 60.0;
                (self.screen(Vec3::new(-0.8 + 1.6 * t, 0.0, 1.0)), f(t))
            })
            .collect()
    }

    /// Front-face texels: (object-space x, alpha).
    fn front(&self, img: &TiledImage) -> Vec<(f32, f32)> {
        let surf = rasterize_surface(&self.mesh, 0..self.mesh.triangle_count(), RES, RES, 0.0);
        (0..surf.triangle.len())
            .filter(|&i| surf.covered(i) && self.mesh.geometric_normal(surf.triangle[i] as usize).z > 0.9)
            .map(|i| {
                let px = img.get(i as u32 % RES, i as u32 / RES);
                (surf.position[i][0], px[img.comps() - 1])
            })
            .collect()
    }
}

/// Per x-band statistics over the front face: (painted texel count, max alpha).
fn bands(texels: &[(f32, f32)], n: usize) -> Vec<(usize, f32)> {
    let mut out = vec![(0usize, 0.0f32); n];
    for &(x, a) in texels {
        // Bands over the middle of the drag only, away from the round end caps.
        let t = (x + 0.6) / 1.2;
        if !(0.0..1.0).contains(&t) {
            continue;
        }
        let b = &mut out[(t * n as f32) as usize];
        if a > 0.01 {
            b.0 += 1;
        }
        b.1 = b.1.max(a);
    }
    out
}

fn base() -> BrushSettings {
    BrushSettings { size: 12.0, hardness: 0.8, spacing: 0.05, ..BrushSettings::default() }
}

#[test]
fn pressure_off_is_bit_identical_to_mouse_painting() {
    let sc = Scene::new();
    let settings = BrushSettings { size_jitter: 0.3, flow: 0.4, flow_jitter: 0.2, stroke_opacity: 0.8, ..base() };
    assert!(!settings.pressure.any_enabled());
    let mouse = sc.paint(settings, &sc.drag(|_| None));
    let wobbly = sc.paint(settings, &sc.drag(|t| Some((t * 17.0).sin().abs())));
    assert!(sc.front(&mouse).iter().any(|&(_, a)| a > 0.1), "the stroke paints");
    assert!(mouse == wobbly, "pressure must have no effect with every curve off");
    // Enabled curves at full pressure are also exact (factor 1.0).
    let mut full = settings;
    full.pressure.size = PressureCurve { enabled: true, min: 0.1, gamma: 1.6 };
    full.pressure.flow = PressureCurve { enabled: true, min: 0.0, gamma: 0.7 };
    full.pressure.opacity = PressureCurve { enabled: true, min: 0.2, gamma: 1.0 };
    full.pressure.hardness = PressureCurve { enabled: true, min: 0.0, gamma: 1.0 };
    assert!(sc.paint(full, &sc.drag(|_| Some(1.0))) == mouse);
}

#[test]
fn ramped_pressure_grows_the_footprint_with_size_dynamics() {
    let sc = Scene::new();
    let mut settings = base();
    settings.pressure.size = PressureCurve { enabled: true, min: 0.1, gamma: 1.0 };
    let img = sc.paint(settings, &sc.drag(|t| Some(t)));
    // Quarter bands: texel-grid noise is small against the growth between them.
    let widths: Vec<usize> = bands(&sc.front(&img), 4).iter().map(|b| b.0).collect();
    for w in widths.windows(2) {
        assert!(w[1] > w[0], "footprint must grow with pressure: {widths:?}");
    }
    assert!(widths[3] as f32 > 2.0 * widths[0] as f32, "{widths:?}");
    // Constant full pressure paints at least as wide as the ramp's widest band everywhere.
    let flat = bands(&sc.front(&sc.paint(settings, &sc.drag(|_| Some(1.0)))), 4);
    assert!(flat.iter().all(|b| b.0 >= widths[3]), "{flat:?} vs {widths:?}");
}

#[test]
fn single_dab_coverage_grows_with_pressure_for_each_parameter() {
    let sc = Scene::new();
    let at = [sc.screen(Vec3::new(0.0, 0.0, 1.0))];
    let curve = PressureCurve { enabled: true, min: 0.0, gamma: 1.0 };
    let mut size = base();
    size.pressure.size = curve;
    let mut flow = base();
    flow.pressure.flow = curve;
    let mut opacity = base();
    opacity.pressure.opacity = curve;
    let mut hardness = base();
    hardness.hardness = 1.0;
    hardness.pressure.hardness = curve;
    for (name, s) in [("size", size), ("flow", flow), ("opacity", opacity), ("hardness", hardness)] {
        let mass: Vec<f32> = [0.15, 0.4, 0.7, 1.0]
            .iter()
            .map(|&p| sc.front(&sc.paint(s, &[(at[0], Some(p))])).iter().map(|t| t.1).sum())
            .collect();
        for m in mass.windows(2) {
            assert!(m[1] > m[0], "{name}: coverage must grow with pressure: {mass:?}");
        }
    }
}

#[test]
fn opacity_pressure_caps_overlapping_stamps() {
    let sc = Scene::new();
    let mut settings = base();
    settings.pressure.opacity = PressureCurve { enabled: true, min: 0.0, gamma: 1.0 };
    // Dense overlapping stamps would saturate to 1.0; the 0.4 pressure ceiling holds instead.
    let light = bands(&sc.front(&sc.paint(settings, &sc.drag(|_| Some(0.4)))), 4);
    for (_, max) in &light {
        assert!((0.35..=0.41).contains(max), "{light:?}");
    }
    // A ramp raises the ceiling along the stroke.
    let ramp = bands(&sc.front(&sc.paint(settings, &sc.drag(Some))), 8);
    for w in ramp.windows(2) {
        assert!(w[1].1 >= w[0].1 - 1e-3, "{ramp:?}");
    }
    assert!(ramp[0].1 < 0.5 && ramp[7].1 > 0.85, "{ramp:?}");
}
