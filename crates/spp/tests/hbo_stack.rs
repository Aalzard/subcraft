use std::path::PathBuf;

use subcraft_spp::{build_stack_from_document, SppLayerNode};

fn scratch_document() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/document.bin");
    p.is_file().then_some(p)
}

fn count_layers(materials: &[subcraft_spp::SppMaterialStack]) -> usize {
    materials
        .iter()
        .flat_map(|m| &m.texture_sets)
        .map(|ts| count_nodes(&ts.layers))
        .sum()
}

fn count_nodes(nodes: &[SppLayerNode]) -> usize {
    nodes
        .iter()
        .map(|n| 1 + count_nodes(&n.children))
        .sum()
}

fn find_layer<'a>(nodes: &'a [SppLayerNode], name: &str) -> Option<&'a SppLayerNode> {
    for n in nodes {
        if n.name == name {
            return Some(n);
        }
        if let Some(found) = find_layer(&n.children, name) {
            return Some(found);
        }
    }
    None
}

#[test]
fn decode_drehgestell_document_matches_golden_stack() {
    let Some(doc_path) = scratch_document() else {
        eprintln!("skip: no scratch/document.bin");
        return;
    };
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/spp-stack.json");
    if !golden_path.is_file() {
        eprintln!("skip: no scratch/spp-stack.json golden");
        return;
    }
    let bytes = std::fs::read(&doc_path).unwrap();
    let (stack, fmt, dv) = build_stack_from_document(&bytes).unwrap();
    assert_eq!(fmt, "registry");
    assert_eq!(dv, 82);
    assert_eq!(stack.materials.len(), 2);
    assert_eq!(stack.materials[0].scene_material, "A3E_bogie");

    let golden = subcraft_spp::load_stack(&golden_path).unwrap();
    assert_eq!(stack.materials.len(), golden.materials.len());
    assert_eq!(count_layers(&stack.materials), count_layers(&golden.materials));
}

#[test]
fn hbo_extracts_channels_blends_and_masks_1to1() {
    let dreh = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Drehgestell.spp");
    if dreh.is_file() {
        let h5 = hdf5_reader::Hdf5File::open(&dreh).unwrap();
        let doc_bytes = h5.dataset("paint/document.bin").unwrap().read_raw_bytes().unwrap();
        let (stack, _, _) = build_stack_from_document(&doc_bytes).unwrap();
        let bogie = &stack.materials[0].texture_sets[0].layers;

        let brems = find_layer(bogie, "Bremsscheibe").expect("Bremsscheibe layer");
        assert!(brems.has_black_mask);
        assert_eq!(brems.metallic, Some(1.0));
        assert!(!brems.mask_decal_hits.is_empty(), "Bremsscheibe should have decal hits");

        let gummi = find_layer(bogie, "Gummi").expect("Gummi group");
        assert!(gummi.has_black_mask);
        assert!(!gummi.mask_decal_hits.is_empty(), "Gummi should have decal hits");

        let rubber_base = find_layer(&gummi.children, "Rubber Base").expect("Rubber Base");
        assert!(rubber_base.base_color.is_some(), "Rubber Base should have procedural base_color");
        assert!(rubber_base.roughness.is_some(), "Rubber Base should have procedural roughness");
    }

    let wagen = PathBuf::from(r"G:\Simuverse-Projekte\A3E\S_Substance\Wagenkasten.spp");
    if wagen.is_file() {
        let h5 = hdf5_reader::Hdf5File::open(&wagen).unwrap();
        let doc_bytes = h5.dataset("paint/document.bin").unwrap().read_raw_bytes().unwrap();
        let (stack, _, _) = build_stack_from_document(&doc_bytes).unwrap();
        let car = &stack
            .materials
            .iter()
            .find(|m| m.scene_material == "A3E_Car")
            .unwrap()
            .texture_sets[0]
            .layers;

        let lack = find_layer(car, "Lack Gelb").expect("Lack Gelb");
        assert!(lack.has_black_mask);
        let bc = lack.base_color.expect("Lack Gelb base_color");
        assert!(bc[0] > 0.4 && bc[1] > 0.15, "Lack Gelb should be yellow-orange: {bc:?}");
        assert!(!lack.mask_decal_hits.is_empty(), "Lack Gelb decal hits");
        assert!(lack.height_proc.is_some(), "Lack Gelb should decode matfinish_rough height_proc");
        assert!(lack.roughness_proc.is_some(), "Lack Gelb should decode matfinish_rough roughness_proc");

        let alu = find_layer(car, "Aluminium Base").expect("Aluminium Base");
        let alu_bc = alu.base_color.expect("Aluminium Base color");
        assert!(alu_bc[0] > 0.5 && alu_bc[1] > 0.5 && alu_bc[2] > 0.5, "Aluminium Base should be silver: {alu_bc:?}");
        assert_eq!(alu.metallic, Some(1.0));
        assert!(alu.height_proc.is_some(), "Aluminium Base should decode finish_brushed_linear height_proc");
        assert!(alu.roughness_proc.is_some(), "Aluminium Base should decode finish_brushed_linear roughness_proc");

        fn dump_blends(node: &subcraft_spp::SppLayerNode, indent: usize) {
            let pad = "  ".repeat(indent);
            eprintln!("{pad}{} (black_mask={}) blends={:?}", node.name, node.has_black_mask, node.channel_blends);
            for fx in &node.mask_effects {
                eprintln!("{pad}  mask_fx: {} blend={} op={} kind={:?}", fx.name, fx.blend, fx.opacity, fx.kind);
            }
            for c in &node.children {
                dump_blends(c, indent + 1);
            }
        }
        eprintln!("=== Wagenkasten.spp ===");
        for l in car {
            dump_blends(l, 1);
        }
    }
}

#[test]
fn plastic_grainy_soft_cells_preserve_painter_uv_tiling() {
    let path = PathBuf::from(
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-materials\Plastic - rubber\Plastic Grainy Soft.spsm",
    );
    if !path.is_file() {
        return;
    }
    let node = subcraft_spp::decode_spsm(&path).expect("decode Plastic Grainy Soft.spsm");
    let bumps = find_layer(std::slice::from_ref(&node), "Bumps").expect("Bumps layer");
    let cells = bumps
        .mask_effects
        .iter()
        .find_map(|fx| match &fx.kind {
            subcraft_spp::SppMaskEffectKind::ProceduralFill {
                pattern,
                scale,
                uv_scale,
                sbsar,
                ..
            } if pattern == "cells" => Some((*scale, *uv_scale, sbsar.clone())),
            _ => None,
        })
        .expect("Bumps must have a Cells procedural mask fill");
    let (pattern_scale, uv_scale, sbsar) = cells;
    let sbsar = sbsar.expect("Ratio Cells must keep urlToSbsar for real SBSAR bake");
    assert!(
        sbsar.url.to_lowercase().contains("noise_cells_1"),
        "expected noise_cells_1 SBSAR url, got {}",
        sbsar.url
    );
    // Painter Soft ships Ratio Cells with Substance `scale` in the dense 64..128 range.
    assert!(
        (64..=128).contains(&sbsar.scale),
        "Substance graph scale should be ~64..128 for Soft, got {}",
        sbsar.scale
    );
    // Painter ships Soft with UV Transformation Scale ≈ 6 (user often raises to ~10 in the UI).
    // That must land on `uv_scale` (Projection tiling), not get folded into pattern scale and capped at 48.
    let uv = uv_scale.expect("Cells fill must preserve Painter uvTransformation.scale");
    assert!(
        uv >= 4.0 && uv <= 16.0,
        "Plastic Grainy Soft Cells UV tiling should be in Painter's typical 4..16 range, got {uv}"
    );
    assert!(
        (pattern_scale - 64.0).abs() < 0.5,
        "approx Pattern scale stays capped at 64, got {pattern_scale}"
    );
    // Effective density ≈ scale×tiling (was ~48 with tiling stuck at 1).
    assert!(
        (sbsar.scale as f32) * uv >= 200.0,
        "effective cells density (sbsar×tiling) too low: {}×{uv}",
        sbsar.scale
    );
}

#[test]
fn decode_spsm_and_spmsk_starter_assets() {
    let spsm_path = PathBuf::from(
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-materials\metal\Steel Painted Scratched.spsm",
    );
    if spsm_path.is_file() {
        let node = subcraft_spp::decode_spsm(&spsm_path).expect("decode Steel Painted Scratched.spsm");
        assert_eq!(node.kind, "folder");
        assert!(!node.children.is_empty(), "smart material folder should have child layers");
        let has_fill = node.children.iter().any(|c| {
            c.base_color.is_some() || c.roughness.is_some() || c.metallic.is_some() || !c.children.is_empty()
        });
        assert!(has_fill, "smart material should decode at least one fill or nested folder");
        let has_mask = node.children.iter().any(|c| !c.mask_effects.is_empty() || c.has_black_mask);
        assert!(has_mask, "smart material should decode at least one masked layer");
    }

    let spmsk_path = PathBuf::from(
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-masks\Edges Scratched.spmsk",
    );
    if spmsk_path.is_file() {
        let effects = subcraft_spp::decode_spmsk(&spmsk_path).expect("decode Edges Scratched.spmsk");
        assert!(!effects.is_empty(), "smart mask should decode into mask effects");
    }

    let steel_path = PathBuf::from(
        r"C:\Users\princ\Documents\SubCraft\Library\smart-materials\Steel Medieval Stylized.spsm",
    );
    if steel_path.is_file() {
        let node = subcraft_spp::decode_spsm(&steel_path).expect("decode Steel Medieval Stylized.spsm");
        let base_metal = node.children.iter().find(|c| c.name == "Base Metal").expect("Base Metal");
        assert!(base_metal.height_proc.is_some(), "Base Metal should decode finish_rough height_proc");
        assert!(base_metal.roughness_proc.is_some(), "Base Metal should decode finish_rough roughness_proc");

        let scratches = node.children.iter().find(|c| c.name == "Scratches").expect("Scratches");
        assert!(scratches.base_color_proc.is_some(), "Scratches should decode base_color_proc");
        assert!(scratches.roughness_proc.is_some(), "Scratches should decode roughness_proc");
        let bc_blend = scratches
            .channel_blends
            .iter()
            .find(|b| b.channel == "base_color")
            .expect("Scratches base_color blend");
        assert_eq!(bc_blend.mode, "overlay", "Scratches base_color uses Overlay (blendingMode=11), overriding Diffuse Normal");
        assert_eq!(
            scratches.channel_blends.iter().filter(|b| b.channel == "base_color").count(),
            1,
            "CH_BASE_COLOR must replace earlier CH_DIFFUSE entry"
        );

        let wear = node.children.iter().find(|c| c.name == "Wear").expect("Wear");
        assert_eq!(
            wear.mask_effects[0].blend, "normal",
            "Wear mask generator must use normal blend, not Height/Normal LinearDodge"
        );
    }

    let rubber_path = PathBuf::from(
        r"C:\Users\princ\Documents\SubCraft\Library\smart-materials\Plastic Rubber.spsm",
    );
    if rubber_path.is_file() {
        let node = subcraft_spp::decode_spsm(&rubber_path).expect("decode Plastic Rubber.spsm");
        let damages = node
            .children
            .iter()
            .find(|c| c.name == "Surface Damages")
            .expect("Surface Damages");
        assert_eq!(damages.mask_effects.len(), 4, "Surface Damages should have 3 procedural fills + 1 Levels");
        let mut seeds = std::collections::HashSet::new();
        for fx in &damages.mask_effects[..3] {
            if let subcraft_spp::SppMaskEffectKind::ProceduralFill { seed, .. } = &fx.kind {
                seeds.insert(*seed);
            }
        }
        assert_eq!(seeds.len(), 3, "Each procedural fill in Surface Damages must have a distinct seed");
    }

    let fg_path = PathBuf::from(
        r"C:\Users\princ\Documents\SubCraft\Library\smart-materials\Fiberglass Worn.spsm",
    );
    if fg_path.is_file() {
        let node = subcraft_spp::decode_spsm(&fg_path).expect("decode Fiberglass Worn.spsm");
        let raw_fiber = node.children.iter().find(|c| c.name == "Raw Fiber").expect("Raw Fiber");
        assert!(
            !raw_fiber.channel_blends.iter().any(|b| b.channel == "base_color"),
            "Raw Fiber base layer must keep Normal 100% base_color blend, not sub-fill Overlay 7%: {:?}",
            raw_fiber.channel_blends
        );
        assert_eq!(
            raw_fiber.content_effects.len(),
            2,
            "Raw Fiber must decode both Anisotropic Noise fill sub-actions"
        );
        let fx0_bc = raw_fiber.content_effects[0]
            .channel_blends
            .iter()
            .find(|b| b.channel == "base_color")
            .expect("sub-fill 0 base_color blend");
        assert_eq!(fx0_bc.mode, "normal");
        assert!((fx0_bc.opacity - 0.10).abs() < 1e-4);
        assert!(raw_fiber.content_effects[0].rotation_deg.is_none());

        let fx1_bc = raw_fiber.content_effects[1]
            .channel_blends
            .iter()
            .find(|b| b.channel == "base_color")
            .expect("sub-fill 1 base_color blend");
        assert_eq!(fx1_bc.mode, "overlay");
        assert!((fx1_bc.opacity - 0.07).abs() < 1e-4);
        assert!((raw_fiber.content_effects[1].rotation_deg.unwrap_or(0.0) - 90.0).abs() < 0.1);
        for (i, fx) in raw_fiber.content_effects.iter().enumerate() {
            let hp = fx.height_proc.as_ref().expect("sub-fill height_proc");
            assert!(
                (hp.scale - 4.0).abs() < 1e-4 && hp.high >= 0.40,
                "sub-fill {i} height_proc must have x_amount=4 scale and strong height amplitude: {hp:?}"
            );
            if let Some(hb) = fx.channel_blends.iter().find(|b| b.channel == "height") {
                assert_eq!(
                    hb.mode, "add",
                    "sub-fill {i} height blend must be Linear Dodge (add) so 90° weave does not overwrite 0° weave"
                );
            }
        }

        let paint = node.children.iter().find(|c| c.name == "Paint").expect("Paint group");
        let rough_grunge = paint
            .children
            .iter()
            .find(|c| c.name == "Roughness Grunge")
            .expect("Roughness Grunge");
        assert!(
            rough_grunge.base_color.is_none() && rough_grunge.base_color_proc.is_none(),
            "Roughness Grunge must only affect Roughness, not overwrite BaseColor with black/white grunge"
        );
        let rg_proc = rough_grunge.roughness_proc.as_ref().expect("Roughness Grunge proc");
        assert!(
            !rg_proc.invert,
            "Roughness Grunge CH_ROUGHNESS has invert=0; legacy CH_GLOSSINESS source must not invert it"
        );
        let rg_blend = rough_grunge
            .channel_blends
            .iter()
            .find(|b| b.channel == "roughness")
            .expect("Roughness Grunge roughness blend");
        assert_eq!(rg_blend.mode, "normal");
        assert!((rg_blend.opacity - 0.15).abs() < 1e-4);
        let color_paint = paint.children.iter().find(|c| c.name == "Color paint").expect("Color paint");
        assert_eq!(color_paint.mask_effects.len(), 2);
        assert_eq!(color_paint.mask_effects[0].blend, "normal");
        if let subcraft_spp::SppMaskEffectKind::Generator { level, softness, noise_amount, invert, .. } = &color_paint.mask_effects[0].kind {
            assert!(*invert);
            let min_thresh = *level - 0.5 * *softness - 0.5 * *noise_amount;
            assert!(
                min_thresh > 0.515,
                "Color paint edge generator threshold ({min_thresh}) must stay above flat curvature 0.50 so flat faces stay painted"
            );
        } else {
            panic!("expected Edges generator on Color paint");
        }
        assert_eq!(
            color_paint.mask_effects[1].blend, "add",
            "clouds_2 on Color paint must decode blendingMode=10 as Linear Dodge (add / Ldge)"
        );
        assert_eq!(color_paint.mask_effects[1].name, "Fill (Clouds 2)");
    }
}

