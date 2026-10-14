//! Saving smart materials / smart masks to the user library and applying them in other sessions.

use std::path::PathBuf;

use subcraft_app::assets::{UserAssetKind, UserLibrary};
use subcraft_app::library::{load_smart_masks, load_smart_materials};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, LayerContent, MaskEffectKind, Projection};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-lib-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Session with a folder "Painted" holding a red fill masked by an image + Edges generator.
fn session_with_folder(dir: &std::path::Path, res: u32) -> (Session, subcraft_core::LayerId) {
    let logo = dir.join("logo.png");
    let data: Vec<f32> = (0..32 * 32).flat_map(|i| if i % 32 < 16 { [1.0, 1.0] } else { [1.0, 0.0] }).collect();
    save_image(&logo, 32, 32, 2, &data, FileFormat::Png8).unwrap();

    let mut s = Session::new(primitives::sample_crate(), res);
    let img = s.import_image(&logo).unwrap();
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let folder = s.selected_layer().unwrap().id;
    s.execute(Command::Rename { layer: folder, name: "Painted".into() }).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selected_layer().unwrap().id;
    s.execute(Command::MoveLayer { layer: fill, parent: Some(folder), index: 0 }).unwrap();
    let red = FillChannel { enabled: true, source: FillSource::uniform(ChannelValue::rgb(0.8, 0.05, 0.02)) };
    s.execute(Command::SetFillChannel { layer: fill, channel: ChannelKind::BaseColor, fill: red }).unwrap();
    s.execute(Command::AddMask { layer: fill, white: false }).unwrap();
    s.execute(Command::AddMaskEffect { layer: fill, kind: MaskEffectKind::Image { resource: img, projection: Projection::default(), invert: false } })
        .unwrap();
    s.execute(Command::AddMaskEffect { layer: fill, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).unwrap();
    s.execute(Command::SelectLayer(Some(folder))).unwrap();
    (s, folder)
}

#[test]
fn smart_material_saves_to_the_library_and_applies_identically() {
    let dir = temp_dir("same-res");
    let lib = UserLibrary::new(dir.join("Library"));
    let (mut a, folder) = session_with_folder(&dir, 128);
    let saved = a.save_smart_material(folder, &lib).unwrap();
    assert_eq!(saved.name, "Painted");
    assert_eq!(lib.list(UserAssetKind::SmartMaterial).len(), 1);
    std::fs::remove_file(dir.join("logo.png")).unwrap(); // the library file must be self-contained

    // "Restart": load from disk, apply in a new session of the same mesh and resolution.
    let (loaded, errors) = load_smart_materials(&lib);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(loaded.len(), 1);
    let mut b = Session::new(primitives::sample_crate(), 128);
    let before = b.history().entries().len();
    b.add_smart_material(&loaded[0]).unwrap();
    assert_eq!(b.history().entries().len(), before + 1, "one undo step");
    let applied = b.selected_layer().unwrap().clone();
    assert_eq!(applied.name, "Painted");
    assert_ne!(applied.id, folder, "fresh ids");

    // Same stack content -> same composite (ids differ, pixels don't).
    a.update_composites();
    b.update_composites();
    let ts_a = a.selected_texture_set().id;
    let ts_b = b.selected_texture_set().id;
    assert_eq!(a.composite(ts_a).unwrap().channels, b.composite(ts_b).unwrap().channels);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn smart_material_with_paint_resamples_to_another_resolution() {
    let dir = temp_dir("resample");
    let lib = UserLibrary::new(dir.join("Library"));
    let mut a = Session::new(primitives::sample_crate(), 128);
    a.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let paint = a.selected_layer().unwrap().id;
    // Paint directly into the layer's roughness channel (stand-in for a brush stroke).
    {
        let ts = a.selected_texture_set().clone();
        let mut l = ts.find(paint).unwrap().clone();
        if let LayerContent::Paint(p) = &mut l.content {
            let img = p.channel_mut(ChannelKind::Roughness, 128);
            for y in 0..64 {
                for x in 0..64 {
                    img.set(x, y, [0.9, 1.0, 0.0, 0.0]);
                }
            }
        }
        a.execute(Command::InsertLayer { layer: Box::new(l), label: "painted copy".into() }).unwrap();
    }
    let copy = a.selected_layer().unwrap().id;
    a.save_smart_material(copy, &lib).unwrap();

    let (loaded, _) = load_smart_materials(&lib);
    let mut b = Session::new(primitives::sample_crate(), 512);
    b.add_smart_material(&loaded[0]).unwrap();
    let LayerContent::Paint(p) = &b.selected_layer().unwrap().content else { panic!("paint layer") };
    let img = &p.channels[&ChannelKind::Roughness];
    assert_eq!((img.width(), img.height()), (512, 512), "resampled to the target resolution");
    assert!((img.get(100, 100)[0] - 0.9).abs() < 2e-3, "painted area kept");
    assert_eq!(img.get(400, 400)[1], 0.0, "unpainted area stays transparent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn smart_mask_saves_effects_and_images() {
    let dir = temp_dir("mask");
    let lib = UserLibrary::new(dir.join("Library"));
    let (a, folder) = session_with_folder(&dir, 128);
    let fill = a.selected_texture_set().find(folder).unwrap().children()[0].id;
    assert!(a.save_smart_mask(folder, &lib).is_err(), "folder has no mask");
    let saved = a.save_smart_mask(fill, &lib).unwrap();
    assert_eq!(saved.effects.len(), 2);

    let (loaded, errors) = load_smart_masks(&lib);
    assert!(errors.is_empty(), "{errors:?}");
    let mut b = Session::new(primitives::cube(1.0), 128);
    b.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    b.apply_smart_mask(&loaded[0]).unwrap();
    let layer = b.selected_layer().unwrap();
    let effects = &layer.mask.as_ref().unwrap().effects;
    assert_eq!(effects.len(), 2);
    let MaskEffectKind::Image { resource, .. } = effects[0].kind else { panic!("image effect first") };
    assert!(b.resources().get(resource).is_some(), "the mask image came with the smart mask");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn broken_library_files_are_skipped() {
    let dir = temp_dir("broken");
    let lib = UserLibrary::new(dir.join("Library"));
    std::fs::create_dir_all(lib.folder(UserAssetKind::SmartMaterial)).unwrap();
    std::fs::write(lib.folder(UserAssetKind::SmartMaterial).join("junk.scsmart"), b"not a zip").unwrap();
    let (ok, errors) = load_smart_materials(&lib);
    assert!(ok.is_empty());
    assert_eq!(errors.len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}
