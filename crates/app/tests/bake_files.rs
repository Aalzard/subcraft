//! Bakes that read more files than one high poly: several high-poly files, a custom cage.

use subcraft_app::{BakeFiles, BakeRequest, BakeSettings, MeshMapKind, Session};
use subcraft_mesh::primitives;

fn run(s: &mut Session, settings: BakeSettings, high: &std::path::Path, files: BakeFiles) -> (Result<usize, String>, Vec<String>) {
    let ts = s.selection().texture_set;
    let mut job = s.start_bake_files(BakeRequest { settings, high_poly: Some(high.to_path_buf()), texture_sets: vec![(ts, 0)] }, None, files);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let result = loop {
        if let Some(result) = job.poll(s) { break result; }
        assert!(std::time::Instant::now() < deadline, "bake worker timed out");
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    (result, job.log().iter().map(|l| l.text.clone()).collect())
}

/// The ID map left and right of the middle of the plane.
fn id_sides(s: &Session) -> ([f32; 4], [f32; 4]) {
    let ts = s.selection().texture_set;
    let id = s.mesh_maps(ts).unwrap().get(MeshMapKind::Id).unwrap().clone();
    // Off-centre along both axes, so the samples are on opposite halves whichever way the UVs run.
    (id.get(24, 40), id.get(104, 88))
}

#[test]
fn several_high_poly_files_bake_as_one_and_a_cage_file_sets_the_ray_starts() {
    let dir = std::env::temp_dir().join(format!("subcraft-bake-files-{}", subcraft_core::LayerId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    // Two high-poly files, each over one half of a 2 x 2 plane, just above it.
    let (left, right) = (dir.join("left_high.obj"), dir.join("right_high.obj"));
    std::fs::write(&left, "v -1 0.005 -1\nv -1 0.005 1\nv -0.02 0.005 1\nv -0.02 0.005 -1\nf 1 2 3\nf 1 3 4\n").unwrap();
    std::fs::write(&right, "v 0.02 0.005 -1\nv 0.02 0.005 1\nv 1 0.005 1\nv 1 0.005 -1\nf 1 2 3\nf 1 3 4\n").unwrap();
    let mut s = Session::new(primitives::plane(2.0, 4), 128);
    let settings = BakeSettings { size: 128, maps: vec![MeshMapKind::Id], ..Default::default() };

    // One file covers one half: the other half's rays miss (reported in the log), and those
    // texels take the baked half's ID instead of black (issue #466).
    let (result, log) = run(&mut s, settings.clone(), &left, BakeFiles::default());
    assert_eq!(result, Ok(1));
    let (a, b) = id_sides(&s);
    assert!(a[..3] != [0.0; 3] && a[..3] == b[..3], "one file covers one half: {a:?} {b:?}");
    assert!(log.iter().any(|l| l.starts_with("Cage: ")), "{log:?}");

    let files = BakeFiles { extra_high_polys: vec![right.clone()], cage: None };
    let (result, log) = run(&mut s, settings.clone(), &left, files);
    assert_eq!(result, Ok(1));
    let (a, b) = id_sides(&s);
    assert!(a[..3] != [0.0; 3] && b[..3] != [0.0; 3], "both files are baked: {a:?} {b:?}");
    assert!(a[..3] != b[..3], "and stay separate mesh elements");
    assert!(log.iter().any(|l| l.contains("right_high.obj")), "{log:?}");

    // A missing extra file fails the bake with its name.
    let files = BakeFiles { extra_high_polys: vec![dir.join("missing_high.obj")], cage: None };
    let (result, log) = run(&mut s, settings.clone(), &left, files);
    assert!(result.is_err(), "{result:?}");
    assert!(log.iter().any(|l| l.contains("missing_high.obj") && l.contains("could not be loaded")), "{log:?}");

    // The same high poly lifted far above the plane is out of reach of the distance cage; a cage
    // file (the plane with its UVs, lifted above the high poly) reaches it.
    let far = dir.join("far_high.obj");
    std::fs::write(&far, "v -1 0.5 -1\nv -1 0.5 1\nv 1 0.5 1\nv 1 0.5 -1\nf 1 2 3\nf 1 3 4\n").unwrap();
    let (result, _) = run(&mut s, settings.clone(), &far, BakeFiles::default());
    assert!(result.is_err(), "the distance cage does not reach it: {result:?}");
    let cage = dir.join("cage.obj");
    let plane = primitives::plane(2.0, 4);
    let mut obj = String::new();
    for p in &plane.positions { obj += &format!("v {} {} {}\n", p.x, p.y + 1.0, p.z); }
    for uv in &plane.uvs { obj += &format!("vt {} {}\n", uv.x, 1.0 - uv.y); }
    for t in plane.indices.chunks_exact(3) { obj += &format!("f {0}/{0} {1}/{1} {2}/{2}\n", t[0] + 1, t[1] + 1, t[2] + 1); }
    std::fs::write(&cage, obj).unwrap();
    let (result, log) = run(&mut s, settings, &far, BakeFiles { extra_high_polys: Vec::new(), cage: Some(cage) });
    assert_eq!(result, Ok(1), "{log:?}");
    let (a, b) = id_sides(&s);
    assert!(a[..3] != [0.0; 3] && b[..3] != [0.0; 3], "the cage's rays reach the far high poly: {a:?} {b:?}");
    assert!(log.iter().any(|l| l.starts_with("Cage: ")), "{log:?}");
    let _ = std::fs::remove_dir_all(dir);
}
