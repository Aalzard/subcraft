//! Matching a folder of PBR texture files to material channels by file name.
//!
//! Handles the common naming styles of texture libraries and engines (`Wood_BaseColor.png`,
//! `brick_diff_2k.jpg`, `rock_nor_gl_4k.exr`, `MetalPlates006_2K_Roughness.png`, `T_Crate_Normal`,
//! camelCase…). Matching is by name only; files are read later by the app.

use std::path::{Path, PathBuf};

use subcraft_core::ChannelKind;

/// How an image must be converted before it fits the channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapConversion {
    None,
    /// Glossiness / smoothness → roughness.
    Invert,
    /// DirectX normal map → OpenGL (SubCraft's convention).
    FlipGreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapMatch {
    pub channel: ChannelKind,
    pub conversion: MapConversion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classified {
    Map(MapMatch),
    /// Several channels packed into R/G/B (ARM / ORM): not supported yet.
    Packed,
    Unknown,
}

const fn map(channel: ChannelKind, conversion: MapConversion) -> Classified {
    Classified::Map(MapMatch { channel, conversion })
}

/// Two-word names, checked before single words (`base_color`, `nor_gl`).
fn classify_pair(a: &str, b: &str) -> Option<Classified> {
    use ChannelKind as C;
    use MapConversion as M;
    Some(match (a, b) {
        ("base", "color" | "colour") => map(C::BaseColor, M::None),
        ("ambient", "occlusion") => map(C::AmbientOcclusion, M::None),
        ("nor" | "normal" | "nrm", "gl" | "opengl") => map(C::Normal, M::None),
        ("nor" | "normal" | "nrm", "dx" | "directx") => map(C::Normal, M::FlipGreen),
        _ => return None,
    })
}

fn classify_word(w: &str) -> Option<Classified> {
    use ChannelKind as C;
    use MapConversion as M;
    Some(match w {
        "basecolor" | "basecolour" | "albedo" | "diffuse" | "diff" | "color" | "colour" | "col" | "bc" => map(C::BaseColor, M::None),
        "roughness" | "rough" | "rgh" => map(C::Roughness, M::None),
        "glossiness" | "gloss" | "smoothness" => map(C::Roughness, M::Invert),
        "metallic" | "metalness" | "metal" | "mtl" => map(C::Metallic, M::None),
        "normal" | "normals" | "nrm" | "nor" | "norm" | "normalgl" | "normalopengl" => map(C::Normal, M::None),
        "normaldx" | "normaldirectx" => map(C::Normal, M::FlipGreen),
        "height" | "displacement" | "disp" | "bump" => map(C::Height, M::None),
        "ao" | "ambientocclusion" | "occlusion" => map(C::AmbientOcclusion, M::None),
        "emissive" | "emission" | "emit" => map(C::Emissive, M::None),
        "opacity" | "alpha" => map(C::Opacity, M::None),
        "arm" | "orm" | "rma" | "mra" => Classified::Packed,
        _ => return None,
    })
}

/// Lowercase words of a file stem, splitting on separators and camelCase boundaries.
pub fn words(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in stem.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = ch.is_ascii_lowercase();
        cur.push(ch.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Classify one file name. Words are scanned from the end, where libraries put the map type
/// (`metal_plate_roughness` is a roughness map, not a metallic one).
pub fn classify(file_name: &str) -> Classified {
    let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    let w = words(stem);
    for i in (0..w.len()).rev() {
        if i > 0
            && let Some(c) = classify_pair(&w[i - 1], &w[i])
        {
            return c;
        }
        if let Some(c) = classify_word(&w[i]) {
            return c;
        }
    }
    Classified::Unknown
}

/// Which file goes to which channel, and why other files were left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureSetPlan {
    /// Suggested layer name (common prefix of the file names).
    pub name: String,
    pub maps: Vec<(PathBuf, MapMatch)>,
    pub skipped: Vec<(PathBuf, String)>,
}

/// Assign files to channels. One file per channel: an OpenGL normal wins over a DirectX one,
/// otherwise the first file in name order.
pub fn plan(paths: &[PathBuf]) -> TextureSetPlan {
    let mut paths: Vec<PathBuf> = paths.to_vec();
    paths.sort();
    let mut out = TextureSetPlan::default();
    for p in &paths {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !subcraft_image::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            out.skipped.push((p.clone(), "not an image".into()));
            continue;
        }
        match classify(name) {
            Classified::Unknown => out.skipped.push((p.clone(), "no channel name recognized".into())),
            Classified::Packed => out.skipped.push((p.clone(), "packed AO/roughness/metallic maps aren't supported yet".into())),
            Classified::Map(m) => {
                if let Some(i) = out.maps.iter().position(|(_, e)| e.channel == m.channel) {
                    let replace = m.channel == ChannelKind::Normal
                        && m.conversion == MapConversion::None
                        && out.maps[i].1.conversion == MapConversion::FlipGreen;
                    if replace {
                        let old = std::mem::replace(&mut out.maps[i], (p.clone(), m));
                        out.skipped.push((old.0, "a DirectX normal; the OpenGL one is used".into()));
                    } else {
                        out.skipped.push((p.clone(), format!("another {} map is already used", m.channel.display_name())));
                    }
                } else {
                    out.maps.push((p.clone(), m));
                }
            }
        }
    }
    out.name = common_name(out.maps.iter().map(|(p, _)| p.as_path()));
    out
}

/// Layer name from the file stems: their common prefix cut at a separator (`Wood_BaseColor` +
/// `Wood_Bump` → `Wood`); a single file drops its last word (`oak_planks_diff` → `oak_planks`).
fn common_name<'a>(paths: impl Iterator<Item = &'a Path>) -> String {
    let stems: Vec<&str> = paths.filter_map(|p| p.file_stem().and_then(|s| s.to_str())).collect();
    let is_sep = |c: char| !c.is_ascii_alphanumeric();
    let name = match stems.as_slice() {
        [] => "",
        [only] => only.rsplit_once(is_sep).map_or("", |(head, _)| head),
        [first, rest @ ..] => {
            let mut len = first.len();
            for s in rest {
                len = first.bytes().zip(s.bytes()).take(len).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count();
            }
            let prefix = &first[..len];
            // Keep whole words only: the prefix must end at a separator or at the end of a stem.
            let ends_at_word = first[len..].starts_with(is_sep) || first.len() == len;
            if ends_at_word { prefix } else { prefix.rsplit_once(is_sep).map_or("", |(head, _)| head) }
        }
    };
    let name = name.trim_matches(is_sep);
    if name.is_empty() { "Imported material".into() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(name: &str) -> Option<(ChannelKind, MapConversion)> {
        match classify(name) {
            Classified::Map(m) => Some((m.channel, m.conversion)),
            _ => None,
        }
    }

    #[test]
    fn library_naming_styles() {
        use ChannelKind::*;
        use MapConversion::*;
        let cases = [
            ("Wood_BaseColor.png", BaseColor, None),
            ("wood_base_color.png", BaseColor, None),
            ("brick_wall_02_diff_2k.jpg", BaseColor, None),
            ("Bricks076C_2K_Color.jpg", BaseColor, None),
            ("T_Crate_Albedo.tga", BaseColor, None),
            ("metal_plate_rough_2k.jpg", Roughness, None),
            ("MetalPlates006_2K_Roughness.png", Roughness, None),
            ("panel_Glossiness.png", Roughness, Invert),
            ("panel_smoothness.tif", Roughness, Invert),
            ("MetalPlates006_2K_Metalness.png", Metallic, None),
            ("crate_metallic.png", Metallic, None),
            ("rock_nor_gl_4k.exr", Normal, None),
            ("rock_nor_dx_4k.exr", Normal, FlipGreen),
            ("Bricks076C_2K_NormalGL.png", Normal, None),
            ("Bricks076C_2K_NormalDX.png", Normal, FlipGreen),
            ("crate_normal.png", Normal, None),
            ("Bricks076C_2K_Displacement.png", Height, None),
            ("rock_disp_4k.png", Height, None),
            ("crate_height.png", Height, None),
            ("Bricks076C_2K_AmbientOcclusion.png", AmbientOcclusion, None),
            ("rock_ao_4k.jpg", AmbientOcclusion, None),
            ("lamp_emissive.png", Emissive, None),
            ("leaf_opacity.png", Opacity, None),
        ];
        for (name, channel, conv) in cases {
            assert_eq!(ch(name), Some((channel, conv)), "{name}");
        }
        assert_eq!(classify("rock_arm_4k.jpg"), Classified::Packed);
        assert_eq!(classify("T_Door_ORM.png"), Classified::Packed);
        assert_eq!(classify("readme_preview.png"), Classified::Unknown);
    }

    #[test]
    fn camel_case_and_separators_split_into_words() {
        assert_eq!(words("MetalPlates006_2K-Roughness"), ["metal", "plates006", "2k", "roughness"]);
        assert_eq!(words("rock nor.gl"), ["rock", "nor", "gl"]);
    }

    #[test]
    fn plan_picks_one_file_per_channel_and_names_the_layer() {
        let files: Vec<PathBuf> = [
            "Bricks076C_2K_Color.jpg",
            "Bricks076C_2K_NormalDX.png",
            "Bricks076C_2K_NormalGL.png",
            "Bricks076C_2K_Roughness.jpg",
            "Bricks076C_2K_Roughness.png",
            "Bricks076C_2K_Displacement.png",
            "Bricks076C.usdc",
            "Bricks076C_2K_ARM.png",
            "Bricks076C_preview.jpg",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let p = plan(&files);
        assert_eq!(p.name, "Bricks076C_2K");
        let got: Vec<(&str, ChannelKind)> = p.maps.iter().map(|(f, m)| (f.to_str().unwrap(), m.channel)).collect();
        assert_eq!(
            got,
            [
                ("Bricks076C_2K_Color.jpg", ChannelKind::BaseColor),
                ("Bricks076C_2K_Displacement.png", ChannelKind::Height),
                ("Bricks076C_2K_NormalGL.png", ChannelKind::Normal),
                ("Bricks076C_2K_Roughness.jpg", ChannelKind::Roughness),
            ]
        );
        // usdc (not an image), ARM (packed), NormalDX (GL preferred), Roughness.png (duplicate), preview.
        assert_eq!(p.skipped.len(), 5, "{:?}", p.skipped);
    }

    #[test]
    fn layer_name_keeps_whole_words() {
        let p = plan(&[PathBuf::from("Wood_BaseColor.png"), PathBuf::from("Wood_Bump.png")]);
        assert_eq!(p.name, "Wood");
        let p = plan(&[PathBuf::from("albedo.png"), PathBuf::from("roughness.png")]);
        assert_eq!(p.name, "Imported material");
    }

    #[test]
    fn single_file_name_drops_the_map_word() {
        let p = plan(&[PathBuf::from("oak_planks_diff.png")]);
        assert_eq!(p.name, "oak_planks");
    }
}
