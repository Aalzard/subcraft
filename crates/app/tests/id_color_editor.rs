//! ID color editor: the palette of the ID map, editing a color (masks follow, one undo step,
//! saved as an edit over the baked map), the 2D eyedropper and feeding picked colors to masks.

use std::collections::BTreeMap;

use glam::Vec2;
use subcraft_app::id_color::same_id;
use subcraft_app::part_workflow::LayerAction;
use subcraft_app::{Command, MeshMapKind, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind};
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;

const RES: u32 = 128;
const RED: [f32; 3] = [0.9, 0.3, 0.3];
const BLUE: [f32; 3] = [0.3, 0.3, 0.9];
const GOLD: [f32; 3] = [0.85, 0.65, 0.1];

/// Left half RED, right half BLUE.
fn id_map() -> TiledImage {
    let mut img = TiledImage::new(RES, RES, 3, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            let c = if x < RES / 2 { RED } else { BLUE };
            img.set(x, y, [c[0], c[1], c[2], 1.0]);
        }
    }
    img
}

/// Plane with an ID map and a green fill layer carrying an empty ID Color mask.
fn setup() -> (Session, subcraft_core::LayerId, subcraft_core::EffectId) {
    let mut s = Session::new(primitives::plane(2.0, 2), RES);
    let ts = s.selection().texture_set;
    s.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, id_map())]));
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 1.0, 0.0))) }).unwrap();
    s.layer_action(layer, LayerAction::IdColorMask).unwrap();
    let effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    (s, layer, effect)
}

fn mask_colors(s: &Session, layer: subcraft_core::LayerId) -> Vec<[f32; 3]> {
    match &s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].kind {
        MaskEffectKind::IdColor(id) => id.colors.clone(),
        _ => unreachable!(),
    }
}

/// Texels where the green fill shows (the mask selects them).
fn selected(s: &mut Session) -> Vec<bool> {
    s.update_composites();
    let img = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap();
    (0..RES * RES).map(|i| img.get(i % RES, i / RES)[1] > 0.999).collect()
}

#[test]
fn palette_lists_the_id_map_colors_with_coverage_and_mask_use() {
    let (mut s, layer, effect) = setup();
    let ts = s.selection().texture_set;
    let palette = s.id_palette(ts).unwrap();
    assert_eq!(palette.len(), 2);
    for e in &palette {
        assert!((e.coverage - 0.5).abs() < 1e-6, "{e:?}");
        assert_eq!(e.masks, 0);
    }
    assert!(palette.iter().any(|e| same_id(e.color, RED)) && palette.iter().any(|e| same_id(e.color, BLUE)));
    // 2D eyedropper: the left half is RED.
    let picked = s.pick_id_color_uv(layer, effect, Vec2::new(0.25, 0.5)).unwrap().unwrap();
    assert!(same_id(picked, RED));
    assert_eq!(s.pick_id_color_uv(layer, effect, Vec2::new(0.3, 0.7)).unwrap(), None, "already picked");
    assert_eq!(s.pick_id_color_uv(layer, effect, Vec2::new(1.5, 0.5)).unwrap(), None, "outside the image");
    let palette = s.id_palette(ts).unwrap();
    assert_eq!(palette.iter().find(|e| same_id(e.color, RED)).unwrap().masks, 1);
}

#[test]
fn editing_a_color_recolors_the_map_and_every_mask_using_it_in_one_undo_step() {
    let (mut s, layer, effect) = setup();
    let ts = s.selection().texture_set;
    let red = s.pick_id_color_uv(layer, effect, Vec2::new(0.25, 0.5)).unwrap().unwrap();
    // A second layer whose mask also uses RED. It sits on top, so it is green too: the texels
    // showing green are then exactly the RED region whichever layer covers them.
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let other = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: other, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 1.0, 0.0))) }).unwrap();
    s.feed_id_color(ts, red).unwrap();
    assert!(matches!(&s.selected_texture_set().find(other).unwrap().mask.as_ref().unwrap().effects[0].kind, MaskEffectKind::IdColor(id) if id.colors.len() == 1));
    s.execute(Command::SelectLayer(Some(layer))).unwrap();
    let before = selected(&mut s);
    assert_eq!(before.iter().filter(|b| **b).count(), (RES * RES / 2) as usize);
    let entries = s.history().entries().len();

    s.edit_id_color(ts, red, GOLD).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1, "one undo step");
    let shown = s.mesh_maps(ts).unwrap().get(MeshMapKind::Id).unwrap().get(10, 10);
    assert!(same_id([shown[0], shown[1], shown[2]], GOLD), "the map shows the new color: {shown:?}");
    assert!(same_id(mask_colors(&s, layer)[0], GOLD), "mask follows the edit");
    let other_colors = match &s.selected_texture_set().find(other).unwrap().mask.as_ref().unwrap().effects[0].kind { MaskEffectKind::IdColor(id) => id.colors.clone(), _ => unreachable!() };
    assert!(same_id(other_colors[0], GOLD), "every mask using the color follows");
    assert_eq!(selected(&mut s), before, "the same texels stay selected");
    let palette = s.id_palette(ts).unwrap();
    assert!(palette.iter().any(|e| same_id(e.color, GOLD) && e.masks == 2));
    assert!(!palette.iter().any(|e| same_id(e.color, RED)));

    // Undo restores map and masks together; redo re-applies both.
    s.execute(Command::Undo).unwrap();
    let shown = s.mesh_maps(ts).unwrap().get(MeshMapKind::Id).unwrap().get(10, 10);
    assert!(same_id([shown[0], shown[1], shown[2]], RED));
    assert!(same_id(mask_colors(&s, layer)[0], RED));
    assert_eq!(selected(&mut s), before);
    s.execute(Command::Redo).unwrap();
    assert!(same_id(mask_colors(&s, layer)[0], GOLD));
    assert!(s.id_palette(ts).unwrap().iter().any(|e| same_id(e.color, GOLD)));

    // Editing it back removes the edit instead of stacking a second one.
    s.edit_id_color(ts, GOLD, red).unwrap();
    assert!(s.selected_texture_set().id_color_edits.is_empty());
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().id_color_edits.len(), 1);
}

#[test]
fn merging_two_colors_selects_both_regions_for_masks_of_either() {
    let (mut s, layer, effect) = setup();
    let ts = s.selection().texture_set;
    let red = s.pick_id_color_uv(layer, effect, Vec2::new(0.25, 0.5)).unwrap().unwrap();
    let blue = s.id_palette(ts).unwrap().into_iter().find(|e| same_id(e.color, BLUE)).unwrap().color;
    assert_eq!(selected(&mut s).iter().filter(|b| **b).count(), (RES * RES / 2) as usize);
    s.edit_id_color(ts, blue, red).unwrap();
    assert_eq!(s.id_palette(ts).unwrap().len(), 1);
    assert_eq!(selected(&mut s).iter().filter(|b| **b).count(), (RES * RES) as usize, "both halves now carry the picked color");
}

#[test]
fn edits_are_saved_over_the_baked_map_and_survive_reopening_and_rebaking() {
    let (mut s, layer, effect) = setup();
    let ts = s.selection().texture_set;
    let red = s.pick_id_color_uv(layer, effect, Vec2::new(0.25, 0.5)).unwrap().unwrap();
    s.edit_id_color(ts, red, GOLD).unwrap();
    let dir = std::env::temp_dir().join(format!("subcraft-id-editor-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ids.subcraft");
    s.save(Some(&path)).unwrap();
    // The file keeps the baked map; the edit is in the document.
    let data = subcraft_project::load_project(&path).unwrap();
    let stored = data.mesh_maps[&ts][&MeshMapKind::Id].get(10, 10);
    assert!(same_id([stored[0], stored[1], stored[2]], RED), "baked map saved unchanged: {stored:?}");
    let mut reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    let shown = reopened.mesh_maps(ts).unwrap().get(MeshMapKind::Id).unwrap().get(10, 10);
    assert!(same_id([shown[0], shown[1], shown[2]], GOLD));
    let before = selected(&mut s);
    assert_eq!(selected(&mut reopened), before);
    // A rebake brings the baked colors back; the edit is applied to it again.
    reopened.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, id_map())]));
    let shown = reopened.mesh_maps(ts).unwrap().get(MeshMapKind::Id).unwrap().get(10, 10);
    assert!(same_id([shown[0], shown[1], shown[2]], GOLD));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn feeding_a_picked_color_creates_or_extends_the_selected_layers_id_mask() {
    let mut s = Session::new(primitives::plane(2.0, 2), RES);
    let ts = s.selection().texture_set;
    s.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, id_map())]));
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    let red = s.id_map_color(ts, Vec2::new(0.1, 0.1)).unwrap();
    let blue = s.id_map_color(ts, Vec2::new(0.9, 0.1)).unwrap();
    s.feed_id_color(ts, red).unwrap();
    assert_eq!(s.selected_id_color_effect().map(|(l, _)| l), Some(layer));
    s.feed_id_color(ts, blue).unwrap();
    assert_eq!(mask_colors(&s, layer).len(), 2);
    let entries = s.history().entries().len();
    s.feed_id_color(ts, blue).unwrap();
    assert_eq!(s.history().entries().len(), entries, "a repeated color adds no history entry");
}
