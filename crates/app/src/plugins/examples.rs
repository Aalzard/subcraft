//! The example plugins in `docs/plugins/examples`: they must load, and the asset files of the
//! "Painted Metal Pack" are made by `write_example_plugin_assets` (run it with `--ignored` after
//! a format change, then commit the files).

use std::path::PathBuf;
use std::sync::Arc;

use subcraft_assets::{Library, UserAssetKind, UserLibrary};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillSource, Layer, LayerContent, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_export::{ExportTemplate, OutputMap, Source};
use subcraft_image::FileFormat;
use subcraft_project::fragment::{Fragment, FragmentKind, save_fragment};

use super::*;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/examples")
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn every_example_plugin_loads_with_its_scripts_and_assets() {
    let found = discover(&examples_dir(), VERSION);
    let names: Vec<String> = found.iter().map(Plugin::name).collect();
    assert_eq!(names, ["Painted Metal Pack", "Weathering Tools"]);
    for p in &found {
        assert!(p.usable(), "{}: {:?}", p.name(), p.problem);
    }
    let metal = &found[0];
    assert_eq!(metal.contents_summary(), "1 script, 2 smart materials, 2 smart masks, 2 export presets");
    let lib = metal.library().unwrap();
    for path in lib.list(UserAssetKind::SmartMaterial) {
        let m = crate::library::load_smart_material_file(&path).unwrap();
        let layer = m.build(&TextureSet::new("T", 0, 64));
        assert!(matches!(layer.content, LayerContent::Group(_)), "{}", m.name);
    }
    for path in lib.list(UserAssetKind::SmartMask) {
        assert!(!crate::library::load_smart_mask_file(&path).unwrap().effects.is_empty());
    }
    // Export presets come in read-only after the built-ins.
    let presets = crate::export_presets::ExportPresets::load(None).with_plugin_presets(std::slice::from_ref(&lib));
    assert!(presets.load_errors.is_empty(), "{:?}", presets.load_errors);
    for name in ["Godot 4 (ORM)", "Roblox SurfaceAppearance"] {
        let p = presets.find(name).unwrap_or_else(|| panic!("{name} is listed"));
        assert!(p.is_builtin(), "plugin presets are read-only");
        assert!(p.problems().is_empty(), "{name}: {:?}", p.problems());
    }
    assert!(presets.user().is_empty());

    let tools = &found[1];
    assert_eq!(tools.contents_summary(), "3 scripts");
}

/// The example scripts are valid Rhai over the real tool catalog: checked by the script runner's
/// tests in `apps/subcraft-mcp` (they run every example script on a demo project).
#[test]
fn example_scripts_are_listed_in_menu_order() {
    let tools = discover(&examples_dir(), VERSION).into_iter().find(|p| p.name() == "Weathering Tools").unwrap();
    let names: Vec<&str> = tools.scripts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Add edge wear", "Add ground grime", "List the layer stack"]);
}

fn set_base_color(layer: &mut Layer, child: &str, rgb: [f32; 3]) {
    let LayerContent::Group(g) = &mut layer.content else { panic!("a folder") };
    let paint = g.children.iter_mut().find(|c| c.name == child).expect("child exists");
    let paint = Arc::make_mut(paint);
    let LayerContent::Fill(f) = &mut paint.content else { panic!("a fill") };
    f.channels.get_mut(&ChannelKind::BaseColor).expect("base color").source = FillSource::uniform(ChannelValue::rgb(rgb[0], rgb[1], rgb[2]));
}

fn smart_material(name: &str, base: &str, paint_rgb: [f32; 3]) -> Fragment {
    let ts = TextureSet::new("T", 0, 1024);
    let builtin = Library::builtin().smart_materials.into_iter().find(|m| m.name == base).expect("built-in smart material");
    let mut layer = builtin.build(&ts);
    layer.name = name.to_string();
    set_base_color(&mut layer, "Paint", paint_rgb);
    Fragment { kind: FragmentKind::SmartMaterial, name: name.into(), resolution: ts.resolution, layers: vec![layer], mask_effects: Vec::new(), resources: ResourceStore::default() }
}

fn smart_mask(name: &str, base: &str, tweak: impl Fn(&mut MaskEffectKind)) -> Fragment {
    let builtin = Library::builtin().smart_masks.into_iter().find(|m| m.name == base).expect("built-in smart mask");
    let effects = builtin
        .effects
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let mut kind = k.clone();
            tweak(&mut kind);
            let (blend, opacity) = builtin.blends.get(i).copied().unwrap_or((subcraft_doc::BlendMode::Normal, 1.0));
            MaskEffect { blend, opacity, ..MaskEffect::new(kind) }
        })
        .collect();
    Fragment { kind: FragmentKind::SmartMask, name: name.into(), resolution: 1024, layers: Vec::new(), mask_effects: effects, resources: ResourceStore::default() }
}

fn roblox() -> ExportTemplate {
    use ChannelKind::*;
    ExportTemplate {
        name: "Roblox SurfaceAppearance".into(),
        maps: vec![
            OutputMap::new("Color", FileFormat::Png8, (0..3).map(|c| Source::ch(BaseColor, c)).collect(), true),
            OutputMap::new("Normal", FileFormat::Png8, (0..3).map(|c| Source::Normal { component: c, directx: false }).collect(), false),
            OutputMap::new("Metalness", FileFormat::Png8, vec![Source::ch(Metallic, 0); 3], false),
            OutputMap::new("Roughness", FileFormat::Png8, vec![Source::ch(Roughness, 0); 3], false),
        ],
        id: Some("6d0f5d1e-7c43-4a8e-b1f0-2f6c5e0a9b31".into()),
        ..Default::default()
    }
}

fn godot() -> ExportTemplate {
    ExportTemplate { name: "Godot 4 (ORM)".into(), id: Some("a3b1f0c2-5e7d-4f9a-8c6b-1d2e3f4a5b60".into()), ..ExportTemplate::gltf() }
}

#[test]
#[ignore = "writes the example plugin's asset files into docs/"]
fn write_example_plugin_assets() {
    let lib = UserLibrary::new(examples_dir().join("painted-metal-pack").join(LIBRARY_DIR));
    for kind in [UserAssetKind::SmartMaterial, UserAssetKind::SmartMask, UserAssetKind::ExportPreset] {
        let _ = std::fs::remove_dir_all(lib.folder(kind));
        std::fs::create_dir_all(lib.folder(kind)).unwrap();
    }
    // Linear colors: a safety yellow and an olive drab paint over the steel.
    for f in [
        smart_material("Hazard Yellow Painted Metal", "Worn Painted Metal", [0.78, 0.5, 0.01]),
        smart_material("Olive Drab Painted Metal", "Worn Painted Metal", [0.1, 0.11, 0.04]),
    ] {
        save_fragment(&lib.folder(UserAssetKind::SmartMaterial).join(format!("{}.scsmart", f.name)), &f).unwrap();
    }
    let heavier = |k: &mut MaskEffectKind| {
        if let MaskEffectKind::Generator(g) = k {
            g.level = (g.level + 0.12).min(1.0);
        }
    };
    let lighter = |k: &mut MaskEffectKind| {
        if let MaskEffectKind::Generator(g) = k {
            g.level = (g.level - 0.12).max(0.0);
        }
    };
    for f in [smart_mask("Heavy Edge Scratches", "Edges Scratched", heavier), smart_mask("Light Dust", "Dust from Above", lighter)] {
        save_fragment(&lib.folder(UserAssetKind::SmartMask).join(format!("{}.scmask", f.name)), &f).unwrap();
    }
    let presets = subcraft_export::preset::PresetLibrary::new(lib.folder(UserAssetKind::ExportPreset));
    for p in [godot(), roblox()] {
        presets.save(&p).unwrap();
    }
}
