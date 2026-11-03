//! Importing smart materials: `.scsmart` files, folders, and ambientCG-style PBR texture sets;
//! exporting them again; and never panicking on bad files.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use subcraft_app::Session;
use subcraft_app::assets::{SmartMaterial, UserAssetKind, UserLibrary};
use subcraft_app::smart_import::{export_smart_material, import_smart_materials};
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, LayerContent};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;
use subcraft_project::fragment::{Fragment, FragmentKind, save_fragment};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-smartimport-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A 8x8 RGB PNG of one color.
fn png(path: &Path, rgb: [f32; 3]) {
    let data: Vec<f32> = (0..64).flat_map(|_| rgb).collect();
    save_image(path, 8, 8, 3, &data, FileFormat::Png8).unwrap();
}

/// An ambientCG-style download: two materials side by side, one with both normal flavours.
fn make_pack(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (map, rgb) in [
        ("Color", [0.6, 0.2, 0.1]),
        ("NormalDX", [0.5, 0.2, 1.0]),
        ("NormalGL", [0.5, 0.8, 1.0]),
        ("Roughness", [0.7, 0.7, 0.7]),
        ("Metalness", [0.0, 0.0, 0.0]),
        ("AmbientOcclusion", [0.9, 0.9, 0.9]),
        ("Displacement", [0.4, 0.4, 0.4]),
        ("Opacity", [1.0, 1.0, 1.0]),
    ] {
        png(&dir.join(format!("Bricks059_2K-PNG_{map}.png")), rgb);
    }
    // Only a DirectX normal here: it must be flipped to OpenGL.
    for (map, rgb) in [("Color", [0.4, 0.3, 0.1]), ("NormalDX", [0.5, 0.2, 1.0]), ("Roughness", [0.5, 0.5, 0.5])] {
        png(&dir.join(format!("Wood001_2K-PNG_{map}.png")), rgb);
    }
    std::fs::write(dir.join("README.txt"), "not a texture").unwrap();
    png(&dir.join("holiday photo.png"), [0.1, 0.2, 0.3]);
}

fn names(v: &[SmartMaterial]) -> Vec<String> {
    let mut n: Vec<String> = v.iter().map(|m| m.name.clone()).collect();
    n.sort();
    n
}

fn channel_resource(m: &SmartMaterial, ch: ChannelKind) -> Option<std::sync::Arc<subcraft_doc::Resource>> {
    let mut s = Session::new(primitives::sample_crate(), 128);
    s.add_smart_material(m).unwrap();
    let layer = s.selected_layer().unwrap().clone();
    let LayerContent::Fill(f) = &layer.content else { panic!("fill layer") };
    let c = f.channels.get(&ch)?;
    if !c.enabled {
        return None;
    }
    match c.source {
        FillSource::Bitmap { resource } => s.resources().get(resource).cloned(),
        _ => None,
    }
}

#[test]
fn ambientcg_folder_becomes_one_smart_material_per_material() {
    let root = temp_dir("pack");
    make_pack(&root.join("downloads"));
    let lib = UserLibrary::new(root.join("library"));

    let report = import_smart_materials(&[root.join("downloads")], &lib, &[]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(names(&report.imported), ["Bricks059", "Wood001"], "resolution/format words are not part of the name");
    assert_eq!(lib.list(UserAssetKind::SmartMaterial).len(), 2, "saved in the user library");

    let bricks = report.imported.iter().find(|m| m.name == "Bricks059").unwrap();
    for ch in [
        ChannelKind::BaseColor,
        ChannelKind::Normal,
        ChannelKind::Roughness,
        ChannelKind::Metallic,
        ChannelKind::AmbientOcclusion,
        ChannelKind::Height,
        ChannelKind::Opacity,
    ] {
        assert!(channel_resource(bricks, ch).is_some(), "{ch:?} filled from its file");
    }
    // The OpenGL normal wins over the DirectX one: green stays 0.8.
    let normal = channel_resource(bricks, ChannelKind::Normal).unwrap();
    assert!(normal.name.contains("NormalGL"), "{}", normal.name);
    assert!((normal.bitmap.texel(1, 1, false)[1] - 0.8).abs() < 0.01);

    // A lone DirectX normal is converted: green 0.2 -> 0.8.
    let wood = report.imported.iter().find(|m| m.name == "Wood001").unwrap();
    let normal = channel_resource(wood, ChannelKind::Normal).unwrap();
    assert!((normal.bitmap.texel(1, 1, false)[1] - 0.8).abs() < 0.01, "green flipped");
    assert!(channel_resource(wood, ChannelKind::Metallic).is_none());

    // Applying to a set that lacks AO / opacity adds those channels (one undo step).
    let mut s = Session::new(primitives::sample_crate(), 128);
    assert!(!s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion));
    s.add_smart_material(bricks).unwrap();
    assert!(s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion));
    assert!(s.selected_texture_set().has_channel(ChannelKind::Opacity));

    // Importing the same folder again adds nothing.
    let again = import_smart_materials(&[root.join("downloads")], &lib, &report.imported);
    assert!(again.imported.is_empty());
    assert_eq!(again.already_present.len(), 2);
    assert_eq!(lib.list(UserAssetKind::SmartMaterial).len(), 2);
}

#[test]
fn texture_files_dropped_directly_and_nested_folders() {
    let root = temp_dir("nested");
    // Files named only by map type: the material takes the folder's name.
    let dir = root.join("in").join("deep").join("Mossy Rock");
    std::fs::create_dir_all(&dir).unwrap();
    png(&dir.join("albedo.png"), [0.2, 0.4, 0.2]);
    png(&dir.join("normal_dx.png"), [0.5, 0.5, 1.0]);
    png(&dir.join("gloss.png"), [0.25, 0.25, 0.25]);
    let lib = UserLibrary::new(root.join("library"));
    let report = import_smart_materials(&[root.join("in")], &lib, &[]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(names(&report.imported), ["Mossy Rock"]);
    // Glossiness inverted to roughness: 0.25 -> 0.75.
    let rough = channel_resource(&report.imported[0], ChannelKind::Roughness).unwrap();
    assert!((rough.bitmap.texel(0, 0, false)[0] - 0.75).abs() < 0.01);

    // Individual image files work too.
    let lib2 = UserLibrary::new(root.join("library2"));
    let files = [dir.join("albedo.png"), dir.join("gloss.png")];
    let report = import_smart_materials(&files, &lib2, &[]);
    assert_eq!(report.imported.len(), 1, "{:?}", report.errors);
}

#[test]
fn smart_material_file_round_trips_through_export_and_import() {
    let root = temp_dir("roundtrip");
    make_pack(&root.join("downloads"));
    let lib_a = UserLibrary::new(root.join("lib_a"));
    let made = import_smart_materials(&[root.join("downloads")], &lib_a, &[]).imported;

    // Export both, one without the extension typed.
    let share = root.join("share");
    std::fs::create_dir_all(&share).unwrap();
    let bricks = made.iter().find(|m| m.name == "Bricks059").unwrap();
    let wood = made.iter().find(|m| m.name == "Wood001").unwrap();
    export_smart_material(bricks, &share.join("Bricks059.scsmart")).unwrap();
    export_smart_material(wood, &share.join("Wood")).unwrap();
    assert!(share.join("Wood.scsmart").is_file());

    // Import the folder of shared files into a fresh library.
    let lib_b = UserLibrary::new(root.join("lib_b"));
    let report = import_smart_materials(&[share.clone()], &lib_b, &[]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(names(&report.imported), ["Bricks059", "Wood"]);
    let back = report.imported.iter().find(|m| m.name == "Bricks059").unwrap();
    let a = channel_resource(bricks, ChannelKind::Roughness).unwrap();
    let b = channel_resource(back, ChannelKind::Roughness).unwrap();
    assert_eq!(a.encoded, b.encoded, "image bytes survive the round trip");
    assert!(channel_resource(back, ChannelKind::AmbientOcclusion).is_some());

    // Importing the same file again: recognised as the same, not duplicated.
    let again = import_smart_materials(&[share.join("Bricks059.scsmart")], &lib_b, &report.imported);
    assert!(again.imported.is_empty() && again.already_present == ["Bricks059"], "{:?} {:?}", again.errors, again.already_present);
    assert_eq!(lib_b.list(UserAssetKind::SmartMaterial).len(), 2);

    // A different material with a taken name is kept under a numbered name.
    export_smart_material(wood, &share.join("Bricks059.scsmart")).unwrap();
    let clash = import_smart_materials(&[share.join("Bricks059.scsmart")], &lib_b, &report.imported);
    assert_eq!(names(&clash.imported), ["Bricks059 (2)"], "{:?}", clash.errors);
    assert!(clash.notes.iter().any(|n| n.contains("Bricks059 (2)")), "{:?}", clash.notes);
    assert_eq!(lib_b.list(UserAssetKind::SmartMaterial).len(), 3);

    // Built-in smart materials have no file to share.
    let builtin = subcraft_app::assets::Library::builtin().smart_materials.remove(0);
    assert!(export_smart_material(&builtin, &share.join("x.scsmart")).is_err());
}

/// Rewrite a zip, passing every entry through `f(name, bytes)`.
fn rewrite_zip(src: &Path, dest: &Path, f: impl Fn(&str, Vec<u8>) -> Vec<u8>) {
    let mut zip = zip::ZipArchive::new(Cursor::new(std::fs::read(src).unwrap())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).unwrap();
        let name = e.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        e.read_to_end(&mut bytes).unwrap();
        out.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&f(&name, bytes)).unwrap();
    }
    std::fs::write(dest, out.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn bad_files_are_reported_never_fatal_and_leave_the_library_alone() {
    let root = temp_dir("bad");
    make_pack(&root.join("downloads"));
    let lib_a = UserLibrary::new(root.join("lib_a"));
    let good = import_smart_materials(&[root.join("downloads")], &lib_a, &[]).imported;
    let valid = good[0].user_file.clone().unwrap();

    let bad = root.join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("garbage.scsmart"), b"this is not a zip").unwrap();
    std::fs::write(bad.join("empty.scsmart"), b"").unwrap();
    let bytes = std::fs::read(&valid).unwrap();
    std::fs::write(bad.join("truncated.scsmart"), &bytes[..bytes.len() / 2]).unwrap();
    // Saved by a newer SubCraft.
    rewrite_zip(&valid, &bad.join("future.scsmart"), |name, b| {
        if name == "fragment.json" {
            String::from_utf8(b).unwrap().replace("\"schema_version\": 1", "\"schema_version\": 999").into_bytes()
        } else {
            b
        }
    });
    // Points its images outside the container.
    rewrite_zip(&valid, &bad.join("traversal.scsmart"), |name, b| {
        if name == "fragment.json" {
            String::from_utf8(b).unwrap().replace("resources/", "../../evil/").into_bytes()
        } else {
            b
        }
    });
    // A smart mask is not a smart material.
    let mask = Fragment {
        kind: FragmentKind::SmartMask,
        name: "m".into(),
        resolution: 128,
        layers: Vec::new(),
        mask_effects: Vec::new(),
        resources: Default::default(),
    };
    save_fragment(&bad.join("mask.scsmart"), &mask).unwrap();
    std::fs::write(bad.join("notes.scmask"), b"x").unwrap();
    // A texture bigger than the limit (8193 x 1 is cheap to make).
    std::fs::create_dir_all(bad.join("huge")).unwrap();
    save_image(&bad.join("huge").join("Big_Color.png"), 8193, 1, 3, &vec![0.5; 8193 * 3], FileFormat::Png8).unwrap();

    let lib_b = UserLibrary::new(root.join("lib_b"));
    let paths: Vec<PathBuf> = ["garbage", "empty", "truncated", "future", "traversal", "mask"].iter().map(|n| bad.join(format!("{n}.scsmart"))).collect();
    let mut all = paths.clone();
    all.push(bad.join("notes.scmask"));
    all.push(bad.join("does-not-exist.scsmart"));
    all.push(bad.join("huge"));
    let report = import_smart_materials(&all, &lib_b, &[]);
    assert!(report.imported.is_empty(), "{:?}", names(&report.imported));
    assert_eq!(report.errors.len(), all.len(), "one clear line per bad input: {:#?}", report.errors);
    let text = report.errors.join("\n");
    assert!(text.contains("newer SubCraft"), "version check: {text}");
    assert!(text.contains("missing entry"), "traversal entry is not read: {text}");
    assert!(text.contains("smart mask"), "{text}");
    assert!(text.contains("larger than 8192"), "{text}");
    assert!(lib_b.list(UserAssetKind::SmartMaterial).is_empty(), "nothing partial was written");
    assert!(!root.join("evil").exists() && !root.parent().unwrap().join("evil").exists());
    assert!(report.summary().contains("problem"));

    // The valid file next to the broken ones still imports.
    let mut with_good = all.clone();
    with_good.push(valid);
    let report = import_smart_materials(&with_good, &lib_b, &[]);
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.errors.len(), all.len());
}

#[test]
fn nothing_recognizable_says_so() {
    let root = temp_dir("empty");
    let dir = root.join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    png(&dir.join("IMG_0001.png"), [0.3, 0.3, 0.3]);
    let lib = UserLibrary::new(root.join("library"));
    let report = import_smart_materials(&[dir], &lib, &[]);
    assert!(report.imported.is_empty() && report.errors.is_empty());
    assert!(report.summary().starts_with("Nothing to import"));
    assert!(!report.notes.is_empty(), "ignored images are mentioned");
}
