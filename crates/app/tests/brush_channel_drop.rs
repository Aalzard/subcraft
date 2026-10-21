use subcraft_app::{Session, asset_apply::Destination};
use subcraft_assets::starter::Kind;
use subcraft_core::ChannelKind;
use subcraft_image::Bitmap;

fn session() -> Session { Session::new(subcraft_mesh::primitives::cube(2.0), 128) }
fn normal(v: u8) -> Bitmap { Bitmap::new_u8(1, 1, 3, vec![128, v, 255]) }

#[test]
fn a_decoded_image_becomes_the_brush_channel_image_without_touching_the_document() {
    let mut s = session();
    let (doc, history) = (s.document().clone(), s.history().entries().len());
    s.set_brush_channel_bitmap(ChannelKind::Normal, "Starter: Brick", normal(10)).unwrap();
    let m = &s.brush.material;
    assert!(m.channels[&ChannelKind::Normal].enabled);
    let id = m.textures[&ChannelKind::Normal];
    assert_eq!(s.resources().bitmap(id).unwrap().texel(0, 0, false)[2], 1.0);
    // Identical images share a resource; a different one adds another.
    s.set_brush_channel_bitmap(ChannelKind::Roughness, "Same", normal(10)).unwrap();
    assert_eq!(s.brush.material.textures[&ChannelKind::Roughness], id);
    s.set_brush_channel_bitmap(ChannelKind::Roughness, "Other", normal(200)).unwrap();
    assert_ne!(s.brush.material.textures[&ChannelKind::Roughness], id);
    assert_eq!(s.brush.material.textures[&ChannelKind::Normal], id);
    assert_eq!(s.document(), &doc);
    assert_eq!(s.history().entries().len(), history);
}

#[test]
fn brush_destination_keeps_selection_and_refuses_a_changed_project_or_missing_channel() {
    let mut s = session();
    let selection = s.selection();
    Destination::capture_brush(&s, ChannelKind::Normal).apply(&mut s, "Starter: Test", Kind::Normal, normal(30)).unwrap();
    assert_eq!(s.selection(), selection);
    assert!(s.brush.material.textures.contains_key(&ChannelKind::Normal));
    let stale = Destination::capture_brush(&s, ChannelKind::Normal);
    let mut other = session();
    assert!(stale.apply(&mut other, "No", Kind::Normal, normal(30)).is_err());
    assert!(other.brush.material.textures.is_empty());
    // A channel the texture set does not have is rejected and leaves the brush alone.
    assert!(!s.selected_texture_set().has_channel(ChannelKind::Emissive));
    let before = s.brush.material.clone();
    assert!(s.set_brush_channel_bitmap(ChannelKind::Emissive, "x", normal(1)).is_err());
    assert_eq!(s.brush.material, before);
}
