//! Auto UV unwrap: a model with bad UVs is unwrapped through the session, painted layers follow
//! the surface onto the generated layout, and a UV-less model opens already unwrapped.
use glam::{Vec2, Vec3};
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind as K;
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::unwrap::{UnwrapSettings, analyze_uvs};
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{import::write_obj, primitives};

const RES: u32 = 256;

fn color(p: Vec3) -> Vec3 {
    p + Vec3::splat(0.5)
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

#[test]
fn unwrapping_a_painted_model_moves_the_paint_with_the_surface() {
    let mut s = Session::new(primitives::cube(1.0), RES);
    let mesh = s.scene().mesh.clone();
    let ts = s.selected_texture_set().clone();
    let surface = rasterize_surface(&mesh, mesh.material_triangles(ts.material_slot), RES, RES, 2.0);
    let mut layer = (*ts.layers[0]).clone();
    let LayerContent::Paint(p) = &mut layer.content else { panic!("paint layer") };
    let img = p.channel_mut(K::BaseColor, RES);
    for y in 0..RES {
        for x in 0..RES {
            let i = (y * RES + x) as usize;
            if surface.covered(i) {
                let c = color(Vec3::from_slice(&surface.position[i][..3]));
                img.set(x, y, [c.x, c.y, c.z, 1.0]);
            }
        }
    }
    s.execute(Command::ReplaceLayer { layer: layer.id, replacement: Box::new(layer), label: "Paint".into() }).unwrap();

    let id = s.document().texture_sets[0].id;
    let summary = s.auto_uv_unwrap(UnwrapSettings { margin_texels: 4.0, resolution: RES, ..Default::default() }).unwrap();
    assert!(summary.contains("6 chart"), "{summary}");
    assert_eq!(s.document().texture_sets[0].id, id, "identity is kept");
    let report = analyze_uvs(&s.scene().mesh);
    assert!(report.usable(), "{}", report.summary());
    assert_eq!(report.flipped, 0);

    let mesh = s.scene().mesh.clone();
    let LayerContent::Paint(p) = &s.document().texture_sets[0].layers[0].content else { panic!("paint layer") };
    let base = &p.channels[&K::BaseColor];
    let (mut worst, mut n) = (0.0f32, 0);
    for t in 0..mesh.triangle_count() {
        for [u, v] in [[0.33, 0.33], [0.1, 0.6], [0.6, 0.1]] {
            let sp = mesh.interpolate(t, u, v);
            let got = bilinear(base, sp.uv);
            let want = color(sp.position);
            worst = worst.max((Vec3::new(got[0], got[1], got[2]) - want).abs().max_element());
            assert!(got[3] > 0.95, "coverage {} at triangle {t}", got[3]);
            n += 1;
        }
    }
    assert!(worst < 0.05, "largest color error {worst} over {n} points");
    s.update_composites();
}

#[test]
fn a_model_without_uvs_opens_unwrapped() {
    let dir = std::env::temp_dir().join(format!("subcraft-autouv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bare.obj");
    // Write the sphere without vt / vn so the importer sees a UV-less model.
    let sphere = primitives::uv_sphere(1.0, 24, 12);
    let mut text = String::new();
    for p in &sphere.positions {
        text += &format!("v {} {} {}\n", p.x, p.y, p.z);
    }
    for t in 0..sphere.triangle_count() {
        let [a, b, c] = sphere.tri(t);
        text += &format!("f {} {} {}\n", a + 1, b + 1, c + 1);
    }
    std::fs::write(&path, text).unwrap();
    let _ = write_obj; // the writer always emits UVs
    let (s, warnings) = Session::open_mesh(&path, RES).unwrap();
    assert!(warnings.iter().any(|w| w.contains("generated automatically")), "{warnings:?}");
    let report = analyze_uvs(&s.scene().mesh);
    assert!(report.usable(), "{}", report.summary());
    std::fs::remove_dir_all(dir).ok();
}
