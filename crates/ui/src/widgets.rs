//! Small shared editors.

use egui::Ui;
use subcraft_core::{ChannelKind, ChannelValue};

/// Result of an editor: `changed` this frame, `done` when an interaction finished.
#[derive(Clone, Copy, Default)]
pub struct Edit {
    pub changed: bool,
    pub done: bool,
}

impl Edit {
    pub fn from(r: &egui::Response) -> Self {
        Self { changed: r.changed(), done: r.drag_stopped() || r.lost_focus() }
    }
    pub fn or(self, o: Edit) -> Edit {
        Edit { changed: self.changed || o.changed, done: self.done || o.done }
    }
}

/// Edit a channel value: linear color picker for color channels, encoded RGB for normals,
/// a slider for scalars (Height is signed).
pub fn channel_value(ui: &mut Ui, kind: ChannelKind, v: &mut ChannelValue) -> Edit {
    if kind.components() == 3 {
        let r = ui.color_edit_button_rgb(&mut v.0);
        Edit::from(&r)
    } else {
        let range = if kind.is_signed() { -1.0..=1.0 } else { 0.0..=1.0 };
        let mut x = v.0[0];
        let r = ui.add(egui::Slider::new(&mut x, range).max_decimals(3));
        if r.changed() {
            *v = ChannelValue::scalar(x);
        }
        Edit::from(&r)
    }
}

/// Section header with a little extra spacing.
pub fn header(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).strong().color(crate::theme::ACCENT));
    ui.separator();
}
