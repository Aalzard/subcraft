//! The user's own image textures: import, fill-layer use, color rules, persistence, undo.

use std::path::PathBuf;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::color::srgb_to_linear;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, LayerContent};
use subcraft_export::{ExportSettings, ExportTemplate};
use subcraft_image::{FileFormat, TiledImage, save_image};
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
        let fill = FillChannel::new(FillSource::Bitmap { resource: res });
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

/// Uniform RGB PNG.
fn write_uniform(path: &std::path::Path, rgb: [f32; 3]) {
    let data: Vec<f32> = (0..16 * 16).flat_map(|_| rgb).collect();
    save_image(path, 16, 16, 3, &data, FileFormat::Png8).unwrap();
}

#[test]
fn texture_set_folder_becomes_one_fill_layer() {
    let dir = temp_dir("texset");
    write_uniform(&dir.join("Wood_BaseColor.png"), [0.6, 0.4, 0.2]);
    write_uniform(&dir.join("Wood_Glossiness.png"), [0.2, 0.2, 0.2]);
    write_uniform(&dir.join("Wood_NormalDX.png"), [0.5, 0.3, 1.0]);
    write_uniform(&dir.join("Wood_Height.png"), [0.25, 0.25, 0.25]);
    write_uniform(&dir.join("Wood_AO.png"), [0.9, 0.9, 0.9]);
    write_uniform(&dir.join("Wood_ARM.png"), [1.0, 0.5, 0.0]);
    std::fs::write(dir.join("notes.txt"), "license").unwrap();
    let files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();

    let mut s = Session::new(primitives::sample_crate(), 64);
    let layers_before = s.selected_texture_set().layers.len();
    let entries_before = s.history().entries().len();
    let r = s.import_texture_set(&files).unwrap();

    assert_eq!(r.layer_name, "Wood");
    let mut used: Vec<ChannelKind> = r.used.iter().map(|(_, c)| *c).collect();
    used.sort();
    assert_eq!(
        used,
        [ChannelKind::BaseColor, ChannelKind::Height, ChannelKind::Roughness, ChannelKind::Normal, ChannelKind::AmbientOcclusion]
    );
    let skipped: Vec<&str> = r.skipped.iter().map(|(f, _)| f.as_str()).collect();
    for f in ["Wood_ARM.png", "notes.txt"] {
        assert!(skipped.contains(&f), "{f} should be skipped: {:?}", r.skipped);
    }
    assert!(r.summary().contains("Wood"));

    // One new layer, selected, one history entry.
    assert_eq!(s.selected_texture_set().layers.len(), layers_before + 1);
    assert_eq!(s.history().entries().len(), entries_before + 1);
    let layer = s.selected_layer().unwrap().clone();
    assert_eq!(layer.name, "Wood");
    let LayerContent::Fill(f) = &layer.content else { panic!("fill layer") };
    assert!(!f.channels[&ChannelKind::Metallic].enabled, "channels without a map stay off");
    let bitmap_of = |ch: ChannelKind| match f.channels[&ch].source {
        FillSource::Bitmap { resource } => s.resources().bitmap(resource).unwrap().clone(),
        ref other => panic!("{ch:?}: {other:?}"),
    };
    // Gloss 0.2 -> roughness 0.8; DirectX green 0.3 -> OpenGL 0.7 (8-bit).
    assert!((bitmap_of(ChannelKind::Roughness).texel(0, 0, false)[0] - 0.8).abs() < 2.0 / 255.0);
    let n = bitmap_of(ChannelKind::Normal).texel(0, 0, false);
    assert!((n[1] - 0.7).abs() < 2.0 / 255.0 && (n[0] - 0.5).abs() < 2.0 / 255.0, "{n:?}");

    s.update_composites();
    let tol = 3e-3;
    assert!((texel(&s, ChannelKind::BaseColor, 0.3, 0.3) - srgb_to_linear(0.6)).abs() < tol);
    assert!((texel(&s, ChannelKind::Roughness, 0.3, 0.3) - 0.8).abs() < tol);
    assert!(texel_g(&s, ChannelKind::Normal) > 0.6, "normal tilts toward +Y (OpenGL)");

    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), layers_before, "one undo removes the import");
    assert!(!s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion), "and the AO channel it added");
    std::fs::remove_dir_all(&dir).ok();
}

/// Value at (0, 0) of the selected texture set's exported `suffix` map.
fn exported_texel(s: &Session, dir: &std::path::Path, suffix: &str) -> [f32; 4] {
    let set = s.selected_texture_set().name.clone();
    let path = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().contains(&format!("_{set}_{suffix}")))
        .unwrap_or_else(|| panic!("no {set} {suffix} map in {dir:?}"));
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    subcraft_image::io::decode_bitmap(&std::fs::read(&path).unwrap(), Some(&name)).unwrap().texel(0, 0, false)
}

#[test]
fn imported_ao_map_adds_the_channel_and_reaches_exports() {
    let dir = temp_dir("texset-ao");
    write_uniform(&dir.join("brick_basecolor.png"), [0.6, 0.3, 0.2]);
    write_uniform(&dir.join("brick_ambient_occlusion.png"), [0.4, 0.4, 0.4]);
    let files = vec![dir.join("brick_basecolor.png"), dir.join("brick_ambient_occlusion.png")];

    let mut s = Session::new(primitives::sample_crate(), 128);
    let ts = s.selected_texture_set().id;
    assert!(!s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion), "default template has no AO");
    let baked = std::collections::BTreeMap::from([(MeshMapKind::AmbientOcclusion, TiledImage::new(128, 128, 1, [0.9; 4]))]);
    s.set_mesh_maps(ts, baked);
    let unreal = ExportSettings { template: ExportTemplate::unreal(), ..Default::default() };

    // Without an AO channel the export uses the baked AO.
    let before = dir.join("before");
    s.export_textures(&before, &unreal).unwrap();
    assert!((exported_texel(&s, &before, "OcclusionRoughnessMetallic")[0] - 0.9).abs() < 2.0 / 255.0);

    let entries_before = s.history().entries().len();
    let r = s.import_texture_set(&files).unwrap();
    assert!(r.used.iter().any(|(f, c)| f == "brick_ambient_occlusion.png" && *c == ChannelKind::AmbientOcclusion), "{r:?}");
    assert!(s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion), "import added the channel");
    assert_eq!(s.history().entries().len(), entries_before + 1, "channel + layer are one undo step");
    s.update_composites();
    let ao = texel(&s, ChannelKind::AmbientOcclusion, 0.3, 0.3);
    assert!((ao - 0.4).abs() < 3e-3, "composite AO {ao}");

    // The texture's AO, not the baked one, goes into Unreal's packed map and Unity's mask map.
    let after = dir.join("after");
    s.export_textures(&after, &unreal).unwrap();
    assert!((exported_texel(&s, &after, "OcclusionRoughnessMetallic")[0] - 0.4).abs() < 2.0 / 255.0);
    let unity = dir.join("unity");
    s.export_textures(&unity, &ExportSettings { template: ExportTemplate::unity_hdrp(), ..Default::default() }).unwrap();
    assert!((exported_texel(&s, &unity, "MaskMap")[1] - 0.4).abs() < 2.0 / 255.0);

    // Saved with the project.
    let project = dir.join("ao.subcraft");
    s.save(Some(&project)).unwrap();
    let reopened = Session::open_project(&project).unwrap();
    assert!(reopened.document().texture_set(ts).unwrap().has_channel(ChannelKind::AmbientOcclusion));

    // Undo removes channel and layer; the baked AO is used again.
    s.execute(Command::Undo).unwrap();
    assert!(!s.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion));
    let undone = dir.join("undone");
    s.export_textures(&undone, &unreal).unwrap();
    assert!((exported_texel(&s, &undone, "OcclusionRoughnessMetallic")[0] - 0.9).abs() < 2.0 / 255.0);
    std::fs::remove_dir_all(&dir).ok();
}

fn texel_g(s: &Session, ch: ChannelKind) -> f32 {
    let ts = s.selected_texture_set().id;
    let img = s.composite(ts).unwrap().channel(ch).unwrap();
    img.get(img.width() / 3, img.height() / 3)[1]
}

#[test]
fn texture_set_without_recognizable_maps_is_an_error() {
    let dir = temp_dir("texset-none");
    write_uniform(&dir.join("preview.png"), [0.5; 3]);
    let mut s = Session::new(primitives::cube(1.0), 64);
    let before = s.document().clone();
    let err = s.import_texture_set(&[dir.join("preview.png")]).unwrap_err();
    assert!(err.contains("no texture maps recognized"), "{err}");
    assert_eq!(s.document(), &before, "nothing changes");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn texture_dropped_on_a_channel_keeps_strength_and_converts_by_name() {
    let dir = temp_dir("drop");
    write_uniform(&dir.join("bricks_normal_dx.png"), [0.5, 0.3, 1.0]);
    write_uniform(&dir.join("photo.png"), [0.5, 0.3, 1.0]);
    let mut s = Session::new(primitives::sample_crate(), 64);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    // Give Normal a strength first: dropping a texture must keep it.
    let mut n = match &s.selected_layer().unwrap().content {
        LayerContent::Fill(f) => f.channels[&ChannelKind::Normal].clone(),
        _ => unreachable!(),
    };
    n.strength = 2.0;
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Normal, fill: n }).unwrap();

    s.set_fill_channel_image(layer, ChannelKind::Normal, &dir.join("bricks_normal_dx.png")).unwrap();
    let ch = |s: &Session, k: ChannelKind| match &s.selected_layer().unwrap().content {
        LayerContent::Fill(f) => f.channels[&k].clone(),
        _ => unreachable!(),
    };
    let normal = ch(&s, ChannelKind::Normal);
    assert!(normal.enabled);
    assert_eq!(normal.strength, 2.0, "strength kept");
    let FillSource::Bitmap { resource } = normal.source else { panic!("image source") };
    let g = s.resources().bitmap(resource).unwrap().texel(0, 0, false)[1];
    assert!((g - 0.7).abs() < 2.0 / 255.0, "DirectX normal converted on the Normal channel: {g}");

    // A name that doesn't say "normal" is used as-is.
    s.set_fill_channel_image(layer, ChannelKind::Normal, &dir.join("photo.png")).unwrap();
    let FillSource::Bitmap { resource } = ch(&s, ChannelKind::Normal).source else { panic!() };
    assert!((s.resources().bitmap(resource).unwrap().texel(0, 0, false)[1] - 0.3).abs() < 2.0 / 255.0);

    // Paint layers have no channels to drop onto.
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let paint = s.selected_layer().unwrap().id;
    assert!(s.set_fill_channel_image(paint, ChannelKind::BaseColor, &dir.join("photo.png")).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn texture_added_as_a_layer_uses_the_channel_its_name_suggests() {
    let dir = temp_dir("texlayer");
    write_uniform(&dir.join("wall_roughness.png"), [0.4; 3]);
    write_uniform(&dir.join("holiday photo.png"), [0.2, 0.4, 0.6]);
    let mut s = Session::new(primitives::sample_crate(), 64);
    let enabled = |s: &Session| match &s.selected_layer().unwrap().content {
        LayerContent::Fill(f) => f.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect::<Vec<_>>(),
        _ => panic!("fill layer"),
    };
    s.add_texture_layer(&dir.join("wall_roughness.png")).unwrap();
    assert_eq!(enabled(&s), [ChannelKind::Roughness]);
    let name = s.add_texture_layer(&dir.join("holiday photo.png")).unwrap();
    assert_eq!(name, "holiday photo");
    assert_eq!(enabled(&s), [ChannelKind::BaseColor], "unknown names become base color");
    std::fs::remove_dir_all(&dir).ok();
}
