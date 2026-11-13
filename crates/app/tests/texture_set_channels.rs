//! Texture Set Settings channel rows: sixteen user channels, renaming them, remove and format.
use subcraft_app::{Action, Command, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::ChannelFormat;
use subcraft_mesh::primitives;

#[test]
fn all_sixteen_user_channels_can_be_added_and_renamed() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let ts = s.selection().texture_set;
    for k in ChannelKind::USER {
        s.execute(Command::AddChannel { texture_set: ts, kind: k }).unwrap();
    }
    assert!(ChannelKind::USER.iter().all(|&k| s.selected_texture_set().has_channel(k)));

    s.execute(Command::RenameUserChannel { texture_set: ts, kind: ChannelKind::User12, name: "  Dirt mask ".into() }).unwrap();
    assert_eq!(s.selected_texture_set().channel_label(ChannelKind::User12), "Dirt mask");
    assert_eq!(s.selected_texture_set().channel_label(ChannelKind::User3), "User 3");
    let steps = s.history().entries().len();
    s.execute(Command::RenameUserChannel { texture_set: ts, kind: ChannelKind::User12, name: "Dirt mask".into() }).unwrap();
    assert_eq!(s.history().entries().len(), steps, "same name: no new step");
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.selected_texture_set().channel_label(ChannelKind::User12), "User 12");
    s.perform(Action::Redo).unwrap();
    // An empty name goes back to the standard one.
    s.execute(Command::RenameUserChannel { texture_set: ts, kind: ChannelKind::User12, name: " ".into() }).unwrap();
    assert_eq!(s.selected_texture_set().channel_label(ChannelKind::User12), "User 12");
    // Standard channels keep their names.
    assert!(s.execute(Command::RenameUserChannel { texture_set: ts, kind: ChannelKind::Roughness, name: "Rough".into() }).is_err());
}

#[test]
fn user_channel_names_are_saved_with_the_project() {
    // 128: the smallest size a project file accepts.
    let mut s = Session::new(primitives::cube(2.0), 128);
    let ts = s.selection().texture_set;
    s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::User15 }).unwrap();
    s.execute(Command::RenameUserChannel { texture_set: ts, kind: ChannelKind::User15, name: "Wetness".into() }).unwrap();
    let dir = std::env::temp_dir().join(format!("subcraft-ts-channels-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("channels.subcraft");
    s.save(Some(&path)).unwrap();
    let back = Session::open_project(&path).unwrap();
    assert!(back.selected_texture_set().has_channel(ChannelKind::User15));
    assert_eq!(back.selected_texture_set().channel_label(ChannelKind::User15), "Wetness");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn remove_keeps_paint_and_format_is_one_step() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let ts = s.selection().texture_set;
    s.execute(Command::SetChannelFormat { texture_set: ts, kind: ChannelKind::Roughness, format: ChannelFormat::Float32 }).unwrap();
    assert_eq!(s.selected_texture_set().channels.iter().find(|c| c.kind == ChannelKind::Roughness).unwrap().format, ChannelFormat::Float32);
    s.execute(Command::RemoveChannel { texture_set: ts, kind: ChannelKind::Roughness }).unwrap();
    assert!(!s.selected_texture_set().has_channel(ChannelKind::Roughness));
    s.perform(Action::Undo).unwrap();
    assert!(s.selected_texture_set().has_channel(ChannelKind::Roughness));
}

#[test]
fn extended_channel_types_are_added_painted_as_fills_and_saved() {
    // 128: the smallest size a project file accepts.
    let mut s = Session::new(primitives::cube(2.0), 128);
    let ts = s.selection().texture_set;
    for k in ChannelKind::EXTENDED {
        s.execute(Command::AddChannel { texture_set: ts, kind: k }).unwrap();
    }
    let set = s.selected_texture_set();
    assert!(ChannelKind::EXTENDED.iter().all(|&k| set.has_channel(k)));
    let format = |k: ChannelKind| set.channels.iter().find(|d| d.kind == k).map(|d| d.format);
    assert_eq!(format(ChannelKind::CoatNormal), Some(ChannelFormat::Float16));
    assert_eq!(format(ChannelKind::Displacement), Some(ChannelFormat::Float16));
    assert_eq!(format(ChannelKind::CoatColor), Some(ChannelFormat::Bit8));
    assert_eq!(set.channel_label(ChannelKind::SpecularColor), "Specular");
    let steps = s.history().entries().len();
    s.perform(Action::Undo).unwrap();
    assert!(!s.selected_texture_set().has_channel(ChannelKind::Transmissive), "adding a channel is one undo step");
    s.perform(Action::Redo).unwrap();
    assert_eq!(s.history().entries().len(), steps);

    let dir = std::env::temp_dir().join(format!("subcraft-ts-extended-channels-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("extended.subcraft");
    s.save(Some(&path)).unwrap();
    let back = Session::open_project(&path).unwrap();
    assert!(ChannelKind::EXTENDED.iter().all(|&k| back.selected_texture_set().has_channel(k)));
    assert_eq!(back.selected_texture_set().channels, s.selected_texture_set().channels);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn several_channels_are_added_as_one_undo_step() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let ts = s.selection().texture_set;
    let before = s.selected_texture_set().channels.len();
    let steps = s.history().entries().len();
    // Roughness is already there and Emissive is named twice: both are skipped.
    let kinds = vec![ChannelKind::Emissive, ChannelKind::Roughness, ChannelKind::CoatColor, ChannelKind::Emissive, ChannelKind::User7];
    s.execute(Command::AddChannels { texture_set: ts, kinds }).unwrap();
    let set = s.selected_texture_set();
    assert_eq!(set.channels.len(), before + 3);
    assert!([ChannelKind::Emissive, ChannelKind::CoatColor, ChannelKind::User7].iter().all(|&k| set.has_channel(k)));
    assert_eq!(set.channels.iter().filter(|d| d.kind == ChannelKind::Emissive).count(), 1);
    assert_eq!(s.history().entries().len(), steps + 1, "one step for all of them");
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.selected_texture_set().channels.len(), before, "undo takes all of them away");
    s.perform(Action::Redo).unwrap();
    assert_eq!(s.selected_texture_set().channels.len(), before + 3);

    // Nothing new: no step.
    let steps = s.history().entries().len();
    s.execute(Command::AddChannels { texture_set: ts, kinds: vec![ChannelKind::Roughness, ChannelKind::User7] }).unwrap();
    s.execute(Command::AddChannels { texture_set: ts, kinds: Vec::new() }).unwrap();
    assert_eq!(s.history().entries().len(), steps);
}
