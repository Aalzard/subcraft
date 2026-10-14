//! The user's own image textures: import, fill-layer use, color rules, persistence, undo.

use std::path::PathBuf;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_core::color::srgb_to_linear;
use subcraft_doc::{FillChannel, FillSource, LayerContent};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-images-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 64x64 RGB image: left half = `left` (8-bit level), right half = `right`.
fn write_halves(path: &std::path::Path, left: u8, right: u8, format: FileFormat) {
    let mut data = Vec::new();
    for _y in 0..64 {
        for x in 0..64 {
            let v = if x < 32 { left } else { right } as f32 / 255.0;
            data.extend_from_slice(&[v, v, v]);
        }
    }
    save_image(path, 64, 64, 3, &data, format).unwrap();
}

/// Session with a fill layer on top whose Base Color and Roughness both use `image`.
fn session_with_image_fill(image: &std::path::Path) -> (Session, subcraft_core::ResourceId) {
    let mut s = Session::new(primitives::sample_crate(), 128);
    let res = s.import_image(image).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    for ch in [ChannelKind::BaseColor, ChannelKind::Roughness] {
        let fill = FillChannel { enabled: true, source: FillSource::Bitmap { resource: res } };
        s.execute(Command::SetFillChannel { layer, channel: ch, fill }).unwrap();
    }
    s.update_composites();
    (s, res)
}

fn texel(s: &Session, ch: ChannelKind, u: f32, v: f32) -> f32 {
    let ts = s.selected_texture_set().id;
    let img = s.composite(ts).unwrap().channel(ch).unwrap();
    let (x, y) = ((u * img.width() as f32) as u32, (v * img.height() as f32) as u32);
    img.get(x, y)[0]
}

#[test]
fn image_fill_decodes_srgb_for_color_and_keeps_data_raw() {
    let dir = temp_dir("rules");
    let path = dir.join("halves.png");
    write_halves(&path, 64, 192, FileFormat::Png8);
    let (s, _) = session_with_image_fill(&path);

    // UV projection, scale 1: left half of UV space samples the left half of the image.
    let (a, b) = (64.0 / 255.0, 192.0 / 255.0);
    let tol = 2e-3; // f16 storage
    assert!((texel(&s, ChannelKind::BaseColor, 0.1, 0.25) - srgb_to_linear(a)).abs() < tol, "base color is linearized");
    assert!((texel(&s, ChannelKind::BaseColor, 0.9, 0.25) - srgb_to_linear(b)).abs() < tol);
    assert!((texel(&s, ChannelKind::Roughness, 0.1, 0.25) - a).abs() < tol, "roughness is raw data");
    assert!((texel(&s, ChannelKind::Roughness, 0.9, 0.25) - b).abs() < tol);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_fill_survives_save_and_reopen_bit_exact() {
    let dir = temp_dir("save");
    for (file, fmt) in [("tex.png", FileFormat::Png8), ("tex.tga", FileFormat::Tga8), ("tex.exr", FileFormat::Exr32)] {
        let img = dir.join(file);
        write_halves(&img, 30, 220, fmt);
        let (mut s, res) = session_with_image_fill(&img);
        let project = dir.join(format!("{file}.subcraft"));
        s.save(Some(&project)).unwrap();
        std::fs::remove_file(&img).unwrap(); // the project must not depend on the source file

        let mut reopened = Session::open_project(&project).unwrap();
        reopened.update_composites();
        let r = reopened.resources().get(res).expect("resource kept with its id");
        assert_eq!(r.name, file);
        let ts = s.selected_texture_set().id;
        assert_eq!(
            reopened.composite(ts).unwrap().channels,
            s.composite(ts).unwrap().channels,
            "{file}: composite identical after reopen"
        );

        // Re-saving a loaded project keeps the image (TGA needs its extension to decode).
        let again = dir.join(format!("{file}.2.subcraft"));
        reopened.save(Some(&again)).unwrap();
        let third = Session::open_project(&again).unwrap();
        assert!(third.resources().get(res).is_some(), "{file}: resource survives a second save");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn importing_the_same_file_twice_reuses_the_resource() {
    let dir = temp_dir("dedup");
    let path = dir.join("a.png");
    write_halves(&path, 10, 20, FileFormat::Png8);
    let mut s = Session::new(primitives::cube(1.0), 64);
    let a = s.import_image(&path).unwrap();
    let b = s.import_image(&path).unwrap();
    assert_eq!(a, b);
    assert_eq!(s.resources().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn choosing_an_image_is_undoable() {
    let dir = temp_dir("undo");
    let path = dir.join("u.png");
    write_halves(&path, 0, 255, FileFormat::Png8);
    let (mut s, res) = session_with_image_fill(&path);
    let source = |s: &Session| match &s.selected_layer().unwrap().content {
        LayerContent::Fill(f) => f.channels[&ChannelKind::Roughness].source.clone(),
        _ => panic!("fill layer"),
    };
    assert_eq!(source(&s), FillSource::Bitmap { resource: res });
    s.execute(Command::Undo).unwrap();
    assert!(matches!(source(&s), FillSource::Uniform { .. }), "undo restores the uniform value");
    s.execute(Command::Redo).unwrap();
    assert_eq!(source(&s), FillSource::Bitmap { resource: res });
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unreadable_files_are_reported() {
    let dir = temp_dir("bad");
    let path = dir.join("broken.png");
    std::fs::write(&path, b"definitely not a png").unwrap();
    let mut s = Session::new(primitives::cube(1.0), 64);
    let err = s.import_image(&path).unwrap_err();
    assert!(err.contains("broken.png"), "{err}");
    assert!(s.import_image(&dir.join("missing.png")).is_err());
    assert!(s.resources().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
