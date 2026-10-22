//! Shared filter parameter controls. Callers dispatch a single existing SetEffect command.
use egui::Ui;
use subcraft_core::ChannelKind;
use subcraft_doc::{Filter, OutlinePosition};
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
        Filter::Blur { radius } | Filter::Sharpen { radius, .. } | Filter::DirectionalBlur {radius,..} | Filter::Highpass {radius,..} | Filter::Bevel {radius,..} | Filter::MaskOutline {radius,..} => {
            changed |= slider_row(ui, "Radius", egui::Slider::new(radius, 0.0..=32.0).suffix(" px")).changed();
        }
        Filter::Clamp { low, high } => {
            let lower = lower.min(*low).min(*high); let upper = 1.0f32.max(*low).max(*high);
            changed |= slider_row(ui, "Minimum", egui::Slider::new(low, lower..=*high)).changed();
            changed |= slider_row(ui, "Maximum", egui::Slider::new(high, *low..=upper)).changed();
        }
    }
    if let Filter::Sharpen { amount, .. } | Filter::Highpass {amount,..} = filter {
        changed |= slider_row(ui, "Amount", egui::Slider::new(amount, 0.0..=4.0)).changed();
    }
    if let Filter::Bevel {amount,..} = filter {
        changed |= slider_row(ui, "Amount", egui::Slider::new(amount, lower..=1.0)).changed();
    }
    if let Filter::Bevel {threshold,..} | Filter::MaskOutline {threshold,..} = filter {
        changed |= slider_row(ui, "Threshold", egui::Slider::new(threshold, lower..=1.0)).changed();
    }
    if let Filter::MaskOutline {position,..} = filter {
        prop_row(ui,"Position",|ui| {
            egui::ComboBox::from_id_salt("outline_position").selected_text(match position {OutlinePosition::Inner=>"Inner",OutlinePosition::Outer=>"Outer",OutlinePosition::Both=>"Both"}).show_ui(ui,|ui| {
                for (value,label) in [(OutlinePosition::Inner,"Inner"),(OutlinePosition::Outer,"Outer"),(OutlinePosition::Both,"Both")] {
                    changed |= ui.selectable_value(position,value,label).changed();
                }
            });
        });
    }
    if filter.is_contour() {
        ui.weak("Threshold defines the shape / radius sets border width.");
        if channel.is_some() {ui.weak("Grayscale result / paint coverage is preserved.");}
        if matches!(filter,Filter::Bevel {..}) {ui.weak("Inward relief ramp / use Height for raised detail.");}
    }
    if let Filter::DirectionalBlur {angle_deg,..}=filter {
        changed |= slider_row(ui, "Angle", egui::Slider::new(angle_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
        ui.weak("0 degrees horizontal / 90 vertical");
    }
    if matches!(filter,Filter::Highpass {..}) {ui.weak(if channel.is_some_and(|c|c.is_signed()) {"Detail only / neutral Height = 0"} else {"Detail only / neutral = 0.5"});}
    if filter.radius().is_some() { ui.weak("UV pixels · texture edges clamp"); }
    if filter.softens_coverage() && channel.is_some() { ui.weak("Blur also softens paint coverage."); }
    changed
}
