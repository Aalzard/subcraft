//! P0.2: Painter noise SBSAR (Ratio Cells) baked into SPSM mask Image effects.

use std::path::PathBuf;
use std::sync::Arc;

use subcraft_app::painter_procedural::resolve_painter_procedural_sbsar;
use subcraft_app::{Session, bake_sbsar};
use subcraft_doc::{Layer, LayerContent, MaskEffectKind};
use subcraft_mesh::primitives;
use subcraft_sbsar::resolve_host;

fn find_layer<'a>(layers: &'a [Arc<Layer>], name: &str) -> Option<&'a Layer> {
    for l in layers {
        if l.name == name {
            return Some(l);
        }
        if let LayerContent::Group(g) = &l.content {
            if let Some(f) = find_layer(&g.children, name) {
                return Some(f);
            }
        }
    }
    None
}

#[test]
fn resolve_ratio_cells_sbsar_from_painter_url() {
    let url = "/noise_cells_1/ratio_cells_1?version=1.0.0";
    let path = resolve_painter_procedural_sbsar(url);
    if path.is_none() {
        return; // Painter starter assets not installed
    }
    let path = path.unwrap();
    assert!(
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("noise_cells_1.sbsar")),
        "resolved {}",
        path.display()
    );
}

#[test]
fn plastic_grainy_soft_bakes_ratio_cells_as_image_mask() {
    if resolve_host().is_none() {
        return;
    }
    let path = PathBuf::from(
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-materials\Plastic - rubber\Plastic Grainy Soft.spsm",
    );
    if !path.is_file() {
        return;
    }
    if resolve_painter_procedural_sbsar("/noise_cells_1/ratio_cells_1").is_none() {
        return;
    }

    let mut s = Session::new(primitives::sample_crate(), 512);
    s.import_spsm(&path).expect("import Plastic Grainy Soft.spsm");

    let bumps = find_layer(&s.selected_texture_set().layers, "Bumps")
        .expect("Bumps layer after Soft import");
    let mask = bumps.mask.as_ref().expect("Bumps has a mask");
    let image = mask.effects.iter().find_map(|e| match &e.kind {
        MaskEffectKind::Image {
            projection,
            resource,
            painter,
            ..
        } => Some((*resource, projection.scale, painter.clone())),
        _ => None,
    });
    let (resource, scale, painter) =
        image.expect("Bumps mask should use baked SBSAR Image (not Pattern Cells)");
    let painter = painter.expect("baked Ratio Cells must keep live PainterNoiseParams for Balance");
    let res = s
        .resources()
        .get(resource)
        .expect("baked Ratio Cells resource must be in the session store");
    assert_eq!(
        res.bitmap.channels, 1,
        "painter-proc mask must be grayscale (RGBA TGA was treated as solid logo mask)"
    );
    assert!(
        scale[0] >= 4.0 && scale[0] <= 16.0 && (scale[0] - scale[1]).abs() < 0.01,
        "UV tiling from Painter should land on Image projection, got {scale:?}"
    );

    // Balance is a live Substance input: changing it must produce a different cached bake.
    let mut low = painter.clone();
    low.balance = 0.15;
    let mut high = painter.clone();
    high.balance = 0.85;
    let id_low = s
        .bake_painter_procedural_mask(&low, 256)
        .expect("bake low balance");
    let id_high = s
        .bake_painter_procedural_mask(&high, 256)
        .expect("bake high balance");
    assert_ne!(
        id_low, id_high,
        "different Balance values must not share the same bake cache key"
    );
    let mean = |id| {
        let bm = &s.resources().get(id).unwrap().bitmap;
        let mut sum = 0.0f64;
        let n = (bm.width * bm.height) as f64;
        for y in 0..bm.height {
            for x in 0..bm.width {
                sum += bm.texel(x, y, false)[0] as f64;
            }
        }
        sum / n
    };
    let mean_low = mean(id_low);
    let mean_high = mean(id_high);
    assert!(
        mean_high > mean_low + 0.02,
        "Painter Balance high should brighten Ratio Cells (low={mean_low:.3} high={mean_high:.3})"
    );

    // Sanity: host can still bake the resolved file directly.
    let sbsar = resolve_painter_procedural_sbsar("/noise_cells_1/ratio_cells_1").unwrap();
    let out = std::env::temp_dir().join(format!("subcraft-soft-bake-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    bake_sbsar(&sbsar, &out, 256, 0).expect("direct bake of noise_cells_1");
    let _ = std::fs::remove_dir_all(&out);
}
