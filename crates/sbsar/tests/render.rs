//! Renders a real archive (see `tests/data/README.md`) and checks parameters, channels and
//! pixel statistics.

use std::collections::BTreeMap;

use subcraft_core::ChannelKind;
use subcraft_doc::GraphParamKind;
use subcraft_image::Bitmap;
use subcraft_sbsar::{GraphRenderer, Package, inspect};

fn cardboard() -> Package {
    inspect(include_bytes!("data/CardBoard.sbsar")).expect("the test archive opens")
}

/// Mean of each of the first three channels, 0..1 in the bitmap's encoded values.
fn mean(b: &Bitmap) -> [f64; 3] {
    let mut s = [0.0f64; 3];
    for y in 0..b.height {
        for x in 0..b.width {
            let t = b.texel(x, y, false);
            for c in 0..3 {
                s[c] += t[c] as f64;
            }
        }
    }
    let n = (b.width * b.height) as f64;
    s.map(|v| v / n)
}

#[test]
fn the_archive_describes_its_parameters_and_channels() {
    let p = cardboard();
    assert_eq!(p.graphs().len(), 1);
    let g = &p.graphs()[0];
    assert_eq!(g.label, "CardBoard");
    let ids: Vec<&str> = g.params.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["$randomseed", "cardboard_color", "tearing"], "$outputsize is hidden; the seed is kept");
    let color = &g.params[1];
    assert_eq!(color.label, "Cardboard Color");
    assert_eq!(color.group, "Advanced parameters");
    assert!(matches!(color.kind, GraphParamKind::Color { alpha: false, linear: false }));
    assert!((color.default[0] - 0.588907).abs() < 1e-6);
    assert!(matches!(&g.params[2].kind, GraphParamKind::Float { components: 1, min, max, .. } if min == &[0.0] && max == &[1.0]));
    assert_eq!(g.channels, [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::AmbientOcclusion]);
}

#[test]
fn rendering_fills_every_channel_and_follows_the_parameters() {
    let p = cardboard();
    let path = p.graphs()[0].path.clone();
    let mut r = GraphRenderer::new(p, &path);
    let out = r.render(&BTreeMap::new(), 6).expect("renders");
    let by = |c: ChannelKind| &out.maps.iter().find(|m| m.channel == c).unwrap_or_else(|| panic!("{c:?} rendered")).bitmap;
    assert_eq!(out.maps.len(), 6);
    let base = by(ChannelKind::BaseColor);
    assert_eq!((base.width, base.height), (64, 64));
    // Default colour is a brown (0.59, 0.46, 0.32 sRGB): the map's mean sits close to it.
    let m = mean(base);
    assert!((m[0] - 0.60).abs() < 0.05 && (m[1] - 0.48).abs() < 0.05 && (m[2] - 0.34).abs() < 0.05, "base colour mean {m:?}");
    assert!(mean(by(ChannelKind::Metallic))[0] < 0.01, "cardboard is not metal");
    // Normals point out of the surface: blue near 1, red and green around 0.5.
    let n = mean(by(ChannelKind::Normal));
    assert!(n[2] > 0.9 && (n[0] - 0.5).abs() < 0.05 && (n[1] - 0.5).abs() < 0.05, "normal mean {n:?}");

    // A blue colour and full tearing: the same renderer (incremental) gives a different map.
    let mut values = BTreeMap::new();
    values.insert("cardboard_color".to_string(), vec![0.2, 0.35, 0.6]);
    values.insert("tearing".to_string(), vec![1.0]);
    let blue = r.render(&values, 6).expect("renders again");
    let m2 = mean(&blue.maps.iter().find(|m| m.channel == ChannelKind::BaseColor).unwrap().bitmap);
    assert!(m2[2] > m2[0] + 0.2, "base colour follows the colour parameter: {m2:?}");
    // Back to defaults gives the first result again, exactly.
    let again = r.render(&BTreeMap::new(), 6).expect("renders a third time");
    assert_eq!(again.maps[0].bitmap, out.maps[0].bitmap);
}

#[test]
fn the_size_is_clamped_to_the_supported_range() {
    let p = cardboard();
    let path = p.graphs()[0].path.clone();
    let out = GraphRenderer::new(p, &path).render(&BTreeMap::new(), 0).expect("renders");
    assert_eq!(out.maps[0].bitmap.width, 1 << subcraft_sbsar::MIN_SIZE_LOG2);
}
