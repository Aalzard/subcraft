//! Shared filter parameter controls. Callers dispatch a single existing SetEffect command.
use egui::Ui;
use subcraft_core::ChannelKind;
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};
use subcraft_doc::{Filter, Gradient, GradientStop, OutlinePosition, GRADIENT_PRESETS, MAX_GRADIENT_STOPS};
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
        Filter::GradientMap { gradient, to_gray } => {
            changed |= gradient_editor(ui, gradient);
            prop_row(ui, "Output", |ui| {
                changed |= ui.selectable_value(to_gray, false, "Color").changed();
                changed |= ui.selectable_value(to_gray, true, "Gray").changed();
            });
            ui.weak(if channel.is_some_and(|c| c.is_color()) { "Looked up by luminance / Gray keeps only brightness" } else { "Looked up by value / result is its brightness" });
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

fn gradient_editor(ui: &mut Ui, gradient: &mut Gradient) -> bool {
    let mut changed = false;
    prop_row(ui, "Preset", |ui| {
        egui::ComboBox::from_id_salt("gradient_preset").selected_text("Load preset...").show_ui(ui, |ui| {
            for (i, (name, _)) in GRADIENT_PRESETS.iter().enumerate() {
                if ui.selectable_label(false, *name).clicked() {
                    if let Some(g) = Gradient::preset(i) { *gradient = g; changed = true; }
                }
            }
        });
    });
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(40.0), 16.0), egui::Sense::hover());
    const SLICES: usize = 48;
    for i in 0..SLICES {
        let t0 = i as f32 / SLICES as f32;
        let c = linear3_to_srgb(gradient.eval((i as f32 + 0.5) / SLICES as f32));
        let slice = egui::Rect::from_min_max(egui::pos2(rect.lerp_inside(egui::vec2(t0, 0.0)).x, rect.top()), egui::pos2(rect.lerp_inside(egui::vec2((i + 1) as f32 / SLICES as f32, 1.0)).x, rect.bottom()));
        ui.painter().rect_filled(slice, 0.0, egui::Color32::from_rgb((c[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
    }
    let mut remove = None;
    let count = gradient.stops().len();
    let mut moved = false;
    for (i, stop) in gradient.stops_mut().iter_mut().enumerate() {
        ui.push_id(("gradient_stop", i), |ui| {
            ui.horizontal(|ui| {
                let mut rgb = linear3_to_srgb(stop.color);
                if ui.color_edit_button_rgb(&mut rgb).changed() { stop.color = srgb3_to_linear(rgb); changed = true; }
                let r = ui.add(egui::Slider::new(&mut stop.position, 0.0..=1.0).fixed_decimals(3));
                if r.changed() { changed = true; moved = true; }
                if ui.add_enabled(count > 1, egui::Button::new("x")).on_hover_text("Remove stop").clicked() { remove = Some(i); }
            });
        });
    }
    if moved { gradient.sort(); }
    if let Some(i) = remove { changed |= gradient.remove(i); }
    if ui.add_enabled(count < MAX_GRADIENT_STOPS, egui::Button::new("Add stop")).clicked() {
        let at = gradient.stops().windows(2).map(|w| (w[1].position - w[0].position, w[0].position)).fold((-1.0, 0.5), |a, b| if b.0 > a.0 { b } else { a });
        let position = if gradient.stops().len() > 1 { at.1 + at.0 * 0.5 } else { 1.0 };
        changed |= gradient.insert(GradientStop { position, color: gradient.eval(position) });
    }
    changed
}
