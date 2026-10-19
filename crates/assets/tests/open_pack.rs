//! The CC0 open pack: every embedded image decodes, materials fill the right channels, the
//! panoramas prefilter, and smart materials built on the pack carry their images.
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;

use subcraft_assets::presets::decode_images;
use subcraft_assets::{EnvironmentSource, ImageIds, Library, open_pack};
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, LayerContent, TextureSet};

#[test]
fn every_material_image_decodes_at_1k_with_data_maps_in_gray() {
    let materials = open_pack::materials();
    assert_eq!(materials.len(), 10);
    let mut names = BTreeSet::new();
    let mut bytes = 0;
    for m in &materials {
        let channels: BTreeSet<_> = m.images.iter().map(|i| i.channel).collect();
        for k in [ChannelKind::BaseColor, ChannelKind::Roughness, ChannelKind::Normal, ChannelKind::Height] {
            assert!(channels.contains(&k), "{} lacks {k:?}", m.name);
        }
        for img in &m.images {
            assert!(names.insert(img.name), "duplicate image name {}", img.name);
            bytes += img.bytes.len();
            let b = subcraft_image::io::decode_bitmap(img.bytes, Some(img.name)).unwrap();
            assert_eq!(b.width, 1024, "{}", img.name);
            // Texels stay square: tiling compensates 2:1 images.
            let aspect = m.projection.scale[1] / m.projection.scale[0];
            assert_eq!(aspect, b.width as f32 / b.height as f32, "{}", img.name);
            let expected = if matches!(img.channel, ChannelKind::BaseColor | ChannelKind::Normal) { 3 } else { 1 };
            assert_eq!(b.channels, expected, "{}", img.name);
        }
    }
    assert!(bytes < 9_000_000, "pack materials grew to {bytes} bytes");
}

#[test]
fn image_materials_fill_their_channels_only_once_the_images_are_imported() {
    let ts = TextureSet::new("T", 0, 64);
    let m = open_pack::materials().into_iter().find(|m| m.name == "Metal Plates").unwrap();
    let fill = |layer: subcraft_doc::Layer| match layer.content {
        LayerContent::Fill(f) => f,
        _ => panic!("fill layer"),
    };
    let without = fill(m.build(&ts));
    assert!(!without.channels[&ChannelKind::BaseColor].enabled, "no ids, no image channel");

    let (ids, resources) = decode_images(&m.images);
    assert_eq!(resources.len(), m.images.len());
    let with = fill(m.build_with(&ts, &ids));
    for img in &m.images {
        let ch = &with.channels[&img.channel];
        assert!(ch.enabled);
        assert!(matches!(ch.source, FillSource::Bitmap { resource } if resource == ids[img.name]));
        assert_eq!(ch.strength, img.strength);
    }
    assert!(with.channels[&ChannelKind::Height].strength < 0.2, "displacement is a gentle bump");
}

#[test]
fn plain_materials_without_metalness_maps_are_dielectric() {
    let ts = TextureSet::new("T", 0, 64);
    let m = open_pack::materials().into_iter().find(|m| m.name == "Bricks").unwrap();
    let LayerContent::Fill(f) = m.build(&ts).content else { panic!() };
    let metal = &f.channels[&ChannelKind::Metallic];
    assert!(metal.enabled);
    assert!(matches!(&metal.source, FillSource::Uniform { value } if value.0[0] == 0.0), "{:?}", metal.source);
}

#[test]
fn panoramas_prefilter_lazily_into_a_lit_environment() {
    let envs = open_pack::environments();
    assert_eq!(envs.len(), 4);
    for e in &envs {
        assert!(matches!(e.source, EnvironmentSource::Embedded(_)));
        let h = e.source.hdri().unwrap().unwrap_or_else(|err| panic!("{}: {err}", e.name));
        assert!(h.levels.len() > 3, "{}", e.name);
        assert!(h.levels[0].width == 2 * h.levels[0].height, "{} is equirectangular", e.name);
        assert!(h.sh[0].iter().all(|&c| c > 0.0 && c.is_finite()), "{} has ambient light: {:?}", e.name, h.sh[0]);
        // The second call reuses the prefiltered panorama.
        assert!(std::sync::Arc::ptr_eq(&h, &e.source.hdri().unwrap().unwrap()));
    }
}

#[test]
fn library_lists_the_pack_and_textured_smart_materials_use_its_images() {
    let lib = Library::builtin();
    assert!(lib.materials.iter().any(|m| m.name == "Woven Fabric"));
    assert!(lib.environments.iter().any(|e| e.name == "Sunrise Field (HDRI)"));
    for name in ["Heavy Edge Wear", "Moss on Top", "Scratches"] {
        assert!(lib.smart_masks.iter().any(|m| m.name == name), "{name}");
    }
    let ids: BTreeSet<_> = lib.materials.iter().map(|m| m.id).chain(lib.smart_materials.iter().map(|m| m.id)).chain(lib.environments.iter().map(|e| e.id)).collect();
    assert_eq!(ids.len(), lib.materials.len() + lib.smart_materials.len() + lib.environments.len(), "unique ids");

    let ts = TextureSet::new("T", 0, 64);
    let smart = lib.smart_materials.iter().find(|m| m.name == "Mossy Bricks").unwrap();
    assert_eq!(smart.images().len(), 4);
    let (ids, _) = decode_images(smart.images());
    let layer = smart.build_with(&ts, &ids);
    assert_eq!(layer.name, "Mossy Bricks");
    let children = layer.children();
    assert_eq!(children.len(), 3, "bricks, grime, moss");
    let LayerContent::Fill(base) = &children[0].content else { panic!() };
    assert!(matches!(base.channels[&ChannelKind::BaseColor].source, FillSource::Bitmap { .. }));
    assert!(children[1].mask.is_some() && children[2].mask.is_some());
    // Without imported images the base keeps its image channels off rather than dangling.
    let bare = smart.build_with(&ts, &ImageIds::new());
    let LayerContent::Fill(base) = &bare.children()[0].content else { panic!() };
    assert!(!base.channels[&ChannelKind::BaseColor].enabled);
}
