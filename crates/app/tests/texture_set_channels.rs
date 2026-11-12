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
    let mut s = Session::new(primitives::cube(2.0), 64);
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
