//! Brush material "Brush stamp" mapping: the channel images follow every dab (turning and
//! scaling with it) instead of sitting fixed on the mesh. Scene as in `normal_stamp.rs`: a plane
//! facing +Y seen from above, screen-right = +X, screen-up = -Z, UV tangent frame T = +X,
//! B = -Z, so an unrotated dab's image frame (x right, y up) equals tangent space.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, ResourceId, normal};
use subcraft_doc::{Layer, LayerContent, Resource, ResourceKind, ResourceStore, TextureSet};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_paint::{
    BrushMaterial, BrushSettings, Camera, DepthCache, MaterialMapping, NormalPaintBlend, PaintContext, PaintTarget, Stroke, StrokeMode,
    StrokeOptions,
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
        let mesh = Arc::new(primitives::plane(4.0, 4));
        let mut ts = TextureSet::new("T", 0, RES);
        let tc = ts.clone();
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
        let camera = Camera::perspective(Vec3::new(0.0, 6.0, 0.0), Vec3::ZERO, -Vec3::Z, 40.0, 0.1, 100.0, [512, 512]);
        Self { bvh: Bvh::build(&mesh), geo: TextureSetGeometry::new(mesh.clone(), 0, RES), depth: DepthCache::new(), mesh, ts, layer, camera }
    }

    /// One dab per point of `at` (plane XZ), each its own stroke.
    fn dab(&mut self, at: Vec2, material: &BrushMaterial, resources: &ResourceStore, settings: BrushSettings) {
        let mut s = Stroke::begin(&self.ts, self.layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), material)
            .unwrap()
            .with_resources(resources);
        let mut ts = self.ts.clone();
        let screen = self.camera.project(Vec3::new(at.x, 0.0, at.y)).unwrap().truncate();
        let ctx = PaintContext { displaced: None, mesh: &self.mesh, bvh: &self.bvh, geometry: &self.geo, camera: &self.camera, depth: &self.depth };
        s.add_point(&mut ts, &ctx, screen).unwrap();
        s.finish(&mut ts, &ctx).unwrap();
        s.flush_deferred(&mut ts, &ctx).unwrap();
        self.ts = ts;
    }

    fn image(&self, kind: ChannelKind) -> TiledImage {
        let LayerContent::Paint(p) = &self.ts.find(self.layer).unwrap().content else { panic!() };
        p.channels.get(&kind).cloned().expect("channel painted")
    }

    /// Texel values (alpha last) under plane point `at`.
    fn texel(&self, kind: ChannelKind, at: Vec2) -> Vec<f32> {
        let img = self.image(kind);
        let (x, y) = (((at.x / 4.0 + 0.5) * RES as f32) as u32, ((at.y / 4.0 + 0.5) * RES as f32) as u32);
        img.get(x, y).to_vec()
    }
}

fn texture(store: &mut ResourceStore, bitmap: Bitmap) -> ResourceId {
    let id = ResourceId::new();
    store.insert(Resource { id, name: "t.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: None });
    id
}

/// Normal (0.6, 0, 0.8): leaning toward image +X.
fn tilted() -> Bitmap {
    Bitmap::new_u8(4, 4, 3, [204u8, 128, 230].repeat(16))
}

/// 8x8: left half red, right half blue.
fn split_colour() -> Bitmap {
    let mut px = Vec::new();
    for _y in 0..8 {
        for x in 0..8 {
            px.extend_from_slice(if x < 4 { &[255u8, 0, 0] } else { &[0u8, 0, 255] });
        }
    }
    Bitmap::new_u8(8, 8, 3, px)
}

/// Dabs about one plane unit wide, hard edged.
fn brush(angle_deg: f32) -> BrushSettings {
    BrushSettings { size: 20.0, hardness: 1.0, angle_deg, ..BrushSettings::default() }
}

fn material(kind: ChannelKind, value: ChannelValue, id: ResourceId, mapping: MaterialMapping) -> BrushMaterial {
    let mut m = BrushMaterial::with(&[(kind, value)]);
    m.textures.insert(kind, id);
    m.mapping = mapping;
    m
}

fn dec(v: &[f32]) -> Vec3 {
    normal::decode([v[0], v[1], v[2]])
}

#[test]
fn brush_stamp_normals_turn_with_the_dab_angle() {
    let mut store = ResourceStore::default();
    let id = texture(&mut store, tilted());
    let m = material(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0), id, MaterialMapping::BrushStamp);
    for (angle, want) in [(0.0, Vec3::new(0.6, 0.0, 0.8)), (90.0, Vec3::new(0.0, -0.6, 0.8)), (180.0, Vec3::new(-0.6, 0.0, 0.8))] {
        let mut sc = Scene::new();
        sc.dab(Vec2::ZERO, &m, &store, brush(angle));
        let t = sc.texel(ChannelKind::Normal, Vec2::ZERO);
        assert!(t[3] > 0.99, "alpha {}", t[3]);
        let d = dec(&t);
        assert!((d - want).length() < 0.03, "angle {angle}: {d:?} want {want:?}");
        assert!((d.length() - 1.0).abs() < 1e-3);
    }
}

#[test]
fn mesh_mapping_keeps_the_old_behaviour_and_ignores_the_dab_angle() {
    let mut store = ResourceStore::default();
    let id = texture(&mut store, tilted());
    let m = material(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0), id, MaterialMapping::Mesh);
    assert_eq!(BrushMaterial::default().mapping, MaterialMapping::Mesh, "the default is unchanged");
    let mut sc = Scene::new();
    sc.dab(Vec2::ZERO, &m, &store, brush(90.0));
    let d = dec(&sc.texel(ChannelKind::Normal, Vec2::ZERO));
    assert!((d - Vec3::new(0.6, 0.0, 0.8)).length() < 0.03, "{d:?}");
}

#[test]
fn brush_stamp_colour_follows_each_dab_instead_of_the_uvs() {
    let mut store = ResourceStore::default();
    let id = texture(&mut store, split_colour());
    let stamp = material(ChannelKind::BaseColor, ChannelValue::rgb(0.5, 0.5, 0.5), id, MaterialMapping::BrushStamp);
    let off = Vec2::new(0.2, 0.0);
    // Two dabs far apart: both show red on their left and blue on their right.
    for centre in [Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.3)] {
        let mut sc = Scene::new();
        sc.dab(centre, &stamp, &store, brush(0.0));
        let l = sc.texel(ChannelKind::BaseColor, centre - off);
        let r = sc.texel(ChannelKind::BaseColor, centre + off);
        assert!(l[0] > 0.9 && l[2] < 0.1 && l[3] > 0.99, "left of {centre}: {l:?}");
        assert!(r[2] > 0.9 && r[0] < 0.1 && r[3] > 0.99, "right of {centre}: {r:?}");
        // Turned 180 degrees, the dab's image is mirrored left-right.
        let mut sc = Scene::new();
        sc.dab(centre, &stamp, &store, brush(180.0));
        let l = sc.texel(ChannelKind::BaseColor, centre - off);
        assert!(l[2] > 0.9 && l[0] < 0.1, "turned dab, left of {centre}: {l:?}");
    }
    // Mesh mapping: the image is fixed in UV space, so both sides of a dab on the plane's left
    // half are red.
    let mesh = material(ChannelKind::BaseColor, ChannelValue::rgb(0.5, 0.5, 0.5), id, MaterialMapping::Mesh);
    let mut sc = Scene::new();
    let centre = Vec2::new(-1.0, 0.0);
    sc.dab(centre, &mesh, &store, brush(0.0));
    let (l, r) = (sc.texel(ChannelKind::BaseColor, centre - off), sc.texel(ChannelKind::BaseColor, centre + off));
    assert!((l[0] - r[0]).abs() < 1e-3 && (l[2] - r[2]).abs() < 1e-3, "mesh mapping: {l:?} {r:?}");
}

#[test]
fn brush_stamp_normal_with_detail_blend_keeps_the_layers_normals() {
    let mut store = ResourceStore::default();
    let tilted_id = texture(&mut store, tilted());
    // Leaning toward image +Y (up on screen = tangent +Y here).
    let up_id = texture(&mut store, Bitmap::new_u8(4, 4, 3, [128u8, 204, 230].repeat(16)));
    let mut sc = Scene::new();
    sc.dab(Vec2::ZERO, &material(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0), tilted_id, MaterialMapping::BrushStamp), &store, brush(0.0));
    let mut detail = material(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0), up_id, MaterialMapping::BrushStamp);
    detail.normal_blend = NormalPaintBlend::Detail;
    sc.dab(Vec2::ZERO, &detail, &store, brush(0.0));
    let d = dec(&sc.texel(ChannelKind::Normal, Vec2::ZERO));
    assert!(d.x > 0.3 && d.y > 0.3, "both leans survive: {d:?}");
    let mut replace = detail.clone();
    replace.normal_blend = NormalPaintBlend::Replace;
    sc.dab(Vec2::ZERO, &replace, &store, brush(0.0));
    let d = dec(&sc.texel(ChannelKind::Normal, Vec2::ZERO));
    assert!((d - Vec3::new(0.0, 0.6, 0.8)).length() < 0.03, "replace covers it: {d:?}");
}

#[test]
fn brush_stamp_height_turns_with_the_dab() {
    // Height ramp: 0 on the image's left, 1 on its right.
    let mut px = Vec::new();
    for _y in 0..16 {
        for x in 0..16u32 {
            px.push((x * 17) as u8);
        }
    }
    let mut store = ResourceStore::default();
    let id = texture(&mut store, Bitmap::new_u8(16, 16, 1, px));
    let m = material(ChannelKind::Height, ChannelValue::scalar(0.5), id, MaterialMapping::BrushStamp);
    let off = Vec2::new(0.2, 0.0);
    let mut sc = Scene::new();
    sc.dab(Vec2::ZERO, &m, &store, brush(0.0));
    let (l, r) = (sc.texel(ChannelKind::Height, -off)[0], sc.texel(ChannelKind::Height, off)[0]);
    assert!(r > l + 0.25, "ramp rises to the right: {l} {r}");
    let mut sc = Scene::new();
    sc.dab(Vec2::ZERO, &m, &store, brush(180.0));
    let (l, r) = (sc.texel(ChannelKind::Height, -off)[0], sc.texel(ChannelKind::Height, off)[0]);
    assert!(l > r + 0.25, "turned dab: ramp rises to the left: {l} {r}");
}

#[test]
fn mapping_is_not_written_at_its_default_and_round_trips() {
    let json = serde_json::to_string(&BrushMaterial::default()).unwrap();
    assert!(!json.contains("mapping"), "{json}");
    let old: BrushMaterial = serde_json::from_str(&json).unwrap();
    assert_eq!(old.mapping, MaterialMapping::Mesh);
    let m = BrushMaterial { mapping: MaterialMapping::BrushStamp, ..BrushMaterial::default() };
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("\"mapping\":\"brush_stamp\""), "{json}");
    assert_eq!(serde_json::from_str::<BrushMaterial>(&json).unwrap(), m);
}
