//! Shared filter parameter controls. Callers dispatch a single existing SetEffect command.
use egui::Ui;
use subcraft_core::ChannelKind;
use subcraft_doc::Filter;
use crate::widgets::{prop_row, slider_row};

pub fn editor(ui: &mut Ui, filter: &mut Filter, channel: Option<ChannelKind>) -> bool {
    let mut changed = false;
    prop_row(ui, "Filter", |ui| {
        egui::ComboBox::from_id_salt("filter_kind").selected_text(filter.label()).show_ui(ui, |ui| {
            for choice in Filter::ALL {
                if channel.map_or(choice.supports_mask(), |k| choice.supports_channel(k))
                    && ui.selectable_label(filter.label() == choice.label(), choice.label()).clicked() {
                    *filter = choice; changed = true;
                }
            }
        });
    });
    let lower = if channel.is_some_and(|c| c.is_signed()) { -1.0 } else { 0.0 };
    match filter {
        Filter::Contrast { gain, pivot } => {
            changed |= slider_row(ui, "Contrast", egui::Slider::new(gain, 0.0..=4.0)).changed();
            changed |= slider_row(ui, "Pivot", egui::Slider::new(pivot, lower..=1.0)).changed();
        }
        Filter::Hsl { hue, saturation, lightness } => {
            changed |= slider_row(ui, "Hue", egui::Slider::new(hue, -180.0..=180.0).suffix("°")).changed();
            changed |= slider_row(ui, "Saturation", egui::Slider::new(saturation, 0.0..=2.0)).changed();
            changed |= slider_row(ui, "Lightness", egui::Slider::new(lightness, -1.0..=1.0)).changed();
        }
        Filter::Threshold { cutoff } => {
            changed |= slider_row(ui, "Cutoff", egui::Slider::new(cutoff, lower..=1.0)).changed();
        }
        Filter::Clamp { low, high } => {
            let lower = lower.min(*low).min(*high); let upper = 1.0f32.max(*low).max(*high);
            changed |= slider_row(ui, "Minimum", egui::Slider::new(low, lower..=*high)).changed();
            changed |= slider_row(ui, "Maximum", egui::Slider::new(high, *low..=upper)).changed();
        }
    }
    changed
}
