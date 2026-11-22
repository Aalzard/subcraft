//! Settings rows that go back to their default: a dot before the name of a changed setting,
//! Ctrl+click on the name or the control, and a right-click menu on the name. Used by the
//! Display Settings panel, the Rendering mode settings and, through the `*_reset` rows of
//! [`crate::widgets`], the property panels (layers, generators, filters, brush, ...).

use crate::i18n::tr;
use crate::theme;
use egui::Ui;

/// A row label that also takes the reset gestures.
pub fn label(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
}

/// Whether a Ctrl+click is the reset gesture right now (Cmd on macOS).
fn ctrl(ui: &Ui) -> bool {
    ui.input(|i| i.modifiers.command)
}

/// The Ctrl+click part of [`wants_reset`] alone, for compact controls in list rows (the layer and
/// effect opacity boxes) that have no label and their own right-click menu. True on a Ctrl+click.
pub fn ctrl_clicked(ui: &Ui, control: &egui::Response) -> bool {
    ctrl(ui) && control.interact_rect.height() > 0.0 && ui.interact(control.interact_rect, control.id.with("ctrl_reset"), egui::Sense::click_and_drag()).clicked()
}

/// The reset gestures of a settings row whose value differs from its default (`changed`).
///
/// While Ctrl is held, a click-and-drag cover is laid over `control`, so the Ctrl+click goes to
/// the reset and never to the control itself: a slider does not jump to the clicked spot, a
/// number field does not enter edit mode, a checkbox does not toggle and a dropdown does not open.
/// (The cover is hit-tested from the frame where Ctrl went down, before the click.)
///
/// The right-click menu sits on the label only: a dropdown's or colour swatch's own popup shares
/// its id with that menu. True when the user asked for the default back.
pub fn wants_reset(ui: &Ui, label: &egui::Response, control: &egui::Response, changed: bool) -> bool {
    // `interact_rect` is the part of the label inside the scroll area: rows scrolled out of view
    // must not paint their dot over other panels.
    if changed && label.interact_rect.height() > 0.0 {
        let at = label.rect.left_center() - egui::vec2(5.0, 0.0);
        let clip = label.interact_rect.expand2(egui::vec2(8.0, 0.0));
        label.ctx.layer_painter(label.layer_id).with_clip_rect(clip).circle_filled(at, 2.5, theme::ACCENT());
    }
    let ctrl = ctrl(ui);
    let mut clicked = ctrl && label.clicked();
    // `interact_rect` is the control's rect clipped to what is visible.
    if ctrl && control.interact_rect.height() > 0.0 {
        clicked |= ui.interact(control.interact_rect, control.id.with("ctrl_reset"), egui::Sense::click_and_drag()).clicked();
    }
    let mut go = changed && clicked;
    label.context_menu(|ui| {
        if ui.add_enabled(changed, egui::Button::new(format!("{}  {}", crate::icons::UNDO, tr("Reset to default")))).clicked() {
            go = true;
            ui.close();
        }
    });
    go
}

/// Make a settings row resettable to `default` (see [`wants_reset`]). True when it was reset.
pub fn reset<T: PartialEq + Clone>(ui: &Ui, label: &egui::Response, control: &egui::Response, value: &mut T, default: &T) -> bool {
    let go = wants_reset(ui, label, control, *value != *default);
    if go {
        *value = default.clone();
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| d.insert_temp(done_key(), (pass, control.id)));
    }
    go
}

fn done_key() -> egui::Id {
    egui::Id::new("reset_row_done")
}

/// Whether [`reset`] put the default back into `control` this pass: a finished edit, like the end
/// of a drag, so panels that commit on "done" (see [`crate::widgets::Edit`]) commit it too.
pub fn was_reset(control: &egui::Response) -> bool {
    let pass = control.ctx.cumulative_pass_nr();
    control.ctx.data(|d| d.get_temp::<(u64, egui::Id)>(done_key())) == Some((pass, control.id))
}
