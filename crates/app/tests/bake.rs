//! Background bake: several high-poly files, matching by mesh name, maps streamed into the session.

use glam::{Vec2, Vec3};
use subcraft_app::bake_job::merge_meshes;
use subcraft_app::{BakeRequest, BakeSettings, MapStatus, MatchMode, MeshMapKind, Session, map_preview_rgba};
use subcraft_image::TiledImage;
use subcraft_mesh::{Corner, Mesh, MeshBuilder, import::write_obj};

fn corner(p: Vec3, uv: Vec2) -> Corner {
    Corner { position: p, normal: Some(Vec3::Y), uv: Some(uv) }
}

/// Square of side 2 at `x0` in the XZ plane, height from `h`, UVs in `[u0, u0 + 0.48]`.
fn grid(b: &mut MeshBuilder, x0: f32, u0: f32, steps: u32, h: impl Fn(f32, f32) -> f32) {
    let m = b.material("Body");
    let p = |i: u32, j: u32| {
        let (x, z) = (j as f32 / steps as f32 * 2.0 - 1.0, i as f32 / steps as f32 * 2.0 - 1.0);
        Corner { position: Vec3::new(x0 + x, h(x, z), z), normal: None, uv: Some(Vec2::new(u0 + (x + 1.0) * 0.24, 0.01 + (z + 1.0) * 0.49)) }
    };
    for i in 0..steps {
        for j in 0..steps {
            b.push_triangle(m, [p(i, j), p(i + 1, j), p(i + 1, j + 1)]);
            b.push_triangle(m, [p(i, j), p(i + 1, j + 1), p(i, j + 1)]);
        }
    }
}

fn low_poly() -> Mesh {
    let mut b = MeshBuilder::new("Low");
    let m = b.material("Body");
    let quad = |b: &mut MeshBuilder, x0: f32, u0: f32| {
        let c = |x: f32, z: f32| corner(Vec3::new(x0 + x, 0.0, z), Vec2::new(u0 + (x + 1.0) * 0.24, 0.01 + (z + 1.0) * 0.49));
        b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(-1.0, -1.0), c(1.0, 1.0), c(1.0, -1.0)]);
    };
    b.object("Panel_low");
    quad(&mut b, 0.0, 0.01);
    b.object("Crate_low");
    quad(&mut b, 10.0, 0.51);
    b.build().unwrap()
}

#[test]
fn bake_streams_maps_from_several_high_poly_files_matched_by_name() {
    let dir = std::env::temp_dir().join(format!("subcraft-bake-match-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // File 1: the panel's high poly with a bump. File 2: an unrelated part above the panel.
    let mut b = MeshBuilder::new("PanelHigh");
    b.object("Panel_high");
    grid(&mut b, 0.0, 0.0, 48, |x, z| 0.1 * (-(x * x + z * z) / 0.08).exp());
    let panel_high = dir.join("panel_high.obj");
    write_obj(&b.build().unwrap(), &panel_high).unwrap();
    let mut b = MeshBuilder::new("Other");
    b.object("Screw_high");
    grid(&mut b, 0.0, 0.0, 2, |x, _| 0.02 + 0.02 * x);
    let screw_high = dir.join("screw_high.obj");
    write_obj(&b.build().unwrap(), &screw_high).unwrap();

    let mut s = Session::new(low_poly(), 128);
    let ts = s.document().texture_sets[0].id;
    let maps = vec![MeshMapKind::Normal, MeshMapKind::Height, MeshMapKind::Opacity, MeshMapKind::BentNormal];
    let diag = s.scene().mesh.bounds.diagonal();
    let req = BakeRequest {
        settings: BakeSettings {
            size: 128,
            maps: maps.clone(),
            ao_samples: 8,
            cage_front: 0.15 / diag,
            cage_rear: 0.15 / diag,
            match_mode: MatchMode::ByMeshName,
            ..BakeSettings::default()
        },
        high_poly: vec![panel_high, screw_high],
        texture_sets: vec![(ts, 0)],
    };
    let mut job = s.start_bake(req);
    assert_eq!(job.status(ts, MeshMapKind::Opacity), MapStatus::Queued);
    let mut seen_lengths = Vec::new();
    let result = loop {
        let r = job.poll(&mut s);
        if seen_lengths.last() != Some(&job.finished().len()) {
            seen_lengths.push(job.finished().len());
            // Every finished map is already installed in the session.
            for (t, k) in job.finished() {
                assert!(s.mesh_maps(*t).and_then(|m| m.get(*k)).is_some());
            }
        }
        if let Some(r) = r {
            break r;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    assert_eq!(result, Ok(1));
    assert_eq!(job.finished().iter().map(|(_, k)| *k).collect::<Vec<_>>(), maps, "maps arrive one by one, in order");
    assert!(maps.iter().all(|k| job.status(ts, *k) == MapStatus::Done));
    assert_eq!(job.current(), None);

    let report = job.matching().expect("by-name report");
    assert_eq!(report.matched, vec!["panel".to_string()]);
    assert_eq!(report.unmatched_low, vec!["Crate_low".to_string()]);
    assert_eq!(report.unmatched_high, vec!["Screw_high".to_string()]);

    let baked = s.mesh_maps(ts).unwrap().clone();
    let (height, opacity, normal) = (baked.get(MeshMapKind::Height).unwrap(), baked.get(MeshMapKind::Opacity).unwrap(), baked.get(MeshMapKind::Normal).unwrap());
    // Panel center (u ≈ 0.25, v ≈ 0.5) sits under the bump, not under the screw; crate (u ≈ 0.75) has no partner.
    let px = |img: &TiledImage, u: f32, v: f32| img.get((u * 128.0) as u32, (v * 128.0) as u32);
    let h = px(height, 0.25, 0.5)[0];
    assert!((h - (0.5 + 0.5 * 0.1 / 0.15)).abs() < 0.04, "bump top height {h}");
    assert_eq!(px(opacity, 0.25, 0.5)[0], 1.0);
    assert_eq!(px(opacity, 0.75, 0.5)[0], 0.0, "unmatched low part");
    let n = px(normal, 0.25 + 0.24 * 0.4, 0.5);
    assert!((n[0] - 0.5).abs() > 0.05, "bump slope in the normal map ({n:?})");

    // New map kinds round-trip through the project file.
    let path = dir.join("baked.subcraft");
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    let again = reopened.mesh_maps(ts).unwrap();
    for k in &maps {
        assert_eq!(again.get(*k).unwrap().to_dense(), baked.get(*k).unwrap().to_dense(), "{k:?} saved");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn merged_high_poly_keeps_objects_and_materials() {
    let mut a = MeshBuilder::new("A");
    a.object("Bolt_high");
    grid(&mut a, 0.0, 0.0, 2, |_, _| 0.0);
    let mut b = MeshBuilder::new("B");
    b.object("Nut_high");
    grid(&mut b, 3.0, 0.0, 3, |_, _| 0.0);
    let (a, b) = (a.build().unwrap(), b.build().unwrap());
    let merged = merge_meshes(vec![a.clone(), b.clone()]);
    assert_eq!(merged.triangle_count(), a.triangle_count() + b.triangle_count());
    let names: Vec<&str> = merged.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Bolt_high", "Nut_high"]);
    assert_eq!(merged.materials.len(), 1, "same material name merges");
    assert!((merged.bounds.max.x - 4.0).abs() < 1e-5);
}

#[test]
fn map_previews_show_stored_values() {
    let mut img = TiledImage::new(256, 256, 1, [0.5; 4]);
    img.set(0, 0, [1.0; 4]);
    let rgba = map_preview_rgba(&img, 64);
    assert_eq!(rgba.len(), 64 * 64 * 4);
    assert_eq!(&rgba[4..8], &[128, 128, 128, 255], "scalar maps are gray");
    let rgb = map_preview_rgba(&TiledImage::new(100, 100, 3, [0.5, 0.5, 1.0, 0.0]), 16);
    assert_eq!(&rgb[..4], &[128, 128, 255, 255], "normal map colors as stored");
}
