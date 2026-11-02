//! Edit in PhotoCraft: files out, a save in the editor comes back as one undo step.
use std::path::{Path, PathBuf};

use photocraft_psd::layer::ChannelData;
use photocraft_psd::{Compression, PsdFile};
use subcraft_app::photocraft::{Exchange, PollEvent};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;

fn session() -> Session {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer, name: "Paint".into() }).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.execute(Command::Seal).unwrap();
    s
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-photocraft-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn start(s: &Session, dir: &Path) -> (Exchange, Vec<PathBuf>) {
    let mut opened = Vec::new();
    let ex = Exchange::start(s, dir, |paths| { opened = paths.to_vec(); Ok(()) }).unwrap();
    (ex, opened)
}

/// Paints the whole "Paint" layer of `path` opaque with one 16-bit value, as an editor save would.
fn paint_file(path: &Path, value: u16) {
    let mut file = PsdFile::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let (depth, version, cc) = (file.header.depth, file.header.version, if file.header.color_mode == photocraft_psd::ColorMode::Rgb { 3 } else { 1 });
    let rec = file.layers_mut().iter_mut().find(|r| r.name() == "Paint").unwrap();
    rec.rect = photocraft_psd::layer::Rect::from_xywh(0, 0, 64, 64);
    let plane = |v: u16| -> Vec<u8> { std::iter::repeat_n(v.to_be_bytes(), 64 * 64).flatten().collect() };
    rec.channels = (0..cc as i16).map(|c| ChannelData::encode(c, Compression::Raw, &plane(value), 64, 64, depth, version).unwrap())
        .chain([ChannelData::encode(-1, Compression::Raw, &plane(u16::MAX), 64, 64, depth, version).unwrap()])
        .collect();
    // A different size than before so the save is seen even within the file time resolution.
    std::fs::write(path, file.to_bytes().unwrap()).unwrap();
}

#[test]
fn start_writes_one_psd_per_channel_and_opens_them_together() {
    let s = session();
    let d = dir("start");
    let (ex, opened) = start(&s, &d);
    let names: Vec<String> = opened.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let set = s.selected_texture_set();
    assert_eq!(names.len(), set.channel_kinds().count());
    assert!(names.iter().any(|n| n.ends_with("_Base_Color.psd") || n.ends_with("_Base Color.psd")), "{names:?}");
    assert!(opened.iter().all(|p| p.is_file() && p.starts_with(&d)));
    assert_eq!(ex.texture_set(), set.id);
    // Opening fails → no exchange.
    assert!(Exchange::start(&s, &dir("fail"), |_| Err("not found".into())).is_err());
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_settled_save_comes_back_as_one_undoable_step() {
    let mut s = session();
    let d = dir("save");
    let (mut ex, opened) = start(&s, &d);
    let roughness = opened.iter().find(|p| p.to_string_lossy().contains("Roughness")).unwrap().clone();
    assert!(ex.check(&mut s).is_empty(), "nothing saved yet");
    paint_file(&roughness, 32768);
    assert!(ex.check(&mut s).is_empty(), "first sighting waits for the write to settle");
    let count = s.history().entries().len();
    let events = ex.check(&mut s);
    assert!(matches!(&events[..], [PollEvent::Applied(t)] if t.contains("Roughness") && t.contains("1 tile")), "{events:?}");
    assert_eq!(s.history().entries().len(), count + 1);
    assert_eq!(s.history().entries().last().unwrap().label, "Edit in PhotoCraft (Roughness)");
    let set = s.selected_texture_set();
    let paint = set.layers.iter().find(|l| l.name == "Paint").unwrap();
    let LayerContent::Paint(p) = &paint.content else { panic!() };
    let v = p.channels[&ChannelKind::Roughness].get(10, 10);
    assert!((v[0] - 0.5).abs() < 1e-3 && v[1] == 1.0, "{v:?}");
    assert!(!p.channels.contains_key(&ChannelKind::Metallic), "untouched channels stay unpainted");
    // Nothing new on the next checks; undo restores the layer.
    assert!(ex.check(&mut s).is_empty() && ex.check(&mut s).is_empty());
    s.execute(Command::Undo).unwrap();
    let paint = s.selected_texture_set().layers.iter().find(|l| l.name == "Paint").unwrap().clone();
    let LayerContent::Paint(p) = &paint.content else { panic!() };
    assert!(!p.channels.contains_key(&ChannelKind::Roughness));
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_broken_save_is_reported_and_a_removed_texture_set_ends_the_exchange() {
    let mut s = session();
    let d = dir("broken");
    let (mut ex, opened) = start(&s, &d);
    std::fs::write(&opened[0], b"not a psd").unwrap();
    ex.check(&mut s);
    assert!(matches!(&ex.check(&mut s)[..], [PollEvent::Failed(_)]));
    // A fresh project without that texture set.
    let mut other = session();
    paint_file(&opened[1], 1000);
    ex.check(&mut other);
    assert!(matches!(&ex.check(&mut other)[..], [PollEvent::Ended(_)]));
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_set_without_paint_layers_is_refused() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::DeleteLayer(layer)).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let err = Exchange::start(&s, &dir("none"), |_| Ok(())).err().unwrap();
    assert!(err.contains("no paint layers"), "{err}");
}
