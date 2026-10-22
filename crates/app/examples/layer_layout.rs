//! Generate a small native UI acceptance project from an existing baked project.
//! Writes only the explicitly supplied output path; never modifies the source.
use subcraft_app::{Command, LayerTarget, Session};
use subcraft_doc::{Levels, MaskEffectKind};
use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = Path::new(args.get(1).expect("source .subcraft"));
    let output = Path::new(args.get(2).expect("new output .subcraft"));
    assert!(!output.exists(), "choose a new output path");
    let mut session = Session::open_project(source).unwrap();
    let layer = session.selection().layer.unwrap();
    assert!(session.selected_layer().unwrap().mask.is_some(), "source needs a masked layer");
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Levels(Levels::default()) }).unwrap();
    session.execute(Command::SelectLayerTarget { layer, target: LayerTarget::Mask }).unwrap();
    session.save(Some(output)).unwrap();
}
