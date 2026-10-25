use std::path::PathBuf;

use subcraft_spp::{inspect, probe_texture_cache, texture_cache_decodable};

const WEB_TEMP: &str =
    r"C:\Users\princ\AppData\Local\Temp\subcraft-spp-web\0d927b8a-857a-451a-8f1d-f53c14c84aa5\project.spp";

fn drehgestell_path() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Drehgestell.spp");
    if root.is_file() {
        return Some(root);
    }
    let web = PathBuf::from(WEB_TEMP);
    if web.is_file() {
        return Some(web);
    }
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/project.spp");
    scratch.is_file().then_some(scratch)
}

#[test]
fn decode_drehgestell_texture_cache_channels() {
    let Some(path) = drehgestell_path() else { return };
    let caches = subcraft_spp::decode_texture_cache(&path, 1024).expect("decode_texture_cache");
    assert_eq!(caches.len(), 2);
    assert_eq!(caches[0].material_name, "A3E_bogie");
    assert_eq!(caches[1].material_name, "A3E_CurrentCollector");
    for c in &caches {
        assert_eq!(c.width, 1024);
        assert_eq!(c.height, 1024);
        let rgba = c.base_color_rgba8.as_ref().unwrap();
        assert_eq!(rgba.len(), 1024 * 1024 * 4);
        assert!(
            rgba.chunks_exact(4).all(|px| px[3] == 255),
            "all base_color texels should be dilated to opaque alpha=255"
        );
        let rough = c.roughness_ga8.as_ref().unwrap();
        assert_eq!(rough.len(), 1024 * 1024 * 2);
        assert!(rough.chunks_exact(2).all(|px| px[1] == 255));
        let metal = c.metallic_ga8.as_ref().unwrap();
        assert_eq!(metal.len(), 1024 * 1024 * 2);
        assert!(metal.chunks_exact(2).all(|px| px[1] == 255));
        assert_eq!(c.height_f32.as_ref().unwrap().len(), 1024 * 1024);
        assert_eq!(c.normal_rgb_f32.as_ref().unwrap().len(), 1024 * 1024 * 3);
    }

    let assbin = subcraft_spp::read_container(&path).unwrap().scene_assbin;
    let rep = subcraft_mesh::import_assbin_bytes(&assbin).unwrap();
    let mesh = &rep.mesh;
    assert_eq!(mesh.submeshes.len(), 2);

    // Verify per-blob surface material alignment with SubCraft's UV convention (1.0 - v_assbin)
    // and vertically-flipped texture-cache bitmaps.
    let tail = subcraft_mesh::assbin::painter_v17_tail_offset(&assbin).unwrap();
    let mut cur = tail;
    for idx in 0..11 {
        let bsz = u32::from_le_bytes(assbin[cur..cur + 4].try_into().unwrap()) as usize;
        let bbody = cur + 4;
        let bend = cur + bsz;
        let nl = u32::from_le_bytes(assbin[bbody + 4..bbody + 8].try_into().unwrap()) as usize;
        let mname = String::from_utf8_lossy(&assbin[bbody + 8..bbody + 8 + nl]).to_string();
        let p1 = u32::from_le_bytes(assbin[bbody + 12 + nl..bbody + 16 + nl].try_into().unwrap());
        let nv = u32::from_le_bytes(assbin[bbody + 20 + nl..bbody + 24 + nl].try_into().unwrap()) as usize;
        let pos_off = bbody + 24 + nl;
        let uv_off = pos_off + 12 * nv + 4 + 12 * nv + 4 + 12 * nv + 4 + 12 * nv + 8;
        let nf_off = uv_off + 12 * nv;
        let nf = u32::from_le_bytes(assbin[nf_off..nf_off + 4].try_into().unwrap()) as usize;
        let faces_off = nf_off + 4;
        let c = if p1 == 1 { &caches[1] } else { &caches[0] };
        let rgba = c.base_color_rgba8.as_ref().unwrap();
        let w = c.width as usize;

        let read_pos = |vi: usize| -> [f32; 3] {
            let o = pos_off + vi * 12;
            [
                f32::from_le_bytes(assbin[o..o + 4].try_into().unwrap()),
                f32::from_le_bytes(assbin[o + 4..o + 8].try_into().unwrap()),
                f32::from_le_bytes(assbin[o + 8..o + 12].try_into().unwrap()),
            ]
        };
        let read_subcraft_uv = |vi: usize| -> (f32, f32) {
            let o = uv_off + vi * 12;
            let u = f32::from_le_bytes(assbin[o..o + 4].try_into().unwrap());
            let v_assbin = f32::from_le_bytes(assbin[o + 4..o + 8].try_into().unwrap());
            (u, 1.0 - v_assbin)
        };

        let mut area_silver = 0.0f64;
        let mut area_dark = 0.0f64;
        let mut area_red = 0.0f64;
        let mut silver_nz = 0.0f64;
        let mut silver_nr = 0.0f64;
        for f_i in 0..nf {
            let fo = faces_off + f_i * 20;
            let i0 = u32::from_le_bytes(assbin[fo + 8..fo + 12].try_into().unwrap()) as usize;
            let i1 = u32::from_le_bytes(assbin[fo + 12..fo + 16].try_into().unwrap()) as usize;
            let i2 = u32::from_le_bytes(assbin[fo + 16..fo + 20].try_into().unwrap()) as usize;
            let p0 = read_pos(i0);
            let p1 = read_pos(i1);
            let p2 = read_pos(i2);
            let ux = p1[0] - p0[0];
            let uy = p1[1] - p0[1];
            let uz = p1[2] - p0[2];
            let vx = p2[0] - p0[0];
            let vy = p2[1] - p0[1];
            let vz = p2[2] - p0[2];
            let cx = uy * vz - uz * vy;
            let cy = uz * vx - ux * vz;
            let cz = ux * vy - uy * vx;
            let len = (cx * cx + cy * cy + cz * cz).sqrt();
            let area = 0.5 * len as f64;
            let nz = if len > 1e-7 { (cz / len).abs() as f64 } else { 0.0 };
            let nr = if len > 1e-7 { ((cx * cx + cy * cy).sqrt() / len) as f64 } else { 0.0 };
            let uv0 = read_subcraft_uv(i0);
            let uv1 = read_subcraft_uv(i1);
            let uv2 = read_subcraft_uv(i2);
            let u = (uv0.0 + uv1.0 + uv2.0) / 3.0;
            let v = (uv0.1 + uv1.1 + uv2.1) / 3.0;
            let x = (u.clamp(0.0, 0.9999) * w as f32) as usize;
            let y = (v.clamp(0.0, 0.9999) * w as f32) as usize;
            let px = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
            if px[0] > px[1].saturating_add(40) && px[0] > px[2].saturating_add(40) {
                area_red += area;
            } else if px[0] > 120 {
                area_silver += area;
                silver_nz += area * nz;
                silver_nr += area * nr;
            } else {
                area_dark += area;
            }
        }
        let tot = (area_silver + area_dark + area_red).max(1e-9);
        let silver_pct = area_silver * 100.0 / tot;
        let red_pct = area_red * 100.0 / tot;
        let dark_pct = area_dark * 100.0 / tot;
        let snz = if area_silver > 1e-9 { silver_nz / area_silver } else { 0.0 };
        let snr = if area_silver > 1e-9 { silver_nr / area_silver } else { 0.0 };
        println!(
            "blob[{idx:2}] {mname:<13}: silver={silver_pct:5.1}% (|nz|={snz:.2} rad={snr:.2}) red={red_pct:5.1}% dark={dark_pct:5.1}%"
        );

        match idx {
            // Wheelsets: dark painted wheels with metallic silver rolling tread (radial_xy > 0.90)
            0 | 1 => {
                assert!(dark_pct > 85.0 && silver_pct > 8.0 && snr > 0.90);
            }
            // Brake discs: flat braking faces (|nz| > 0.99) are metallic silver
            7 | 8 => {
                assert!(silver_pct > 55.0 && snz > 0.99);
            }
            // Current collector: > 48% red insulating bar, ~28% metallic contact shoe
            3 => {
                assert!(red_pct > 48.0 && silver_pct > 25.0);
            }
            // Bogie frame & dampers: > 96% dark painted metal
            _ => {
                assert!(dark_pct > 96.0);
            }
        }
        cur = bend;
    }
}

#[test]
fn wagenkasten_autosave_smoke_test() {
    for p in [
        std::env::temp_dir().join("subcraft-spp-cache").join("Wagenkasten.spp"),
        PathBuf::from(r"G:\Simuverse-Projekte\A3E\S_Substance\Wagenkasten.spp"),
        PathBuf::from(r"G:\Simuverse-Projekte\A3E\S_Substance\Wagenkasten_autosave_1.spp"),
    ] {
        if !p.is_file() {
            continue;
        }
        let caches = subcraft_spp::decode_texture_cache(&p, 256).expect("decode_texture_cache");
        assert!(!caches.is_empty(), "{}: expected at least 1 cached material", p.display());
        let car = &caches[0];
        assert_eq!(car.material_name, "A3E_Car");
        let rgba = car.base_color_rgba8.as_ref().expect("base_color_rgba8");
        assert_eq!(rgba.len(), 256 * 256 * 4);
        assert!(
            rgba.chunks_exact(4).all(|px| px[3] == 255),
            "{}: all base_color texels should be dilated to opaque alpha=255",
            p.display()
        );
        // Verify warm orange paint (`R > 180`, `G > 100`, `B < 30`) is present and not corrupted into pink/blue
        let orange_count = rgba
            .chunks_exact(4)
            .filter(|px| px[0] > 180 && px[1] > 100 && px[2] < 30 && px[0] > px[1] + 40)
            .count();
        assert!(
            orange_count > 1000,
            "{}: expected warm orange car paint in A3E_Car base_color, got {orange_count} orange texels",
            p.display()
        );
    }

    let path = PathBuf::from(r"G:\Simuverse-Projekte\A3E\S_Substance\Wagenkasten_autosave_1.spp");
    if !path.is_file() {
        return;
    }
    let container = subcraft_spp::read_container(&path).expect("read_container");
    let (stack, fmt, dv) =
        subcraft_spp::build_stack_from_document(&container.document_bin).expect("build_stack_from_document");
    assert_eq!(fmt, "registry");
    assert_eq!(dv, 91);
    assert_eq!(stack.materials.len(), 16);

    let mesh_rep = subcraft_mesh::import_assbin_bytes(&container.scene_assbin).expect("import_assbin_bytes");
    assert_eq!(mesh_rep.mesh.materials.len(), 15);
    assert_eq!(mesh_rep.mesh.submeshes.len(), 15);
    assert!(
        mesh_rep.mesh.triangle_count() > 80_000,
        "expected full Wagenkasten mesh including coupler (>80k tris), got {}",
        mesh_rep.mesh.triangle_count()
    );
}

#[test]
fn drehgestell_extracts_edge_wear_and_cavity_mask_generators() {
    let Some(path) = drehgestell_path() else {
        return;
    };
    let container = subcraft_spp::read_container(&path).expect("read_container");
    let (stack, _, _) =
        subcraft_spp::build_stack_from_document(&container.document_bin).expect("build_stack_from_document");

    fn find_layer<'a>(
        layers: &'a [subcraft_spp::SppLayerNode],
        name: &str,
    ) -> Option<&'a subcraft_spp::SppLayerNode> {
        for l in layers {
            if l.name == name {
                return Some(l);
            }
            if let Some(found) = find_layer(&l.children, name) {
                return Some(found);
            }
        }
        None
    }

    let bogie_layers = &stack.materials[0].texture_sets[0].layers;
    let edge_wear = find_layer(bogie_layers, "Edge Wear").expect("Edge Wear layer");
    assert!(
        edge_wear.mask_effects.iter().any(|fx| matches!(
            &fx.kind,
            subcraft_spp::SppMaskEffectKind::Generator { generator, .. } if generator == "edges"
        )),
        "Edge Wear layer should have an 'edges' generator: {:?}",
        edge_wear.mask_effects
    );

    let add_bump = find_layer(bogie_layers, "Additionnal Bump").expect("Additionnal Bump layer");
    assert!(
        add_bump.mask_effects.iter().any(|fx| matches!(
            &fx.kind,
            subcraft_spp::SppMaskEffectKind::Levels { .. }
        )),
        "Additionnal Bump layer should have a Levels mask effect: {:?}",
        add_bump.mask_effects
    );

    let cc_layers = &stack.materials[1].texture_sets[0].layers;
    let edge_damage = find_layer(cc_layers, "Edge Damage").expect("Edge Damage layer");
    assert!(
        edge_damage.mask_effects.iter().any(|fx| matches!(
            &fx.kind,
            subcraft_spp::SppMaskEffectKind::Generator { generator, .. } if generator == "edges"
        )),
        "Edge Damage layer should have an 'edges' generator: {:?}",
        edge_damage.mask_effects
    );
}

#[test]
fn texture_cache_and_generators_on_drehgestell() {
    let Some(path) = drehgestell_path() else {
        return;
    };
    let probe = probe_texture_cache(&path).expect("probe");
    assert!(probe.present);
    assert_eq!(probe.header_version, Some(3));
    assert!(probe.material_caches >= 2);
    assert!(probe.tile_datasets >= 5);
    assert!(texture_cache_decodable(&probe));

    let report = inspect(&path, true).expect("inspect");
    assert!(report.decode.painted_pixels);
    assert!(report.decode.procedural_generators);
    assert!(report.hbo_generators.procedural_nodes > 0);
    assert!(report.texture_cache.total_tile_bytes > 1_000_000);
}
