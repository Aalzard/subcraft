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

/// Issue #733: additional ID maps bake beside the ID map, each from its own source file with its
/// own colour options, and are stored as their own maps.
#[test]
fn additional_id_maps_bake_from_their_own_source_and_settings() {
    use subcraft_app::{ExtraIdBake, IdGenerator};
    let dir = std::env::temp_dir().join(format!("subcraft-extra-ids-{}", subcraft_core::LayerId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let (whole, left) = (dir.join("whole_high.obj"), dir.join("left_high.obj"));
    // The main high poly covers the plane in two separate halves; the ID 2 source only the left one.
    std::fs::write(&whole, "v -1 0.005 -1\nv -1 0.005 1\nv -0.02 0.005 1\nv -0.02 0.005 -1\nv 0.02 0.005 -1\nv 0.02 0.005 1\nv 1 0.005 1\nv 1 0.005 -1\nf 1 2 3\nf 1 3 4\nf 5 6 7\nf 5 7 8\n").unwrap();
    std::fs::write(&left, "v -1 0.005 -1\nv -1 0.005 1\nv -0.02 0.005 1\nv -0.02 0.005 -1\nf 1 2 3\nf 1 3 4\n").unwrap();
    let mut s = Session::new(primitives::plane(2.0, 4), 128);
    let ts = s.selection().texture_set;
    let mut from_file = ExtraIdBake::new(MeshMapKind::Id2);
    from_file.source = left.display().to_string();
    let mut gray = ExtraIdBake::new(MeshMapKind::Id3);
    gray.id_generator = IdGenerator::Grayscale;
    let mut off = ExtraIdBake::new(MeshMapKind::Id4);
    off.enabled = false;
    let settings = BakeSettings { size: 128, maps: vec![MeshMapKind::Id], extra_ids: vec![from_file, gray, off], ..Default::default() };
    let (result, log) = run(&mut s, settings, &whole, BakeFiles::default());
    assert_eq!(result, Ok(1), "{log:?}");
    let maps = s.mesh_maps(ts).unwrap();
    let at = |k: MeshMapKind| { let m = maps.get(k).unwrap_or_else(|| panic!("{k:?} baked")); (m.get(24, 40), m.get(104, 88)) };
    let (a, b) = at(MeshMapKind::Id);
    assert!(a[..3] != [0.0; 3] && b[..3] != [0.0; 3] && a[..3] != b[..3], "the ID map covers both halves: {a:?} {b:?}");
    // ID 2: one half has its ID, the half its file lacks stays black (no neighbour's ID).
    let (a2, b2) = at(MeshMapKind::Id2);
    let black = |c: [f32; 4]| c[..3] == [0.0; 3];
    assert!(black(a2) != black(b2), "only the file's half has an ID: {a2:?} {b2:?}");
    // ID 3: the main high poly with gray IDs, so both halves differ from the colour IDs.
    let (a3, b3) = at(MeshMapKind::Id3);
    assert!(a3[0] == a3[1] && a3[1] == a3[2] && b3[0] == b3[1] && a3[..3] != b3[..3], "gray IDs: {a3:?} {b3:?}");
    assert!(maps.get(MeshMapKind::Id4).is_none(), "an unticked entry is not baked");
    assert!(log.iter().any(|l| l.contains("ID 2: source") && l.contains("left_high.obj")), "{log:?}");

    // A missing source fails that map with its name; the others still bake.
    let mut missing = ExtraIdBake::new(MeshMapKind::Id5);
    missing.source = dir.join("nope.obj").display().to_string();
    let settings = BakeSettings { size: 128, maps: vec![MeshMapKind::Id], extra_ids: vec![missing], ..Default::default() };
    let (result, log) = run(&mut s, settings, &whole, BakeFiles::default());
    assert!(result.is_err(), "{result:?}");
    assert!(log.iter().any(|l| l.contains("ID 5") && l.contains("nope.obj") && l.contains("could not be loaded")), "{log:?}");
    let _ = std::fs::remove_dir_all(dir);
}
