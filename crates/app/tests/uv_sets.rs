//! Several UV sets: a texture set switches to another UV layout of the model, its paint follows
//! the surface, the other texture sets are untouched, and the choice is saved with the project.
use glam::{Vec2, Vec3};
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind as K;
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{Corner, Mesh, MeshBuilder, primitives};

const RES: u32 = 128;

/// Two quads, one material each. UV0 fills 0..1 per quad; "lightmap" packs the right quad into the
/// right half, rotated a quarter turn so a wrong transfer cannot pass by accident.
fn two_quads() -> Mesh {
    let mut b = MeshBuilder::new("quads");
    b.set_uv_set_name(1, "lightmap");
    for (q, name) in ["left", "right"].iter().enumerate() {
        let m = b.material(name);
        let x0 = q as f32 * 1.5;
        let p = [Vec3::new(x0, 0., 0.), Vec3::new(x0 + 1., 0., 0.), Vec3::new(x0 + 1., 1., 0.), Vec3::new(x0, 1., 0.)];
        let uv0 = [Vec2::new(0., 1.), Vec2::new(1., 1.), Vec2::new(1., 0.), Vec2::new(0., 0.)];
        let uv1 = uv0.map(|uv| Vec2::new(0.5 + 0.45 * uv.y, 0.05 + 0.9 * uv.x));
        for tri in [[0, 1, 2], [0, 2, 3]] {
            let c = tri.map(|i| Corner { position: p[i], normal: Some(Vec3::Z), uv: Some(uv0[i]) });
            b.push_triangle_uv_sets(m, c, &[], &[tri.map(|i| Some(uv1[i]))]);
        }
    }
    b.build().unwrap()
}

fn color(p: Vec3) -> Vec3 {
    Vec3::new(p.x / 2.5, p.y, 0.5)
}

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (fx, fy) = (uv.x * w - 0.5, uv.y * h - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |x: f32, y: f32| img.get(x.clamp(0.0, w - 1.0) as u32, y.clamp(0.0, h - 1.0) as u32);
    let mut out = [0.0; 4];
    for (x, y, wgt) in [(x0, y0, (1.0 - tx) * (1.0 - ty)), (x0 + 1.0, y0, tx * (1.0 - ty)), (x0, y0 + 1.0, (1.0 - tx) * ty), (x0 + 1.0, y0 + 1.0, tx * ty)] {
        let v = at(x, y);
        for c in 0..4 {
            out[c] += v[c] * wgt;
        }
    }
    out
}

/// Paint every texture set's first layer with a colour taken from the surface position.
fn paint_by_position(s: &mut Session) {
    let mesh = s.scene().mesh.clone();
    for i in 0..s.document().texture_sets.len() {
        let ts = s.document().texture_sets[i].clone();
        let surface = rasterize_surface(&mesh, mesh.material_triangles(ts.material_slot), RES, RES, 2.0);
        let mut layer = (*ts.layers[0]).clone();
        let LayerContent::Paint(p) = &mut layer.content else { panic!("paint layer") };
        let img = p.channel_mut(K::BaseColor, RES);
        for y in 0..RES {
            for x in 0..RES {
                let k = (y * RES + x) as usize;
                if surface.covered(k) {
                    let c = color(Vec3::from_slice(&surface.position[k][..3]));
                    img.set(x, y, [c.x, c.y, c.z, 1.0]);
                }
            }
        }
        s.execute(Command::ReplaceLayer { layer: layer.id, replacement: Box::new(layer), label: "Paint".into() }).unwrap();
    }
}

/// Largest colour error of texture set `index`'s paint, sampled through the current UVs.
fn paint_error(s: &Session, index: usize) -> f32 {
    let mesh = s.scene().mesh.clone();
    let ts = &s.document().texture_sets[index];
    let LayerContent::Paint(p) = &ts.layers[0].content else { panic!("paint layer") };
    let base = &p.channels[&K::BaseColor];
    let mut worst = 0.0f32;
    for t in mesh.material_triangles(ts.material_slot) {
        for [u, v] in [[0.33, 0.33], [0.15, 0.6], [0.6, 0.15]] {
            let sp = mesh.interpolate(t, u, v);
            let got = bilinear(base, sp.uv);
            assert!(got[3] > 0.95, "coverage {} on triangle {t}", got[3]);
            worst = worst.max((Vec3::new(got[0], got[1], got[2]) - color(sp.position)).abs().max_element());
        }
    }
    worst
}

#[test]
fn switching_a_texture_set_to_another_uv_set_moves_its_paint_and_nothing_else() {
    let mut s = Session::new(two_quads(), RES);
    paint_by_position(&mut s);
    let (left, right) = (s.document().texture_sets[0].id, s.document().texture_sets[1].id);
    let choice = s.uv_set_choice(right).unwrap();
    assert_eq!(choice.names, ["UV0", "lightmap"]);
    assert_eq!(choice.current, 0);
    let left_layers = s.document().texture_sets[0].layers.clone();

    let summary = s.set_uv_set(right, 1).unwrap();
    assert!(summary.contains("right is painted with UV set lightmap"), "{summary}");
    assert_eq!(s.document().texture_sets[1].id, right, "identity is kept");
    assert_eq!(s.uv_set_choice(right).unwrap().current, 1);
    assert_eq!(s.uv_set_choice(left).unwrap().current, 0);
    // The right quad now lives in the right half of its texture.
    let mesh = s.scene().mesh.clone();
    for t in mesh.material_triangles(s.document().texture_sets[1].material_slot) {
        assert!(mesh.tri_uvs(t).iter().all(|uv| uv.x >= 0.5 - 1e-6), "{:?}", mesh.tri_uvs(t));
    }
    let err = paint_error(&s, 1);
    assert!(err < 0.06, "paint followed the surface (largest error {err})");
    assert!(paint_error(&s, 0) < 1e-3, "left quad untouched");
    assert_eq!(s.document().texture_sets[0].layers, left_layers, "left layers are the same data");
    s.update_composites();

    // Saved and reopened: the choice and the layout come back; then back to UV0.
    let path = std::env::temp_dir().join(format!("uv-sets-{}.subcraft", subcraft_core::ObjectId::new()));
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.uv_set_choice(right).unwrap().current, 1);
    assert_eq!(reopened.scene().mesh.uvs, s.scene().mesh.uvs);
    reopened.set_uv_set(right, 0).unwrap();
    assert_eq!(reopened.uv_set_choice(right).unwrap().current, 0);
    let err = paint_error(&reopened, 1);
    assert!(err < 0.08, "back on UV0 the paint is still in place (largest error {err})");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_model_with_one_uv_set_offers_no_choice() {
    let mut s = Session::new(primitives::cube(1.0), 64);
    let ts = s.document().texture_sets[0].id;
    assert!(s.uv_set_choice(ts).is_none());
    assert!(s.set_uv_set(ts, 1).is_err());
    let mut q = Session::new(two_quads(), 64);
    let right = q.document().texture_sets[1].id;
    assert!(q.set_uv_set(right, 2).unwrap_err().contains("no UV set 2"));
}
