//! Export `scene.assbin` → GLB via Painter's `assimp.dll` when available.

use std::path::PathBuf;

use subcraft_spp::{open_container, try_assbin_to_glb, painter_assimp_dll_available};

fn fixture_spp() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUBCRAFT_SPP_FIXTURE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let local = PathBuf::from(r"C:\Users\princ\Desktop\Substance Klon\Drehgestell.spp");
    local.is_file().then_some(local)
}

#[test]
fn painter_dll_exports_drehgestell_mesh_to_glb() {
    if !painter_assimp_dll_available() {
        eprintln!("skip: Painter assimp.dll not found (SUBCRAFT_PAINTER_ASSIMP or default install)");
        return;
    }
    let Some(spp) = fixture_spp() else {
        eprintln!("skip: no .spp fixture");
        return;
    };
    let container = open_container(&spp).expect("hdf5");
    let dir = std::env::temp_dir().join("subcraft-painter-assimp-test");
    std::fs::create_dir_all(&dir).unwrap();
    let assbin = dir.join("scene.assbin");
    let glb = dir.join("scene.glb");
    std::fs::write(&assbin, &container.scene_assbin).unwrap();
    let out = try_assbin_to_glb(&assbin, &glb).expect("export");
    assert!(out.is_file());
    assert!(out.metadata().unwrap().len() > 100_000);
    let mesh = subcraft_mesh::import_mesh(&out).expect("import export");
    assert!(mesh.mesh.positions.len() > 1000);
}
