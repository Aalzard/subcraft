//! Controls of a material filter (MatFinish / MatFX / PBR Validate). The caller dispatches the
//! one existing `SetContentEffect` command.
use egui::Ui;
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};
use subcraft_doc::{MAX_MATERIAL_FILTER_SCALE, MaterialFilter};

use crate::widgets::{check_row_reset, prop_row, prop_row_reset, slider_row_reset};

/// Filters whose name contains every word of `query` (case-insensitive), in list order.
pub fn search(query: &str) -> Vec<MaterialFilter> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    MaterialFilter::ALL.into_iter().filter(|f| words.iter().all(|w| f.label().to_lowercase().contains(w))).collect()
}

fn unit(ui: &mut Ui, label: &str, value: &mut f32, default: f32) -> bool {
    slider_row_reset(ui, label, value, default, |v| egui::Slider::new(v, 0.0..=1.0)).changed()
}

fn scale(ui: &mut Ui, label: &str, hover: &str, value: &mut f32, default: f32) -> bool {
    slider_row_reset(ui, label, value, default, |v| egui::Slider::new(v, 1.0..=MAX_MATERIAL_FILTER_SCALE).logarithmic(true)).on_hover_text(hover).changed()
}

fn seed(ui: &mut Ui, value: &mut u32, default: u32) -> bool {
    prop_row_reset(ui, "Seed", value, default, |ui, seed| {
        let mut r = ui.add(egui::DragValue::new(seed).speed(1.0));
        if ui.small_button("Random").on_hover_text("Pick a new random seed").clicked() {
            *seed = super::properties::random_seed();
            r.mark_changed();
        }
        r
    })
    .changed()
}

fn color(ui: &mut Ui, label: &str, value: &mut [f32; 3], default: [f32; 3]) -> bool {
    let mut changed = false;
    prop_row_reset(ui, label, value, default, |ui, c| {
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

/// The filter's rows; true when a parameter changed.
pub fn editor(ui: &mut Ui, filter: &mut MaterialFilter) -> bool {
    let mut changed = false;
    prop_row(ui, "Filter", |ui| {
        egui::ComboBox::from_id_salt("material_filter_kind").selected_text(filter.label()).show_ui(ui, |ui| {
            for choice in MaterialFilter::ALL {
                if ui.selectable_label(choice.label() == filter.label(), choice.label()).clicked() && choice.label() != filter.label() {
                    *filter = choice;
                    changed = true;
                }
            }
        });
    });
    ui.weak(filter.hint());
    let def = filter.default_like();
    const ACROSS: &str = "How many of them fit across the model's longest side";
    match (&mut *filter, def) {
        (
            MaterialFilter::Galvanized { scale: s, spangle, tint, roughness, roughness_variation, relief, seed: sd },
            MaterialFilter::Galvanized { scale: d_s, spangle: d_spangle, tint: d_tint, roughness: d_roughness, roughness_variation: d_rv, relief: d_relief, seed: d_sd },
        ) => {
            changed |= scale(ui, "Spangle count", ACROSS, s, d_s);
            changed |= unit(ui, "Spangle contrast", spangle, d_spangle);
            changed |= unit(ui, "Zinc tint", tint, d_tint);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= unit(ui, "Roughness variation", roughness_variation, d_rv);
            changed |= unit(ui, "Relief", relief, d_relief);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::Hammered { scale: s, depth, irregularity, roughness_variation, seed: sd },
            MaterialFilter::Hammered { scale: d_s, depth: d_depth, irregularity: d_irr, roughness_variation: d_rv, seed: d_sd },
        ) => {
            changed |= scale(ui, "Dimple count", ACROSS, s, d_s);
            changed |= unit(ui, "Depth", depth, d_depth);
            changed |= unit(ui, "Irregularity", irregularity, d_irr);
            changed |= unit(ui, "Roughness variation", roughness_variation, d_rv);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::PowderCoated { color: c, color_amount, roughness, orange_peel, grain, scale: s, seed: sd },
            MaterialFilter::PowderCoated { color: d_c, color_amount: d_ca, roughness: d_roughness, orange_peel: d_peel, grain: d_grain, scale: d_s, seed: d_sd },
        ) => {
            changed |= color(ui, "Coat color", c, d_c);
            changed |= unit(ui, "Color amount", color_amount, d_ca);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= unit(ui, "Orange peel", orange_peel, d_peel);
            changed |= unit(ui, "Grain", grain, d_grain);
            changed |= scale(ui, "Peel count", ACROSS, s, d_s);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::EdgeDamages { amount, spread, scale: s, depth, color: c, roughness, metallic, contrast, seed: sd },
            MaterialFilter::EdgeDamages { amount: d_amount, spread: d_spread, scale: d_s, depth: d_depth, color: d_c, roughness: d_roughness, metallic: d_metallic, contrast: d_contrast, seed: d_sd },
        ) => {
            changed |= unit(ui, "Amount", amount, d_amount);
            changed |= unit(ui, "Contrast", contrast, d_contrast);
            changed |= slider_row_reset(ui, "Spread", spread, d_spread, |v| egui::Slider::new(v, 0.0..=12.5).suffix("%")).on_hover_text("Reach from the edges, percent of the model's longest side").changed();
            changed |= scale(ui, "Chip count", ACROSS, s, d_s);
            changed |= unit(ui, "Depth", depth, d_depth);
            ui.label(egui::RichText::new("Exposed material").strong());
            changed |= color(ui, "Color", c, d_c);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= unit(ui, "Metallic", metallic, d_metallic);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::RustWeathering { amount, edges, cavities, drips, scale: s, relief, roughness, seed: sd },
            MaterialFilter::RustWeathering { amount: d_amount, edges: d_edges, cavities: d_cav, drips: d_drips, scale: d_s, relief: d_relief, roughness: d_roughness, seed: d_sd },
        ) => {
            changed |= unit(ui, "Amount", amount, d_amount);
            changed |= unit(ui, "Edges", edges, d_edges);
            changed |= unit(ui, "Cavities", cavities, d_cav);
            changed |= unit(ui, "Runs", drips, d_drips);
            changed |= scale(ui, "Patch count", ACROSS, s, d_s);
            changed |= unit(ui, "Relief", relief, d_relief);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::PeelingPaint { amount, scale: s, edge_bias, thickness, curl, color: c, roughness, metallic, rust, seed: sd },
            MaterialFilter::PeelingPaint { amount: d_amount, scale: d_s, edge_bias: d_edge, thickness: d_thick, curl: d_curl, color: d_c, roughness: d_roughness, metallic: d_metallic, rust: d_rust, seed: d_sd },
        ) => {
            changed |= unit(ui, "Amount", amount, d_amount);
            changed |= scale(ui, "Flake count", ACROSS, s, d_s);
            changed |= unit(ui, "Edge bias", edge_bias, d_edge);
            changed |= unit(ui, "Paint thickness", thickness, d_thick);
            changed |= unit(ui, "Curl", curl, d_curl);
            ui.label(egui::RichText::new("Exposed material").strong());
            changed |= color(ui, "Color", c, d_c);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= unit(ui, "Metallic", metallic, d_metallic);
            changed |= unit(ui, "Rust", rust, d_rust);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::PbrValidate { strict, flag_partial_metal, flat },
            MaterialFilter::PbrValidate { strict: d_strict, flag_partial_metal: d_partial, flat: d_flat },
        ) => {
            changed |= check_row_reset(ui, "", "Strict dark limit (50 sRGB)", strict, d_strict).changed();
            changed |= check_row_reset(ui, "", "Flag partial Metallic", flag_partial_metal, d_partial).changed();
            changed |= check_row_reset(ui, "", "Flat shading", flat, d_flat).on_hover_text("Sets Metallic 0 and Roughness 1 so reflections do not tint the result").changed();
            ui.weak("Dielectrics: 30 to 240 sRGB. Metals: 180 sRGB or more. Put it on a Passthrough layer above everything, and hide it before exporting.");
        }
        (MaterialFilter::Grinded { scale: s, intensity, seed: sd }, MaterialFilter::Grinded { scale: d_s, intensity: d_int, seed: d_sd }) => {
            changed |= scale(ui, "Pass count", ACROSS, s, d_s);
            changed |= unit(ui, "Grinding intensity", intensity, d_int);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::Rough { scale: s, grunge_scale, brushing, warp, grunge, seed: sd },
            MaterialFilter::Rough { scale: d_s, grunge_scale: d_gs, brushing: d_brushing, warp: d_warp, grunge: d_grunge, seed: d_sd },
        ) => {
            changed |= scale(ui, "Brush line count", ACROSS, s, d_s);
            changed |= unit(ui, "Brushing intensity", brushing, d_brushing);
            changed |= unit(ui, "Brushing warp", warp, d_warp);
            changed |= unit(ui, "Grunge amount", grunge, d_grunge);
            changed |= scale(ui, "Grunge count", ACROSS, grunge_scale, d_gs);
            changed |= seed(ui, sd, d_sd);
        }
        (MaterialFilter::Raw { scale: s, intensity, seed: sd }, MaterialFilter::Raw { scale: d_s, intensity: d_int, seed: d_sd }) => {
            changed |= scale(ui, "Patch count", ACROSS, s, d_s);
            changed |= unit(ui, "Details intensity", intensity, d_int);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::DetailEdgeWear { level, detail, contrast, grunge, grunge_scale, color: c, roughness, metallic, seed: sd },
            MaterialFilter::DetailEdgeWear { level: d_level, detail: d_detail, contrast: d_contrast, grunge: d_grunge, grunge_scale: d_gs, color: d_c, roughness: d_roughness, metallic: d_metallic, seed: d_sd },
        ) => {
            changed |= unit(ui, "Wear level", level, d_level);
            changed |= slider_row_reset(ui, "Painted detail", detail, d_detail, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text("How much the Height and Normal detail below is worn like an edge").changed();
            changed |= unit(ui, "Wear contrast", contrast, d_contrast);
            changed |= unit(ui, "Grunge amount", grunge, d_grunge);
            changed |= scale(ui, "Grunge count", ACROSS, grunge_scale, d_gs);
            ui.label(egui::RichText::new("Exposed material").strong());
            changed |= color(ui, "Color", c, d_c);
            changed |= unit(ui, "Roughness", roughness, d_roughness);
            changed |= unit(ui, "Metallic", metallic, d_metallic);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::WaterDrops { amount, scale: s, size_variation, direction, top, sides, underside, height, roughness, wet, seed: sd },
            MaterialFilter::WaterDrops { amount: d_amount, scale: d_s, size_variation: d_size, direction: d_dir, top: d_top, sides: d_sides, underside: d_under, height: d_height, roughness: d_roughness, wet: d_wet, seed: d_sd },
        ) => {
            changed |= unit(ui, "Drops amount", amount, d_amount);
            changed |= scale(ui, "Drop count", ACROSS, s, d_s);
            changed |= unit(ui, "Size variation", size_variation, d_size);
            changed |= unit(ui, "Direction intensity", direction, d_dir);
            ui.label(egui::RichText::new("Where the drops sit").strong());
            changed |= unit(ui, "Top", top, d_top);
            changed |= unit(ui, "Sides", sides, d_sides);
            changed |= unit(ui, "Underside", underside, d_under);
            ui.label(egui::RichText::new("Water").strong());
            changed |= unit(ui, "Drops height", height, d_height);
            changed |= unit(ui, "Drops roughness", roughness, d_roughness);
            changed |= unit(ui, "Wet darkening", wet, d_wet);
            changed |= seed(ui, sd, d_sd);
        }
        (
            MaterialFilter::Hbao { radius, intensity, ambient_occlusion, base_color, roughness },
            MaterialFilter::Hbao { radius: d_radius, intensity: d_int, ambient_occlusion: d_ao, base_color: d_base, roughness: d_roughness },
        ) => {
            changed |= unit(ui, "Radius", radius, d_radius);
            changed |= unit(ui, "Intensity", intensity, d_int);
            ui.label(egui::RichText::new("Blending").strong());
            changed |= unit(ui, "Ambient Occlusion intensity", ambient_occlusion, d_ao);
            changed |= unit(ui, "Base Color intensity", base_color, d_base);
            changed |= unit(ui, "Roughness intensity", roughness, d_roughness);
        }
        (
            MaterialFilter::ShutLine { width, slope, emboss, height, color: c, line_opacity, source_opacity },
            MaterialFilter::ShutLine { width: d_width, slope: d_slope, emboss: d_emboss, height: d_height, color: d_c, line_opacity: d_line, source_opacity: d_source },
        ) => {
            changed |= unit(ui, "Cavity width", width, d_width);
            changed |= unit(ui, "Slope intensity", slope, d_slope);
            changed |= unit(ui, "Emboss strength", emboss, d_emboss);
            changed |= unit(ui, "Height intensity", height, d_height);
            ui.label(egui::RichText::new("Material").strong());
            changed |= color(ui, "Line color", c, d_c);
            changed |= unit(ui, "Line opacity", line_opacity, d_line);
            changed |= slider_row_reset(ui, "Source material opacity", source_opacity, d_source, |v| egui::Slider::new(v, 0.0..=1.0)).on_hover_text("How much of the painted material itself stays visible. 0 keeps only the groove.").changed();
        }
        (
            MaterialFilter::BrushedLinear { scale: s, intensity, direction, seed: sd },
            MaterialFilter::BrushedLinear { scale: d_s, intensity: d_int, direction: d_dir, seed: d_sd },
        ) => {
            changed |= scale(ui, "Brush line count", ACROSS, s, d_s);
            changed |= unit(ui, "Brushing intensity", intensity, d_int);
            let mut degrees = *direction * 180.0;
            if slider_row_reset(ui, "Brushing direction", &mut degrees, d_dir * 180.0, |v| egui::Slider::new(v, 0.0..=180.0).suffix("\u{00b0}")).on_hover_text("0: the lines run level around the model. 90: they run up and down the walls.").changed() {
                *direction = (degrees / 180.0).clamp(0.0, 1.0);
                changed = true;
            }
            changed |= seed(ui, sd, d_sd);
        }
        (MaterialFilter::Grainy { scale: s, intensity, seed: sd }, MaterialFilter::Grainy { scale: d_s, intensity: d_int, seed: d_sd }) => {
            changed |= scale(ui, "Grain count", ACROSS, s, d_s);
            changed |= unit(ui, "Grain intensity", intensity, d_int);
            changed |= seed(ui, sd, d_sd);
        }
        (MaterialFilter::PerforatedCircles { scale: s, radius, depth }, MaterialFilter::PerforatedCircles { scale: d_s, radius: d_radius, depth: d_depth }) => {
            changed |= scale(ui, "Hole count", ACROSS, s, d_s);
            changed |= unit(ui, "Hole size", radius, d_radius);
            changed |= unit(ui, "Hole depth", depth, d_depth);
        }
        _ => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_filters_by_any_word() {
        assert_eq!(search("").len(), MaterialFilter::ALL.len());
        assert_eq!(search("rust").len(), 1);
        assert_eq!(search("matfinish").len(), 9);
        assert_eq!(search("perforated").len(), 1);
        assert_eq!(search("edge").len(), 2);
        assert_eq!(search("water").len(), 1);
        assert_eq!(search("MATFX peel").len(), 1);
        assert!(search("nothing like this").is_empty());
    }

    /// Every filter's rows draw, and drawing alone never reports a change.
    #[test]
    fn every_editor_draws_without_changing_the_filter() {
        for f in MaterialFilter::ALL {
            let mut edited = f;
            let mut changed = false;
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| changed = editor(ui, &mut edited));
            out.textures_delta.clear();
            assert!(!changed, "{}", f.label());
            assert_eq!(edited, f);
        }
    }
}
