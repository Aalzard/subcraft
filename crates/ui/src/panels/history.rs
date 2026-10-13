//! Undo history list; clicking an entry jumps there.

use egui::{RichText, Ui};
use subcraft_app::Command;

use crate::state::UiState;

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let h = state.session.history();
    let cursor = h.cursor();
    egui::ScrollArea::vertical().id_salt("history").stick_to_bottom(true).show(ui, |ui| {
        for (i, e) in h.entries().iter().enumerate() {
            let mut text = RichText::new(format!("{i:>3}  {}", e.label));
            if i > cursor {
                text = text.weak().italics();
            }
            if ui.selectable_label(i == cursor, text).clicked() && i != cursor {
                cmds.push(Command::JumpToHistory(i));
            }
        }
    });
    state.run(cmds, now);
}
