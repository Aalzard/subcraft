//! Issue #93: export several raw baked mesh maps of every texture set in one go, with file names
//! that name the set and the map, kept apart from the material textures.
use subcraft_app::{BakeRequest, BakeSettings, MeshMapKind, Session};
use subcraft_export::{ExportSettings, Padding};
use subcraft_image::FileFormat;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-baked-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// The sample crate (two texture sets: Body and Trim) with Normal, AO and Curvature baked on both.
fn baked() -> Session {
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), 64);
    let sets: Vec<_> = s.document().texture_sets.iter().enumerate().map(|(i, ts)| (ts.id, i)).collect();
    assert_eq!(sets.len(), 2);
    let request = BakeRequest {
        settings: BakeSettings { size: 64, maps: vec![MeshMapKind::Normal, MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature], ao_samples: 8, ..BakeSettings::default() },
        high_poly: None,
        texture_sets: sets.iter().map(|&(id, _)| (id, 0)).collect(),
    };
    let mut job = s.start_bake(request);
    let result = loop {
        if let Some(r) = job.poll(&mut s) {
            break r;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(result, Ok(2));
    s
}

#[test]
fn every_selected_map_of_every_set_is_one_named_file_with_the_baked_pixels() {
    let mut s = baked();
    assert_eq!(s.baked_map_kinds(), vec![MeshMapKind::Normal, MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature]);
    let settings = ExportSettings { padding: Padding::None, ..Default::default() };
    let dir = temp("all");
    let kinds = [MeshMapKind::Normal, MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature];
    let files = s.export_baked_maps(&dir, &kinds, false, &settings).unwrap();
    assert_eq!(files.len(), 6, "3 maps × 2 texture sets: {files:?}");
    let mut names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 6, "no two files share a name");
    for set in ["Body", "Trim"] {
        for suffix in ["baked_normal", "baked_ambient_occlusion", "baked_curvature"] {
            assert!(names.iter().any(|n| n.contains(&format!("_{set}_{suffix}."))), "{set} {suffix} in {names:?}");
        }
    }

    // The AO file holds the baked AO of its set, as one grey channel.
    let body = s.document().texture_sets.iter().find(|ts| ts.name == "Body").unwrap().id;
    let ao = s.mesh_maps(body).unwrap().get(MeshMapKind::AmbientOcclusion).unwrap().clone();
    let file = files.iter().find(|f| f.to_string_lossy().contains("_Body_baked_ambient_occlusion.")).unwrap();
    let out = subcraft_image::load_bitmap(file).unwrap();
    assert_eq!((out.width, out.height), (ao.width(), ao.height()));
    for (x, y) in [(5, 5), (20, 33), (40, 12), (60, 60)] {
        let want = ao.get(x, y)[0].clamp(0.0, 1.0);
        let got = out.texel(x, y, false)[0];
        assert!((got - want).abs() <= 0.5 / 255.0 + 1e-4, "AO ({x},{y}): {got} vs {want}");
    }

    // The material textures are not part of it.
    assert!(names.iter().all(|n| n.contains("_baked_")), "only raw baked maps: {names:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn selected_set_only_unbaked_maps_and_the_format_override() {
    let mut s = baked();
    let dir = temp("one");
    let settings = ExportSettings { padding: Padding::None, format_override: Some(FileFormat::Png16), ..Default::default() };
    // Thickness was not baked: skipped, not an error.
    let files = s.export_baked_maps(&dir, &[MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness], true, &settings).unwrap();
    assert_eq!(files.len(), 1, "only the selected set's AO: {files:?}");
    let selected = s.selected_texture_set().name.clone();
    assert!(files[0].to_string_lossy().contains(&format!("_{selected}_baked_ambient_occlusion.png")));
    std::fs::remove_dir_all(dir).unwrap();

    let mut fresh = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    assert!(fresh.baked_map_kinds().is_empty(), "nothing baked yet");
    let dir = temp("none");
    assert!(fresh.export_baked_maps(&dir, &MeshMapKind::ALL, false, &ExportSettings::default()).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
