use std::path::PathBuf;

use subcraft_spp::{decode_painter_mesh_maps, PainterMeshMapKind};

fn drehgestell_path() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Drehgestell.spp");
    if root.is_file() {
        return Some(root);
    }
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/project.spp");
    scratch.is_file().then_some(scratch)
}

#[test]
fn drehgestell_imports_painter_ao_and_curvature() {
    let Some(path) = drehgestell_path() else {
        return;
    };
    let maps = decode_painter_mesh_maps(&path, 256).expect("decode_painter_mesh_maps");
    assert!(
        maps.len() >= 8,
        "expected mesh maps for both materials, got {}",
        maps.len()
    );
    let bogie_ao = maps
        .iter()
        .find(|m| m.material_name == "A3E_bogie" && m.kind == PainterMeshMapKind::AmbientOcclusion)
        .expect("bogie AO");
    assert_eq!(bogie_ao.width, 256);
    assert_eq!(bogie_ao.height, 256);
    assert_eq!(bogie_ao.channels, 1);
    assert_eq!(bogie_ao.pixels_f32.len(), 256 * 256);
    let mean = bogie_ao.pixels_f32.iter().sum::<f32>() / bogie_ao.pixels_f32.len() as f32;
    let dark = bogie_ao.pixels_f32.iter().filter(|&&v| v < 0.5).count();
    assert!(mean < 0.95, "imported AO should not be empty-white, mean={mean}");
    assert!(dark > 100, "imported AO should have occluded texels, dark={dark}");

    let bogie_curv = maps
        .iter()
        .find(|m| m.material_name == "A3E_bogie" && m.kind == PainterMeshMapKind::Curvature)
        .expect("bogie curvature");
    let cmean = bogie_curv.pixels_f32.iter().sum::<f32>() / bogie_curv.pixels_f32.len() as f32;
    let var = bogie_curv
        .pixels_f32
        .iter()
        .map(|v| (v - cmean) * (v - cmean))
        .sum::<f32>()
        / bogie_curv.pixels_f32.len() as f32;
    assert!(var > 1e-4, "curvature should vary, var={var}");

    assert!(maps.iter().any(|m| {
        m.material_name == "A3E_bogie" && m.kind == PainterMeshMapKind::WorldSpaceNormal && m.channels == 3
    }));
    assert!(maps.iter().any(|m| {
        m.material_name == "A3E_CurrentCollector" && m.kind == PainterMeshMapKind::AmbientOcclusion
    }));

    // Verify that JP2 tile rect placement (x0, y0) + top-down rows match mesh UVs and normals.
    let container = subcraft_spp::read_container(&path).expect("read_container");
    let mesh = subcraft_mesh::import_assbin_bytes(&container.scene_assbin)
        .expect("import_assbin_bytes")
        .mesh;
    let bogie_slot = mesh
        .materials
        .iter()
        .position(|m| m.name == "A3E_bogie")
        .expect("A3E_bogie slot") as u32;
    let bogie_wsn = maps
        .iter()
        .find(|m| m.material_name == "A3E_bogie" && m.kind == PainterMeshMapKind::WorldSpaceNormal)
        .expect("bogie WSN");
    let w = bogie_wsn.width as usize;
    let h = bogie_wsn.height as usize;
    let mut dot_sum = 0.0f64;
    let mut cnt = 0usize;
    for (ti, tri) in mesh.indices.chunks_exact(3).enumerate() {
        if mesh.tri_material[ti] != bogie_slot {
            continue;
        }
        let n = (mesh.normals[tri[0] as usize]
            + mesh.normals[tri[1] as usize]
            + mesh.normals[tri[2] as usize])
            .normalize_or_zero();
        if n.length_squared() < 0.5 {
            continue;
        }
        let cuv = (mesh.uvs[tri[0] as usize]
            + mesh.uvs[tri[1] as usize]
            + mesh.uvs[tri[2] as usize])
            / 3.0;
        let x = ((cuv.x * w as f32) as usize).min(w - 1);
        let y = ((cuv.y * h as f32) as usize).min(h - 1);
        let idx = (y * w + x) * 3;
        let wx = bogie_wsn.pixels_f32[idx] * 2.0 - 1.0;
        let wy = bogie_wsn.pixels_f32[idx + 1] * 2.0 - 1.0;
        let wz = bogie_wsn.pixels_f32[idx + 2] * 2.0 - 1.0;
        let wlen = (wx * wx + wy * wy + wz * wz).sqrt().max(1e-6);
        dot_sum += ((n.x * wx + n.y * wy + n.z * wz) / wlen) as f64;
        cnt += 1;
    }
    let avg_dot = dot_sum / cnt.max(1) as f64;
    assert!(
        avg_dot > 0.3,
        "WSN map must align with mesh normals & UVs after JP2 tile rect placement, got avg_dot={avg_dot:.4}"
    );
}
