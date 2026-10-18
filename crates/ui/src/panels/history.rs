//! Undo history list; clicking an entry jumps there.

use egui::{RichText, Ui};
use subcraft_app::Command;

use crate::state::UiState;
use crate::theme::{self, TEXT_FAINT};
use crate::widgets::striped_list_row;

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let h = state.session.history();
    let cursor = h.cursor();
    egui::ScrollArea::vertical().id_salt("history").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(4.0);
        for (i, e) in h.entries().iter().enumerate() {
            let undone = i > cursor;
            let resp = striped_list_row(ui, i, i == cursor, egui::Sense::click(), |ui| {
                ui.add_sized([22.0, 16.0], egui::Label::new(RichText::new(i.to_string()).font(theme::font(10.5)).color(TEXT_FAINT)));
                let mut text = RichText::new(&e.label).color(if undone { TEXT_FAINT } else if i == cursor { theme::TEXT } else { theme::TEXT_DIM });
                if undone {
                    text = text.italics();
                }
                ui.add(egui::Label::new(text).truncate().selectable(false));
            });
            if resp.clicked() && i != cursor {
                cmds.push(Command::JumpToHistory(i));
            }
        }
    });
    state.run(cmds, now);
}
