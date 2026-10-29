//! Opening a Substance Painter project end to end, from a synthetic `.spp` (see the mesh crate's
//! `tests/support/spp_fixture.rs`): the texture set, flattened channel, baked maps and notes.

#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod fixture;

use subcraft_app::Session;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::LayerContent;

fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("subcraft-spp-open-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.spp"));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_painter_project_opens_with_its_flattened_channels_and_baked_maps() {
    let path = write("full", &fixture::project_file());
    let (session, notes) = Session::open_mesh(&path, 1024).unwrap();
    let doc = session.document();
    assert_eq!(doc.texture_sets.len(), 1);
    let ts = &doc.texture_sets[0];
    assert_eq!((ts.name.as_str(), ts.resolution), ("Cube", 256));
    assert!(ts.has_channel(ChannelKind::BaseColor) && ts.has_channel(ChannelKind::Roughness));
    // The flattened result, the rebuilt Painter stack, and an empty layer to paint on.
    assert_eq!(ts.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Painter project (flattened)", "Painter stack (reconstructed)", "Layer 1"]);
    // The rebuilt stack here cannot match the flattened roughness, so it stays hidden.
    assert!(ts.layers[0].visible && !ts.layers[1].visible);
    let children: Vec<(&str, bool)> = ts.layers[1].children().iter().map(|l| (l.name.as_str(), l.visible)).collect();
    assert_eq!(children, [("Rust", true), ("Dirt", false)]);
    let dirt = &ts.layers[1].children()[1];
    assert_eq!(dirt.children().iter().map(|l| (l.name.as_str(), l.visible)).collect::<Vec<_>>(), [("Scratches (not reconstructed)", false)]);
    // The uniform fill keeps its Painter color on Base Color only.
    let rust = &ts.layers[1].children()[0];
    let LayerContent::Fill(fill) = &rust.content else { panic!("Rust is a fill layer") };
    assert_eq!(fill.channels.keys().copied().collect::<Vec<_>>(), [ChannelKind::BaseColor]);
    let LayerContent::Paint(content) = &ts.layers[0].content else { panic!("flattened layer is a paint layer") };
    let rough = content.channels.get(&ChannelKind::Roughness).expect("roughness imported from the texture cache");
    assert_eq!((rough.width(), rough.height()), (256, 256));
    // Fully opaque; the 4x2 source (value 0 .. 70/255 left to right) is smoothly resampled.
    let left = rough.get(0, 128);
    let right = rough.get(255, 128);
    assert_eq!(left[1], 1.0);
    assert!(right[0] > left[0], "{left:?} {right:?}");
    // Baked maps, with the same texture-set resolution and padded background.
    let maps = session.mesh_maps(ts.id).expect("baked maps imported");
    assert!(maps.get(MeshMapKind::AmbientOcclusion).is_some() && maps.get(MeshMapKind::Position).is_some());
    assert!(maps.get(MeshMapKind::Curvature).is_none());
    assert_eq!(maps.get(MeshMapKind::Position).unwrap().comps(), 3);
    let text = notes.join("\n");
    assert!(text.contains("Texture set 'Cube'") && text.contains("Rust") && text.contains("Dirt") && text.contains("Rebuilt stack"), "{text}");
    assert!(text.contains("Baked maps: Ambient Occlusion, Position") || text.contains("Baked maps:"), "{text}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_project_without_a_readable_document_still_gives_the_model() {
    use std::io::Write;
    // Just a model: no document, no textures.
    let bytes = {
        // Reuse the full file but break the document by replacing it with garbage of the same kind.
        let mut b = fixture::project_file();
        // Corrupt the document magic: find it and overwrite.
        let magic = 0x1b7c_2fddu32.to_le_bytes();
        let at = b.windows(4).position(|w| w == magic).unwrap();
        b[at] ^= 0xff;
        b
    };
    let path = write("nodoc", &bytes);
    let (session, notes) = Session::open_mesh(&path, 512).unwrap();
    assert_eq!(session.scene().mesh.triangle_count(), 2);
    assert!(notes.iter().any(|n| n.contains("only the model was imported")), "{notes:?}");
    std::io::stderr().flush().ok();
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn files_that_are_not_projects_are_reported() {
    let path = write("junk", b"not an hdf5 file at all");
    let err = Session::open_mesh(&path, 512).err().expect("junk is rejected");
    assert!(err.contains("Substance Painter"), "{err}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

fn layer_shape(session: &Session) -> Vec<(String, bool)> {
    session.document().texture_sets[0].layers.iter().map(|l| (l.name.clone(), l.visible)).collect()
}

/// Polls `job` the way the UI does until it is over; returns the session and the events seen.
fn run_job(job: &mut subcraft_app::SppImportJob, mut session: Option<Session>) -> (Option<Session>, Vec<&'static str>, Result<(), String>) {
    use subcraft_app::SppPoll;
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut placeholder = Session::new(subcraft_mesh::primitives::cube(1.0), 16);
    loop {
        assert!(std::time::Instant::now() < deadline, "the import did not finish");
        let target = session.as_mut().unwrap_or(&mut placeholder);
        match job.poll(target) {
            SppPoll::Idle => std::thread::sleep(std::time::Duration::from_millis(2)),
            SppPoll::Opened(s, _) => {
                seen.push("opened");
                session = Some(*s);
            }
            SppPoll::StackInstalled(_) => seen.push("stack"),
            SppPoll::Discarded => seen.push("discarded"),
            SppPoll::Finished(r) => return (session, seen, r),
        }
    }
}

#[test]
fn a_background_import_opens_first_and_ends_with_the_same_stack_as_the_blocking_open() {
    let path = write("job", &fixture::project_file());
    let (blocking, _) = Session::open_mesh(&path, 1024).unwrap();
    let mut job = subcraft_app::SppImportJob::start(&path);
    let (session, seen, result) = run_job(&mut job, None);
    assert_eq!(result, Ok(()));
    assert_eq!(seen, ["opened", "stack"]);
    let session = session.unwrap();
    assert_eq!(layer_shape(&session), layer_shape(&blocking));
    let (a, b) = (&session.document().texture_sets[0], &blocking.document().texture_sets[0]);
    let names = |l: &subcraft_doc::Layer| l.children().iter().map(|c| (c.name.clone(), c.visible)).collect::<Vec<_>>();
    assert_eq!(names(&a.layers[1]), names(&b.layers[1]));
    // Still as opened: the stack replaced the opened state, there is nothing to undo.
    assert!(!session.history().can_undo());
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_cancelled_background_import_ends_without_a_project() {
    let path = write("cancel", &fixture::project_file());
    let control = subcraft_app::ImportControl::new();
    control.cancel();
    let mut events = 0;
    let result = subcraft_app::spp_import::import_spp(&path, &control, &mut |_| events += 1);
    assert_eq!(result, Err("cancelled".to_string()));
    assert_eq!(events, 0, "nothing is handed out after cancelling");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_rebuilt_stack_is_not_installed_into_another_project() {
    let path = write("other", &fixture::project_file());
    let mut job = subcraft_app::SppImportJob::start(&path);
    // Hand the job a different session once it has opened its own.
    let (_, _, _) = (0, 0, 0);
    let mut other = Session::new(subcraft_mesh::primitives::cube(1.0), 16);
    let before = layer_shape(&other);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut seen = Vec::new();
    loop {
        assert!(std::time::Instant::now() < deadline);
        match job.poll(&mut other) {
            subcraft_app::SppPoll::Idle => std::thread::sleep(std::time::Duration::from_millis(2)),
            subcraft_app::SppPoll::Opened(..) => seen.push("opened"), // dropped: the user opened something else
            subcraft_app::SppPoll::Discarded => { seen.push("discarded"); }
            subcraft_app::SppPoll::StackInstalled(_) => seen.push("installed"),
            subcraft_app::SppPoll::Finished(_) => break,
        }
    }
    assert_eq!(seen, ["opened", "discarded"]);
    assert_eq!(layer_shape(&other), before);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_stack_arriving_after_the_user_edited_is_one_undo_step_and_keeps_the_edits() {
    let path = write("edited", &fixture::project_file());
    let mut job = subcraft_app::SppImportJob::start(&path);
    let mut session: Option<Session> = None;
    let mut placeholder = Session::new(subcraft_mesh::primitives::cube(1.0), 16);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        assert!(std::time::Instant::now() < deadline);
        match job.poll(session.as_mut().unwrap_or(&mut placeholder)) {
            subcraft_app::SppPoll::Opened(mut s, _) => {
                // The user renames the layer to paint on before the rebuilt stack arrives.
                let top = s.document().texture_sets[0].layers.last().unwrap().id;
                s.execute(subcraft_app::Command::Rename { layer: top, name: "My paint".into() }).unwrap();
                session = Some(*s);
            }
            subcraft_app::SppPoll::StackInstalled(_) => break,
            subcraft_app::SppPoll::Idle => std::thread::sleep(std::time::Duration::from_millis(2)),
            subcraft_app::SppPoll::Finished(r) => panic!("finished without a stack: {r:?}"),
            subcraft_app::SppPoll::Discarded => panic!("discarded"),
        }
    }
    let mut s = session.unwrap();
    assert_eq!(layer_shape(&s).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Painter project (flattened)", "Painter stack (reconstructed)", "My paint"]);
    assert_eq!(s.history().undo_label(), Some("Rebuild Painter stack"));
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert_eq!(layer_shape(&s).len(), 2);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}
