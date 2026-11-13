//! Texture set mixing settings at export: normal mixing, height-to-normal method, AO mixing.
use super::*;
use subcraft_doc::{AoMixing, HeightToNormal, MixingSettings, NormalMixing};

fn with(mixing: MixingSettings, f: impl FnOnce(&mut MixingSettings)) -> MixingSettings {
    let mut m = mixing;
    f(&mut m);
    m
}

/// One raised texel at (64, 64) on a flat height channel.
fn bump_channels() -> BTreeMap<ChannelKind, TiledImage> {
    let mut ch = channels();
    let mut h = TiledImage::new(RES, RES, 1, [0.0; 4]);
    h.set(64, 64, [1.0; 4]);
    ch.insert(ChannelKind::Height, h);
    ch
}

fn normal_at(ch: &BTreeMap<ChannelKind, TiledImage>, mixing: MixingSettings, name: &str, x: u32, y: u32) -> [f32; 4] {
    let mesh = primitives::plane(2.0, 1);
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: ch, mesh_maps: None, mixing }];
    let dir = tmp(name);
    export(&mesh, "M", &sets, &ExportSettings::default(), &dir).unwrap();
    let n = px(&dir.join("M_P_Normal.png"), x, y);
    std::fs::remove_dir_all(&dir).unwrap();
    n
}

#[test]
fn painter_defaults_and_the_legacy_settings() {
    let d = MixingSettings::default();
    assert_eq!((d.normal, d.height_to_normal, d.ao), (NormalMixing::Combine, HeightToNormal::Smooth, AoMixing::Multiply));
    assert_eq!(MixingSettings::LEGACY.height_to_normal, HeightToNormal::Sharp);
    assert_eq!(MixingSettings::LEGACY.ao, AoMixing::Replace);
}

#[test]
fn sobel_reaches_the_diagonal_neighbours_and_central_differences_do_not() {
    let ch = bump_channels();
    let smooth = with(MixingSettings::LEGACY, |m| m.height_to_normal = HeightToNormal::Smooth);
    // (65, 65) is a diagonal neighbour of the bump: only the 3x3 Sobel filter sees it.
    let sharp = normal_at(&ch, MixingSettings::LEGACY, "mix-sharp", 65, 65);
    assert!((sharp[0] - 0.5).abs() < 2.0 / 255.0, "Sharp: flat beside the bump on the diagonal: {sharp:?}");
    let sobel = normal_at(&ch, smooth, "mix-sobel", 65, 65);
    assert!(sobel[0] > 0.55, "Smooth (Sobel): the slope reaches the diagonal neighbour: {sobel:?}");
    // Both tilt the direct neighbour the same way.
    let sharp_side = normal_at(&ch, MixingSettings::LEGACY, "mix-sharp-side", 65, 64);
    let sobel_side = normal_at(&ch, smooth, "mix-sobel-side", 65, 64);
    assert!(sharp_side[0] > 0.55 && sobel_side[0] > 0.55, "{sharp_side:?} {sobel_side:?}");
}

#[test]
fn normal_mixing_replace_ignores_the_height_channel() {
    let ch = bump_channels();
    let combine = normal_at(&ch, MixingSettings::LEGACY, "mix-combine", 65, 64);
    assert!(combine[0] > 0.55, "Combine folds the height in: {combine:?}");
    let replace = with(MixingSettings::LEGACY, |m| m.normal = NormalMixing::Replace);
    let n = normal_at(&ch, replace, "mix-replace", 65, 64);
    assert!((n[0] - 0.5).abs() < 2.0 / 255.0 && (n[1] - 0.5).abs() < 2.0 / 255.0, "Replace: the Normal channel alone: {n:?}");
}

#[test]
fn ao_mixing_multiply_combines_the_channel_with_the_baked_map() {
    let mesh = primitives::plane(2.0, 1);
    let mut ch = channels();
    ch.insert(ChannelKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.6; 4]));
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.5; 4]));
    let gl = ExportSettings { template: ExportTemplate::gltf(), ..ExportSettings::default() };
    for (ao, expect) in [(AoMixing::Multiply, 0.3), (AoMixing::Replace, 0.6)] {
        let mixing = with(MixingSettings::LEGACY, |m| m.ao = ao);
        let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked), mixing }];
        let dir = tmp(&format!("mix-ao-{ao:?}"));
        export(&mesh, "M", &sets, &gl, &dir).unwrap();
        let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 10, 10);
        assert!((orm[0] - expect).abs() < 1.5 / 255.0, "{ao:?}: AO {orm:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
    // Without a painted AO channel the baked map is used either way.
    ch.remove(&ChannelKind::AmbientOcclusion);
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked), mixing: MixingSettings::default() }];
    let dir = tmp("mix-ao-baked-only");
    export(&mesh, "M", &sets, &gl, &dir).unwrap();
    assert!((px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 10, 10)[0] - 0.5).abs() < 1.5 / 255.0);
    std::fs::remove_dir_all(&dir).unwrap();
}
