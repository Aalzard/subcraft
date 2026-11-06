//! Texture Set Settings: the selected texture set's material channels (format, show alone,
//! remove, "+ Add channel"), then its baked mesh maps with thumbnails and the Bake button, then
//! texture filtering and the mesh. Every document change is one undo step.
use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::ChannelFormat;
use subcraft_image::TiledImage;
use subcraft_render::ViewMode;

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{caption, icon_button};

const FORMATS: [ChannelFormat; 4] = [ChannelFormat::Bit8, ChannelFormat::Bit16, ChannelFormat::Float16, ChannelFormat::Float32];
const THUMB: u32 = 48;

/// The 3D view mode that shows `kind` on its own, if there is one.
fn view_for(kind: ChannelKind) -> Option<ViewMode> {
    Some(match kind {
        ChannelKind::BaseColor => ViewMode::BaseColor,
        ChannelKind::Normal => ViewMode::Normal,
        ChannelKind::Roughness => ViewMode::Roughness,
        ChannelKind::Metallic => ViewMode::Metallic,
        ChannelKind::Height => ViewMode::Height,
        ChannelKind::AmbientOcclusion => ViewMode::AmbientOcclusion,
        _ => return None,
    })
}

/// What each baked map is, in a few words (shown under its name).
fn mesh_map_note(kind: MeshMapKind) -> &'static str {
    match kind {
        MeshMapKind::Normal => "Tangent-space normal from the mesh",
        MeshMapKind::WorldSpaceNormal => "Surface direction in world space",
        MeshMapKind::Id => "Color per part or material",
        MeshMapKind::AmbientOcclusion => "Crevices and contact shadows",
        MeshMapKind::Curvature => "Edges (light) and cavities (dark)",
        MeshMapKind::Position => "Position in the model's bounds",
        MeshMapKind::Thickness => "How thin the model is",
        MeshMapKind::Height => "High poly height over the low poly",
        MeshMapKind::BentNormal => "Least occluded direction",
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    egui::ScrollArea::vertical().id_salt("texture_set_settings").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(theme::PAD, 10)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            channels(state, ui);
            ui.add_space(10.0);
            mesh_maps(state, ui);
            ui.add_space(10.0);
            ui.label(caption("Display"));
            ui.horizontal(|ui| crate::viewport_bar::albedo_filter(ui, state));
            ui.add_space(10.0);
            ui.label(caption("Mesh"));
            let mesh = &state.session.scene().mesh;
            egui::Grid::new("tss_mesh").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for (k, v) in [("Name", mesh.name.clone()), ("Triangles", mesh.triangle_count().to_string()), ("Vertices", mesh.vertex_count().to_string())] {
                    ui.label(RichText::new(k).color(TEXT_FAINT));
                    ui.label(RichText::new(v).color(TEXT_DIM));
                    ui.end_row();
                }
            });
        });
    });
}

fn channels(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let ts = state.session.selected_texture_set().clone();
    let mut cmds = Vec::new();
    ui.horizontal(|ui| {
        ui.label(caption("Channels"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let missing: Vec<ChannelKind> = ChannelKind::ALL.into_iter().filter(|k| !ts.has_channel(*k)).collect();
            ui.add_enabled_ui(!missing.is_empty(), |ui| {
                ui.menu_button(RichText::new(icons::PLUS).font(theme::font(14.0)), |ui| {
                    for kind in &missing {
                        if ui.button(kind.display_name()).clicked() {
                            cmds.push(Command::AddChannel { texture_set: ts.id, kind: *kind });
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Add a channel");
            });
        });
    });

    let current_view = {
        let prefs = &state.recovery.prefs.viewport;
        (prefs.mesh_map.is_none()).then(|| prefs.view_mode())
    };
    let mut show_view: Option<Option<ViewMode>> = None;
    egui::Grid::new("channels_grid").num_columns(4).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
        for def in &ts.channels {
            let kind = def.kind;
            let name_w = (ui.available_width() - 170.0).clamp(80.0, 180.0);
            ui.allocate_ui_with_layout(egui::vec2(name_w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_width(name_w);
                ui.add(egui::Label::new(RichText::new(kind.display_name()).color(theme::TEXT).font(theme::font_strong(12.5))).truncate());
            });
            let mut format = def.format;
            egui::ComboBox::from_id_salt(("channel_format", kind as u32)).width(84.0).selected_text(format.label()).show_ui(ui, |ui| {
                for f in FORMATS {
                    ui.selectable_value(&mut format, f, f.label());
                }
            });
            if format != def.format {
                cmds.push(Command::SetChannelFormat { texture_set: ts.id, kind, format });
            }
            match view_for(kind) {
                Some(mode) => {
                    let on = current_view == Some(mode);
                    let tip = if on { "Back to the material view" } else { "Show this channel alone in the 3D view" };
                    if icon_button(ui, if on { icons::EYE } else { icons::EYE_SLASH }, tip).clicked() {
                        show_view = Some(if on { None } else { Some(mode) });
                    }
                }
                None => {
                    ui.label("");
                }
            }
            let last = ts.channels.len() == 1;
            let resp = ui
                .add_enabled(!last, egui::Button::new(icons::TRASH).frame(false))
                .on_hover_text(format!("Remove the {} channel (undo brings it back)", kind.display_name()))
                .on_disabled_hover_text("A texture set needs at least one channel");
            if resp.clicked() {
                cmds.push(Command::RemoveChannel { texture_set: ts.id, kind });
            }
            ui.end_row();
        }
    });
    if let Some(view) = show_view {
        let prefs = &mut state.recovery.prefs.viewport;
        prefs.mesh_map = None;
        let mode = view.unwrap_or(ViewMode::Material);
        prefs.view_mode = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
    }
    if !cmds.is_empty() {
        state.run(cmds, now);
    }
}

fn mesh_maps(state: &mut UiState, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(caption("Mesh maps"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(format!("{}  Bake Mesh Maps", icons::MAGIC_WAND)).on_hover_text(crate::keys::native("Bake mesh maps (Ctrl+B)")).clicked() {
                state.bake.open = true;
            }
        });
    });
    if let Some(job) = &state.bake.job {
        ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(theme::ACCENT));
    }
    let ts = state.session.selection().texture_set;
    let shown = state.recovery.prefs.viewport.mesh_map;
    let mut toggle = None;
    for kind in MeshMapKind::ALL {
        let image = state.session.mesh_maps(ts).and_then(|m| m.get(kind)).cloned();
        let baked = image.is_some();
        let frame = egui::Frame::new()
            .fill(if shown == Some(kind) { theme::ACCENT_SOFT } else { theme::BG_2 })
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(6));
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let size = egui::vec2(THUMB as f32, THUMB as f32);
                    match image.as_ref().map(|img| thumbnail(ui.ctx(), ts, kind, img)) {
                        Some(tex) => {
                            ui.add(egui::Image::new((tex.id(), size)));
                        }
                        None => {
                            let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            ui.painter().rect_filled(r, 2.0, theme::FIELD);
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icons::IMAGE, theme::font(16.0), TEXT_FAINT);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(kind.display_name()).font(theme::font_strong(12.5)).color(if baked { theme::TEXT } else { TEXT_DIM }));
                        let note = if baked { mesh_map_note(kind).to_string() } else { "Not baked yet".to_string() };
                        ui.add(egui::Label::new(RichText::new(note).font(theme::font(11.0)).color(TEXT_FAINT)).truncate());
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        let tip = if !baked { "Bake mesh maps to fill this one" } else if shown == Some(kind) { "Click: back to the material view" } else { "Click: show this map on the model" };
        if resp.on_hover_text(tip).clicked() && baked {
            toggle = Some(kind);
        }
    }
    if let Some(kind) = toggle {
        let prefs = &mut state.recovery.prefs.viewport;
        prefs.mesh_map = if prefs.mesh_map == Some(kind) { None } else { Some(kind) };
    }
}

/// Small preview of a baked map, cached until the map is baked again.
fn thumbnail(ctx: &egui::Context, ts: subcraft_core::TextureSetId, kind: MeshMapKind, image: &TiledImage) -> egui::TextureHandle {
    let id = egui::Id::new(("mesh_map_thumb", ts, kind as u32));
    if let Some((cached, tex)) = ctx.data(|d| d.get_temp::<(TiledImage, egui::TextureHandle)>(id))
        && cached.same_storage(image)
    {
        return tex;
    }
    let (w, h) = (image.width().max(1), image.height().max(1));
    let comps = image.comps();
    let mut pixels = Vec::with_capacity((THUMB * THUMB) as usize);
    // Normals may be stored signed; show them the usual 0..1 way.
    let signed = matches!(kind, MeshMapKind::Normal | MeshMapKind::WorldSpaceNormal | MeshMapKind::BentNormal);
    for y in 0..THUMB {
        for x in 0..THUMB {
            let v = image.get((x * w / THUMB).min(w - 1), (y * h / THUMB).min(h - 1));
            let f = |c: f32| {
                let c = if signed && c < 0.0 { c * 0.5 + 0.5 } else { c };
                (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
            };
            pixels.push(if comps >= 3 { egui::Color32::from_rgb(f(v[0]), f(v[1]), f(v[2])) } else { let g = f(v[0]); egui::Color32::from_gray(g) });
        }
    }
    let img = egui::ColorImage { size: [THUMB as usize, THUMB as usize], pixels, source_size: egui::vec2(THUMB as f32, THUMB as f32) };
    let tex = ctx.load_texture(format!("mesh_map_{}", kind.key()), img, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (image.clone(), tex.clone())));
    tex
}
