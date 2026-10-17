use glam::{Vec2, Vec3};
use std::path::{Path, PathBuf};
use subcraft_app::{
    Camera, Command, Session,
    assets::{UserLibrary, alphas},
    brush_preset::{load_brush_preset, save_brush_preset},
};
use subcraft_core::{ChannelKind as K, normal};
use subcraft_doc::{LayerContent, ProjectionMode};
use subcraft_image::{Bitmap, TiledImage, encode_bitmap};
use subcraft_mesh::primitives;
use subcraft_paint::NormalPaintBlend;

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "subcraft-brush-textures-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn image(path: &Path, rgba: [u8; 4]) {
    let (bytes, _) = encode_bitmap(&Bitmap::new_u8(2, 2, 4, rgba.repeat(4))).unwrap();
    std::fs::write(path, bytes).unwrap();
}
fn paint(s: &mut Session) {
    let cam = Camera::perspective(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    );
    s.begin_stroke().unwrap();
    for x in [210.0, 230.0, 256.0, 280.0, 300.0, 256.0, 210.0] {
        s.stroke_to(&cam, Vec2::new(x, 256.0), false).unwrap();
    }
    s.end_stroke();
}
fn raster(s: &Session, k: K) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else {
        panic!()
    };
    p.channels[&k].clone()
}

#[test]
fn normal_texture_stroke_converts_dx_is_one_undo_step_and_survives_project_roundtrip() {
    let dir = folder("stroke");
    let path = dir.join("Plate_normalDX.png");
    image(&path, [180, 70, 240, 255]);
    let mut s = Session::new(primitives::cube(2.0), 128);
    let before = s.document().clone();
    let count = s.history().entries().len();
    let result = s.import_brush_texture_set(std::slice::from_ref(&path)).unwrap();
    assert_eq!(result.used.len(), 1);
    assert_eq!(
        s.document(),
        &before,
        "choosing brush material is tool state only"
    );
    assert_eq!(s.history().entries().len(), count);
    let bmp = s
        .resources()
        .bitmap(s.brush.material.textures[&K::Normal])
        .unwrap();
    assert!((bmp.sample(0.5, 0.5, Default::default(), false)[1] - 185.0 / 255.0).abs() < 0.001);
    s.brush.settings.size = 25.0;
    s.brush.settings.stroke_opacity = 0.6;
    paint(&mut s);
    let after = s.document().clone();
    assert_eq!(s.history().entries().len(), count + 1);
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else {
        panic!()
    };
    assert_eq!(
        p.channels.len(),
        1,
        "normal-only import must not alter flat channels"
    );
    let raster = raster(&s, K::Normal);
    let expected = normal::decode([180.0 / 255.0, 185.0 / 255.0, 240.0 / 255.0]).normalize();
    let mut checked = 0;
    for y in 0..128 {
        for x in 0..128 {
            let px = raster.get(x, y);
            assert!(px[3] <= 0.601);
            if px[3] > 0.1 {
                assert!((normal::decode([px[0], px[1], px[2]]) - expected).length() < 0.002);
                checked += 1;
            }
        }
    }
    assert!(checked > 30);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    s.execute(Command::Redo).unwrap();
    assert_eq!(s.document(), &after);
    let project = dir.join("normal.subcraft");
    s.save(Some(&project)).unwrap();
    std::fs::remove_file(path).unwrap();
    let reopened = Session::open_project(&project).unwrap();
    assert_eq!(raster, self::raster(&reopened, K::Normal));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preset_embeds_and_remaps_images_in_a_fresh_document_and_missing_images_do_not_change_brush() {
    let dir = folder("preset");
    let path = dir.join("Brass_basecolor.png");
    image(&path, [150, 100, 20, 255]);
    let mut a = Session::new(primitives::cube(2.0), 128);
    a.set_brush_channel_image(K::BaseColor, &path).unwrap();
    a.brush.material.projection.mode = ProjectionMode::Triplanar;
    a.brush.material.projection.scale = [2.0, 3.0];
    a.brush.material.projection.offset = [0.25, -0.3];
    a.brush.material.projection.rotation_deg = 30.0;
    a.brush.material.texture_tiling = false;
    a.brush.material.normal_blend = NormalPaintBlend::Detail;
    let lib = UserLibrary::new(dir.join("library"));
    let saved = save_brush_preset(&lib, &a.brush_preset("Brass")).unwrap();
    std::fs::remove_file(path).unwrap();
    let loaded = load_brush_preset(saved.user_file.as_ref().unwrap()).unwrap();
    assert_eq!(loaded.version, 2);
    assert_eq!(loaded.images.len(), 1);
    let mut b = Session::new(primitives::cube(2.0), 128);
    assert!(b.apply_brush_preset(&loaded, &alphas::builtin()).is_none());
    let old = a.brush.material.textures[&K::BaseColor];
    let new = b.brush.material.textures[&K::BaseColor];
    assert_ne!(old, new, "fresh resource identity");
    assert_eq!(a.resources().bitmap(old), b.resources().bitmap(new));
    let mut expected = a.brush.material.clone();
    expected.textures.insert(K::BaseColor, new);
    assert_eq!(b.brush.material, expected);
    let mut broken = loaded;
    broken.images.clear();
    let before = b.brush.material.clone();
    assert!(
        b.apply_brush_preset(&broken, &alphas::builtin())
            .unwrap()
            .contains("missing")
    );
    assert_eq!(b.brush.material, before);
    assert!(save_brush_preset(&lib, &broken).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn whole_material_import_is_atomic_and_gloss_conversion_matches_fill_import() {
    let dir = folder("atomic");
    let gloss = dir.join("Metal_gloss.png");
    let base = dir.join("Metal_basecolor.png");
    image(&gloss, [51, 51, 51, 255]);
    std::fs::write(&base, b"broken").unwrap();
    let mut s = Session::new(primitives::cube(2.0), 128);
    let before = s.brush.material.clone();
    assert!(
        s.import_brush_texture_set(&[gloss.clone(), base.clone()])
            .is_err()
    );
    assert_eq!(s.brush.material, before);
    image(&base, [128, 128, 128, 255]);
    let report = s.import_brush_texture_set(&[gloss, base]).unwrap();
    assert_eq!(report.used.len(), 2);
    let id = s.brush.material.textures[&K::Roughness];
    assert!(
        (s.resources()
            .bitmap(id)
            .unwrap()
            .sample(0.5, 0.5, Default::default(), false)[0]
            - 0.8)
            .abs()
            < 0.001
    );
    assert!(!s.brush.material.channels[&K::Normal].enabled);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancel_restores_snapshot_and_active_stroke_uses_captured_images() {
    let dir = folder("cancel");
    let path = dir.join("detail_normal.png");
    image(&path, [180, 128, 240, 255]);
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.import_brush_texture_set(&[path]).unwrap();
    let before = s.document().clone();
    let count = s.history().entries().len();
    let cam = Camera::perspective(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    );
    s.begin_stroke().unwrap();
    s.brush.material.textures.clear();
    s.stroke_to(&cam, Vec2::splat(256.0), false).unwrap();
    assert!(raster(&s, K::Normal).allocated_tiles() > 0);
    s.cancel_stroke();
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), count);
    std::fs::remove_dir_all(dir).unwrap();
}
