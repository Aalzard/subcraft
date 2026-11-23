//! Tool Properties of the painting tools, in Painter's order: Brush, Alpha, Stencil, Material.

use egui::Ui;
use subcraft_app::brush_preset::PresetKind;
use subcraft_paint::{Alignment, PaintTarget};

use crate::state::{Tool, UiState};
use crate::widgets::{check_row_reset, hint, panel, prop_row, prop_row_reset, section, segmented, segmented_response, slider_row_reset};

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
                    let ds = crate::state::FILL_DEFAULTS;
                    prop_row_reset(ui, "Selection", &mut state.fill_selection, ds.selection, |ui, v| {
                        use subcraft_paint::PolygonSelectMode;
                        egui::ComboBox::from_id_salt("fill_selection").selected_text(v.label()).show_ui(ui, |ui| {
                            for m in PolygonSelectMode::ALL {
                                ui.selectable_value(v, m, m.label());
                            }
                        }).response
                    });
                    prop_row_reset(ui, "Fill by", &mut state.fill_mode, ds.mode, |ui, v| crate::brush_bar::fill_mode_combo(ui, v, "fill_mode_panel"));
                    // Greyed out when the fill lands in a mask (Painter): masks take the value as is.
                    let on_mask = !crate::brush_bar::blending_applies(state.session.polygon_fill_destination());
                    prop_row_reset(ui, "Blending", &mut state.session.brush.fill_blend, subcraft_doc::BlendMode::Normal, |ui, v| {
                        let current = *v;
                        ui.add_enabled_ui(!on_mask, |ui| {
                            egui::ComboBox::from_id_salt("fill_blend").selected_text(current.label()).show_ui(ui, |ui| {
                                for m in subcraft_doc::BlendMode::ALL.into_iter().filter(|m| !matches!(m, subcraft_doc::BlendMode::Passthrough | subcraft_doc::BlendMode::Disable)) {
                                    ui.selectable_value(v, m, m.label());
                                }
                            }).response
                        }).inner
                    }).on_hover_text(if on_mask { "Blending does not apply when the fill paints a mask" } else { "How the fill combines with what the layer already holds (the layer blending modes); Stroke opacity is its opacity" });
                    if !state.fill_selection.is_loop() {
                        prop_row_reset(ui, "Hit-test", &mut state.fill_hit_test, ds.hit_test, |ui, v| {
                            use subcraft_paint::PolygonHitTest;
                            ui.horizontal(|ui| {
                                for h in PolygonHitTest::ALL {
                                    let tip = match h {
                                        PolygonHitTest::Intersection => "A box fills every element it touches",
                                        PolygonHitTest::ContainedCenter => "A box fills only the elements whose centre is inside it",
                                    };
                                    ui.selectable_value(v, h, h.label()).on_hover_text(tip);
                                }
                            }).response
                        });
                    }
                    check_row_reset(ui, "", "Visible polygons only", &mut state.fill_visible_only, ds.visible_only)
                        .on_hover_text("A box drag in the 3D view fills only what the camera sees: no back faces, nothing behind the model. Off: everything under the box.");
                    match state.fill_selection {
                        subcraft_paint::PolygonSelectMode::Marquee => hint(ui, "Click to fill one element, drag a box to fill the elements it picks. Works in the 3D and 2D views. Hold Shift+A for a polygon loop instead."),
                        subcraft_paint::PolygonSelectMode::Loop => hint(ui, "Drag along the model to fill the loop of quads in that direction (a click: across the nearest edge). 3D view; the 2D view keeps the marquee."),
                        subcraft_paint::PolygonSelectMode::PartialLoop => hint(ui, "Drag from one quad to another to fill the loop between them; hold Alt when releasing to add as many on the other side. 3D view."),
                    }
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
                let d = subcraft_paint::BrushSettings::default();
                super::brush_rows::size_flow_opacity(ui, s, brush);
                slider_row_reset(ui, "Spacing", &mut s.spacing, d.spacing, |v| egui::Slider::new(v, 0.02..=2.0).logarithmic(true).clamping(egui::SliderClamping::Edits))
                    .on_hover_text("Distance between stamps, in brush diameters");
                slider_row_reset(ui, "Angle", &mut s.angle_deg, d.angle_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("°"));
                check_row_reset(ui, "", "Follow Path", &mut s.follow_path, d.follow_path);
                slider_row_reset(ui, "Alternate offset", &mut s.alternate_offset, d.alternate_offset, |v| egui::Slider::new(v, subcraft_paint::BrushSettings::ALTERNATE_OFFSET))
                    .on_hover_text("Every second stamp moves across the stroke by this fraction of the radius (stitches, zigzags)");
                super::brush_rows::jitters(ui, s);
                prop_row_reset(ui, "Alignment", &mut s.alignment, d.alignment, |ui, alignment| {
                    egui::ComboBox::from_id_salt("align")
                        .selected_text(match *alignment {
                            Alignment::TangentWrap => "Tangent | Wrap",
                            Alignment::TangentPlanar => "Tangent | Planar",
                            Alignment::Camera => "Camera",
                            Alignment::Uv => "UV",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(alignment, Alignment::Camera, "Camera");
                            ui.selectable_value(alignment, Alignment::TangentWrap, "Tangent | Wrap");
                            ui.selectable_value(alignment, Alignment::TangentPlanar, "Tangent | Planar");
                            ui.selectable_value(alignment, Alignment::Uv, "UV");
                        })
                        .response
                });
                check_row_reset(ui, "", "Backface Culling", &mut s.backface_culling, d.backface_culling);
                ui.add_enabled_ui(s.backface_culling, |ui| {
                    slider_row_reset(ui, "Backface Culling angle", &mut s.backface_angle_deg, d.backface_angle_deg, |v| egui::Slider::new(v, 10.0..=90.0).suffix("°"));
                });
                super::brush_rows::size_space(ui, s);
            });

            section(ui, "alpha", "Alpha", true, |ui| {
                super::brush_rows::alpha_slot(state, ui);
                let s = &mut state.session.brush.settings;
                let d = subcraft_paint::BrushSettings::default();
                let mut hardness = s.hardness * 100.0;
                if slider_row_reset(ui, "Hardness", &mut hardness, d.hardness * 100.0, |v| egui::Slider::new(v, 0.0..=100.0).max_decimals(0).suffix("%"))
                    .on_hover_text("Tip edge: 0 % fades from the centre, 100 % is a hard edge")
                    .changed()
                {
                    s.hardness = (hardness * 0.01).clamp(0.0, 1.0);
                }
                slider_row_reset(ui, "Roundness", &mut s.roundness, d.roundness, |v| egui::Slider::new(v, 0.05..=1.0));
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
                    changed |= slider_row_reset(ui, "Scale", &mut s.scale, subcraft_paint::Stencil::DEFAULT_SCALE, |v| egui::Slider::new(v, 0.05..=4.0).logarithmic(true)).changed();
                    let mut deg = s.rotation.to_degrees();
                    if slider_row_reset(ui, "Rotation", &mut deg, 0.0, |v| egui::Slider::new(v, -180.0..=180.0).suffix("°")).changed() {
                        s.rotation = deg.to_radians();
                        changed = true;
                    }
                    changed |= prop_row_reset(ui, "Tiling mode", &mut s.tiling, subcraft_paint::StencilTiling::None, |ui, v| {
                        let mut picked = false;
                        let mut r = egui::ComboBox::from_id_salt("stencil_tiling").selected_text(v.label()).show_ui(ui, |ui| {
                            for mode in subcraft_paint::StencilTiling::ALL {
                                picked |= ui.selectable_value(v, mode, mode.label()).changed();
                            }
                        }).response;
                        if picked {
                            r.mark_changed();
                        }
                        r
                    }).changed();
                    ui.add_enabled_ui(s.is_transformed(), |ui| {
                        if crate::widgets::quiet_button(ui, crate::icons::UNDO, "Reset stencil")
                            .on_hover_text("Back to the center, upright, at its default size. Also at the top right of the 3D view while S is held")
                            .clicked()
                        {
                            s.reset_transform();
                            changed = true;
                        }
                    });
                    let dd = crate::state::StencilDisplay::default();
                    let mut opacity = state.stencil_display.opacity * 100.0;
                    if slider_row_reset(ui, "Stencil opacity", &mut opacity, dd.opacity * 100.0, |v| egui::Slider::new(v, 0.0..=100.0).max_decimals(0).suffix("%"))
                        .on_hover_text("How strongly the stencil shows over the 3D view. Display only: it masks paint the same at any opacity")
                        .changed()
                    {
                        state.stencil_display.opacity = (opacity * 0.01).clamp(0.0, 1.0);
                    }
                    check_row_reset(ui, "", "Hide stencil when painting", &mut state.stencil_display.hide_when_painting, dd.hide_when_painting);
                    if changed {
                        state.session.brush.stencil = Some(std::sync::Arc::new(s));
                    }
                }
                hint(ui, "White paints, black blocks. In the 3D view hold S and drag: left = rotate (Shift snaps to 90°), middle = move, right = scale. Hold N to paint without it.");
            });

            section(ui, "material", "Material", true, |ui| {
                if state.session.brush.target.is_mask() {
                    let mut material = state.session.brush.material.clone();
                    let d = subcraft_paint::BrushMaterial::default().mask_value;
                    if slider_row_reset(ui, "Mask value", &mut material.mask_value, d, |v| egui::Slider::new(v, 0.0..=1.0)).changed() {
                        let _ = state.session.execute(subcraft_app::Command::SetBrushMaterial(material));
                    }
                } else if state.tool == Tool::Eraser {
                    eraser_channels(state, ui);
                } else { super::brush_material::show(state, ui); }
            });

            if state.tool == Tool::Clone {
                section(ui, "clone", "Clone", true, |ui| {
                    use subcraft_app::CloneSourceMode;
                    prop_row_reset(ui, "Source", &mut state.session.brush.clone_mode, CloneSourceMode::Relative, |ui, v| {
                        segmented_response(ui, v, &[(CloneSourceMode::Relative, "Relative"), (CloneSourceMode::Absolute, "Absolute")])
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
                let d = subcraft_paint::BrushSettings::default();
                slider_row_reset(ui, "Strength", &mut s.smudge_strength, d.smudge_strength, |v| egui::Slider::new(v, 0.0..=1.0));
                slider_row_reset(ui, "Smear length", &mut s.smudge_carry, d.smudge_carry, |v| egui::Slider::new(v, 0.0..=0.99));
                slider_row_reset(ui, "Falloff", &mut s.smudge_falloff, d.smudge_falloff, |v| egui::Slider::new(v, 0.25..=8.0).logarithmic(true));
                slider_row_reset(ui, "Material", &mut s.smudge_material, d.smudge_material, |v| egui::Slider::new(v, 0.0..=1.0));
                hint(ui, "Used by the Smudge tool. Low smear length blends softly; high drags paint far. Material mixes the brush paint into the smear.");
            });

            section(ui, "scatter", "Scatter", s_open(&b.settings), |ui| {
                let s = &mut b.settings;
                let d = subcraft_paint::BrushSettings::default();
                let mut count = s.scatter_count;
                slider_row_reset(ui, "Count", &mut count, d.scatter_count, |v| egui::Slider::new(v, 0..=subcraft_paint::MAX_SCATTER));
                s.scatter_count = count;
                ui.add_enabled_ui(count > 0, |ui| {
                    slider_row_reset(ui, "Radius", &mut s.scatter_radius, d.scatter_radius, |v| egui::Slider::new(v, 0.0..=4.0));
                    slider_row_reset(ui, "Size random", &mut s.scatter_size_random, d.scatter_size_random, |v| egui::Slider::new(v, 0.0..=1.0));
                    slider_row_reset(ui, "Rotation", &mut s.scatter_rotation_deg, d.scatter_rotation_deg, |v| egui::Slider::new(v, 0.0..=180.0).suffix("°"));
                    slider_row_reset(ui, "Opacity random", &mut s.scatter_opacity_random, d.scatter_opacity_random, |v| egui::Slider::new(v, 0.0..=1.0));
                    slider_row_reset(ui, "Fall", &mut s.scatter_fall, d.scatter_fall, |v| egui::Slider::new(v, 0.0..=8.0));
                });
                hint(ui, "0 = normal brush. Particles scatter around each stamp; Fall pulls them down the screen (drips, dust). Raise Spacing for sparse scatter.");
            });

            section(ui, "pen_tilt", "Pen tilt", b.settings.uses_pen_pose(), |ui| {
                let s = &mut b.settings;
                let d = subcraft_paint::BrushSettings::default();
                check_row_reset(ui, "", "Angle follows tilt", &mut s.tilt_angle, d.tilt_angle);
                check_row_reset(ui, "", "Angle follows pen rotation", &mut s.rotation_angle, d.rotation_angle);
                slider_row_reset(ui, "Tilt size", &mut s.tilt_size, d.tilt_size, |v| egui::Slider::new(v, 0.0..=4.0));
                slider_row_reset(ui, "Tilt flow", &mut s.tilt_flow, d.tilt_flow, |v| egui::Slider::new(v, 0.0..=1.0));
                slider_row_reset(ui, "Tilt flatten", &mut s.tilt_roundness, d.tilt_roundness, |v| egui::Slider::new(v, 0.0..=1.0));
                hint(ui, "For pens that report tilt (Wacom, Huion, XP-Pen, Surface). Leaning the pen widens the brush, lowers its flow and flattens it like the side of a pencil; Angle follows tilt turns it toward the lean. A mouse is never affected. Rotation needs a pen with barrel rotation (Art Pen).");
            });

            section(ui, "particles", "Particles", b.settings.particle_count > 0, |ui| {
                let s = &mut b.settings;
                let d = subcraft_paint::BrushSettings::default();
                let mut count = s.particle_count;
                slider_row_reset(ui, "Count", &mut count, d.particle_count, |v| egui::Slider::new(v, 0..=subcraft_paint::particles::MAX_PARTICLES));
                s.particle_count = count;
                ui.add_enabled_ui(count > 0, |ui| {
                    slider_row_reset(ui, "Gravity", &mut s.particle_gravity, d.particle_gravity, |v| egui::Slider::new(v, -1.0..=1.0));
                    slider_row_reset(ui, "Trail length", &mut s.particle_length, d.particle_length, |v| egui::Slider::new(v, 0.0..=24.0));
                    slider_row_reset(ui, "Particle size", &mut s.particle_size, d.particle_size, |v| egui::Slider::new(v, 0.02..=1.0));
                    slider_row_reset(ui, "End size", &mut s.particle_size_end, d.particle_size_end, |v| egui::Slider::new(v, 0.0..=1.0));
                    slider_row_reset(ui, "Wander", &mut s.particle_wander, d.particle_wander, |v| egui::Slider::new(v, 0.0..=1.0));
                    slider_row_reset(ui, "Spread", &mut s.particle_spread, d.particle_spread, |v| egui::Slider::new(v, 0.0..=1.0));
                });
                hint(ui, "0 = normal brush. Each stamp releases particles that run over the model and paint a trail: Gravity 1 runs straight down (rain, drips, leaks), negative rises (burn, soot). Trails follow curves, wrap over edges and drop off undersides. Lengths are in brush diameters.");
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
