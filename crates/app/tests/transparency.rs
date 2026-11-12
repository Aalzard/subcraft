//! Issue #145 end to end: a transparent texture set exports Base Color with alpha 0 where nothing
//! is painted; the commands are undoable; an added Opacity channel is live on existing layers.

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::{AlphaMode, AlphaSettings};
use subcraft_export::{ExportSettings, ExportTemplate, Padding};
use subcraft_mesh::primitives;

fn base_color_alpha(s: &mut Session, name: &str) -> (u8, u8) {
    let dir = std::env::temp_dir().join(format!("subcraft-transparency-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = ExportSettings { template: ExportTemplate::pbr_metal_rough_opacity(), padding: Padding::None, ..Default::default() };
    let files = s.export_textures(&dir, &settings).unwrap();
    let bc = files.iter().find(|p| p.to_string_lossy().ends_with("_BaseColor.png")).expect("Base Color written");
    let b = subcraft_image::load_bitmap(bc).unwrap();
    assert_eq!(b.channels, 4);
    let alphas: Vec<u8> = (0..b.height).flat_map(|y| (0..b.width).map(move |x| (x, y))).map(|(x, y)| (b.texel(x, y, false)[3] * 255.0).round() as u8).collect();
    std::fs::remove_dir_all(&dir).ok();
    (*alphas.iter().min().unwrap(), *alphas.iter().max().unwrap())
}

#[test]
fn a_transparent_set_exports_empty_alpha_until_something_covers_it() {
    let mut s = Session::new(primitives::plane(2.0, 1), 128);
    let ts = s.selected_texture_set().id;
    assert_eq!(base_color_alpha(&mut s, "opaque"), (255, 255), "opaque by default");
    s.execute(Command::SetTransparent { texture_set: ts, transparent: true }).unwrap();
    assert!(s.selected_texture_set().transparent);
    assert_eq!(base_color_alpha(&mut s, "empty"), (0, 0), "nothing painted: fully transparent");
    // A fill layer covers everything.
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert_eq!(base_color_alpha(&mut s, "filled"), (255, 255));
    s.execute(Command::Undo).unwrap();
    assert_eq!(base_color_alpha(&mut s, "undo-fill"), (0, 0));
    s.execute(Command::Undo).unwrap();
    assert!(!s.selected_texture_set().transparent, "one undo step");
    assert_eq!(base_color_alpha(&mut s, "undo"), (255, 255));
}

#[test]
fn alpha_settings_are_undoable_and_clamped() {
    let mut s = Session::new(primitives::plane(2.0, 1), 128);
    let ts = s.selected_texture_set().id;
    s.execute(Command::SetAlphaSettings { texture_set: ts, alpha: AlphaSettings { mode: AlphaMode::Test, threshold: 1.5, dithering: true } }).unwrap();
    assert_eq!(s.selected_texture_set().alpha, AlphaSettings { mode: AlphaMode::Test, threshold: 1.0, dithering: true });
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().alpha, AlphaSettings::default());
}

#[test]
fn an_added_opacity_channel_takes_part_on_existing_layers() {
    let mut s = Session::new(primitives::plane(2.0, 1), 128);
    let ts = s.selected_texture_set().id;
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Opacity }).unwrap();
    let set = s.selected_texture_set();
    assert!(set.has_channel(ChannelKind::Opacity));
    assert!(set.layers.iter().all(|l| l.blend_for(ChannelKind::Opacity).is_active()), "every layer can paint Opacity at once");
    s.execute(Command::RemoveChannel { texture_set: ts, kind: ChannelKind::Opacity }).unwrap();
    assert!(!s.selected_texture_set().has_channel(ChannelKind::Opacity));
}
