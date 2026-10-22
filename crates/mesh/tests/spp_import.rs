//! Reads real Substance Painter projects when they are present (set `SUBCRAFT_SPP_SAMPLES` to a
//! folder-separated list of .spp files); the structural tests below use a hand-built file.

use subcraft_mesh::import::import_mesh;

#[test]
fn real_projects_import_when_samples_are_provided() {
    let Ok(list) = std::env::var("SUBCRAFT_SPP_SAMPLES") else { return };
    for path in std::env::split_paths(&list) {
        let report = import_mesh(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(report.mesh.triangle_count() > 0, "{} has triangles", path.display());
        assert!(!report.mesh.objects.is_empty(), "{} keeps object names", path.display());
        assert!(report.stats.missing_uv_triangles == 0, "{} has UVs", path.display());
    }
}

#[test]
fn files_that_are_not_hdf5_are_rejected() {
    let dir = std::env::temp_dir().join(format!("subcraft-spp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bad.spp");
    std::fs::write(&p, b"definitely not hdf5").unwrap();
    let err = import_mesh(&p).unwrap_err().to_string();
    assert!(err.contains("Substance Painter project"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}
