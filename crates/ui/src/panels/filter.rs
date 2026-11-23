//! Shared filter parameter controls. Callers dispatch a single existing SetEffect command.
use egui::Ui;
use subcraft_core::ChannelKind;
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};
use subcraft_doc::{Filter, Gradient, GradientStop, OutlinePosition, GRADIENT_PRESETS, MAX_GRADIENT_STOPS};
use crate::widgets::{prop_row, prop_row_reset, slider_row_reset};

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

/// A noise seed with Painter's Random button (a new seed from the old one, so it is repeatable).
/// The seed field resets to `default` (the reset gestures of [`crate::reset_row`]).
fn seed_row(ui: &mut Ui, seed: &mut u32, default: u32) -> bool {
    prop_row_reset(ui, "Seed", seed, default, |ui, seed| {
        let mut r = ui.add(egui::DragValue::new(seed).range(0..=9999));
        if ui.small_button("Random").on_hover_text("Pick another seed").clicked() {
            *seed = next_seed(*seed);
            r.mark_changed();
        }
        r
    })
    .changed()
}

/// The seed the Random button gives after `seed`: another value in 0..=9999.
pub fn next_seed(seed: u32) -> u32 {
    let mut h = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h ^= h >> 16;
    let next = h % 10_000;
    if next == seed { (next + 1) % 10_000 } else { next }
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
    let def = default_like(filter);
    match (&mut *filter, &def) {
        (Filter::Contrast { gain, pivot }, Filter::Contrast { gain: d_gain, pivot: d_pivot }) => {
            changed |= slider_row_reset(ui, "Contrast", gain, *d_gain, |v| egui::Slider::new(v, 0.0..=4.0)).changed();
            changed |= slider_row_reset(ui, "Pivot", pivot, *d_pivot, |v| egui::Slider::new(v, lower..=1.0)).changed();
        }
        (Filter::Hsl { hue, saturation, lightness }, Filter::Hsl { hue: d_hue, saturation: d_saturation, lightness: d_lightness }) | (Filter::HslPerceptive { hue, saturation, lightness }, Filter::HslPerceptive { hue: d_hue, saturation: d_saturation, lightness: d_lightness }) => {
            changed |= slider_row_reset(ui, "Hue", hue, *d_hue, |v| egui::Slider::new(v, -180.0..=180.0).suffix("°")).changed();
            changed |= slider_row_reset(ui, "Saturation", saturation, *d_saturation, |v| egui::Slider::new(v, 0.0..=2.0)).changed();
            changed |= slider_row_reset(ui, "Lightness", lightness, *d_lightness, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
        }
        (Filter::Threshold { cutoff }, Filter::Threshold { cutoff: d_cutoff }) | (Filter::ThresholdCompare { cutoff, .. }, Filter::ThresholdCompare { cutoff: d_cutoff, .. }) => {
            changed |= slider_row_reset(ui, "Cutoff", cutoff, *d_cutoff, |v| egui::Slider::new(v, lower..=1.0)).changed();
        }
        (Filter::Smoothstep { min, max }, Filter::Smoothstep { min: d_min, max: d_max }) => {
            changed |= slider_row_reset(ui, "Minimum", min, *d_min, |v| egui::Slider::new(v, lower..=1.0)).changed();
            changed |= slider_row_reset(ui, "Maximum", max, *d_max, |v| egui::Slider::new(v, lower..=1.0)).changed();
            ui.weak("Smooth ramp from Minimum (0) to Maximum (1)");
        }
        (Filter::ContrastLuminosity { contrast, luminosity }, Filter::ContrastLuminosity { contrast: d_contrast, luminosity: d_luminosity }) => {
            changed |= slider_row_reset(ui, "Contrast", contrast, *d_contrast, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "Luminosity", luminosity, *d_luminosity, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
        }
        (Filter::ColorCorrect { shadows, midtones, highlights }, Filter::ColorCorrect { shadows: d_shadows, midtones: d_midtones, highlights: d_highlights }) => {
            for (name, range, d_range) in [("Shadows", shadows, d_shadows), ("Midtones", midtones, d_midtones), ("Highlights", highlights, d_highlights)] {
                ui.label(egui::RichText::new(name).strong());
                ui.push_id(name, |ui| {
                    for ((label, v), d) in ["Contrast", "Luminosity", "Saturation"].into_iter().zip(range.iter_mut()).zip(d_range) {
                        changed |= slider_row_reset(ui, label, v, *d, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
                    }
                });
            }
        }
        (Filter::Transform { repeat, flip, scale, offset, rotation_deg, tile_safe }, Filter::Transform { repeat: d_repeat, flip: d_flip, scale: d_scale, offset: d_offset, rotation_deg: d_rotation_deg, tile_safe: d_tile_safe }) => {
            changed |= prop_row_reset(ui, "Repeat mode", repeat, *d_repeat, |ui, repeat| {
                egui::ComboBox::from_id_salt("transform_repeat").selected_text(repeat.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::RepeatMode::ALL { changed |= ui.selectable_value(repeat, m, m.label()).changed(); }
                }).response
            }).changed();
            changed |= prop_row_reset(ui, "Flip mode", flip, *d_flip, |ui, flip| {
                egui::ComboBox::from_id_salt("transform_flip").selected_text(flip.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::FlipMode::ALL { changed |= ui.selectable_value(flip, m, m.label()).changed(); }
                }).response
            }).changed();
            changed |= slider_row_reset(ui, "Scaling X", &mut scale[0], d_scale[0], |v| egui::Slider::new(v, 0.01..=8.0).logarithmic(true)).changed();
            changed |= slider_row_reset(ui, "Scaling Y", &mut scale[1], d_scale[1], |v| egui::Slider::new(v, 0.01..=8.0).logarithmic(true)).changed();
            changed |= slider_row_reset(ui, "Offset X", &mut offset[0], d_offset[0], |v| egui::Slider::new(v, -1.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "Offset Y", &mut offset[1], d_offset[1], |v| egui::Slider::new(v, -1.0..=1.0)).changed();
            changed |= prop_row_reset(ui, "Tile Safe Rotation", tile_safe, *d_tile_safe, |ui, v| ui.checkbox(v, "").on_hover_text("Quarter turns and whole repetitions only, so a tiling image still tiles")).changed();
            changed |= slider_row_reset(ui, "Rotation", rotation_deg, *d_rotation_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("\u{00b0}")).changed();
            ui.weak("UV space / also moves paint coverage");
        }
        (Filter::Mirror { mode, offset, invert_x, invert_y, corner }, Filter::Mirror { mode: d_mode, offset: d_offset, invert_x: d_invert_x, invert_y: d_invert_y, corner: d_corner }) => {
            use subcraft_doc::MirrorMode;
            changed |= prop_row_reset(ui, "Mode", mode, *d_mode, |ui, mode| {
                egui::ComboBox::from_id_salt("mirror_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                    for m in MirrorMode::ALL { changed |= ui.selectable_value(mode, m, m.label()).changed(); }
                }).response
            }).changed();
            if *mode != MirrorMode::AxisY {
                changed |= slider_row_reset(ui, "Axis X Offset", &mut offset[0], d_offset[0], |v| egui::Slider::new(v, 0.0..=1.0)).changed();
                changed |= prop_row_reset(ui, "Invert Axis X", invert_x, *d_invert_x, |ui, v| ui.checkbox(v, "")).changed();
            }
            if *mode != MirrorMode::AxisX {
                changed |= slider_row_reset(ui, "Axis Y Offset", &mut offset[1], d_offset[1], |v| egui::Slider::new(v, 0.0..=1.0)).changed();
                changed |= prop_row_reset(ui, "Invert Axis Y", invert_y, *d_invert_y, |ui, v| ui.checkbox(v, "")).changed();
            }
            if *mode == MirrorMode::Corner {
                changed |= prop_row_reset(ui, "Corner Type", corner, *d_corner, |ui, corner| {
                    egui::ComboBox::from_id_salt("mirror_corner").selected_text(corner.label()).show_ui(ui, |ui| {
                        for c in subcraft_doc::MirrorCorner::ALL { changed |= ui.selectable_value(corner, c, c.label()).changed(); }
                    }).response
                }).changed();
            }
            ui.weak("UV space / the kept side is copied onto the other");
        }
        (Filter::Grayscale, _) | (Filter::GrayscaleConvert { .. }, _) => {
            // The conversion options swap to the variant that stores them (see `Filter::grayscale`).
            if let Some(mut p) = filter.grayscale_parts() {
                let before = p;
                let d = subcraft_doc::GrayParts::default();
                let custom_weights = p.weights.is_some();
                prop_row_reset(ui, "Mode", &mut p.mode, d.mode, |ui, mode| {
                    ui.add_enabled_ui(!custom_weights, |ui| {
                        egui::ComboBox::from_id_salt("grayscale_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                            for m in subcraft_doc::GrayscaleType::ALL { ui.selectable_value(mode, m, m.label()); }
                        }).response
                    }).inner
                });
                // Painter's Red / Green / Blue weights: a custom mix instead of the mode.
                let mut custom = p.weights.is_some();
                if prop_row_reset(ui, "Custom weights", &mut custom, d.weights.is_some(), |ui, v| ui.checkbox(v, "").on_hover_text("Mix Red, Green and Blue with your own weights instead of the mode")).changed() {
                    p.weights = custom.then_some(subcraft_doc::GrayParts::LUMA_WEIGHTS);
                }
                if let Some(w) = p.weights.as_mut() {
                    for ((name, v), d) in ["Red", "Green", "Blue"].into_iter().zip(w.iter_mut()).zip(subcraft_doc::GrayParts::LUMA_WEIGHTS) {
                        slider_row_reset(ui, name, v, d, |v| egui::Slider::new(v, subcraft_doc::GrayParts::WEIGHT));
                    }
                }
                prop_row_reset(ui, "Invert", &mut p.invert, d.invert, |ui, v| ui.checkbox(v, ""));
                slider_row_reset(ui, "Balance", &mut p.balance, d.balance, |v| egui::Slider::new(v, 0.0..=1.0));
                slider_row_reset(ui, "Contrast", &mut p.contrast, d.contrast, |v| egui::Slider::new(v, 0.0..=1.0));
                if p != before { *filter = Filter::grayscale(p); changed = true; }
            }
            ui.weak("Color to gray, then Invert, Balance and Contrast / color channels only");
        }
        (Filter::Invert, _) => { ui.weak("1 - value / signed Height flips its sign"); }
        (Filter::Posterize { levels }, Filter::Posterize { levels: d_levels }) => {
            changed |= slider_row_reset(ui, "Levels", levels, *d_levels, |v| egui::Slider::new(v, 2..=subcraft_doc::MAX_POSTERIZE_LEVELS)).changed();
        }
        (Filter::HistogramShift { shift }, Filter::HistogramShift { shift: d_shift }) | (Filter::HistogramShiftWrap { shift }, Filter::HistogramShiftWrap { shift: d_shift }) => {
            changed |= slider_row_reset(ui, "Shift", shift, *d_shift, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
        }
        (Filter::ColorBalance { shadows, midtones, highlights }, Filter::ColorBalance { shadows: d_shadows, midtones: d_midtones, highlights: d_highlights }) => {
            for (name, range, d_range) in [("Shadows", shadows, d_shadows), ("Midtones", midtones, d_midtones), ("Highlights", highlights, d_highlights)] {
                for ((axis, v), d) in ["R", "G", "B"].into_iter().zip(range.iter_mut()).zip(d_range) {
                    changed |= slider_row_reset(ui, &format!("{name} {axis}"), v, *d, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
                }
            }
            ui.weak("Offsets per tonal range (linear RGB)");
        }
        (Filter::ReplaceColor { from, to, tolerance, softness }, Filter::ReplaceColor { from: d_from, to: d_to, tolerance: d_tolerance, softness: d_softness }) => {
            for (name, c, d) in [("From", from, d_from), ("To", to, d_to)] {
                changed |= color_row(ui, name, c, *d);
            }
            changed |= slider_row_reset(ui, "Tolerance", tolerance, *d_tolerance, |v| egui::Slider::new(v, 0.0..=2.0)).changed();
            changed |= slider_row_reset(ui, "Softness", softness, *d_softness, |v| egui::Slider::new(v, 0.0..=2.0)).changed();
        }
        (Filter::NormalInvert { flip_x, flip_y, flip_z }, Filter::NormalInvert { flip_x: d_flip_x, flip_y: d_flip_y, flip_z: d_flip_z }) => {
            changed |= prop_row_reset(ui, "Flip X (red)", flip_x, *d_flip_x, |ui, v| ui.checkbox(v, "")).changed();
            changed |= prop_row_reset(ui, "Flip Y (green)", flip_y, *d_flip_y, |ui, v| ui.checkbox(v, "")).changed();
            changed |= prop_row_reset(ui, "Flip Z (blue)", flip_z, *d_flip_z, |ui, v| ui.checkbox(v, "")).changed();
            ui.weak("Flip Y converts OpenGL and DirectX normal maps");
        }
        (Filter::EdgeDetect { radius, strength }, Filter::EdgeDetect { radius: d_radius, strength: d_strength }) => {
            changed |= slider_row_reset(ui, "Width", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Strength", strength, *d_strength, |v| egui::Slider::new(v, 0.0..=8.0)).changed();
            ui.weak("Sobel edges of the brightness / grayscale result");
        }
        (Filter::GradientDynamic { vertical, position, .. }, Filter::GradientDynamic { vertical: d_vertical, position: d_position, .. }) => {
            changed |= prop_row_reset(ui, "Gradient Orientation", vertical, *d_vertical, |ui, vertical| ui.selectable_value(vertical, false, "Horizontal") | ui.selectable_value(vertical, true, "Vertical")).changed();
            changed |= slider_row_reset(ui, "Gradient Input Position", position, *d_position, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            ui.weak("Values pick colours along one row (or column) of the Gradient Source image");
        }
        (Filter::HeightAdjust { invert, offset, multiply }, Filter::HeightAdjust { invert: d_invert, offset: d_offset, multiply: d_multiply }) => {
            changed |= prop_row_reset(ui, "Invert", invert, *d_invert, |ui, v| ui.checkbox(v, "")).changed();
            changed |= slider_row_reset(ui, "Offset", offset, *d_offset, |v| egui::Slider::new(v, -1.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "Multiply", multiply, *d_multiply, |v| egui::Slider::new(v, 0.0..=4.0)).changed();
            ui.weak("Invert, then value x Multiply + Offset");
        }
        (Filter::GradientCurve { preset, invert, balance, contrast }, Filter::GradientCurve { preset: d_preset, invert: d_invert, balance: d_balance, contrast: d_contrast }) => {
            changed |= prop_row_reset(ui, "Preset", preset, *d_preset, |ui, preset| {
                egui::ComboBox::from_id_salt("gradient_curve_preset").selected_text(preset.label()).show_ui(ui, |ui| {
                    for p in subcraft_doc::CurvePreset::ALL { changed |= ui.selectable_value(preset, p, p.label()).changed(); }
                }).response
            }).changed();
            changed |= prop_row_reset(ui, "Invert", invert, *d_invert, |ui, v| ui.checkbox(v, "")).changed();
            changed |= slider_row_reset(ui, "Balance", balance, *d_balance, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "Contrast", contrast, *d_contrast, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            ui.weak("Sends each value through the curve, then Balance and Contrast");
        }
        (Filter::BevelSmooth { radius, direction, smoothing, curve_offset, curve_shape, threshold }, Filter::BevelSmooth { radius: d_radius, direction: d_direction, smoothing: d_smoothing, curve_offset: d_curve_offset, curve_shape: d_curve_shape, threshold: d_threshold }) => {
            changed |= prop_row_reset(ui, "Direction", direction, *d_direction, |ui, direction| {
                egui::ComboBox::from_id_salt("bevel_smooth_direction").selected_text(direction.label()).show_ui(ui, |ui| {
                    for d in subcraft_doc::BevelDirection::ALL { changed |= ui.selectable_value(direction, d, d.label()).changed(); }
                }).response
            }).changed();
            changed |= slider_row_reset(ui, "Distance", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Smoothing", smoothing, *d_smoothing, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "Curve Offset", curve_offset, *d_curve_offset, |v| egui::Slider::new(v, -0.9..=0.9)).on_hover_text("Above 0 the ramp starts further from the contour; below 0 it ends sooner").changed();
            changed |= slider_row_reset(ui, "Curve Shape", curve_shape, *d_curve_shape, |v| egui::Slider::new(v, -1.0..=1.0)).on_hover_text("Below 0 pinched, above 0 rounded").changed();
            changed |= slider_row_reset(ui, "Mask Threshold", threshold, *d_threshold, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text("Values at or above this are the shape").changed();
            if channel.is_some() { ui.weak("On layer content the ramp shows where there is paint: use Inward, or put the filter on a mask"); }
        }
        (Filter::DirectionalDistance { distance, angle_deg, contrast }, Filter::DirectionalDistance { distance: d_distance, angle_deg: d_angle_deg, contrast: d_contrast }) => {
            changed |= slider_row_reset(ui, "Distance", distance, *d_distance, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Angle", angle_deg, *d_angle_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("\u{00b0}")).changed();
            changed |= slider_row_reset(ui, "Contrast", contrast, *d_contrast, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            ui.weak("Spreads bright values along the angle, fading with the distance");
        }
        (Filter::Fxaa { alpha }, Filter::Fxaa { alpha: d_alpha }) => {
            changed |= prop_row_reset(ui, "Affect Alpha Channel", alpha, *d_alpha, |ui, v| ui.checkbox(v, "").on_hover_text("Also smooth the edges of the paint coverage")).changed();
            ui.weak("Smooths stair-stepped edges along their direction");
        }
        (Filter::DropShadow { angle_deg, distance, size, spread, opacity, color }, Filter::DropShadow { angle_deg: d_angle_deg, distance: d_distance, size: d_size, spread: d_spread, opacity: d_opacity, color: d_color }) => {
            changed |= slider_row_reset(ui, "Angle", angle_deg, *d_angle_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("\u{00b0}")).changed();
            changed |= slider_row_reset(ui, "Distance", distance, *d_distance, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Size", size, *d_size, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Spread", spread, *d_spread, |v| egui::Slider::new(v, 0.0..=0.99)).changed();
            changed |= slider_row_reset(ui, "Opacity", opacity, *d_opacity, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= color_row(ui, "Color", color, *d_color);
            ui.weak("Shadow of the paint coverage, under the content");
        }
        (Filter::OuterGlow { width, spread, opacity, color, precise }, Filter::OuterGlow { width: d_width, spread: d_spread, opacity: d_opacity, color: d_color, precise: d_precise }) => {
            changed |= prop_row_reset(ui, "Mode", precise, *d_precise, |ui, precise| ui.selectable_value(precise, false, "Soft") | ui.selectable_value(precise, true, "Precise")).changed();
            changed |= slider_row_reset(ui, "Width", width, *d_width, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Spread", spread, *d_spread, |v| egui::Slider::new(v, 0.0..=0.99)).changed();
            changed |= slider_row_reset(ui, "Opacity", opacity, *d_opacity, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= color_row(ui, "Color", color, *d_color);
            ui.weak(if channel.is_some_and(|c| c.is_color()) { "Glow around the paint coverage, under the content" } else { "Glow around bright values (masks and gray channels use the color's brightness)" });
        }
        (Filter::BevelShape { radius, .. }, Filter::BevelShape { radius: d_radius, .. }) | (Filter::MaskOutlineShape { radius, .. }, Filter::MaskOutlineShape { radius: d_radius, .. }) => {
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
        }
        (Filter::Glow { radius, amount, threshold }, Filter::Glow { radius: d_radius, amount: d_amount, threshold: d_threshold }) => {
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Amount", amount, *d_amount, |v| egui::Slider::new(v, 0.0..=4.0)).changed();
            changed |= slider_row_reset(ui, "Threshold", threshold, *d_threshold, |v| egui::Slider::new(v, 0.0..=0.99)).changed();
        }
        (Filter::DirectionalWarp { intensity, angle_deg }, Filter::DirectionalWarp { intensity: d_intensity, angle_deg: d_angle_deg }) => {
            changed |= slider_row_reset(ui, "Intensity", intensity, *d_intensity, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Angle", angle_deg, *d_angle_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("\u{00b0}")).changed();
            ui.weak("Pushes pixels along the angle by their own brightness");
        }
        (Filter::Mosaic { size }, Filter::Mosaic { size: d_size }) => {
            changed |= slider_row_reset(ui, "Block size", size, *d_size, |v| egui::Slider::new(v, 1..=subcraft_doc::MAX_MOSAIC_SIZE).suffix(" px")).changed();
        }
        (Filter::Blur { radius }, Filter::Blur { radius: d_radius }) | (Filter::BlurWrap { radius }, Filter::BlurWrap { radius: d_radius }) | (Filter::Sharpen { radius, .. }, Filter::Sharpen { radius: d_radius, .. }) | (Filter::DirectionalBlur { radius, .. }, Filter::DirectionalBlur { radius: d_radius, .. }) | (Filter::Highpass { radius, .. }, Filter::Highpass { radius: d_radius, .. }) | (Filter::Bevel { radius, .. }, Filter::Bevel { radius: d_radius, .. }) | (Filter::MaskOutline { radius, .. }, Filter::MaskOutline { radius: d_radius, .. }) => {
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
        }
        (Filter::GradientMap { gradient, to_gray }, Filter::GradientMap { to_gray: d_to_gray, .. }) => {
            changed |= gradient_editor(ui, gradient);
            changed |= prop_row_reset(ui, "Output", to_gray, *d_to_gray, |ui, to_gray| ui.selectable_value(to_gray, false, "Color") | ui.selectable_value(to_gray, true, "Gray")).changed();
            ui.weak(if channel.is_some_and(|c| c.is_color()) { "Looked up by luminance / Gray keeps only brightness" } else { "Looked up by value / result is its brightness" });
        }
        (Filter::Median { radius }, Filter::Median { radius: d_radius }) => {
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_MEDIAN_RADIUS).integer().suffix(" px")).changed();
            ui.weak("Exact median over the square window / removes speckle, keeps edges");
        }
        (Filter::Quantize { colors, contour_smoothing, dithering, pattern, lab, apply_to_alpha, alpha_threshold }, Filter::Quantize { colors: d_colors, contour_smoothing: d_contour_smoothing, dithering: d_dithering, pattern: d_pattern, lab: d_lab, apply_to_alpha: d_apply_to_alpha, alpha_threshold: d_alpha_threshold }) => {
            changed |= slider_row_reset(ui, "Color Amount", colors, *d_colors, |v| egui::Slider::new(v, 2..=subcraft_doc::MAX_QUANTIZE_COLORS)).changed();
            changed |= slider_row_reset(ui, "Contour Smoothing", contour_smoothing, *d_contour_smoothing, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_QUANTIZE_SMOOTHING).suffix(" px")).on_hover_text("Blur before quantizing: smoother region borders").changed();
            changed |= slider_row_reset(ui, "Dithering", dithering, *d_dithering, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            if *dithering > 0.0 {
                changed |= prop_row_reset(ui, "Dithering Pattern", pattern, *d_pattern, |ui, pattern| {
                    egui::ComboBox::from_id_salt("quantize_dither").selected_text(pattern.label()).show_ui(ui, |ui| {
                        for p in subcraft_doc::DitherPattern::ALL { changed |= ui.selectable_value(pattern, p, p.label()).changed(); }
                    }).response
                }).changed();
            }
            changed |= prop_row_reset(ui, "Distance Color Space", lab, *d_lab, |ui, lab| ui.selectable_value(lab, true, "Lab (Color)") | ui.selectable_value(lab, false, "RGB (Data)")).changed();
            changed |= prop_row_reset(ui, "Apply To Alpha", apply_to_alpha, *d_apply_to_alpha, |ui, v| ui.checkbox(v, "")).changed();
            if *apply_to_alpha {
                changed |= slider_row_reset(ui, "Alpha Threshold", alpha_threshold, *d_alpha_threshold, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            }
            ui.weak("The palette is picked from the whole texture / data channels always compare values");
        }
        (Filter::AnisotropicKuwahara { radius, smoothness, sharpness, tensor_smoothness, anisotropy, angle, extract_direction }, Filter::AnisotropicKuwahara { radius: d_radius, smoothness: d_smoothness, sharpness: d_sharpness, tensor_smoothness: d_tensor_smoothness, anisotropy: d_anisotropy, angle: d_angle, extract_direction: d_extract_direction }) => {
            changed |= prop_row_reset(ui, "Extract Direction", extract_direction, *d_extract_direction, |ui, extract_direction| ui.selectable_value(extract_direction, true, "From the image") | ui.selectable_value(extract_direction, false, "Fixed angle")).changed();
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_KUWAHARA_RADIUS).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Smoothness", smoothness, *d_smoothness, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text("Wider, softer sectors").changed();
            changed |= slider_row_reset(ui, "Sharpness", sharpness, *d_sharpness, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_KUWAHARA_SHARPNESS)).on_hover_text("How strongly the calmest sectors win: higher keeps edges crisper").changed();
            if *extract_direction {
                changed |= slider_row_reset(ui, "Tensor Smoothness", tensor_smoothness, *d_tensor_smoothness, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_KUWAHARA_TENSOR).suffix(" px")).on_hover_text("How far the edge direction is averaged").changed();
            }
            changed |= slider_row_reset(ui, "Anisotropy", anisotropy, *d_anisotropy, |v| egui::Slider::new(v, 0.0..=subcraft_doc::MAX_KUWAHARA_ANISOTROPY)).on_hover_text("How much the window stretches along edges (0: round)").changed();
            if !*extract_direction {
                changed |= slider_row_reset(ui, "Anisotropy Angle", angle, *d_angle, |v| egui::Slider::new(v, 0.0..=1.0).suffix(" turn")).changed();
            }
            ui.weak("Painterly smoothing that keeps edges: each texel takes the calmest part of its window");
        }
        (Filter::BlurSlope { intensity, scale, seed, samples }, Filter::BlurSlope { intensity: d_intensity, scale: d_scale, seed: d_seed, samples: d_samples }) => {
            changed |= slider_row_reset(ui, "Intensity", intensity, *d_intensity, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Source Tiling", scale, *d_scale, |v| egui::Slider::new(v, 1..=subcraft_doc::MAX_WARP_SCALE)).on_hover_text("Noise cells across the texture").changed();
            changed |= seed_row(ui, seed, *d_seed);
            changed |= slider_row_reset(ui, "Quality", samples, *d_samples, |v| egui::Slider::new(v, 2..=subcraft_doc::MAX_SLOPE_SAMPLES)).on_hover_text("Reads per texel along the slope").changed();
            ui.weak("Blurs each texel along the direction a noise gives it");
        }
        (Filter::BlurSlopeMode { intensity, scale, seed, samples, source, .. }, Filter::BlurSlopeMode { intensity: d_intensity, scale: d_scale, seed: d_seed, samples: d_samples, .. }) => {
            changed |= slider_row_reset(ui, "Intensity", intensity, *d_intensity, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            if *source == subcraft_doc::SlopeSource::Noise {
                changed |= slider_row_reset(ui, "Source Tiling", scale, *d_scale, |v| egui::Slider::new(v, 1..=subcraft_doc::MAX_WARP_SCALE)).on_hover_text("Noise cells across the texture").changed();
                changed |= seed_row(ui, seed, *d_seed);
            }
            changed |= slider_row_reset(ui, "Quality", samples, *d_samples, |v| egui::Slider::new(v, 2..=subcraft_doc::MAX_SLOPE_SAMPLES)).on_hover_text("Reads per texel along the slope").changed();
            ui.weak(if *source == subcraft_doc::SlopeSource::Noise { "Blurs each texel along the direction a noise gives it" } else { "Blurs each texel across the edges of the image itself; flat areas stay as they are" });
        }
        (Filter::NonUniformBlur { radius, map, scale, seed, samples }, Filter::NonUniformBlur { radius: d_radius, map: d_map, scale: d_scale, seed: d_seed, samples: d_samples }) => {
            changed |= slider_row_reset(ui, "Radius", radius, *d_radius, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).on_hover_text("Blur radius where the map is at its highest").changed();
            changed |= prop_row_reset(ui, "Blur", map, *d_map, |ui, map| {
                egui::ComboBox::from_id_salt("non_uniform_blur_map").selected_text(map.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::BlurMap::ALL { ui.selectable_value(map, m, m.label()); }
                }).response.on_hover_text("Which areas get blurred")
            }).changed();
            if *map == subcraft_doc::BlurMap::Noise {
                changed |= slider_row_reset(ui, "Noise Tiling", scale, *d_scale, |v| egui::Slider::new(v, 1..=subcraft_doc::MAX_WARP_SCALE)).on_hover_text("Noise cells across the texture").changed();
                changed |= seed_row(ui, seed, *d_seed);
            }
            changed |= slider_row_reset(ui, "Quality", samples, *d_samples, |v| egui::Slider::new(v, 4..=subcraft_doc::MAX_BLUR_SAMPLES)).on_hover_text("Reads per texel; raise it if a large radius looks grainy").changed();
            ui.weak("A blur whose strength changes across the image");
        }
        (Filter::ColorMatch { source_mode, source, target, hue, chroma, luma }, Filter::ColorMatch { source_mode: d_mode, source: d_source, target: d_target, hue: d_hue, chroma: d_chroma, luma: d_luma }) => {
            changed |= prop_row_reset(ui, "Source Color Mode", source_mode, *d_mode, |ui, mode| {
                egui::ComboBox::from_id_salt("color_match_source").selected_text(mode.label()).show_ui(ui, |ui| {
                    for m in subcraft_doc::MatchSource::ALL { ui.selectable_value(mode, m, m.label()); }
                }).response.on_hover_text("Average: the mean colour of what is below the filter. Parameter: the colour picked here")
            }).changed();
            if *source_mode == subcraft_doc::MatchSource::Parameter {
                changed |= color_row(ui, "Source Color", source, *d_source);
            }
            changed |= color_row(ui, "Target Color", target, *d_target);
            ui.label(egui::RichText::new("Color variation").strong());
            for (name, v, d, tip) in [
                ("Hue", hue, d_hue, "How far in hue from the source a colour may be and still move (1: any)"),
                ("Chroma", chroma, d_chroma, "How far in saturation from the source a colour may be and still move (1: any)"),
                ("Luma", luma, d_luma, "How far in lightness from the source a colour may be and still move (1: any)"),
            ] {
                changed |= slider_row_reset(ui, name, v, *d, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text(tip).changed();
            }
            ui.weak("Moves the source colour to the target and keeps the variation around it");
        }
        (Filter::Warp { intensity, scale, seed }, Filter::Warp { intensity: d_intensity, scale: d_scale, seed: d_seed }) => {
            changed |= slider_row_reset(ui, "Intensity", intensity, *d_intensity, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).changed();
            changed |= slider_row_reset(ui, "Scale", scale, *d_scale, |v| egui::Slider::new(v, 1..=subcraft_doc::MAX_WARP_SCALE)).changed();
            changed |= seed_row(ui, seed, *d_seed);
            ui.weak("Noise-driven displacement / Scale = cells across the texture");
        }
        (Filter::HistogramScan { position, contrast, invert }, Filter::HistogramScan { position: d_position, contrast: d_contrast, invert: d_invert }) => {
            changed |= slider_row_reset(ui, "Position", position, *d_position, |v| egui::Slider::new(v, lower..=1.0)).changed();
            changed |= slider_row_reset(ui, "Contrast", contrast, *d_contrast, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= prop_row_reset(ui, "Invert", invert, *d_invert, |ui, v| ui.checkbox(v, "")).changed();
            ui.weak("Soft threshold / contrast 1 is a hard edge");
        }
        (Filter::HistogramRange { position, range, softness, invert }, Filter::HistogramRange { position: d_position, range: d_range, softness: d_softness, invert: d_invert }) => {
            changed |= slider_row_reset(ui, "Position", position, *d_position, |v| egui::Slider::new(v, lower..=1.0)).changed();
            changed |= slider_row_reset(ui, "Range", range, *d_range, |v| egui::Slider::new(v, 0.0..=if lower < 0.0 { 2.0 } else { 1.0 })).changed();
            changed |= slider_row_reset(ui, "Softness", softness, *d_softness, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= prop_row_reset(ui, "Invert", invert, *d_invert, |ui, v| ui.checkbox(v, "")).changed();
            ui.weak("Selects values near Position / outside the band is 0");
        }
        (Filter::Clamp { low, high }, Filter::Clamp { low: d_low, high: d_high }) => {
            let lower = lower.min(*low).min(*high); let upper = 1.0f32.max(*low).max(*high);
            changed |= slider_row_reset(ui, "Minimum", low, *d_low, |v| egui::Slider::new(v, lower..=*high)).changed();
            changed |= slider_row_reset(ui, "Maximum", high, *d_high, |v| egui::Slider::new(v, *low..=upper)).changed();
        }
        _ => {}
    }
    // Mode / wrap rows: these swap between two variants of the same filter (see `Filter::label`).
    // Their defaults are the plain variant's (GreaterOrEqual, Regular, no wrap).
    if let Some((cutoff, mut mode)) = filter.threshold_parts() {
        let before = mode;
        prop_row_reset(ui, "Mode", &mut mode, subcraft_doc::ThresholdMode::GreaterOrEqual, |ui, mode| {
            egui::ComboBox::from_id_salt("threshold_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::ThresholdMode::ALL { ui.selectable_value(mode, m, m.label()); }
            }).response
        });
        if mode != before { *filter = Filter::threshold(cutoff, mode); changed = true; }
    }
    if let Some(mut parts) = filter.slope_parts() {
        let before = parts;
        prop_row_reset(ui, "Source Type", &mut parts.source, subcraft_doc::SlopeSource::Noise, |ui, source| {
            egui::ComboBox::from_id_salt("blur_slope_source").selected_text(source.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::SlopeSource::ALL { ui.selectable_value(source, m, m.label()); }
            }).response.on_hover_text("Where the slope comes from: a noise, or the image being filtered")
        });
        prop_row_reset(ui, "Blending Mode", &mut parts.mode, subcraft_doc::SlopeMode::Blur, |ui, mode| {
            egui::ComboBox::from_id_salt("blur_slope_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::SlopeMode::ALL { ui.selectable_value(mode, m, m.label()); }
            }).response.on_hover_text("Blur averages along the slope. Min lets dark areas eat into bright ones, Max the opposite")
        });
        if parts != before { *filter = Filter::blur_slope(parts); changed = true; }
    }
    if let Some((mut mode, [hue, saturation, lightness])) = filter.hsl_parts() {
        let before = mode;
        prop_row_reset(ui, "Mode", &mut mode, subcraft_doc::HslMode::Regular, |ui, mode| {
            egui::ComboBox::from_id_salt("hsl_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in subcraft_doc::HslMode::ALL { ui.selectable_value(mode, m, m.label()); }
            }).response
        });
        if mode != before { *filter = Filter::hsl(mode, hue, saturation, lightness); changed = true; }
    }
    if let Some((radius, mut wrap)) = filter.blur_parts()
        && prop_row_reset(ui, "Wrap", &mut wrap, false, |ui, v| ui.checkbox(v, "").on_hover_text("The blur continues across the texture borders, for tiling textures")).changed()
    {
        *filter = Filter::blur(radius, wrap);
        changed = true;
    }
    if let Some((shift, mut wrap)) = filter.histogram_shift_parts()
        && prop_row_reset(ui, "Wrap", &mut wrap, false, |ui, v| ui.checkbox(v, "").on_hover_text("Values pushed past one end of the range re-enter at the other")).changed()
    {
        *filter = Filter::histogram_shift(shift, wrap);
        changed = true;
    }
    if let (Filter::Sharpen { amount, .. }, Filter::Sharpen { amount: d, .. }) | (Filter::Highpass { amount, .. }, Filter::Highpass { amount: d, .. }) = (&mut *filter, &def) {
        changed |= slider_row_reset(ui, "Amount", amount, *d, |v| egui::Slider::new(v, 0.0..=4.0)).changed();
    }
    if let (Filter::Bevel { amount, .. }, Filter::Bevel { amount: d, .. }) | (Filter::BevelShape { amount, .. }, Filter::BevelShape { amount: d, .. }) = (&mut *filter, &def) {
        changed |= slider_row_reset(ui, "Amount", amount, *d, |v| egui::Slider::new(v, lower..=1.0)).changed();
    }
    if let (Filter::Bevel { threshold, .. }, Filter::Bevel { threshold: d, .. })
        | (Filter::MaskOutline { threshold, .. }, Filter::MaskOutline { threshold: d, .. })
        | (Filter::BevelShape { threshold, .. }, Filter::BevelShape { threshold: d, .. })
        | (Filter::MaskOutlineShape { threshold, .. }, Filter::MaskOutlineShape { threshold: d, .. }) = (&mut *filter, &def)
    {
        changed |= slider_row_reset(ui, "Threshold", threshold, *d, |v| egui::Slider::new(v, lower..=1.0)).changed();
    }
    // Bevel's Corner Type and Smoothing, Mask Outline's shape options: these swap to the
    // variant that stores them (see `Filter::bevel`, `Filter::mask_outline`). Their defaults are
    // the plain variant's.
    if let Some(mut p) = filter.bevel_parts() {
        let before = p;
        prop_row_reset(ui, "Corner Type", &mut p.corner, subcraft_doc::BevelCorner::Rounded, |ui, corner| {
            egui::ComboBox::from_id_salt("bevel_corner").selected_text(corner.label()).show_ui(ui, |ui| {
                for c in subcraft_doc::BevelCorner::ALL { ui.selectable_value(corner, c, c.label()); }
            }).response
        });
        slider_row_reset(ui, "Smoothing", &mut p.smoothing, 0.0, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text("Eases the ramp from straight to rounded");
        if p != before { *filter = Filter::bevel(p); changed = true; }
    }
    if let Some(mut o) = filter.outline_parts() {
        let before = o;
        prop_row_reset(ui, "Start From Black", &mut o.start_black, false, |ui, v| ui.checkbox(v, "").on_hover_text("The band ramps up from black at the contour"));
        slider_row_reset(ui, "Blur", &mut o.blur, 0.0, |v| egui::Slider::new(v, 0.0..=32.0).suffix(" px")).on_hover_text("Fades the band's outer edge over this width");
        slider_row_reset(ui, "Curve Shape", &mut o.curve, 0.0, |v| egui::Slider::new(v, -1.0..=1.0)).on_hover_text("Bends the band's falloff: above 0 sharper, below 0 softer");
        if o != before { *filter = Filter::mask_outline(o); changed = true; }
    }
    if let (Filter::MaskOutline { position, .. }, Filter::MaskOutline { position: d, .. }) | (Filter::MaskOutlineShape { position, .. }, Filter::MaskOutlineShape { position: d, .. }) = (&mut *filter, &def) {
        let name = |p: &OutlinePosition| match p { OutlinePosition::Inner => "Inner", OutlinePosition::Outer => "Outer", OutlinePosition::Both => "Both" };
        changed |= prop_row_reset(ui, "Position", position, *d, |ui, position| {
            egui::ComboBox::from_id_salt("outline_position").selected_text(name(position)).show_ui(ui, |ui| {
                for value in [OutlinePosition::Inner, OutlinePosition::Outer, OutlinePosition::Both] {
                    ui.selectable_value(position, value, name(&value));
                }
            }).response
        }).changed();
    }
    if filter.is_contour() && !matches!(filter, Filter::BevelSmooth { .. }) {
        ui.weak("Threshold defines the shape / radius sets border width.");
        if channel.is_some() {ui.weak("Grayscale result / paint coverage is preserved.");}
        if matches!(filter,Filter::Bevel {..}) {ui.weak("Inward relief ramp / use Height for raised detail.");}
    }
    if let (Filter::DirectionalBlur { angle_deg, .. }, Filter::DirectionalBlur { angle_deg: d, .. }) = (&mut *filter, &def) {
        changed |= slider_row_reset(ui, "Angle", angle_deg, *d, |v| egui::Slider::new(v, -180.0..=180.0).suffix("\u{00b0}")).changed();
        ui.weak("0 degrees horizontal / 90 vertical");
    }
    if matches!(filter,Filter::Highpass {..}) {ui.weak(if channel.is_some_and(|c|c.is_signed()) {"Detail only / neutral Height = 0"} else {"Detail only / neutral = 0.5"});}
    if filter.radius().is_some() && !filter.reads_whole_texture() { ui.weak("UV pixels · texture edges clamp"); }
    if matches!(filter, Filter::BlurWrap { .. }) { ui.weak("UV pixels · wraps around the texture"); }
    if filter.softens_coverage() && channel.is_some() { ui.weak("Blur also softens paint coverage."); }
    changed
}

/// A linear colour row with a swatch (edited in sRGB) that resets to `default`.
fn color_row(ui: &mut Ui, label: &str, color: &mut [f32; 3], default: [f32; 3]) -> bool {
    let mut changed = false;
    prop_row_reset(ui, label, color, default, |ui, c| {
        let mut srgb = linear3_to_srgb(*c);
        let r = crate::color_picker::swatch_srgb(ui, &mut srgb);
        if r.changed() {
            *c = srgb3_to_linear(srgb);
            changed = true;
        }
        r
    })
    .changed()
        || changed
}

/// `f` with its default parameters, in the same variant as `f`, so every field has its default beside it
/// for the reset gestures (see [`crate::reset_row`]). The defaults come from [`Filter::ALL`]. Variants that
/// only add options to a plain filter (wrap, comparison mode, perceptive HSL, the Bevel and Mask Outline shape
/// options) take the plain filter's values and keep their own options, which have rows of their own. A filter
/// without a listed default (Gradient Dynamic needs its image) is its own default: nothing shows as changed.
pub fn default_like(f: &Filter) -> Filter {
    let listed = |probe: &Filter| Filter::ALL.iter().find(|e| std::mem::discriminant(*e) == std::mem::discriminant(probe)).cloned();
    if let Some(d) = listed(f) {
        return d;
    }
    let plain = |probe: Filter| listed(&probe).unwrap_or(probe);
    match f {
        Filter::ThresholdCompare { mode, .. } => match plain(Filter::Threshold { cutoff: 0.5 }) {
            Filter::Threshold { cutoff } => Filter::ThresholdCompare { cutoff, mode: *mode },
            _ => *f,
        },
        Filter::BlurWrap { .. } => match plain(Filter::Blur { radius: 0.0 }) {
            Filter::Blur { radius } => Filter::BlurWrap { radius },
            _ => *f,
        },
        Filter::HistogramShiftWrap { .. } => match plain(Filter::HistogramShift { shift: 0.0 }) {
            Filter::HistogramShift { shift } => Filter::HistogramShiftWrap { shift },
            _ => *f,
        },
        Filter::HslPerceptive { .. } => match plain(Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 }) {
            Filter::Hsl { hue, saturation, lightness } => Filter::HslPerceptive { hue, saturation, lightness },
            _ => *f,
        },
        Filter::BlurSlopeMode { source, mode, .. } => match plain(Filter::BlurSlope { intensity: 0.0, scale: 1, seed: 0, samples: 2 }) {
            Filter::BlurSlope { intensity, scale, seed, samples } => Filter::BlurSlopeMode { intensity, scale, seed, samples, source: *source, mode: *mode },
            _ => *f,
        },
        Filter::BevelShape { corner, smoothing, .. } => match plain(Filter::Bevel { radius: 0.0, amount: 0.0, threshold: 0.0 }) {
            Filter::Bevel { radius, amount, threshold } => Filter::BevelShape { radius, amount, threshold, corner: *corner, smoothing: *smoothing },
            _ => *f,
        },
        Filter::MaskOutlineShape { start_black, blur, curve, .. } => match plain(Filter::MaskOutline { radius: 0.0, threshold: 0.0, position: OutlinePosition::Both }) {
            Filter::MaskOutline { radius, threshold, position: p } => Filter::MaskOutlineShape { radius, threshold, position: p, start_black: *start_black, blur: *blur, curve: *curve },
            _ => *f,
        },
        _ => *f,
    }
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
                if crate::color_picker::swatch_srgb(ui, &mut rgb).changed() { stop.color = srgb3_to_linear(rgb); changed = true; }
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
        // Random seeds stay in range and move.
        for s in [0u32, 1, 9999, 4242] {
            let n = next_seed(s);
            assert!(n <= 9999 && n != s);
            assert_eq!(n, next_seed(s), "repeatable");
        }
        // Colour-only filters are not offered for a grey channel.
        let grey = search("", |f| f.supports_channel(ChannelKind::Roughness));
        assert!(!grey.iter().any(|f| f.label() == "Grayscale"));
    }
}
