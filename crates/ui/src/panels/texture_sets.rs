//! Painter's TEXTURE SET LIST (one row per set) and TEXTURE SET SETTINGS (general properties,
//! channels, mesh maps) panels. Everything goes through `app` commands.
use crate::i18n::tr;
use egui::{Color32, RichText, Ui};
use subcraft_app::Command;
use subcraft_core::{ChannelKind, MeshMapKind, TextureSetId};
use subcraft_doc::ChannelFormat;
use crate::{icons, state::UiState, theme::{self, ACCENT, TEXT, TEXT_DIM, TEXT_FAINT}};

/// Name of the one shader every texture set uses (Painter's "Main shader").
pub const SHADER_NAME: &str = "Main shader";

// ---------------------------------------------------------------------------------------------
// TEXTURE SET LIST
// ---------------------------------------------------------------------------------------------

pub fn list(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    egui::ScrollArea::vertical().id_salt("texture_set_list").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        set_rows(state, ui, &mut cmds);
    });
    state.run(cmds, now);
}

/// "4096x4096", the way Painter prints a texture set's size.
pub fn size_label(resolution: u32) -> String { format!("{resolution}x{resolution}") }

fn set_rows(state: &UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    let sel = state.session.selection().texture_set;
    let rename_id = egui::Id::new("texture_set_rename");
    // (set being renamed, text, first frame)
    let mut rename: Option<(TextureSetId, String, bool)> = ui.data(|d| d.get_temp(rename_id));
    ui.spacing_mut().item_spacing.y = 2.0;
    for ts in &state.session.document().texture_sets {
        let selected = ts.id == sel;
        let hidden = state.session.is_set_hidden(ts.id);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
        let inner = rect.shrink2(egui::vec2(2.0, 0.0));
        let painter = ui.painter();
        painter.rect_filled(inner, 0.0, if resp.hovered() && !selected { theme::BG_3 } else { theme::BG_2 });
        if selected { painter.rect_stroke(inner, 0.0, egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside); }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner.shrink2(egui::vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let (eye, tip) = if hidden { (icons::EYE_SLASH, "Show this texture set in the 3D view") } else { (icons::EYE, "Hide this texture set in the 3D view") };
            if ui.add(egui::Button::new(RichText::new(eye).font(theme::font(14.0)).color(if hidden { TEXT_FAINT } else { TEXT })).frame(false)).on_hover_text(tip).clicked() {
                cmds.push(Command::SetTextureSetHidden { texture_set: ts.id, hidden: !hidden });
            }
            let total = ui.available_width();
            let name_w = (total * 0.38).max(20.0);
            let size_w = 74.0f32.min(total * 0.3);
            if let Some((_, text, first)) = rename.as_mut().filter(|(id, ..)| *id == ts.id) {
                let r = ui.add_sized([name_w, 18.0], egui::TextEdit::singleline(text));
                if *first { r.request_focus(); *first = false; }
                else if r.lost_focus() {
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) { cmds.push(Command::RenameTextureSet { texture_set: ts.id, name: text.clone() }); }
                    rename = None;
                }
            } else {
                let color = if hidden { TEXT_FAINT } else { TEXT };
                ui.add_sized([name_w, 18.0], egui::Label::new(RichText::new(&ts.name).font(theme::font_strong(12.0)).color(color)).truncate().selectable(false)).on_hover_text(&ts.name);
            }
            // The size is a dropdown, as in Painter: picking one resizes the set (undoable).
            let size = ui.add_sized([size_w, 18.0], egui::Button::new(RichText::new(size_label(ts.resolution)).font(theme::font(11.5)).color(TEXT_DIM)).frame(false).truncate())
                .on_hover_text(crate::keys::native("Texture set size: click to change it. Painted strokes are resampled; Ctrl+Z restores them."));
            if size.hovered() { ui.painter().rect_stroke(size.rect, 2.0, egui::Stroke::new(1.0, theme::BORDER_STRONG), egui::StrokeKind::Inside); }
            egui::Popup::menu(&size).id(egui::Id::new(("texture_set_size", ts.id))).show(|ui| {
                ui.set_min_width(size_w.max(96.0));
                for r in subcraft_doc::RESOLUTIONS.iter().rev().copied() {
                    if ui.selectable_label(ts.resolution == r, size_label(r)).clicked() {
                        if r != ts.resolution { cmds.push(Command::SetResolution { texture_set: Some(ts.id), resolution: r }); }
                        ui.close();
                    }
                }
            });
            ui.add(egui::Label::new(RichText::new(SHADER_NAME).font(theme::font(11.5)).color(TEXT_DIM)).truncate().selectable(false));
        });
        if resp.clicked() && !selected { cmds.push(Command::SelectTextureSet(ts.id)); }
        if resp.double_clicked() { rename = Some((ts.id, ts.name.clone(), true)); }
        resp.context_menu(|ui| {
            if ui.button(tr("Rename")).clicked() { rename = Some((ts.id, ts.name.clone(), true)); ui.close(); }
            if ui.button(if hidden { "Show in 3D view" } else { "Hide in 3D view" }).clicked() {
                cmds.push(Command::SetTextureSetHidden { texture_set: ts.id, hidden: !hidden }); ui.close();
            }
            ui.menu_button(tr("Resolution"), |ui| {
                for r in subcraft_doc::RESOLUTIONS {
                    if ui.selectable_label(ts.resolution == r, format!("{r} × {r}")).clicked() {
                        cmds.push(Command::SetResolution { texture_set: Some(ts.id), resolution: r }); ui.close();
                    }
                }
            });
        });
    }
    ui.data_mut(|d| match rename { Some(r) => { d.insert_temp(rename_id, r); }, None => { d.remove_temp::<(TextureSetId, String, bool)>(rename_id); } });
}

// ---------------------------------------------------------------------------------------------
// TEXTURE SET SETTINGS
// ---------------------------------------------------------------------------------------------

/// Painter's channel format badge: sRGB8, L8, RGB16F, L16F.
pub fn format_badge(kind: ChannelKind, format: ChannelFormat) -> String {
    let prefix = if kind.is_color() && kind.components() >= 3 && matches!(kind, ChannelKind::BaseColor | ChannelKind::Emissive) { "sRGB" }
        else if kind.components() >= 3 { "RGB" } else { "L" };
    let suffix = match format { ChannelFormat::Bit8 => "8", ChannelFormat::Bit16 => "16", ChannelFormat::Float16 => "16F", ChannelFormat::Float32 => "32F" };
    format!("{prefix}{suffix}")
}

/// The seven mesh maps Painter lists, in its order, with the description it prints.
pub const MESH_MAP_ROWS: [(MeshMapKind, &str, &str); 7] = [
    (MeshMapKind::Normal, "Normal", "Normal Map from Mesh"),
    (MeshMapKind::WorldSpaceNormal, "World space normal", "World Space Normals"),
    (MeshMapKind::Id, "ID", "Color Map from Mesh"),
    (MeshMapKind::AmbientOcclusion, "Ambient occlusion", "Ambient Occlusion Map from Mesh"),
    (MeshMapKind::Curvature, "Curvature", "Curvature Map from Mesh"),
    (MeshMapKind::Position, "Position", "Position"),
    (MeshMapKind::Thickness, "Thickness", "Thickness Map from Mesh"),
];

pub fn settings(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let jump_id = egui::Id::new("tss_jump");
    let jump: Option<u8> = ui.data(|d| d.get_temp(jump_id));
    ui.data_mut(|d| d.remove_temp::<u8>(jump_id));
    // Icon tab row: jumps to a section (the panel is one scrolling column, like Painter's).
    egui::Frame::new().fill(theme::BG_2).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (i, (icon, tip)) in [(icons::SLIDERS, "General properties and channels"), (icons::SPARKLE, "Channels"), (icons::CUBE_WIRE, "Mesh maps"), (icons::SQUARES, "ID colors")].into_iter().enumerate() {
                if crate::widgets::icon_button(ui, icon, tip).clicked() { ui.data_mut(|d| d.insert_temp(jump_id, i as u8)); ui.ctx().request_repaint(); }
            }
        });
    });
    egui::ScrollArea::vertical().id_salt("tss").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if jump == Some(0) { ui.scroll_to_cursor(Some(egui::Align::TOP)); }
            general(state, ui, &mut cmds);
            section_rule(ui);
            if jump == Some(1) { ui.scroll_to_cursor(Some(egui::Align::TOP)); }
            channels(state, ui, &mut cmds);
            section_rule(ui);
            if jump == Some(2) { ui.scroll_to_cursor(Some(egui::Align::TOP)); }
            mesh_maps(state, ui, &mut cmds);
            section_rule(ui);
            if jump == Some(3) { ui.scroll_to_cursor(Some(egui::Align::TOP)); }
            crate::panels::id_colors::section(state, ui);
        });
    });
    state.run(cmds, now);
}

fn section_rule(ui: &mut Ui) {
    ui.add_space(6.0);
    crate::widgets::divider(ui);
    ui.add_space(6.0);
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(theme::font_strong(12.0)).color(TEXT));
    ui.add_space(2.0);
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.add_sized([86.0, 20.0], egui::Label::new(RichText::new(text).font(theme::font(11.5)).color(TEXT_DIM)).selectable(false));
}

fn general(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    heading(ui, "General properties");
    let (id, name, current) = { let ts = state.session.selected_texture_set(); (ts.id, ts.name.clone(), ts.resolution) };
    // Name: edited in place, committed when the field loses focus.
    let name_id = egui::Id::new(("tss_name", id));
    let mut text: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_else(|| name.clone());
    ui.horizontal(|ui| {
        field_label(ui, "Name");
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 12.0));
        if r.lost_focus() && text != name && !text.trim().is_empty() { cmds.push(Command::RenameTextureSet { texture_set: id, name: text.clone() }); }
        if r.has_focus() { ui.data_mut(|d| d.insert_temp(name_id, text.clone())); } else { ui.data_mut(|d| d.remove_temp::<String>(name_id)); }
    });
    ui.horizontal_top(|ui| {
        field_label(ui, "Description");
        let mut empty = String::new();
        ui.add_enabled(false, egui::TextEdit::multiline(&mut empty).hint_text("Description").desired_rows(4).desired_width(ui.available_width() - 12.0))
            .on_disabled_hover_text("Texture set descriptions are not stored in SubCraft projects yet");
    });
    // Size: square sets only; the lock is shown engaged and the second field mirrors the first.
    let mut picked = current;
    ui.horizontal(|ui| {
        field_label(ui, "Size");
        let w = ((ui.available_width() - 64.0) / 2.0).max(40.0);
        egui::ComboBox::from_id_salt("tss_size").width(w).selected_text(current.to_string()).show_ui(ui, |ui| {
            for r in subcraft_doc::RESOLUTIONS.iter().rev() { ui.selectable_value(&mut picked, *r, r.to_string()); }
        }).response.on_hover_text(crate::keys::native("Working resolution. Painted strokes are resampled; Ctrl+Z restores them."));
        ui.add_enabled(false, egui::Button::new(RichText::new(icons::CROSSHAIR).font(theme::font(13.0)))).on_disabled_hover_text("Width and height are linked: texture sets are square");
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt("tss_size_h").width(w).selected_text(current.to_string()).show_ui(ui, |_| {});
        });
    });
    if picked != current { cmds.push(Command::SetResolution { texture_set: Some(id), resolution: picked }); }
    let all_same = state.session.document().texture_sets.iter().all(|ts| ts.resolution == current);
    ui.horizontal(|ui| {
        field_label(ui, "");
        if ui.add_enabled(!all_same, egui::Button::new("Apply size to all")).on_hover_text(tr("Use this resolution for every texture set")).clicked() {
            cmds.push(Command::SetResolution { texture_set: None, resolution: current });
        }
    });
    ui.horizontal(|ui| {
        field_label(ui, "Shader Instance");
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt("tss_shader").width(ui.available_width() - 28.0).selected_text(SHADER_NAME).show_ui(ui, |_| {});
        });
    });
    ui.horizontal(|ui| { crate::viewport_bar::albedo_filter(ui, state); });
}

fn channels(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    heading(ui, "Channels");
    let (id, defs) = { let ts = state.session.selected_texture_set(); (ts.id, ts.channels.clone()) };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Channels").font(theme::font(11.5)).color(TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let addable: Vec<ChannelKind> = ChannelKind::ALL.iter().copied().filter(|k| !defs.iter().any(|d| d.kind == *k)).collect();
            let r = crate::widgets::icon_button(ui, icons::PLUS, "Add a channel");
            egui::Popup::menu(&r).show(|ui| {
                if addable.is_empty() { ui.label("Every channel is already in this texture set"); }
                for k in addable {
                    if ui.button(k.display_name()).clicked() { cmds.push(Command::AddChannel { texture_set: id, kind: k }); ui.close(); }
                }
            });
        });
    });
    for d in &defs {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(d.kind.display_name()).font(theme::font_strong(12.0)).color(TEXT)).selectable(false));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled(false, egui::Button::new(RichText::new(icons::X).font(theme::font(12.0))).frame(false))
                    .on_disabled_hover_text("Removing a channel is not supported yet (layers store data per channel)");
                ui.add_enabled(false, egui::Button::new(RichText::new(icons::GEAR).font(theme::font(12.0))).frame(false))
                    .on_disabled_hover_text("Channel format is fixed for now");
                ui.label(RichText::new(format_badge(d.kind, d.format)).font(theme::font(11.0)).color(TEXT_DIM));
            });
        });
    }
}

fn mesh_maps(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    heading(ui, "Mesh maps");
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if ui.add(egui::Button::new(RichText::new("Bake Mesh Maps").font(theme::font(12.0)).color(TEXT)).min_size(egui::vec2(110.0, 24.0)))
            .on_hover_text(crate::keys::native("Bake mesh maps (Ctrl+B)")).clicked() { state.bake.open = true; }
        if ui.add(egui::Button::new(RichText::new("Import mesh maps…").font(theme::font(12.0)).color(TEXT)).min_size(egui::vec2(110.0, 24.0)))
            .on_hover_text("Use maps baked elsewhere (normal, AO, curvature, ID…): matched to texture sets by file name and UDIM tile, no re-bake").clicked() {
            crate::files::pick(ui.ctx(), crate::files::Purpose::ImportMeshMaps, crate::files::IMAGES, true, Some("Choose baked mesh maps"));
        }
    });
    if let Some(job) = &state.bake.job { ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(ACCENT)); }
    let (id, set_name) = { let ts = state.session.selected_texture_set(); (ts.id, ts.name.clone()) };
    if state.session.mesh_maps_stale(id) {
        ui.label(RichText::new(format!("{}  Baked for an earlier version of the mesh", icons::WARNING)).small().color(theme::WARNING));
    }
    let mut any = false;
    for (kind, title, what) in MESH_MAP_ROWS {
        let Some(thumb) = map_thumbnail(state, ui.ctx(), id, kind) else { continue };
        any = true;
        map_row(ui, Some(&thumb), title, &format!("{what} {set_name}"), || cmds.push(Command::ClearMeshMap { texture_set: id, kind }));
    }
    for (kind, title, what, empty) in [(MeshMapKind::Height, "Height", "Height Map from Mesh", "Select height map"), (MeshMapKind::BentNormal, "Bent normals", "Bent Normals Map from Mesh", "Select bent normals map")] {
        match map_thumbnail(state, ui.ctx(), id, kind) {
            Some(thumb) => { any = true; map_row(ui, Some(&thumb), title, &format!("{what} {set_name}"), || cmds.push(Command::ClearMeshMap { texture_set: id, kind })); }
            None => {
                if ui.add_sized([ui.available_width(), 38.0], egui::Button::new(RichText::new(empty).font(theme::font(12.0)).color(TEXT_DIM)))
                    .on_hover_text("Opens the bake window: pick a high-poly mesh and tick this map").clicked() {
                    state.bake.open = true;
                    if !state.bake.settings.maps.contains(&kind) { state.bake.settings.maps.push(kind); }
                }
            }
        }
    }
    if !any { ui.label(RichText::new("No mesh maps baked yet. Use Bake Mesh Maps.").small().color(TEXT_FAINT)); }
}

fn map_row(ui: &mut Ui, thumb: Option<&egui::TextureHandle>, title: &str, desc: &str, mut clear: impl FnMut()) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, theme::BG_2);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
    let img = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(38.0, 38.0));
    p.rect_filled(img, 0.0, theme::FIELD);
    if let Some(t) = thumb { p.image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE); }
    p.text(egui::pos2(img.right() + 8.0, rect.top() + 14.0), egui::Align2::LEFT_CENTER, title, theme::font_strong(12.0), TEXT);
    let mut job = egui::text::LayoutJob::simple_singleline(desc.to_string(), theme::font(11.0), TEXT_DIM);
    job.wrap.max_width = rect.right() - img.right() - 40.0; job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    p.galley(egui::pos2(img.right() + 8.0, rect.top() + 24.0), galley, TEXT_DIM);
    let x = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, rect.center().y), egui::vec2(20.0, 20.0));
    let r = ui.interact(x, ui.id().with(("clear_map", title)), egui::Sense::click());
    ui.painter().text(x.center(), egui::Align2::CENTER_CENTER, icons::X, theme::font(12.0), if r.hovered() { TEXT } else { TEXT_DIM });
    if r.on_hover_text("Remove this baked map (bake again to get it back)").clicked() { clear(); }
    ui.add_space(2.0);
}

/// Side of the generated mesh-map thumbnails.
const THUMB: usize = 38;

/// Thumbnail of a baked map, cached per (set, map) and rebuilt when its content changes.
fn map_thumbnail(state: &UiState, ctx: &egui::Context, ts: TextureSetId, kind: MeshMapKind) -> Option<egui::TextureHandle> {
    let img = state.session.mesh_maps(ts)?.get(kind)?;
    let (w, h, comps) = (img.width(), img.height(), img.comps());
    if w == 0 || h == 0 { return None; }
    // Cheap fingerprint: the dimensions and a sparse sample, enough to notice a re-bake.
    let mut fp = (w as u64) << 32 | h as u64;
    for i in 0..8u32 { for j in 0..8u32 {
        let px = img.get(((i * 2 + 1) * w / 16).min(w - 1), ((j * 2 + 1) * h / 16).min(h - 1));
        for c in px.iter().take(comps.max(1)) { fp = fp.wrapping_mul(1099511628211).wrapping_add(c.to_bits() as u64); }
    } }
    let key = egui::Id::new(("mesh_map_thumb", ts, kind));
    if let Some((old, tex)) = ctx.data(|d| d.get_temp::<(u64, egui::TextureHandle)>(key)) && old == fp { return Some(tex); }
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut px = Vec::with_capacity(THUMB * THUMB);
    for y in 0..THUMB { for x in 0..THUMB {
        let s = img.get(((x as u32 * 2 + 1) * w / (THUMB as u32 * 2)).min(w - 1), ((y as u32 * 2 + 1) * h / (THUMB as u32 * 2)).min(h - 1));
        px.push(if comps >= 3 { Color32::from_rgb(to8(s[0]), to8(s[1]), to8(s[2])) } else { Color32::from_gray(to8(s[0])) });
    } }
    let tex = ctx.load_texture(format!("mesh-map-{ts:?}-{}", kind.key()), egui::ColorImage { size: [THUMB, THUMB], pixels: px, source_size: egui::vec2(THUMB as f32, THUMB as f32) }, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, (fp, tex.clone())));
    Some(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_and_sizes_match_painter() {
        assert_eq!(format_badge(ChannelKind::BaseColor, ChannelFormat::Bit8), "sRGB8");
        assert_eq!(format_badge(ChannelKind::Metallic, ChannelFormat::Bit8), "L8");
        assert_eq!(format_badge(ChannelKind::Normal, ChannelFormat::Float16), "RGB16F");
        assert_eq!(format_badge(ChannelKind::Height, ChannelFormat::Float16), "L16F");
        assert_eq!(size_label(4096), "4096x4096");
    }

    fn frame(state: &mut UiState, ctx: &egui::Context, f: fn(&mut UiState, &mut Ui)) -> egui::FullOutput {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0))), ..Default::default() }, |ui| f(state, ui));
        out.textures_delta.clear();
        out
    }
    fn texts(out: &egui::FullOutput) -> Vec<String> {
        out.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect()
    }

    #[test]
    fn list_shows_one_row_with_size_and_shader_and_settings_show_channels_and_map_slots() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 256), None);
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        let t = texts(&frame(&mut state, &ctx, list));
        assert!(t.iter().any(|s| s == "256x256"), "{t:?}");
        assert!(t.iter().any(|s| s == SHADER_NAME));
        for _ in 0..2 { frame(&mut state, &ctx, settings); }
        let t = texts(&frame(&mut state, &ctx, settings));
        for want in ["GENERAL PROPERTIES", "CHANNELS", "MESH MAPS", "Base Color", "sRGB8", "Select height map", "Select bent normals map"] {
            assert!(t.iter().any(|s| s.contains(want)), "missing {want}: {t:?}");
        }
    }

    #[test]
    fn baked_maps_become_rows_and_x_clears_them_through_the_command() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ts = state.session.selection().texture_set;
        let mut maps = std::collections::BTreeMap::new();
        maps.insert(MeshMapKind::Curvature, subcraft_image::TiledImage::new(64, 64, 1, [0.7; 4]));
        state.session.set_mesh_maps(ts, maps);
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, settings); }
        let t = texts(&frame(&mut state, &ctx, settings));
        assert!(t.iter().any(|s| s == "Curvature") && t.iter().any(|s| s.starts_with("Curvature Map from Mesh")), "{t:?}");
        assert!(!t.iter().any(|s| s.starts_with("Normal Map from Mesh")), "unbaked maps are not listed");
        let tex = map_thumbnail(&state, &ctx, ts, MeshMapKind::Curvature).unwrap();
        assert_eq!(tex.size(), [THUMB, THUMB]);
        state.run(vec![Command::ClearMeshMap { texture_set: ts, kind: MeshMapKind::Curvature }], 0.0);
        assert!(state.session.mesh_maps(ts).is_none());
    }

    fn click_frame(state: &mut UiState, ctx: &egui::Context, at: egui::Pos2) {
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0)));
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: screen, events, ..Default::default() }, |ui| list(state, ui));
            out.textures_delta.clear();
        }
    }
    fn text_pos(out: &egui::FullOutput, want: &str) -> Option<egui::Pos2> {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == want => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        })
    }

    #[test]
    fn clicking_the_size_in_the_list_offers_sizes_and_resizes_with_undo() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 256), None);
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        let at = text_pos(&frame(&mut state, &ctx, list), "256x256").expect("size shown in the row");
        click_frame(&mut state, &ctx, at);
        let out = frame(&mut state, &ctx, list);
        let t = texts(&out);
        for r in subcraft_doc::RESOLUTIONS { assert!(t.iter().any(|s| *s == size_label(r)), "dropdown lists {r}: {t:?}"); }
        let pick = text_pos(&out, "1024x1024").unwrap();
        click_frame(&mut state, &ctx, pick);
        assert_eq!(state.session.selected_texture_set().resolution, 1024, "picking a size resizes the set");
        state.session.perform(subcraft_app::Action::Undo).unwrap();
        assert_eq!(state.session.selected_texture_set().resolution, 256, "undo restores the size");
    }
}
