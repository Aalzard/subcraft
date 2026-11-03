use subcraft_assets::texture_maps::{Classified, MapConversion, classify, split_map_name};
use subcraft_core::ChannelKind;

fn split(name: &str) -> Option<(String, ChannelKind, MapConversion)> {
    match split_map_name(name)? {
        (prefix, Classified::Map(m)) => Some((prefix, m.channel, m.conversion)),
        _ => None,
    }
}

#[test]
fn ambientcg_names_split_into_material_and_map() {
    use ChannelKind::*;
    use MapConversion::*;
    let cases = [
        ("Bricks059_2K-JPG_Color.jpg", "Bricks059_2K-JPG_", BaseColor, None),
        ("Bricks059_2K-JPG_NormalGL.jpg", "Bricks059_2K-JPG_", Normal, None),
        ("Bricks059_2K-JPG_NormalDX.jpg", "Bricks059_2K-JPG_", Normal, FlipGreen),
        ("Bricks059_2K-JPG_AmbientOcclusion.jpg", "Bricks059_2K-JPG_", AmbientOcclusion, None),
        ("Bricks059_2K-JPG_Displacement.jpg", "Bricks059_2K-JPG_", Height, None),
        ("Bricks059_2K-JPG_Metalness.jpg", "Bricks059_2K-JPG_", Metallic, None),
        ("Bricks059_2K-JPG_Opacity.jpg", "Bricks059_2K-JPG_", Opacity, None),
        ("metal_plate_roughness.png", "metal_plate_", Roughness, None),
        ("albedo.png", "", BaseColor, None),
    ];
    for (file, prefix, channel, conversion) in cases {
        assert_eq!(split(file), Some((prefix.to_string(), channel, conversion)), "{file}");
    }
    assert!(split_map_name("holiday photo.png").is_none());
}

#[test]
fn classify_still_agrees_with_split() {
    for f in ["Wood_BaseColor.png", "brick_diff_2k.jpg", "rock_nor_gl_4k.exr", "T_Crate_Normal.png", "x_orm.png", "unknown.png"] {
        let via_split = split_map_name(f).map_or(Classified::Unknown, |(_, c)| c);
        assert_eq!(classify(f), via_split, "{f}");
    }
}
