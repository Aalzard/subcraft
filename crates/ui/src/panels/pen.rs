//! The Pen panel of the "Painter (pen)" workspace: for tablet users, the brush colour on a full
//! colour wheel at the top, big tappable buttons for the actions a pen user reaches for (undo /
//! redo, eraser, size and flow steps, colour picker, swap colours, symmetry, 2D / 3D), then the
//! active brush with all its settings. It reuses the colour picker body and the Brush panel; the
//! buttons queue the same app actions the shortcuts run ([`UiState::pending_actions`]).

use egui::{RichText, Ui};
use subcraft_app::{Action, Command};
use subcraft_core::{ChannelKind, ChannelValue};

use crate::state::{Tool, UiState};
use crate::theme;

/// Height of the big buttons (points): comfortable for a pen tap.
pub const BUTTON_HEIGHT: f32 = 44.0;
/// Flow change of one Flow - / Flow + tap.
pub const FLOW_STEP: f32 = 0.1;

/// Flow after one tap of Flow + (`up`) or Flow -, kept in 0.05..=1 like the brush's flow row.
pub fn flow_step(flow: f32, up: bool) -> f32 {
    let next = if up { flow + FLOW_STEP } else { flow - FLOW_STEP };
    ((next * 100.0).round() / 100.0).clamp(0.05, 1.0)
}

/// The brush colour (base colour, linear) and the second colour, swapped.
pub fn swapped(brush: [f32; 3], second: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    (second, brush)
}

/// One big button of the grid; true when tapped.
fn big(ui: &mut Ui, icon: &str, label: &str, tip: &str, selected: bool) -> bool {
    let w = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(60.0);
    ui.add_sized([w, BUTTON_HEIGHT], egui::Button::new(RichText::new(format!("{icon}\n{label}")).size(12.0)).selected(selected))
        .on_hover_text(tip)
        .clicked()
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    // The brush colour on the colour wheel (when the brush paints a base colour).
    let mut material = state.session.brush.material.clone();
    if let Some(entry) = material.channels.get_mut(&ChannelKind::BaseColor).filter(|c| c.enabled) {
        let before = entry.value.0;
        let mut rgb = before;
        crate::color_picker::inline_picker(ui, "pen_panel", &mut rgb, &mut state.recovery.prefs.recent_colors);
        if rgb != before {
            entry.value = ChannelValue(rgb);
            if let Err(e) = state.session.execute(Command::SetBrushMaterial(material.clone())) {
                state.set_status(e.to_string(), now);
            }
        }
    } else {
        ui.label(RichText::new("The brush paints no base colour: turn Base Color on in the brush material below.").small().color(theme::TEXT_DIM()));
    }
    ui.add_space(6.0);
    let eraser = state.tool == Tool::Eraser;
    let mut queue = |a: Action| state.pending_actions.push(a);
    let mut flow = None;
    let mut swap = false;
    ui.horizontal_wrapped(|ui| {
        if big(ui, crate::icons::UNDO, "Undo", "Undo the last step", false) { queue(Action::Undo); }
        if big(ui, crate::icons::REDO, "Redo", "Redo", false) { queue(Action::Redo); }
        if big(ui, crate::icons::ERASER, if eraser { "Brush" } else { "Eraser" }, "Switch between the brush and the eraser", eraser) {
            queue(if eraser { Action::BrushTool } else { Action::EraserTool });
        }
        if big(ui, crate::icons::ARROW_DOWN, "Size -", "Smaller brush", false) { queue(Action::BrushSmaller); }
        if big(ui, crate::icons::ARROW_UP, "Size +", "Bigger brush", false) { queue(Action::BrushLarger); }
        if big(ui, crate::icons::CROSSHAIR, "Pick colour", "Eyedropper: tap the model to take its colour", false) { queue(Action::EyedropperTool); }
        if big(ui, crate::icons::ARROW_DOWN, "Flow -", "Less paint per stamp", false) { flow = Some(false); }
        if big(ui, crate::icons::ARROW_UP, "Flow +", "More paint per stamp", false) { flow = Some(true); }
        if big(ui, crate::icons::DROP, "Swap colours", "Swap the brush colour with the second colour", false) { swap = true; }
        if big(ui, crate::icons::SYMMETRY, "Symmetry", "Symmetry on / off", false) { queue(Action::ToggleSymmetry); }
        if big(ui, crate::icons::GRID, "2D / 3D", "Swap the 2D and 3D views", false) { queue(Action::SwapViews); }
        if big(ui, crate::icons::SQUARES, "Hide UI", "Hide every panel (Tab brings them back)", false) { queue(Action::HideUi); }
    });
    if let Some(up) = flow {
        let flow = flow_step(state.session.brush.settings.flow, up);
        state.session.brush.settings.flow = flow;
        state.set_status(format!("Flow {:.0}%", flow * 100.0), now);
    }
    if swap {
        let mut material = state.session.brush.material.clone();
        let base = material.channels.get(&ChannelKind::BaseColor).map(|c| c.value.0);
        if let Some(base) = base {
            let (brush, second) = swapped(base, state.pen_second_color);
            material.set(ChannelKind::BaseColor, ChannelValue(brush));
            state.pen_second_color = second;
            if let Err(e) = state.session.execute(Command::SetBrushMaterial(material)) {
                state.set_status(e.to_string(), now);
            }
        }
    }
    // The second colour, small, under the buttons (a tap opens its picker).
    ui.horizontal(|ui| {
        ui.label(RichText::new("Second colour").small().color(theme::TEXT_DIM()));
        crate::color_picker::color_button(ui, "pen_second", &mut state.pen_second_color, &mut state.recovery.prefs.recent_colors);
    });
    ui.separator();
    // The active brush with all its settings: the Brush panel itself (its own ids, so it does not
    // clash with the same panel in Properties).
    ui.push_id("pen_panel_brush", |ui| super::brush::show(state, ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_steps_and_colour_swap() {
        assert_eq!(flow_step(0.5, true), 0.6);
        assert_eq!(flow_step(0.5, false), 0.4);
        assert_eq!(flow_step(0.98, true), 1.0, "capped at full flow");
        assert_eq!(flow_step(0.1, false), 0.05, "never zero");
        assert_eq!(swapped([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    }
}
