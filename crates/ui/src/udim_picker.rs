//! The 2D view's UDIM tile picker: for a texture set split into UDIM tiles, the tiles laid out as
//! in UV space (1001 bottom left, 1011 above it) to jump between, plus previous / next buttons.

use egui::{RichText, Ui};
use subcraft_core::TextureSetId;
use subcraft_doc::Document;

use crate::i18n::tr;
use crate::state::UiState;

/// The tiles of `selected`'s UDIM group as (set, UDIM number), by number. Empty for a set that is
/// not a tile, or a group of one.
pub fn tiles(doc: &Document, selected: TextureSetId) -> Vec<(TextureSetId, u32)> {
    let mut out: Vec<(TextureSetId, u32)> =
        doc.udim_group(selected).into_iter().filter_map(|id| doc.texture_set(id).and_then(|t| t.udim_number()).map(|n| (id, n))).collect();
    out.sort_by_key(|t| t.1);
    if out.len() < 2 {
        out.clear();
    }
    out
}

/// The tiles in rows as UV space shows them: the top row first (highest numbers), ten columns
/// (1001..1010 is the bottom row). Rows and columns without a tile in between are kept so the
/// layout stays true; columns right of the last used one are dropped.
pub fn grid(tiles: &[(TextureSetId, u32)]) -> Vec<Vec<Option<(TextureSetId, u32)>>> {
    let Some(rows) = tiles.iter().map(|t| (t.1.saturating_sub(1001)) / 10).max() else { return Vec::new() };
    let first_row = tiles.iter().map(|t| (t.1.saturating_sub(1001)) / 10).min().unwrap_or(0);
    let cols = tiles.iter().map(|t| (t.1.saturating_sub(1001)) % 10).max().unwrap_or(0) + 1;
    (first_row..=rows)
        .rev()
        .map(|r| (0..cols).map(|c| tiles.iter().copied().find(|t| t.1.saturating_sub(1001) == r * 10 + c)).collect())
        .collect()
}

/// The tile `step` places after (or before) `current`, by UDIM number, wrapping around.
pub fn step(tiles: &[(TextureSetId, u32)], current: TextureSetId, step: i32) -> Option<TextureSetId> {
    let i = tiles.iter().position(|t| t.0 == current)? as i32;
    let n = tiles.len() as i32;
    (n > 0).then(|| tiles[(i + step).rem_euclid(n) as usize].0)
}

/// Previous / tile grid / next, as a row under the 2D view's header (the header itself is often
/// too narrow). Nothing for a set that is not a tile.
pub fn header(state: &mut UiState, ui: &mut Ui) {
    let selected = state.session.selected_texture_set().id;
    let tiles = tiles(state.session.document(), selected);
    if tiles.is_empty() {
        return;
    }
    let current = tiles.iter().find(|t| t.0 == selected).map_or(0, |t| t.1);
    let mut pick = None;
    ui.horizontal(|ui| {
        if ui.small_button("<").on_hover_text(tr("Previous UDIM tile")).clicked() {
            pick = step(&tiles, selected, -1);
        }
        egui::ComboBox::from_id_salt("udim_tile").width(84.0).selected_text(format!("UDIM {current}")).show_ui(ui, |ui| {
            ui.label(RichText::new(tr("UDIM tiles of this texture set, as laid out in UV space")).small().color(crate::theme::TEXT_DIM()));
            egui::Grid::new("udim_grid").spacing([4.0, 4.0]).show(ui, |ui| {
                for row in grid(&tiles) {
                    for cell in row {
                        match cell {
                            Some((id, n)) => {
                                if ui.add_sized([44.0, 26.0], egui::Button::selectable(id == selected, n.to_string())).clicked() {
                                    pick = Some(id);
                                }
                            }
                            None => {
                                ui.add_sized([44.0, 26.0], egui::Label::new(RichText::new("·").color(crate::theme::TEXT_FAINT())));
                            }
                        }
                    }
                    ui.end_row();
                }
            });
    });
    if ui.small_button(">").on_hover_text(tr("Next UDIM tile")).clicked() {
        pick = step(&tiles, selected, 1);
    }
    ui.label(RichText::new(format!("{} / {}", tiles.iter().position(|t| t.0 == selected).map_or(0, |i| i + 1), tiles.len())).small().color(crate::theme::TEXT_DIM()));
    });
    if let Some(id) = pick.filter(|id| *id != selected) {
        let now = ui.input(|i| i.time);
        if let Err(e) = state.session.execute(subcraft_app::Command::SelectTextureSet(id)) {
            state.set_status(e.to_string(), now);
        }
        state.gpu_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<TextureSetId> {
        (0..n).map(|_| TextureSetId::new()).collect()
    }

    #[test]
    fn grid_lays_tiles_out_as_uv_space_with_gaps() {
        let id = ids(4);
        let tiles = vec![(id[0], 1001), (id[1], 1002), (id[2], 1004), (id[3], 1011)];
        let g = grid(&tiles);
        assert_eq!(g.len(), 2, "two rows");
        assert_eq!(g[0].len(), 4, "columns up to 1004");
        assert_eq!(g[0][0], Some((id[3], 1011)), "1011 is above 1001");
        assert_eq!(g[0][1], None);
        assert_eq!(g[1].iter().map(|c| c.map(|t| t.1)).collect::<Vec<_>>(), [Some(1001), Some(1002), None, Some(1004)]);
    }

    #[test]
    fn step_wraps_in_number_order() {
        let id = ids(3);
        let tiles = vec![(id[0], 1001), (id[1], 1002), (id[2], 1011)];
        assert_eq!(step(&tiles, id[0], 1), Some(id[1]));
        assert_eq!(step(&tiles, id[2], 1), Some(id[0]));
        assert_eq!(step(&tiles, id[0], -1), Some(id[2]));
        assert_eq!(step(&tiles, TextureSetId::new(), 1), None);
    }
}
