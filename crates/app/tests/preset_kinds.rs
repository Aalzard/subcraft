//! Tool, brush and material presets: what each kind stores and changes, updating, renaming,
//! duplicating and favorites in the user library.
use std::path::PathBuf;
use std::sync::Arc;

use subcraft_app::Session;
use subcraft_app::assets::{UserAssetKind, UserLibrary, alphas};
use subcraft_app::brush_preset::{
    BrushPreset, Favorites, PresetKind, PresetTool, ShelfBrush, duplicate_brush_preset, favorite_key, load_brush_presets, rename_brush_preset,
};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_image::Bitmap;
use subcraft_mesh::primitives;
use subcraft_paint::{Stencil, StencilTiling};

fn temp_lib(name: &str) -> (PathBuf, UserLibrary) {
    let d = std::env::temp_dir().join(format!("subcraft-preset-kinds-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    (d.clone(), UserLibrary::new(d))
}

/// A session whose whole tool differs from the defaults: tip, settings, material and stencil.
fn configured() -> Session {
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::cube(1.0), 64);
    s.set_brush_alpha(tips.iter().find(|t| t.name == "Splatter").unwrap());
    s.brush.settings.size = 12.5;
    s.brush.settings.flow = 0.35;
    s.brush.settings.size_jitter = 0.4;
    s.brush.options.lazy_distance_px = 30.0;
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.1, 0.6, 0.2));
    s.brush.material.set(ChannelKind::Roughness, ChannelValue::scalar(0.9));
    let checker = Bitmap::new_u8(2, 2, 1, vec![255, 0, 0, 255]);
    s.brush.stencil = Some(Arc::new(Stencil { center: glam::Vec2::new(0.3, 0.6), scale: 1.5, rotation: 0.5, tiling: StencilTiling::Horizontal, ..Stencil::new(Arc::new(checker)) }));
    s
}

fn reload(lib: &UserLibrary, name: &str) -> BrushPreset {
    let (loaded, errors) = load_brush_presets(lib);
    assert!(errors.is_empty(), "{errors:?}");
    loaded.into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no preset {name}"))
}

#[test]
fn a_tool_preset_restores_brush_alpha_stencil_material_and_names_its_tool() {
    let (dir, lib) = temp_lib("tool");
    let tips = alphas::builtin();
    let mut a = configured();
    let (saved, _) = a.create_preset(&lib, "Mossy", PresetKind::Tool, Some(PresetTool::Smudge)).unwrap();
    assert_eq!((saved.kind, saved.tool, saved.version), (PresetKind::Tool, Some(PresetTool::Smudge), 3));
    let p = reload(&lib, "Mossy");

    let mut b = Session::new(primitives::cube(1.0), 64);
    let selected = b.select_shelf_brush(ShelfBrush::Preset(&p), &tips).unwrap();
    assert_eq!((selected.tool, selected.kind), (Some(PresetTool::Smudge), Some(PresetKind::Tool)));
    assert_eq!(b.brush.settings, a.brush.settings);
    assert_eq!(b.brush.options, a.brush.options);
    assert_eq!(b.brush.material, a.brush.material);
    assert_eq!(b.brush.alpha_name, "Splatter");
    let (sa, sb) = (a.brush.stencil.as_ref().unwrap(), b.brush.stencil.as_ref().expect("the stencil is part of a tool preset"));
    assert_eq!((sb.center, sb.scale, sb.rotation, sb.tiling), (sa.center, sa.scale, sa.rotation, sa.tiling));
    assert_eq!((sb.bitmap.width, sb.bitmap.height), (2, 2));
    for px in [glam::Vec2::new(1.0, 1.0), glam::Vec2::new(6.0, 2.0), glam::Vec2::new(3.0, 7.0)] {
        assert!((sb.value_at(px, [8, 8]) - sa.value_at(px, [8, 8])).abs() < 0.01, "same image at {px:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_brush_preset_leaves_the_material_and_a_material_preset_leaves_the_brush() {
    let (dir, lib) = temp_lib("parts");
    let tips = alphas::builtin();
    let mut a = configured();
    a.create_preset(&lib, "Only brush", PresetKind::Brush, Some(PresetTool::Paint)).unwrap();
    a.create_preset(&lib, "Only material", PresetKind::Material, Some(PresetTool::Paint)).unwrap();
    let (brush, material) = (reload(&lib, "Only brush"), reload(&lib, "Only material"));
    assert_eq!((brush.tool, material.tool), (None, None), "only tool presets store a tool");
    assert!(brush.material.channels.is_empty() && brush.images.is_empty(), "a brush preset stores no material");
    assert!(brush.stencil.is_some() && material.stencil.is_none() && material.tip.is_none());

    // Brush preset onto a fresh session: brush, alpha and stencil arrive; the material stays.
    let mut b = Session::new(primitives::cube(1.0), 64);
    b.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.9, 0.9, 0.0));
    let kept = b.brush.material.clone();
    let selected = b.select_shelf_brush(ShelfBrush::Preset(&brush), &tips).unwrap();
    assert_eq!((selected.tool, selected.kind), (None, Some(PresetKind::Brush)));
    assert_eq!(b.brush.settings, a.brush.settings);
    assert_eq!(b.brush.alpha_name, "Splatter");
    assert!(b.brush.stencil.is_some());
    assert_eq!(b.brush.material, kept, "the material in use is untouched");

    // Material preset: only the material changes.
    let mut c = Session::new(primitives::cube(1.0), 64);
    c.brush.settings.size = 33.0;
    let (settings, options, tip) = (c.brush.settings, c.brush.options, c.brush.alpha_name.clone());
    c.select_shelf_brush(ShelfBrush::Preset(&material), &tips).unwrap();
    assert_eq!(c.brush.material, a.brush.material);
    assert_eq!((c.brush.settings, c.brush.options, c.brush.alpha_name.clone()), (settings, options, tip));
    assert!(c.brush.stencil.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_preset_made_without_a_stencil_removes_the_stencil_and_older_files_leave_it() {
    let (dir, lib) = temp_lib("stencil");
    let tips = alphas::builtin();
    let mut plain = Session::new(primitives::cube(1.0), 64);
    let (no_stencil, _) = plain.create_preset(&lib, "Plain", PresetKind::Brush, None).unwrap();
    let mut s = configured();
    s.apply_brush_preset(&no_stencil, &tips);
    assert!(s.brush.stencil.is_none(), "the preset says: no stencil");

    // A version 2 file knows nothing about stencils.
    let folder = lib.folder(UserAssetKind::BrushPreset);
    std::fs::write(folder.join("Old.scbrush"), r#"{"format":"subcraft-brush","version":2,"name":"x","tip":null,"settings":{"size":20.0}}"#).unwrap();
    let old = reload(&lib, "Old");
    assert_eq!((old.kind, old.tool), (PresetKind::Tool, None), "older files are tool presets without a tool");
    let mut s = configured();
    s.apply_brush_preset(&old, &tips);
    assert!(s.brush.stencil.is_some());
    assert_eq!(s.brush.settings.size, 20.0);
    // So does the legacy "current brush as a preset".
    assert_eq!(configured().brush_preset("x").version, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_from_current_tool_overwrites_the_same_file_and_keeps_kind_and_name() {
    let (dir, lib) = temp_lib("update");
    let mut a = configured();
    let (saved, _) = a.create_preset(&lib, "Evolving", PresetKind::Tool, Some(PresetTool::Paint)).unwrap();
    a.brush.settings.size = 44.0;
    a.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 0.0, 1.0));
    a.brush.stencil = None;
    let (updated, _) = a.update_preset(&lib, &saved, Some(PresetTool::Eraser)).unwrap();
    assert_eq!((updated.user_file.clone(), updated.name.as_str(), updated.kind), (saved.user_file.clone(), "Evolving", PresetKind::Tool));
    assert_eq!(updated.tool, Some(PresetTool::Eraser), "a tool preset takes the tool in use");
    assert_eq!(lib.list(UserAssetKind::BrushPreset).len(), 1, "no second file");
    let on_disk = reload(&lib, "Evolving");
    assert_eq!(on_disk.settings.size, 44.0);
    assert_eq!(on_disk.material, a.brush.material);
    assert!(on_disk.stencil.is_none());

    // A material preset stays a material preset: the brush change is not stored.
    let (material, _) = a.create_preset(&lib, "Paint", PresetKind::Material, None).unwrap();
    a.brush.settings.size = 3.0;
    a.brush.material.set(ChannelKind::Roughness, ChannelValue::scalar(0.05));
    let (updated, _) = a.update_preset(&lib, &material, Some(PresetTool::Paint)).unwrap();
    assert_eq!((updated.kind, updated.tool), (PresetKind::Material, None));
    assert_eq!(updated.settings, subcraft_paint::BrushSettings::default());
    assert_eq!(reload(&lib, "Paint").material, a.brush.material);

    // Built-in presets have no file to overwrite.
    let builtin = subcraft_app::brush_preset::builtin().remove(0);
    assert!(a.update_preset(&lib, &builtin, None).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_moves_the_file_and_duplicate_adds_a_copy() {
    let (dir, lib) = temp_lib("rename");
    let mut a = configured();
    let (saved, _) = a.create_preset(&lib, "First", PresetKind::Brush, None).unwrap();
    let renamed = rename_brush_preset(&lib, &saved, "  Second  ").unwrap();
    assert_eq!(renamed.name, "Second");
    let files: Vec<String> = lib.list(UserAssetKind::BrushPreset).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["Second.scbrush"], "the old file is gone");
    assert_eq!(reload(&lib, "Second").settings, saved.settings);
    assert!(rename_brush_preset(&lib, &renamed, "   ").is_err(), "an empty name is refused");
    assert_eq!(rename_brush_preset(&lib, &renamed, "Second").unwrap(), renamed, "the same name changes nothing");
    assert_eq!(lib.list(UserAssetKind::BrushPreset).len(), 1);

    let copy = duplicate_brush_preset(&lib, &renamed).unwrap();
    assert_eq!(copy.name, "Second copy");
    assert_ne!(copy.user_file, renamed.user_file);
    assert_eq!((copy.settings, copy.kind, copy.stencil.clone()), (renamed.settings, renamed.kind, renamed.stencil.clone()));
    // A name already taken is numbered, not overwritten.
    let third = rename_brush_preset(&lib, &copy, "Second").unwrap();
    assert_eq!(third.name, "Second (2)");
    // Built-ins: duplicate works (it becomes a library preset), rename does not.
    let builtin = subcraft_app::brush_preset::builtin().remove(0);
    assert!(rename_brush_preset(&lib, &builtin, "Mine").is_err());
    let mine = duplicate_brush_preset(&lib, &builtin).unwrap();
    assert!(mine.user_file.is_some() && mine.name == format!("{} copy", builtin.name));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn favorites_persist_in_the_library_and_follow_a_rename() {
    let (dir, lib) = temp_lib("favorites");
    assert!(Favorites::load(&lib).is_empty(), "no file yet");
    let mut f = Favorites::default();
    let (a, b) = (favorite_key("brush", "Dirt Grunge"), favorite_key("brush", "Mine"));
    assert!(f.toggle(&a) && f.toggle(&b));
    f.save(&lib).unwrap();
    let mut loaded = Favorites::load(&lib);
    assert_eq!(loaded, f);
    assert!(loaded.contains(&a) && loaded.len() == 2);
    loaded.rename(&b, &favorite_key("brush", "Yours"));
    assert!(!loaded.contains(&b) && loaded.contains(&favorite_key("brush", "Yours")));
    loaded.rename("brush/Nobody", "brush/Somebody");
    assert_eq!(loaded.len(), 2, "renaming a non-favorite adds nothing");
    assert!(!loaded.toggle(&a), "toggling again removes it");
    loaded.save(&lib).unwrap();
    assert_eq!(Favorites::load(&lib).len(), 1);
    // A damaged file is an empty list, not an error.
    std::fs::write(lib.root().join("favorites.json"), "{").unwrap();
    assert!(Favorites::load(&lib).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// QA 2026-11: renaming to a name that is stored as the same file ("Rust." becomes "Rust") came
/// back as "Rust (2)", because the preset's own old file counted as taken.
#[test]
fn renaming_to_a_name_with_the_same_file_keeps_one_unnumbered_file() {
    let (dir, lib) = temp_lib("rename-same-file");
    let mut a = configured();
    let (saved, _) = a.create_preset(&lib, "Rust", PresetKind::Brush, None).unwrap();
    let renamed = rename_brush_preset(&lib, &saved, "Rust.").unwrap();
    assert_eq!(renamed.name, "Rust");
    let files: Vec<String> = lib.list(UserAssetKind::BrushPreset).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["Rust.scbrush"]);
    assert_eq!(reload(&lib, "Rust").settings, saved.settings);
    std::fs::remove_dir_all(&dir).ok();
}
