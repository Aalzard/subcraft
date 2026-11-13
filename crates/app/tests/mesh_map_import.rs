//! Importing already-baked mesh maps into a new project's texture sets (UDIM tiles matched by
//! the number in the file name), and deriving curvature from an imported normal map.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use subcraft_app::mesh_map_import::plan_import;
use subcraft_app::new_project::NewProjectSettings;
use subcraft_app::{MeshMapKind, Session};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::{Corner, MeshBuilder};

/// Two quads side by side: the left one on UDIM 1001, the right one on 1002.
fn two_tile_model() -> subcraft_mesh::Mesh {
    let mut b = MeshBuilder::new("Tiles");
    let m = b.material("Mat");
    for (x0, u0) in [(-1.0f32, 0.0f32), (0.0, 1.0)] {
        let c = |x: f32, z: f32, u: f32, v: f32| Corner { position: Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(u, v)) };
        let (a, bb, cc, d) = (c(x0, 0.0, u0 + 0.05, 0.95), c(x0 + 1.0, 0.0, u0 + 0.95, 0.95), c(x0 + 1.0, 1.0, u0 + 0.95, 0.05), c(x0, 1.0, u0 + 0.05, 0.05));
        b.push_triangle(m, [a, cc, bb]);
        b.push_triangle(m, [a, d, cc]);
    }
    b.build().unwrap()
}

fn write(dir: &std::path::Path, name: &str, size: u32, comps: usize, px: &[f32]) -> PathBuf {
    let path = dir.join(name);
    let data: Vec<f32> = px.iter().copied().cycle().take((size * size) as usize * comps).collect();
    save_image(&path, size, size, comps, &data, FileFormat::Png8).unwrap();
    path
}

#[test]
fn baked_maps_go_into_the_udim_sets_they_name_without_a_rebake() {
    let dir = std::env::temp_dir().join(format!("subcraft-mesh-map-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = NewProjectSettings { resolution: 256, udim: true, ..Default::default() };
    let mut s = Session::from_mesh_with(two_tile_model(), &settings).unwrap();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.udim_number())).collect();
    assert_eq!(sets.iter().map(|s| s.1).collect::<Vec<_>>(), vec![Some(1001), Some(1002)]);

    let files = vec![
        write(&dir, "tiles_normal.1001.png", 64, 3, &[0.5, 0.5, 1.0]),
        write(&dir, "tiles_ambient_occlusion.1002.png", 128, 1, &[0.25]),
        write(&dir, "tiles_id.1002.png", 256, 3, &[1.0, 0.0, 0.0]),
        write(&dir, "tiles_basecolor.1001.png", 16, 3, &[0.2, 0.2, 0.2]),
    ];
    let plan = plan_import(&files, &s.mesh_map_set_keys());
    assert_eq!(plan[0].kind, Some(MeshMapKind::Normal));
    assert_eq!(plan[0].set, Some(0));
    assert_eq!(plan[1].kind, Some(MeshMapKind::AmbientOcclusion));
    assert_eq!(plan[1].set, Some(1));
    assert_eq!(plan[2].set, Some(1));
    assert_eq!(plan[3].kind, None, "not a mesh map: skipped");

    let assignments = s.mesh_map_assignments(&plan);
    assert_eq!(assignments.len(), 3);
    let report = s.import_mesh_maps(&assignments, true);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.imported.len(), 3);
    assert_eq!(report.derived.len(), 1, "curvature derived for the set that got a normal map");

    let tile1 = s.mesh_maps(sets[0].0).unwrap();
    let normal = tile1.get(MeshMapKind::Normal).unwrap();
    assert_eq!((normal.width(), normal.comps()), (256, 3), "resampled to the set resolution");
    let c = tile1.get(MeshMapKind::Curvature).expect("curvature from the flat normal map");
    assert!((c.get(128, 128)[0] - 0.5).abs() < 1e-3, "flat plane: flat curvature");
    assert!(tile1.get(MeshMapKind::AmbientOcclusion).is_none(), "the AO of tile 1002 stays there");
    let tile2 = s.mesh_maps(sets[1].0).unwrap();
    assert!((tile2.get(MeshMapKind::AmbientOcclusion).unwrap().get(100, 100)[0] - 0.25).abs() < 3e-3);
    assert!(s.has_id_map(sets[1].0));
    assert_eq!(s.id_palette(sets[1].0).unwrap().len(), 1);
    let summary = report.summary();
    assert!(summary.starts_with("Imported 3 mesh maps") && summary.contains("curvature derived"), "{summary}");
    std::fs::remove_dir_all(&dir).ok();
}
