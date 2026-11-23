//! Normal maps on 3D fill projections: the bumps follow the projected image, not the UV layout.

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::{FillChannel, FillSource, Projection, ProjectionMode};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;

/// Object-space direction of the composited normal where the ray from `eye` along `dir` hits
/// the model, through the UV tangent frame of the triangle there (T toward +U, B toward V up).
fn world_normal(s: &Session, eye: Vec3, dir: Vec3) -> Vec3 {
    let mesh = &s.scene().mesh;
    let hit = s.scene().bvh.intersect(eye, dir, 0.0, f32::INFINITY).expect("the ray hits the model");
    let tri = hit.tri as usize;
    let sp = mesh.interpolate(tri, hit.u, hit.v);
    let pos = mesh.tri_positions(tri);
    let uv = mesh.tri_uvs(tri).map(|u| Vec2::new(u.x, 1.0 - u.y));
    let (e1, e2, d1, d2) = (pos[1] - pos[0], pos[2] - pos[0], uv[1] - uv[0], uv[2] - uv[0]);
    let det = d1.x * d2.y - d2.x * d1.y;
    let t = ((e1 * d2.y - e2 * d1.y) / det).normalize();
    let b = ((e2 * d1.x - e1 * d2.x) / det).normalize();
    let img = s.composite(s.selected_texture_set().id).unwrap().channel(ChannelKind::Normal).unwrap();
    let c = img.get((sp.uv.x * img.width() as f32) as u32, (sp.uv.y * img.height() as f32) as u32);
    let n = Vec3::new(c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
    (t * n.x + b * n.y + sp.normal * n.z).normalize()
}

/// The tilt every pixel of the test normal map has: towards the image's right and a little up.
fn tilt() -> Vec3 {
    Vec3::new(0.5, 0.2, 0.84).normalize()
}

/// A cube of side 2 whose fill layer reads the tilted normal map through `projection`.
fn session(name: &str, projection: Projection) -> Session {
    let dir = std::env::temp_dir().join(format!("subcraft-projected-normals-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tilt.png");
    let n = tilt();
    let data: Vec<f32> = (0..32 * 32).flat_map(|_| [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]).collect();
    save_image(&path, 32, 32, 3, &data, FileFormat::Png16).unwrap();
    let mut s = Session::new(primitives::cube(2.0), 256);
    let resource = s.import_image(&path).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Normal, fill: FillChannel::new(FillSource::Bitmap { resource }) }).unwrap();
    s.execute(Command::SetFillProjection { layer, projection }).unwrap();
    s.update_composites();
    std::fs::remove_dir_all(dir).unwrap();
    s
}

#[test]
fn a_triplanar_normal_map_tilts_along_the_image_on_every_side() {
    let n = tilt();
    for unique in [true, false] {
        let mut projection = Projection { mode: ProjectionMode::Triplanar, triplanar_unique: unique, ..Projection::default() };
        projection.set_hardness(1.0);
        let s = session("triplanar", projection);
        // (side the ray comes from, image right there, image up there). The planes read
        // +Z: u = x; +X: u = z; +Y: u = x, v = z. With unique sides the far side runs the
        // other way, so the image reads the right way round from outside.
        let sides = [
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Z, if unique { Vec3::NEG_X } else { Vec3::X }, Vec3::Y),
            (Vec3::NEG_X, if unique { Vec3::NEG_Z } else { Vec3::Z }, Vec3::Y),
        ];
        for (out, right, up) in sides {
            let want = (right * n.x + up * n.y + out * n.z).normalize();
            for off in [0.0, 0.31, -0.27] {
                let got = world_normal(&s, out * 3.0 + (right + up) * off, -out);
                assert!((got - want).length() < 0.03, "unique {unique}, side {out:?} at {off}: {got:?}, want {want:?}");
            }
        }
    }
}

#[test]
fn a_soft_triplanar_blend_of_a_flat_normal_map_stays_flat() {
    // A flat map (0.5, 0.5, 1) has nothing to turn: every side and every blend stays flat.
    let dir = std::env::temp_dir().join(format!("subcraft-projected-normals-{}-flat", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flat.png");
    save_image(&path, 8, 8, 3, &[0.5, 0.5, 1.0].repeat(64), FileFormat::Png16).unwrap();
    let mut s = Session::new(primitives::uv_sphere(1.0, 48, 24), 256);
    let resource = s.import_image(&path).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Normal, fill: FillChannel::new(FillSource::Bitmap { resource }) }).unwrap();
    s.execute(Command::SetFillProjection { layer, projection: Projection { mode: ProjectionMode::Triplanar, ..Projection::default() } }).unwrap();
    s.update_composites();
    let img = s.composite(s.selected_texture_set().id).unwrap().channel(ChannelKind::Normal).unwrap();
    let (mut worst, mut seen) = (0.0f32, 0);
    for y in (0..img.height()).step_by(3) {
        for x in (0..img.width()).step_by(3) {
            let c = img.get(x, y);
            if c.iter().take(3).all(|v| v.is_finite()) {
                seen += 1;
                worst = worst.max((c[0] - 0.5).abs()).max((c[1] - 0.5).abs());
            }
            assert!(c.iter().all(|v| v.is_finite()), "texel {x},{y}: {c:?}");
        }
    }
    assert!(seen > 1000 && worst < 0.01, "{seen} texels, worst tilt {worst}");
    std::fs::remove_dir_all(dir).unwrap();
}
