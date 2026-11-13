//! ID color editor (Texture Set Settings): the colors of the selected set's ID map, an
//! eyedropper for the 3D and 2D views, and editing a color (every ID Color mask using it follows).
//! All changes go through `subcraft_app` (`Session::edit_id_color`, `Session::feed_id_color`).

use egui::{Color32, RichText, Sense, Ui, vec2};
use subcraft_app::id_color::{IdPaletteEntry, same_id};
use subcraft_core::TextureSetId;

use crate::state::UiState;
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM, TEXT_FAINT};

/// The ID map stores raw values; swatches show them as sRGB-encoded bytes like the 2D view.
pub fn swatch_color(c: [f32; 3]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

pub fn hex(c: [f32; 3]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", q(c[0]), q(c[1]), q(c[2]))
}

/// Palette of `ts`, recomputed only when its maps or the document change.
pub fn palette(state: &mut UiState, ts: TextureSetId) -> Option<Vec<IdPaletteEntry>> {
    let key = (ts, state.session.maps_generation(), state.session.revision());
    if let Some((k, p)) = &state.id_palette_cache && *k == key {
        return Some(p.clone());
    }
    let p = state.session.id_palette(ts)?;
    state.id_palette_cache = Some((key, p.clone()));
    Some(p)
}

/// A color picked in the 3D / 2D view with the editor's eyedropper: select it and feed it to the
/// selected layer's ID Color mask.
pub fn picked(state: &mut UiState, ts: TextureSetId, color: [f32; 3], now: f64) {
    state.id_palette_selected = Some(color);
    state.id_palette_new = color;
    match state.session.feed_id_color(ts, color) {
        Ok(what) => state.set_status(format!("ID color {} picked. {what}", hex(color)), now),
        Err(e) => state.set_status(format!("ID color {} picked (not added to a mask: {e})", hex(color)), now),
    }
}

pub fn section(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    ui.label(RichText::new("ID COLORS").font(theme::font_strong(12.0)).color(TEXT()));
    ui.add_space(2.0);
    let ts = state.session.selected_texture_set().id;
    let Some(entries) = palette(state, ts) else {
        state.picking_id_palette = false;
        ui.label(RichText::new("No ID map yet: bake one (Bake Mesh Maps) or import one (Import mesh maps…).").small().color(TEXT_FAINT()));
        return;
    };
    ui.horizontal(|ui| {
        let on = state.picking_id_palette;
        let text = format!("{}  {}", crate::icons::CROSSHAIR, if on { "Picking… (Esc)" } else { "Pick from the view" });
        let button = egui::Button::new(RichText::new(text).font(theme::font(12.0)).color(if on { ACCENT() } else { TEXT() }))
            .fill(if on { theme::ACCENT_SOFT() } else { theme::BG_3() });
        if ui.add(button).on_hover_text("Click the model in the 3D view or the 2D view: the ID color there is selected here and added to the selected layer's ID Color mask (a new mask if it has none)").clicked() {
            state.picking_id_palette = !on;
            if !on { state.picking_id_color = None; }
        }
        ui.label(RichText::new(format!("{} color{}", entries.len(), if entries.len() == 1 { "" } else { "s" })).small().color(TEXT_DIM()));
    });
    if entries.len() >= subcraft_app::id_color::MAX_PALETTE {
        ui.label(RichText::new("Showing the 256 most common colors: this map has more than an ID map usually does.").small().color(theme::WARNING()));
    }
    ui.add_space(2.0);
    // Swatch grid: click selects.
    let size = 26.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for e in &entries {
            let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
            let selected = state.id_palette_selected.is_some_and(|c| same_id(c, e.color));
            ui.painter().rect_filled(rect, 3.0, swatch_color(e.color));
            let stroke = if selected { egui::Stroke::new(2.0, ACCENT()) } else { egui::Stroke::new(1.0, theme::BORDER()) };
            ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);
            if e.masks > 0 {
                ui.painter().circle_filled(rect.right_top() + vec2(-5.0, 5.0), 3.5, Color32::WHITE);
                ui.painter().circle_stroke(rect.right_top() + vec2(-5.0, 5.0), 3.5, egui::Stroke::new(1.0, Color32::BLACK));
            }
            let tip = format!("{}  ·  {:.1}% of the map  ·  used by {} ID mask{}", hex(e.color), 100.0 * e.coverage, e.masks, if e.masks == 1 { "" } else { "s" });
            if response.on_hover_text(tip).clicked() {
                state.id_palette_selected = Some(e.color);
                state.id_palette_new = e.color;
            }
        }
    });
    let Some(sel) = state.id_palette_selected.filter(|c| entries.iter().any(|e| same_id(e.color, *c))) else {
        ui.label(RichText::new("Select a color (or pick one from the view) to edit it.").small().color(TEXT_FAINT()));
        return;
    };
    let entry = entries.iter().find(|e| same_id(e.color, sel)).cloned().expect("selected color is listed");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, swatch_color(sel));
        ui.label(RichText::new(hex(sel)).monospace().color(TEXT()));
        ui.label(RichText::new(format!("{:.1}% · {} mask{}", 100.0 * entry.coverage, entry.masks, if entry.masks == 1 { "" } else { "s" })).small().color(TEXT_DIM()));
    });
    let mut apply = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("New color").small().color(TEXT_DIM()));
        egui::color_picker::color_edit_button_rgb(ui, &mut state.id_palette_new);
        // The text being typed is kept while the field has focus: rebuilt from the color every
        // frame, a partial value ("#3A7FC" after a Backspace) snapped back (QA 2026-11).
        let mut text: String = ui.data(|d| d.get_temp(hex_text())).unwrap_or_else(|| hex(state.id_palette_new));
        let r = ui.add(egui::TextEdit::singleline(&mut text).id(hex_field()).desired_width(70.0).font(egui::TextStyle::Monospace));
        if r.changed() && let Some(c) = parse_hex(&text) { state.id_palette_new = c; }
        if r.has_focus() { ui.data_mut(|d| d.insert_temp(hex_text(), text)); } else { ui.data_mut(|d| d.remove_temp::<String>(hex_text())); }
        let differs = !same_id(state.id_palette_new, sel);
        if ui.add_enabled(differs, egui::Button::new("Apply")).on_hover_text("Recolor this ID in the map; every ID Color mask using it follows (one undo step)").clicked() {
            apply = true;
        }
    });
    if let Some(other) = entries.iter().find(|e| !same_id(e.color, sel) && same_id(e.color, state.id_palette_new)) {
        ui.label(RichText::new(format!("{} is already in the map: applying merges the two IDs ({} more mask{} select both).", hex(other.color), other.masks, if other.masks == 1 { "" } else { "s" })).small().color(theme::WARNING()));
    }
    ui.horizontal(|ui| {
        if ui.button("Mask selected layer by this color").on_hover_text("Add it to the selected layer's ID Color mask, or give the layer one").clicked() {
            match state.session.feed_id_color(ts, sel) {
                Ok(what) => state.set_status(what, now),
                Err(e) => state.set_status(format!("ID color: {e}"), now),
            }
        }
    });
    if apply {
        let to = state.id_palette_new;
        match state.session.edit_id_color(ts, sel, to) {
            Ok(()) => {
                state.id_palette_selected = Some(to);
                state.set_status(format!("ID color {} → {} ({} mask{} updated)", hex(sel), hex(to), entry.masks, if entry.masks == 1 { "" } else { "s" }), now);
            }
            Err(e) => state.set_status(format!("ID color: {e}"), now),
        }
    }
}

/// The new color's hex field.
pub fn hex_field() -> egui::Id {
    egui::Id::new("id_palette_hex_field")
}
/// The text typed in the hex field while it has focus.
fn hex_text() -> egui::Id {
    egui::Id::new("id_palette_hex_text")
}

pub fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let t = text.trim().trim_start_matches('#');
    if t.len() != 6 { return None; }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

/// Issue #333: while an ID eyedropper is armed (an ID Color mask's "Pick from model", or the ID
/// color editor's), the 3D view shows the ID map; when the pick ends (the button again, Esc, or
/// another layer) the view goes back to the mode it was in. Call once per frame.
pub fn sync_pick_view(state: &mut UiState) {
    use subcraft_core::MeshMapKind;
    let picking = state.picking_id_color.is_some() || state.picking_id_palette;
    let view = &mut state.recovery.prefs.viewport.mesh_map;
    match (picking, state.id_pick_view) {
        (true, None) => {
            // Only when there is an ID map to look at.
            if state.session.has_id_map(state.session.selected_texture_set().id) {
                state.id_pick_view = Some(*view);
                *view = Some(MeshMapKind::Id);
            }
        }
        (false, Some(before)) => {
            state.id_pick_view = None;
            // A mode the owner picked during the pick stays.
            if *view == Some(MeshMapKind::Id) { *view = before; }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The section lists the map's colors; a pick selects the color and masks the selected layer.
    #[test]
    fn section_lists_colors_and_a_pick_feeds_the_selected_layer() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2.0, 2), 64), None);
        let ts = state.session.selected_texture_set().id;
        let mut img = subcraft_image::TiledImage::new(64, 64, 3, [0.0; 4]);
        for y in 0..64 { for x in 0..64 { img.set(x, y, if x < 16 { [0.2, 0.6, 0.9, 1.0] } else { [0.9, 0.3, 0.3, 1.0] }); } }
        state.session.set_mesh_maps(ts, std::collections::BTreeMap::from([(subcraft_core::MeshMapKind::Id, img)]));
        let _ = state.session.execute(subcraft_app::Command::AddLayer(subcraft_app::NewLayer::Fill));
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))), ..Default::default() }, |ui| section(&mut state, ui));
            out.textures_delta.clear();
            texts = out.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect::<Vec<_>>();
        }
        assert!(texts.iter().any(|t| t == "2 colors"), "{texts:?}");
        let blue = state.session.id_map_color(ts, glam::Vec2::new(0.1, 0.5)).unwrap();
        picked(&mut state, ts, blue, 0.0);
        assert_eq!(state.id_palette_selected, Some(blue));
        assert!(state.session.selected_id_color_effect().is_some(), "the selected layer got an ID Color mask");
        assert_eq!(palette(&mut state, ts).unwrap().iter().find(|e| same_id(e.color, blue)).unwrap().masks, 1);
    }

    /// QA 2026-11: the hex field was rebuilt from the color every frame, so anything that did
    /// not parse yet (a partly typed value) was thrown away.
    #[test]
    fn the_hex_field_keeps_a_partly_typed_value() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2.0, 2), 64), None);
        let ts = state.session.selected_texture_set().id;
        let mut img = subcraft_image::TiledImage::new(64, 64, 3, [0.0; 4]);
        for y in 0..64 { for x in 0..64 { img.set(x, y, if x < 16 { [0.2, 0.6, 0.9, 1.0] } else { [0.9, 0.3, 0.3, 1.0] }); } }
        state.session.set_mesh_maps(ts, std::collections::BTreeMap::from([(subcraft_core::MeshMapKind::Id, img)]));
        let red = state.session.id_map_color(ts, glam::Vec2::new(0.5, 0.5)).unwrap();
        state.id_palette_selected = Some(red);
        state.id_palette_new = red;
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let frame = |state: &mut UiState, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))), ..Default::default() }, |ui| section(state, ui));
            out.textures_delta.clear();
        };
        frame(&mut state, vec![]);
        ctx.memory_mut(|m| m.request_focus(hex_field()));
        frame(&mut state, vec![]);
        frame(&mut state, vec![egui::Event::Text("1".into())]);
        frame(&mut state, vec![]);
        let typed: Option<String> = ctx.data(|d| d.get_temp(hex_text()));
        assert_eq!(typed.as_deref().map(str::len), Some(8), "the extra digit stays in the field: {typed:?}");
        assert!(same_id(state.id_palette_new, red), "a value that does not parse changes nothing: {:?}", state.id_palette_new);
    }

    #[test]
    fn picking_an_id_color_shows_the_id_map_and_ending_it_restores_the_view() {
        use subcraft_core::MeshMapKind;
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2.0, 2), 64), None);
        state.recovery.prefs.viewport.mesh_map = Some(MeshMapKind::Normal);
        // No ID map yet: nothing to show, the view stays.
        state.picking_id_palette = true;
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Normal));
        state.picking_id_palette = false;
        sync_pick_view(&mut state);
        let ts = state.session.selected_texture_set().id;
        state.session.set_mesh_maps(ts, std::collections::BTreeMap::from([(MeshMapKind::Id, subcraft_image::TiledImage::new(64, 64, 3, [0.5; 4]))]));
        // An ID Color mask's Pick from model.
        state.picking_id_color = Some((subcraft_core::LayerId::new(), subcraft_core::EffectId::new()));
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Id), "the ID map while picking");
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Id));
        state.picking_id_color = None; // Esc or the button again
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Normal), "back to the mode before");
        // The editor's eyedropper from the material view.
        state.recovery.prefs.viewport.mesh_map = None;
        state.picking_id_palette = true;
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Id));
        // Switching modes during the pick: that choice stays when the pick ends.
        state.recovery.prefs.viewport.mesh_map = Some(MeshMapKind::Curvature);
        state.picking_id_palette = false;
        sync_pick_view(&mut state);
        assert_eq!(state.recovery.prefs.viewport.mesh_map, Some(MeshMapKind::Curvature));
        assert!(state.id_pick_view.is_none());
    }

    #[test]
    fn hex_round_trips() {
        let c = parse_hex("#3A7FC0").unwrap();
        assert_eq!(hex(c), "#3A7FC0");
        assert_eq!(parse_hex("12345"), None);
    }
}
