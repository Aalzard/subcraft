//! Anchor points through the session (ADR-0045): adding, naming, referencing, undo / redo,
//! duplication with remapped references, deletion reported as a missing anchor, and save / load.

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{AnchorIndex, AnchorInput, ContentEffectKind, FillChannel, FillSource, MaskEffectKind, RefStatus};

const RES: u32 = 128;

fn layer_ids(s: &Session) -> Vec<LayerId> {
    s.document().texture_sets[0].layers.iter().map(|l| l.id).collect()
}

fn index(s: &Session) -> AnchorIndex {
    AnchorIndex::build(&s.document().texture_sets[0].layers)
}

/// Bottom fill with a content anchor, top fill whose mask reads it.
fn setup() -> (Session, LayerId, LayerId, subcraft_core::EffectId) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let low = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: low, channel: ChannelKind::Height, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.7))) }).unwrap();
    s.execute(Command::AddContentEffect { layer: low, kind: ContentEffectKind::Anchor }).unwrap();
    let anchor = s.document().texture_sets[0].find(low).unwrap().content_effects[0].id;
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: top, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0))) }).unwrap();
    s.execute(Command::AddMask { layer: top, white: false }).unwrap();
    s.execute(Command::AddMaskEffect { layer: top, kind: MaskEffectKind::AnchorRef { input: AnchorInput::new(anchor, ChannelKind::Height), levels: None } }).unwrap();
    (s, low, top, anchor)
}

#[test]
fn anchors_are_named_referenced_and_undoable() {
    let (mut s, low, top, anchor) = setup();
    let name = |s: &Session| index(s).anchor(anchor).map(|a| a.name.clone());
    let layer_name = s.document().texture_sets[0].find(low).unwrap().name.clone();
    assert_eq!(name(&s), Some(format!("{layer_name} anchor")), "named after its layer");
    let i = index(&s);
    assert_eq!(i.refs.len(), 1);
    assert_eq!(i.refs[0].status, RefStatus::Ok);
    assert_eq!(i.refs[0].site.layer(), top);
    // A second anchor in the same layer gets a distinct name.
    s.execute(Command::AddContentEffect { layer: low, kind: ContentEffectKind::Anchor }).unwrap();
    assert_eq!(s.document().texture_sets[0].find(low).unwrap().content_effects[1].name, format!("{layer_name} anchor 2"));
    s.execute(Command::Undo).unwrap();
    // Undo the reference, then the mask, then redo both: the reference resolves again.
    s.execute(Command::Undo).unwrap();
    assert!(index(&s).refs.is_empty());
    s.execute(Command::Redo).unwrap();
    assert_eq!(index(&s).refs[0].status, RefStatus::Ok);
    // The mask reads the anchored height (0.7): the top fill's base color shows at 70 %.
    s.update_composites();
    let c = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    let anchored: Vec<[f32; 4]> = (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).map(|(x, y)| c.get(x, y)).collect();
    // Deleting the anchor leaves the reference in place, reported missing, and changes the result.
    s.execute(Command::RemoveContentEffect { layer: low, effect: anchor }).unwrap();
    assert_eq!(index(&s).refs[0].status, RefStatus::Missing);
    s.update_composites();
    let c2 = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    let missing: Vec<[f32; 4]> = (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).map(|(x, y)| c2.get(x, y)).collect();
    // Anchored: red at 70 % over the default gray; missing: the mask stays black.
    assert!(anchored.iter().all(|p| p[0] > p[1] + 0.3), "{:?}", anchored[0]);
    assert!(missing.iter().all(|p| (p[0] - p[1]).abs() < 1e-3), "{:?}", missing[0]);
    s.execute(Command::Undo).unwrap();
    assert_eq!(index(&s).refs[0].status, RefStatus::Ok);
}

#[test]
fn moving_the_reader_below_the_anchor_is_reported_and_duplicates_remap() {
    let (mut s, low, top, anchor) = setup();
    // Move the reader under the anchored layer: not below any more.
    s.execute(Command::MoveLayer { layer: top, parent: None, index: 0 }).unwrap();
    let ids = layer_ids(&s);
    let pos = |id| ids.iter().position(|l| *l == id).unwrap();
    assert!(pos(top) < pos(low), "the reader is now under the anchored layer");
    assert_eq!(index(&s).refs[0].status, RefStatus::NotBelow);
    s.execute(Command::Undo).unwrap();
    assert_eq!(index(&s).refs[0].status, RefStatus::Ok);
    // Duplicating both layers together: the copied reader reads the copied anchor.
    s.execute(Command::DuplicateLayers(vec![low, top])).unwrap();
    let i = index(&s);
    assert_eq!(i.anchors.len(), 2);
    assert_eq!(i.refs.len(), 2);
    assert!(i.refs.iter().all(|r| r.status == RefStatus::Ok), "{:?}", i.refs);
    let targets: std::collections::BTreeSet<_> = i.refs.iter().map(|r| r.input.anchor).collect();
    assert_eq!(targets.len(), 2, "each reader reads its own anchor");
    assert!(targets.contains(&anchor));
}

#[test]
fn anchors_survive_save_and_load() {
    let (s, _, _, _) = setup();
    let dir = std::env::temp_dir().join(format!("subcraft-anchor-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("anchors.subcraft");
    let mut s = s;
    s.save(Some(&path)).unwrap();
    let before = s.document().clone();
    let loaded = Session::open_project(&path).unwrap();
    assert_eq!(loaded.document().texture_sets[0].layers, before.texture_sets[0].layers);
    assert_eq!(index(&loaded).refs[0].status, RefStatus::Ok);
    let _ = std::fs::remove_dir_all(&dir);
}
