//! Tool Properties of the painting tools, in Painter's order: Brush, Alpha, Stencil, Material.

use egui::{RichText, Ui};
use subcraft_app::brush_preset::PresetKind;
use subcraft_paint::{Alignment, PaintTarget, PolygonFillMode};

use crate::state::{Tool, UiState};
use crate::widgets::{hint, panel, prop_row, section, segmented, slider_row};

/// Id of the panel background that takes the right-click of the tool Properties.
pub fn properties_menu_id() -> egui::Id {
    egui::Id::new("tool-properties-menu")
}

/// Create a preset of `kind` from the current tool in the user library and list it in the Shelf.
/// The name typed in the Presets block is used when there is one; otherwise the brush's name.
pub fn create_preset(state: &mut UiState, kind: PresetKind, now: f64) {
    let Some(lib) = state.user_library.clone() else {
        state.set_status("Presets need a user library folder", now);
        return;
    };
    let typed = state.preset_name.trim().to_string();
    let name = if !typed.is_empty() {
        typed
    } else if kind == PresetKind::Material {
        "Material".to_string()
    } else {
        super::brush_header::title(state)
    };
    match state.session.create_preset(&lib, &name, kind, state.tool.preset_tool()) {
        Ok((p, tip)) => {
            if let Some(tip) = tip { state.library.alphas.push(tip); }
            state.set_status(format!("Created {} \"{}\": it is in Shelf › Brushes (right-click it to rename or update)", kind.label(), p.name), now);
            state.brush_presets.push(p);
            state.preset_name.clear();
        }
        Err(e) => state.set_status(format!("Create {}: {e}", kind.label()), now),
    }
}

fn create_label(kind: PresetKind) -> String {
    format!("Create {}", kind.label())
}

fn create_hint(kind: PresetKind) -> &'static str {
    match kind {
        PresetKind::Tool => "Everything: brush, alpha, stencil, material and the tool in use",
        PresetKind::Material => "The material only: channel values and images",
        PresetKind::Brush => "Brush parameters, alpha and stencil, without the material",
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    // Right-click on the panel (outside its controls, which are laid on top of this) opens the
    // preset menu, as in Painter's tool Properties.
    let now = ui.input(|i| i.time);
    let background = ui.interact(ui.max_rect(), properties_menu_id(), egui::Sense::click());
    let can_save = state.user_library.is_some();
    let mut create = None;
    background.context_menu(|ui| {
        for kind in PresetKind::ALL {
            if ui.add_enabled(can_save, egui::Button::new(create_label(kind))).on_hover_text(create_hint(kind)).clicked() {
                create = Some(kind);
                ui.close();
            }
        }
    });
    if let Some(kind) = create {
        create_preset(state, kind, now);
    }
    let mut area = egui::ScrollArea::vertical().id_salt("brush").auto_shrink([false, false]);
    // A Shelf click on a brush scrolls back to the selected brush's preview at the top.
    if std::mem::take(&mut state.brush_scroll_top) {
        area = area.vertical_scroll_offset(0.0);
    }
    area.show(ui, |ui| {
        panel(ui, |ui| {
            super::stamp_brush::show(state, ui);
            super::brush_header::show(state, ui);
            section(ui, "tool", "Paint target", true, |ui| {
                if state.tool == Tool::PolygonFill {
                    prop_row(ui, "Fill by", |ui| crate::brush_bar::fill_mode_combo(ui, &mut state.fill_mode, "fill_mode_panel"));
                    prop_row(ui, "", |ui| {
                        ui.checkbox(&mut state.fill_visible_only, "Visible polygons only")
                            .on_hover_text("A box drag in the 3D view fills only what the camera sees: no back faces, nothing behind the model. Off: everything under the box.");
                    });
                    hint(ui, "Click to fill one element, drag a box to fill every element it touches. Works in the 3D and 2D views.");
                }
                let b = &mut state.session.brush;
                prop_row(ui, "Paint on", |ui| {
                    segmented(ui, &mut b.target, &[(PaintTarget::Content, "Layer"), (PaintTarget::MaskBase, "Mask")]);
                });
            });

            // Painter's order: Brush, Alpha, Stencil, Material; SubCraft's own blocks follow.
            let brush = state.tool != Tool::PolygonFill;
            section(ui, "brush", "Brush", true, |ui| {
                let s = &mut state.session.brush.settings;
                super::brush_rows::size_flow_opacity(ui, s, brush);
                slider_row(ui, "Spacing", egui::Slider::new(&mut s.spacing, 0.02..=2.0).logarithmic(true).clamping(egui::SliderClamping::Edits))
                    .on_hover_text("Distance between stamps, in brush diameters");
                slider_row(ui, "Angle", egui::Slider::new(&mut s.angle_deg, -180.0..=180.0).suffix("°"));
                prop_row(ui, "", |ui| ui.checkbox(&mut s.follow_path, "Follow Path"));
                super::brush_rows::jitters(ui, s);
                prop_row(ui, "Alignment", |ui| {
                    egui::ComboBox::from_id_salt("align")
                        .selected_text(match s.alignment {
                            Alignment::TangentWrap => "Tangent | Wrap",
                            Alignment::TangentPlanar => "Tangent | Planar",
                            Alignment::Camera => "Camera",
                            Alignment::Uv => "UV",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.alignment, Alignment::Camera, "Camera");
                            ui.selectable_value(&mut s.alignment, Alignment::TangentWrap, "Tangent | Wrap");
                            ui.selectable_value(&mut s.alignment, Alignment::TangentPlanar, "Tangent | Planar");
                            ui.selectable_value(&mut s.alignment, Alignment::Uv, "UV");
                        });
                });
                prop_row(ui, "", |ui| ui.checkbox(&mut s.backface_culling, "Backface Culling"));
                ui.add_enabled_ui(s.backface_culling, |ui| {
                    slider_row(ui, "Backface Culling angle", egui::Slider::new(&mut s.backface_angle_deg, 10.0..=90.0).suffix("°"));
                });
                super::brush_rows::size_space(ui, s);
            });

            section(ui, "alpha", "Alpha", true, |ui| {
                super::brush_rows::alpha_slot(state, ui);
                let s = &mut state.session.brush.settings;
                let mut hardness = s.hardness * 100.0;
                if slider_row(ui, "Hardness", egui::Slider::new(&mut hardness, 0.0..=100.0).max_decimals(0).suffix("%"))
                    .on_hover_text("Tip edge: 0 % fades from the centre, 100 % is a hard edge")
                    .changed()
                {
                    s.hardness = (hardness * 0.01).clamp(0.0, 1.0);
                }
                slider_row(ui, "Roundness", egui::Slider::new(&mut s.roundness, 0.05..=1.0));
            });

            section(ui, "stencil", "Stencil", false, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(format!("{}  Choose image…", crate::icons::IMAGE)).clicked() {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::Stencil, crate::files::IMAGES, false, None);
                    }
                    if state.session.brush.stencil.is_some() && ui.button(format!("{}  Remove", crate::icons::X)).clicked() {
                        state.session.brush.stencil = None;
                    }
                });
                if let Some(st) = state.session.brush.stencil.clone() {
                    let mut s = (*st).clone();
                    let mut changed = false;
                    changed |= slider_row(ui, "Scale", egui::Slider::new(&mut s.scale, 0.05..=4.0).logarithmic(true)).changed();
                    let mut deg = s.rotation.to_degrees();
                    if slider_row(ui, "Rotation", egui::Slider::new(&mut deg, -180.0..=180.0).suffix("°")).changed() {
                        s.rotation = deg.to_radians();
                        changed = true;
                    }
                    prop_row(ui, "Tiling mode", |ui| {
                        egui::ComboBox::from_id_salt("stencil_tiling").selected_text(s.tiling.label()).show_ui(ui, |ui| {
                            for mode in subcraft_paint::StencilTiling::ALL {
                                changed |= ui.selectable_value(&mut s.tiling, mode, mode.label()).changed();
                            }
                        });
                    });
                    ui.add_enabled_ui(s.is_transformed(), |ui| {
                        if crate::widgets::quiet_button(ui, crate::icons::UNDO, "Reset stencil")
                            .on_hover_text("Back to the center, upright, at its default size. Also at the top right of the 3D view while S is held")
                            .clicked()
                        {
                            s.reset_transform();
                            changed = true;
                        }
                    });
                    let mut opacity = state.stencil_display.opacity * 100.0;
                    if slider_row(ui, "Stencil opacity", egui::Slider::new(&mut opacity, 0.0..=100.0).max_decimals(0).suffix("%"))
                        .on_hover_text("How strongly the stencil shows over the 3D view. Display only: it masks paint the same at any opacity")
                        .changed()
                    {
                        state.stencil_display.opacity = (opacity * 0.01).clamp(0.0, 1.0);
                    }
                    prop_row(ui, "", |ui| ui.checkbox(&mut state.stencil_display.hide_when_painting, "Hide stencil when painting"));
                    if changed {
                        state.session.brush.stencil = Some(std::sync::Arc::new(s));
                    }
                }
                hint(ui, "White paints, black blocks. In the 3D view hold S and drag: left = rotate (Shift snaps to 90°), middle = move, right = scale. Hold N to paint without it.");
            });

            section(ui, "material", "Material", true, |ui| {
                if state.session.brush.target.is_mask() {
                    let mut material = state.session.brush.material.clone();
                    if slider_row(ui, "Mask value", egui::Slider::new(&mut material.mask_value, 0.0..=1.0)).changed() {
                        let _ = state.session.execute(subcraft_app::Command::SetBrushMaterial(material));
                    }
                } else if state.tool == Tool::Eraser {
                    eraser_channels(state, ui);
                } else { super::brush_material::show(state, ui); }
            });

            if state.tool == Tool::Clone {
                section(ui, "clone", "Clone", true, |ui| {
                    use subcraft_app::CloneSourceMode;
                    prop_row(ui, "Source", |ui| {
                        segmented(ui, &mut state.session.brush.clone_mode, &[(CloneSourceMode::Relative, "Relative"), (CloneSourceMode::Absolute, "Absolute")]);
                    });
                    hint(ui, match state.session.brush.clone_mode {
                        CloneSourceMode::Relative => "Relative: the source follows the brush, so each stroke continues the copy. Set the source with V + click.",
                        CloneSourceMode::Absolute => "Absolute: every stroke starts copying from the source point again. Set the source with V + click.",
                    });
                });
            }

            section(ui, "presets", "Presets", false, |ui| {
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.preset_name).hint_text("Preset name").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 5.0)));
                    let can_save = state.user_library.is_some();
                    let now = ui.input(|i| i.time);
                    for kind in PresetKind::ALL {
                        if ui.add_enabled(can_save, egui::Button::new(format!("{}  {}", crate::icons::SAVE, create_label(kind)))).on_hover_text(create_hint(kind)).clicked() {
                            create_preset(state, kind, now);
                        }
                    }
                });
                hint(ui, "Also on right-click anywhere in this panel. Presets appear in Shelf › Brushes; right-click one there to update, rename, duplicate or favorite it.");
            });

            let now = ui.input(|i| i.time);
            crate::text_panel::show(state, ui, now);

            let b = &mut state.session.brush;
            section(ui, "smudge", "Smudge", false, |ui| {
                let s = &mut b.settings;
                slider_row(ui, "Strength", egui::Slider::new(&mut s.smudge_strength, 0.0..=1.0));
                slider_row(ui, "Smear length", egui::Slider::new(&mut s.smudge_carry, 0.0..=0.99));
                slider_row(ui, "Falloff", egui::Slider::new(&mut s.smudge_falloff, 0.25..=8.0).logarithmic(true));
                slider_row(ui, "Material", egui::Slider::new(&mut s.smudge_material, 0.0..=1.0));
                hint(ui, "Used by the Smudge tool. Low smear length blends softly; high drags paint far. Material mixes the brush paint into the smear.");
            });

            section(ui, "scatter", "Scatter", s_open(&b.settings), |ui| {
                let s = &mut b.settings;
                let mut count = s.scatter_count;
                slider_row(ui, "Count", egui::Slider::new(&mut count, 0..=subcraft_paint::MAX_SCATTER));
                s.scatter_count = count;
                ui.add_enabled_ui(count > 0, |ui| {
                    slider_row(ui, "Radius", egui::Slider::new(&mut s.scatter_radius, 0.0..=4.0));
                    slider_row(ui, "Size random", egui::Slider::new(&mut s.scatter_size_random, 0.0..=1.0));
                    slider_row(ui, "Rotation", egui::Slider::new(&mut s.scatter_rotation_deg, 0.0..=180.0).suffix("°"));
                    slider_row(ui, "Opacity random", egui::Slider::new(&mut s.scatter_opacity_random, 0.0..=1.0));
                    slider_row(ui, "Fall", egui::Slider::new(&mut s.scatter_fall, 0.0..=8.0));
                });
                hint(ui, "0 = normal brush. Particles scatter around each stamp; Fall pulls them down the screen (drips, dust). Raise Spacing for sparse scatter.");
            });


        });
    });
}

/// Eraser: the Material block is only the channel toggles; a stroke erases the channels that
/// are switched on and leaves the others as painted.
fn eraser_channels(state: &mut UiState, ui: &mut Ui) {
    let states: Vec<_> = state.session.channels().into_iter().map(|k| (k, state.session.brush.material.is_enabled(k))).collect();
    if let Some((channel, solo)) = super::properties::channel_toggles(ui, &states) {
        let _ = state.session.execute(subcraft_app::Command::ToggleBrushChannel { channel, solo });
    }
    hint(ui, "The eraser removes paint from the channels that are on. Alt+click a channel to erase only that one.");
}

fn s_open(s: &subcraft_paint::BrushSettings) -> bool {
    s.scatter_count > 0
}
