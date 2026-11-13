//! Shared filter parameter controls. Callers dispatch a single existing SetEffect command.
use egui::Ui;
use subcraft_core::ChannelKind;
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};
use subcraft_doc::{Filter, Gradient, GradientStop, OutlinePosition, GRADIENT_PRESETS, MAX_GRADIENT_STOPS};
use crate::widgets::{prop_row, slider_row};

/// Filters whose name contains every word of `query` (case-insensitive), in list order.
pub fn search(query: &str, usable: impl Fn(&Filter) -> bool) -> Vec<Filter> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    Filter::ALL
        .into_iter()
        .filter(|f| usable(f))
        .filter(|f| {
            let name = f.label().to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .collect()
}

/// Painter's filter mini shelf: a button with the current filter that opens a searchable list.
/// Returns the picked filter (with its default parameters).
pub fn picker(ui: &mut Ui, current: &str, usable: impl Fn(&Filter) -> bool) -> Option<Filter> {
    let button = ui.button(format!("{current}  \u{25be}")).on_hover_text("Pick another filter (type to search)");
    let query_id = button.id.with("filter_search");
    let mut picked = None;
    egui::Popup::menu(&button).id(button.id.with("filter_picker")).show(|ui| {
        ui.set_min_width(200.0);
        let mut query = ui.data(|d| d.get_temp::<String>(query_id)).unwrap_or_default();
        let field = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search filters").desired_width(190.0));
        if button.clicked() { field.request_focus(); }
        let found = search(&query, &usable);
        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
            // Gradient Dynamic reads an image, so it is not in `Filter::ALL`: it starts without one.
            let dynamic = Filter::gradient_dynamic();
            let offer_dynamic = usable(&dynamic) && query.split_whitespace().all(|w| dynamic.label().to_lowercase().contains(&w.to_lowercase()));
            if found.is_empty() && !offer_dynamic { ui.weak("No filter matches"); }
            for f in found {
                if ui.selectable_label(f.label() == current, f.label()).clicked() {
                    picked = Some(f);
                    ui.close();
                }
            }
            if offer_dynamic && ui.selectable_label(current == dynamic.label(), dynamic.label()).clicked() {
                if current != dynamic.label() { picked = Some(dynamic); }
                ui.close();
            }
        });
        // Enter takes the first match.
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && let Some(f) = search(&query, &usable).into_iter().next() {
            picked = Some(f);
            ui.close();
        }
        ui.data_mut(|d| d.insert_temp(query_id, query));
    });
    picked
}

pub fn editor(ui: &mut Ui, filter: &mut Filter, channel: Option<ChannelKind>) -> bool {
    let mut changed = false;
    prop_row(ui, "Filter", |ui| {
        if let Some(choice) = picker(ui, filter.label(), |f| channel.map_or(f.supports_mask(), |k| f.supports_channel(k))) {
            if choice.label() != filter.label() {
                *filter = choice;
                changed = true;
            }
        }
    });
    let lower = if channel.is_some_and(|c| c.is_signed()) { -1.0 } else { 0.0 };
    match filter {
        Filter::Contrast { gain, pivot } => {
            changed |= slider_row(ui, "Contrast", egui::Slider::new(gain, 0.0..=4.0)).changed();
            changed |= slider_row(ui, "Pivot", egui::Slider::new(pivot, lower..=1.0)).changed();
        }
        Filter::Hsl { hue, saturation, lightness } | Filter::HslPerceptive { hue, saturation, lightness } => {
            changed |= slider_row(ui, "Hue", egui::Slider::new(hue, -180.0..=180.0).suffix("°")).changed();
            changed |= slider_row(ui, "Saturation", egui::Slider::new(saturation, 0.0..=2.0)).changed();
            changed |= slider_row(ui, "Lightness", egui::Slider::new(lightness, -1.0..=1.0)).changed();
        }
        Filter::Threshold { cutoff } | Filter::ThresholdCompare { cutoff, .. } => {
            changed |= slider_row(ui, "Cutoff", egui::Slider::new(cutoff, lower..=1.0)).changed();
        }
        Filter::Smoothstep { min, max } => {
            changed |= slider_row(ui, "Minimum", egui::Slider::new(min, lower..=1.0)).changed();
            changed |= slider_row(ui, "Maximum", egui::Slider::new(max, lower..=1.0)).changed();
            ui.weak("Smooth ramp from Minimum (0) to Maximum (1)");
        }
        Filter::ContrastLuminosity { contrast, luminosity } => {
            changed |= slider_row(ui, "Contrast", egui::Slider::new(contrast, -1.0..=1.0)).changed();
            changed |= slider_row(ui, "Luminosity", egui::Slider::new(luminosity, -1.0..=1.0)).changed();
        }
        Filter::ColorCorrect { shadows, midtones, highlights } => {
            for (name, range) in [("Shadows", shadows), ("Midtones", midtones), ("Highlights", highlights)] {
                ui.label(egui::RichText::new(name).strong());
                ui.push_id(name, |ui| {
                    for (label, v) in ["Contrast", "Luminosity", "Saturation"].into_iter().zip(range.iter_mut()) {
                        changed |= slider_row(ui, label, egui::Slider::new(v, -1.0..=1.0)).changed();
                    }
                });
            }
        }
        Filter::Transform { repeat, flip, scale, offset, rotation_deg, tile_safe } => {
            prop_row(ui, "Repeat mode", |ui| {
                egui::ComboBox::from_id_salt("transform_repeat").selected_text(repeat.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::RepeatMode::ALL { changed |= ui.selectable_value(repeat, m, m.label()).changed(); }
                });
            });
            prop_row(ui, "Flip mode", |ui| {
                egui::ComboBox::from_id_salt("transform_flip").selected_text(flip.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::FlipMode::ALL { changed |= ui.selectable_value(flip, m, m.label()).changed(); }
                });
            });
            changed |= slider_row(ui, "Scaling X", egui::Slider::new(&mut scale[0], 0.01..=8.0).logarithmic(true)).changed();
            changed |= slider_row(ui, "Scaling Y", egui::Slider::new(&mut scale[1], 0.01..=8.0).logarithmic(true)).changed();
            changed |= slider_row(ui, "Offset X", egui::Slider::new(&mut offset[0], -1.0..=1.0)).changed();
            changed |= slider_row(ui, "Offset Y", egui::Slider::new(&mut offset[1], -1.0..=1.0)).changed();
            prop_row(ui, "Tile Safe Rotation", |ui| { changed |= ui.checkbox(tile_safe, "").on_hover_text("Quarter turns and whole repetitions only, so a tiling image still tiles").changed(); });
            changed |= slider_row(ui, "Rotation", egui::Slider::new(rotation_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
            ui.weak("UV space / also moves paint coverage");
        }
        Filter::Mirror { mode, offset, invert_x, invert_y, corner } => {
            use subcraft_doc::MirrorMode;
            prop_row(ui, "Mode", |ui| {
                egui::ComboBox::from_id_salt("mirror_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                    for m in MirrorMode::ALL { changed |= ui.selectable_value(mode, m, m.label()).changed(); }
                });
            });
            if *mode != MirrorMode::AxisY {
                changed |= slider_row(ui, "Axis X Offset", egui::Slider::new(&mut offset[0], 0.0..=1.0)).changed();
                prop_row(ui, "Invert Axis X", |ui| { changed |= ui.checkbox(invert_x, "").changed(); });
            }
            if *mode != MirrorMode::AxisX {
                changed |= slider_row(ui, "Axis Y Offset", egui::Slider::new(&mut offset[1], 0.0..=1.0)).changed();
                prop_row(ui, "Invert Axis Y", |ui| { changed |= ui.checkbox(invert_y, "").changed(); });
            }
            if *mode == MirrorMode::Corner {
                prop_row(ui, "Corner Type", |ui| {
                    egui::ComboBox::from_id_salt("mirror_corner").selected_text(corner.label()).show_ui(ui, |ui| {
                        for c in subcraft_doc::MirrorCorner::ALL { changed |= ui.selectable_value(corner, c, c.label()).changed(); }
                    });
                });
            }
            ui.weak("UV space / the kept side is copied onto the other");
        }
        Filter::Grayscale => { ui.weak("Luminance to gray / color channels only"); }
        Filter::Invert => { ui.weak("1 - value / signed Height flips its sign"); }
        Filter::Posterize { levels } => {
            changed |= slider_row(ui, "Levels", egui::Slider::new(levels, 2..=subcraft_doc::MAX_POSTERIZE_LEVELS)).changed();
        }
        Filter::HistogramShift { shift } | Filter::HistogramShiftWrap { shift } => {
            changed |= slider_row(ui, "Shift", egui::Slider::new(shift, -1.0..=1.0)).changed();
        }
        Filter::ColorBalance { shadows, midtones, highlights } => {
            for (name, range) in [("Shadows", shadows), ("Midtones", midtones), ("Highlights", highlights)] {
                for (axis, v) in ["R", "G", "B"].into_iter().zip(range.iter_mut()) {
                    changed |= slider_row(ui, &format!("{name} {axis}"), egui::Slider::new(v, -1.0..=1.0)).changed();
                }
            }
            ui.weak("Offsets per tonal range (linear RGB)");
        }
        Filter::ReplaceColor { from, to, tolerance, softness } => {
            for (name, c) in [("From", from), ("To", to)] {
                prop_row(ui, name, |ui| {
                    let mut srgb = linear3_to_srgb(*c);
                    if ui.color_edit_button_rgb(&mut srgb).changed() { *c = srgb3_to_linear(srgb); changed = true; }
                });
            }
            changed |= slider_row(ui, "Tolerance", egui::Slider::new(tolerance, 0.0..=2.0)).changed();
            changed |= slider_row(ui, "Softness", egui::Slider::new(softness, 0.0..=2.0)).changed();
        }
        Filter::NormalInvert { flip_x, flip_y, flip_z } => {
            prop_row(ui, "Flip X (red)", |ui| { changed |= ui.checkbox(flip_x, "").changed(); });
            prop_row(ui, "Flip Y (green)", |ui| { changed |= ui.checkbox(flip_y, "").changed(); });
            prop_row(ui, "Flip Z (blue)", |ui| { changed |= ui.checkbox(flip_z, "").changed(); });
            ui.weak("Flip Y converts OpenGL and DirectX normal maps");
        }
        Filter::EdgeDetect { radius, strength } => {
            changed |= slider_row(ui, "Width", egui::Slider::new(radius, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Strength", egui::Slider::new(strength, 0.0..=8.0)).changed();
            ui.weak("Sobel edges of the brightness / grayscale result");
        }
        Filter::GradientDynamic { vertical, position, .. } => {
            prop_row(ui, "Gradient Orientation", |ui| {
                changed |= ui.selectable_value(vertical, false, "Horizontal").changed();
                changed |= ui.selectable_value(vertical, true, "Vertical").changed();
            });
            changed |= slider_row(ui, "Gradient Input Position", egui::Slider::new(position, 0.0..=1.0)).changed();
            ui.weak("Values pick colours along one row (or column) of the Gradient Source image");
        }
        Filter::HeightAdjust { invert, offset, multiply } => {
            prop_row(ui, "Invert", |ui| { changed |= ui.checkbox(invert, "").changed(); });
            changed |= slider_row(ui, "Offset", egui::Slider::new(offset, -1.0..=1.0)).changed();
            changed |= slider_row(ui, "Multiply", egui::Slider::new(multiply, 0.0..=4.0)).changed();
            ui.weak("Invert, then value x Multiply + Offset");
        }
        Filter::GradientCurve { preset, invert, balance, contrast } => {
            prop_row(ui, "Preset", |ui| {
                egui::ComboBox::from_id_salt("gradient_curve_preset").selected_text(preset.label()).show_ui(ui, |ui| {
                    for p in subcraft_doc::CurvePreset::ALL { changed |= ui.selectable_value(preset, p, p.label()).changed(); }
                });
            });
            prop_row(ui, "Invert", |ui| { changed |= ui.checkbox(invert, "").changed(); });
            changed |= slider_row(ui, "Balance", egui::Slider::new(balance, 0.0..=1.0)).changed();
            changed |= slider_row(ui, "Contrast", egui::Slider::new(contrast, 0.0..=1.0)).changed();
            ui.weak("Sends each value through the curve, then Balance and Contrast");
        }
        Filter::BevelSmooth { radius, direction, smoothing, curve_offset, curve_shape, threshold } => {
            prop_row(ui, "Direction", |ui| {
                egui::ComboBox::from_id_salt("bevel_smooth_direction").selected_text(direction.label()).show_ui(ui, |ui| {
                    for d in subcraft_doc::BevelDirection::ALL { changed |= ui.selectable_value(direction, d, d.label()).changed(); }
                });
            });
            changed |= slider_row(ui, "Distance", egui::Slider::new(radius, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Smoothing", egui::Slider::new(smoothing, 0.0..=1.0)).changed();
            changed |= slider_row(ui, "Curve Offset", egui::Slider::new(curve_offset, -0.9..=0.9)).on_hover_text("Above 0 the ramp starts further from the contour; below 0 it ends sooner").changed();
            changed |= slider_row(ui, "Curve Shape", egui::Slider::new(curve_shape, -1.0..=1.0)).on_hover_text("Below 0 pinched, above 0 rounded").changed();
            changed |= slider_row(ui, "Mask Threshold", egui::Slider::new(threshold, 0.0..=1.0)).on_hover_text("Values at or above this are the shape").changed();
            if channel.is_some() { ui.weak("On layer content the ramp shows where there is paint: use Inward, or put the filter on a mask"); }
        }
        Filter::DirectionalDistance { distance, angle_deg, contrast } => {
            changed |= slider_row(ui, "Distance", egui::Slider::new(distance, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Angle", egui::Slider::new(angle_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
            changed |= slider_row(ui, "Contrast", egui::Slider::new(contrast, 0.0..=1.0)).changed();
            ui.weak("Spreads bright values along the angle, fading with the distance");
        }
        Filter::Fxaa { alpha } => {
            prop_row(ui, "Affect Alpha Channel", |ui| { changed |= ui.checkbox(alpha, "").on_hover_text("Also smooth the edges of the paint coverage").changed(); });
            ui.weak("Smooths stair-stepped edges along their direction");
        }
        Filter::DropShadow { angle_deg, distance, size, spread, opacity, color } => {
            changed |= slider_row(ui, "Angle", egui::Slider::new(angle_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
            changed |= slider_row(ui, "Distance", egui::Slider::new(distance, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Size", egui::Slider::new(size, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Spread", egui::Slider::new(spread, 0.0..=0.99)).changed();
            changed |= slider_row(ui, "Opacity", egui::Slider::new(opacity, 0.0..=1.0)).changed();
            prop_row(ui, "Color", |ui| {
                let mut srgb = linear3_to_srgb(*color);
                if ui.color_edit_button_rgb(&mut srgb).changed() { *color = srgb3_to_linear(srgb); changed = true; }
            });
            ui.weak("Shadow of the paint coverage, under the content");
        }
        Filter::OuterGlow { width, spread, opacity, color, precise } => {
            prop_row(ui, "Mode", |ui| {
                changed |= ui.selectable_value(precise, false, "Soft").changed();
                changed |= ui.selectable_value(precise, true, "Precise").changed();
            });
            changed |= slider_row(ui, "Width", egui::Slider::new(width, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Spread", egui::Slider::new(spread, 0.0..=0.99)).changed();
            changed |= slider_row(ui, "Opacity", egui::Slider::new(opacity, 0.0..=1.0)).changed();
            prop_row(ui, "Color", |ui| {
                let mut srgb = linear3_to_srgb(*color);
                if ui.color_edit_button_rgb(&mut srgb).changed() { *color = srgb3_to_linear(srgb); changed = true; }
            });
            ui.weak(if channel.is_some_and(|c| c.is_color()) { "Glow around the paint coverage, under the content" } else { "Glow around bright values (masks and gray channels use the color's brightness)" });
        }
        Filter::BevelShape { radius, .. } | Filter::MaskOutlineShape { radius, .. } => {
            changed |= slider_row(ui, "Radius", egui::Slider::new(radius, 0.0..=32.0).suffix(" px")).changed();
        }
        Filter::Glow { radius, amount, threshold } => {
            changed |= slider_row(ui, "Radius", egui::Slider::new(radius, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Amount", egui::Slider::new(amount, 0.0..=4.0)).changed();
            changed |= slider_row(ui, "Threshold", egui::Slider::new(threshold, 0.0..=0.99)).changed();
        }
        Filter::DirectionalWarp { intensity, angle_deg } => {
            changed |= slider_row(ui, "Intensity", egui::Slider::new(intensity, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Angle", egui::Slider::new(angle_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
            ui.weak("Pushes pixels along the angle by their own brightness");
        }
        Filter::Mosaic { size } => {
            changed |= slider_row(ui, "Block size", egui::Slider::new(size, 1..=subcraft_doc::MAX_MOSAIC_SIZE).suffix(" px")).changed();
        }
        Filter::Blur { radius } | Filter::BlurWrap { radius } | Filter::Sharpen { radius, .. } | Filter::DirectionalBlur {radius,..} | Filter::Highpass {radius,..} | Filter::Bevel {radius,..} | Filter::MaskOutline {radius,..} => {
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
        Filter::Median { radius } => {
            changed |= slider_row(ui, "Radius", egui::Slider::new(radius, 0.0..=subcraft_doc::MAX_MEDIAN_RADIUS).integer().suffix(" px")).changed();
            ui.weak("Exact median over the square window / removes speckle, keeps edges");
        }
        Filter::Warp { intensity, scale, seed } => {
            changed |= slider_row(ui, "Intensity", egui::Slider::new(intensity, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row(ui, "Scale", egui::Slider::new(scale, 1..=subcraft_doc::MAX_WARP_SCALE)).changed();
            changed |= slider_row(ui, "Seed", egui::Slider::new(seed, 0..=9999)).changed();
            ui.weak("Noise-driven displacement / Scale = cells across the texture");
        }
        Filter::HistogramScan { position, contrast, invert } => {
            changed |= slider_row(ui, "Position", egui::Slider::new(position, lower..=1.0)).changed();
            changed |= slider_row(ui, "Contrast", egui::Slider::new(contrast, 0.0..=1.0)).changed();
            prop_row(ui, "Invert", |ui| { changed |= ui.checkbox(invert, "").changed(); });
            ui.weak("Soft threshold / contrast 1 is a hard edge");
        }
        Filter::HistogramRange { position, range, softness, invert } => {
            changed |= slider_row(ui, "Position", egui::Slider::new(position, lower..=1.0)).changed();
            changed |= slider_row(ui, "Range", egui::Slider::new(range, 0.0..=if lower < 0.0 { 2.0 } else { 1.0 })).changed();
            changed |= slider_row(ui, "Softness", egui::Slider::new(softness, 0.0..=1.0)).changed();
            prop_row(ui, "Invert", |ui| { changed |= ui.checkbox(invert, "").changed(); });
            ui.weak("Selects values near Position / outside the band is 0");
        }
        Filter::Clamp { low, high } => {
            let lower = lower.min(*low).min(*high); let upper = 1.0f32.max(*low).max(*high);
            changed |= slider_row(ui, "Minimum", egui::Slider::new(low, lower..=*high)).changed();
            changed |= slider_row(ui, "Maximum", egui::Slider::new(high, *low..=upper)).changed();
        }
    }
    // Mode / wrap rows: these swap between two variants of the same filter (see `Filter::label`).
    if let Some((cutoff, mut mode)) = filter.threshold_parts() {
        prop_row(ui, "Mode", |ui| {
            egui::ComboBox::from_id_salt("threshold_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::ThresholdMode::ALL {
                    if ui.selectable_value(&mut mode, m, m.label()).changed() { *filter = Filter::threshold(cutoff, mode); changed = true; }
                }
            });
        });
    }
    if let Some((mut mode, [hue, saturation, lightness])) = filter.hsl_parts() {
        prop_row(ui, "Mode", |ui| {
            egui::ComboBox::from_id_salt("hsl_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::HslMode::ALL {
                    if ui.selectable_value(&mut mode, m, m.label()).changed() { *filter = Filter::hsl(mode, hue, saturation, lightness); changed = true; }
                }
            });
        });
    }
    if let Some((radius, mut wrap)) = filter.blur_parts() {
        prop_row(ui, "Wrap", |ui| {
            if ui.checkbox(&mut wrap, "").on_hover_text("The blur continues across the texture borders, for tiling textures").changed() { *filter = Filter::blur(radius, wrap); changed = true; }
        });
    }
    if let Some((shift, mut wrap)) = filter.histogram_shift_parts() {
        prop_row(ui, "Wrap", |ui| {
            if ui.checkbox(&mut wrap, "").on_hover_text("Values pushed past one end of the range re-enter at the other").changed() { *filter = Filter::histogram_shift(shift, wrap); changed = true; }
        });
    }
    if let Filter::Sharpen { amount, .. } | Filter::Highpass {amount,..} = filter {
        changed |= slider_row(ui, "Amount", egui::Slider::new(amount, 0.0..=4.0)).changed();
    }
    if let Filter::Bevel {amount,..} | Filter::BevelShape {amount,..} = filter {
        changed |= slider_row(ui, "Amount", egui::Slider::new(amount, lower..=1.0)).changed();
    }
    if let Filter::Bevel {threshold,..} | Filter::MaskOutline {threshold,..} | Filter::BevelShape {threshold,..} | Filter::MaskOutlineShape {threshold,..} = filter {
        changed |= slider_row(ui, "Threshold", egui::Slider::new(threshold, lower..=1.0)).changed();
    }
    // Bevel's Corner Type and Smoothing, Mask Outline's shape options: these swap to the
    // variant that stores them (see `Filter::bevel`, `Filter::mask_outline`).
    if let Some(mut p) = filter.bevel_parts() {
        let before = p;
        prop_row(ui, "Corner Type", |ui| {
            egui::ComboBox::from_id_salt("bevel_corner").selected_text(p.corner.label()).show_ui(ui, |ui| {
                for c in subcraft_doc::BevelCorner::ALL { ui.selectable_value(&mut p.corner, c, c.label()); }
            });
        });
        slider_row(ui, "Smoothing", egui::Slider::new(&mut p.smoothing, 0.0..=1.0)).on_hover_text("Eases the ramp from straight to rounded");
        if p != before { *filter = Filter::bevel(p); changed = true; }
    }
    if let Some(mut o) = filter.outline_parts() {
        let before = o;
        prop_row(ui, "Start From Black", |ui| { ui.checkbox(&mut o.start_black, "").on_hover_text("The band ramps up from black at the contour"); });
        slider_row(ui, "Blur", egui::Slider::new(&mut o.blur, 0.0..=32.0).suffix(" px")).on_hover_text("Fades the band's outer edge over this width");
        slider_row(ui, "Curve Shape", egui::Slider::new(&mut o.curve, -1.0..=1.0)).on_hover_text("Bends the band's falloff: above 0 sharper, below 0 softer");
        if o != before { *filter = Filter::mask_outline(o); changed = true; }
    }
    if let Filter::MaskOutline {position,..} | Filter::MaskOutlineShape {position,..} = filter {
        prop_row(ui,"Position",|ui| {
            egui::ComboBox::from_id_salt("outline_position").selected_text(match position {OutlinePosition::Inner=>"Inner",OutlinePosition::Outer=>"Outer",OutlinePosition::Both=>"Both"}).show_ui(ui,|ui| {
                for (value,label) in [(OutlinePosition::Inner,"Inner"),(OutlinePosition::Outer,"Outer"),(OutlinePosition::Both,"Both")] {
                    changed |= ui.selectable_value(position,value,label).changed();
                }
            });
        });
    }
    if filter.is_contour() && !matches!(filter, Filter::BevelSmooth { .. }) {
        ui.weak("Threshold defines the shape / radius sets border width.");
        if channel.is_some() {ui.weak("Grayscale result / paint coverage is preserved.");}
        if matches!(filter,Filter::Bevel {..}) {ui.weak("Inward relief ramp / use Height for raised detail.");}
    }
    if let Filter::DirectionalBlur {angle_deg,..}=filter {
        changed |= slider_row(ui, "Angle", egui::Slider::new(angle_deg, -180.0..=180.0).suffix("\u{00b0}")).changed();
        ui.weak("0 degrees horizontal / 90 vertical");
    }
    if matches!(filter,Filter::Highpass {..}) {ui.weak(if channel.is_some_and(|c|c.is_signed()) {"Detail only / neutral Height = 0"} else {"Detail only / neutral = 0.5"});}
    if filter.radius().is_some() && !filter.reads_whole_texture() { ui.weak("UV pixels · texture edges clamp"); }
    if matches!(filter, Filter::BlurWrap { .. }) { ui.weak("UV pixels · wraps around the texture"); }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_search_matches_every_word_and_respects_what_the_channel_supports() {
        let all = search("", |_| true);
        assert_eq!(all.len(), Filter::ALL.len());
        let blur: Vec<&str> = search("blur", |_| true).iter().map(|f| f.label()).collect();
        assert!(!blur.is_empty() && blur.iter().all(|l| l.to_lowercase().contains("blur")), "{blur:?}");
        assert_eq!(search("BLUR", |_| true).len(), blur.len(), "case does not matter");
        assert!(search("zzz", |_| true).is_empty());
        // Words can come in any order.
        if let Some(two) = Filter::ALL.iter().map(|f| f.label()).find(|l| l.contains(' ')) {
            let mut words: Vec<&str> = two.split(' ').collect();
            words.reverse();
            assert!(search(&words.join(" "), |_| true).iter().any(|f| f.label() == two));
        }
        // Colour-only filters are not offered for a grey channel.
        let grey = search("", |f| f.supports_channel(ChannelKind::Roughness));
        assert!(!grey.iter().any(|f| f.label() == "Grayscale"));
    }
}
