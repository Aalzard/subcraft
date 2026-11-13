//! Painter's TEXTURE SET LIST (one row per set) and TEXTURE SET SETTINGS (general properties,
//! channels, mesh maps) panels. Everything goes through `app` commands.
use crate::i18n::tr;
use egui::{Color32, RichText, Ui};
use subcraft_app::{Command, SetsVisibility};
use subcraft_core::{ChannelKind, MeshMapKind, TextureSetId};
use subcraft_doc::{AoMixing, ChannelFormat, HeightToNormal, MixingSettings, NormalMixing, UvPadding};
use crate::{icons, state::UiState, theme::{self, ACCENT, TEXT, TEXT_DIM, TEXT_FAINT}};

/// Name of the shader instance every texture set starts with (Painter's "Main shader").
pub const SHADER_NAME: &str = subcraft_doc::MAIN_SHADER;

// ---------------------------------------------------------------------------------------------
// TEXTURE SET LIST
// ---------------------------------------------------------------------------------------------

/// Texture Set List > Settings: which description lines the rows show (Painter's three modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DescriptionDisplay {
    /// A description line only under sets that have one.
    #[default]
    HideEmpty,
    /// No description lines.
    HideAll,
    /// A description line under every set, empty ones included.
    ShowAll,
}

impl DescriptionDisplay {
    pub const ALL: [DescriptionDisplay; 3] = [DescriptionDisplay::HideEmpty, DescriptionDisplay::HideAll, DescriptionDisplay::ShowAll];

    pub fn label(self) -> &'static str {
        match self {
            DescriptionDisplay::HideEmpty => "Hide Empty Descriptions",
            DescriptionDisplay::HideAll => "Hide All Descriptions",
            DescriptionDisplay::ShowAll => "Show All Descriptions",
        }
    }

    /// Whether a row with this description shows a description line.
    pub fn shows(self, description: &str) -> bool {
        match self {
            DescriptionDisplay::HideEmpty => !description.trim().is_empty(),
            DescriptionDisplay::HideAll => false,
            DescriptionDisplay::ShowAll => true,
        }
    }
}

/// Labels of the visibility menu, in Painter's order.
pub const VISIBILITY_MENU: [(SetsVisibility, &str); 3] =
    [(SetsVisibility::ShowAll, "Show All"), (SetsVisibility::HideAll, "Hide All"), (SetsVisibility::Invert, "Invert Show/Hide")];

/// Ids of the header buttons (tests click them).
pub fn visibility_menu_id() -> egui::Id { egui::Id::new("tsl_visibility") }
pub fn focus_button_id() -> egui::Id { egui::Id::new("tsl_focus") }
pub fn list_settings_id() -> egui::Id { egui::Id::new("tsl_settings") }

pub fn list(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    list_header(state, ui, &mut cmds);
    let mut shader_ops = Vec::new();
    let mut hovered = None;
    egui::ScrollArea::vertical().id_salt("texture_set_list").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        set_rows(state, ui, &mut cmds, &mut shader_ops, &mut hovered);
    });
    // Hovering a row highlights its texture set in the 3D view (Painter).
    if let Some(slot) = hovered {
        state.highlight_slot = Some(slot);
    }
    state.run(cmds, now);
    crate::panels::shader_settings::apply(state, shader_ops);
}

/// The small header of the list: visibility menu, Focus mode, settings menu.
fn list_header(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    let focus = state.session.texture_set_focus();
    egui::Frame::new().fill(theme::BG_2).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let vis = ui.push_id(visibility_menu_id(), |ui| crate::widgets::icon_button(ui, icons::EYE, "Show or hide all texture sets")).inner;
            egui::Popup::menu(&vis).id(visibility_menu_id().with("popup")).show(|ui| {
                for (op, label) in VISIBILITY_MENU {
                    if ui.button(label).clicked() { cmds.push(Command::SetTextureSetsVisibility(op)); ui.close(); }
                }
            });
            let tip = if focus { "Focus mode is on: only the selected texture set is shown. Click to show the others again" }
                else { "Focus mode: show only the selected texture set (it follows the selection)" };
            let r = ui.push_id(focus_button_id(), |ui| ui.add(egui::Button::new(RichText::new(icons::CROSSHAIR).font(theme::font(14.0)).color(if focus { ACCENT } else { TEXT_DIM }))
                .selected(focus).frame(focus))).inner.on_hover_text(tip);
            if r.clicked() { cmds.push(Command::SetTextureSetFocus(!focus)); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let gear = ui.push_id(list_settings_id(), |ui| crate::widgets::icon_button(ui, icons::GEAR, "Texture Set List settings")).inner;
                egui::Popup::menu(&gear).id(list_settings_id().with("popup")).show(|ui| {
                    for mode in DescriptionDisplay::ALL {
                        if ui.radio(state.set_descriptions == mode, mode.label()).clicked() { state.set_descriptions = mode; ui.close(); }
                    }
                });
            });
        });
    });
}

/// "4096x4096", the way Painter prints a texture set's size.
pub fn size_label(resolution: u32) -> String { format!("{resolution}x{resolution}") }

fn set_rows(state: &UiState, ui: &mut Ui, cmds: &mut Vec<Command>, shader_ops: &mut Vec<crate::panels::shader_settings::ShaderOp>, hovered: &mut Option<u32>) {
    let sel = state.session.selection().texture_set;
    let rename_id = egui::Id::new("texture_set_rename");
    // (set being renamed, text, first frame)
    let mut rename: Option<(TextureSetId, String, bool)> = ui.data(|d| d.get_temp(rename_id));
    let describe_id = egui::Id::new("texture_set_describe");
    let mut describe: Option<(TextureSetId, String, bool)> = ui.data(|d| d.get_temp(describe_id));
    ui.spacing_mut().item_spacing.y = 2.0;
    for ts in &state.session.document().texture_sets {
        let selected = ts.id == sel;
        let hidden = state.session.is_set_hidden(ts.id);
        let editing_description = describe.as_ref().is_some_and(|(id, ..)| *id == ts.id);
        let show_description = editing_description || state.set_descriptions.shows(&ts.description);
        let height = if show_description { 40.0 } else { 24.0 };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
        let inner = rect.shrink2(egui::vec2(2.0, 0.0));
        let painter = ui.painter();
        painter.rect_filled(inner, 0.0, if resp.hovered() && !selected { theme::BG_3 } else { theme::BG_2 });
        if selected { painter.rect_stroke(inner, 0.0, egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside); }
        let top = egui::Rect::from_min_max(inner.min, egui::pos2(inner.max.x, inner.min.y + 24.0));
        if show_description {
            let line = egui::Rect::from_min_max(egui::pos2(inner.min.x + 30.0, top.max.y - 4.0), egui::pos2(inner.max.x - 6.0, inner.max.y - 2.0));
            if let Some((_, text, first)) = describe.as_mut().filter(|(id, ..)| *id == ts.id) {
                let r = ui.put(line, egui::TextEdit::singleline(text).hint_text("Description").font(theme::font(11.0)));
                if *first { r.request_focus(); *first = false; }
                else if r.lost_focus() {
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) { cmds.push(Command::SetTextureSetDescription { texture_set: ts.id, description: text.clone() }); }
                    describe = None;
                }
            } else {
                let (text, color) = if ts.description.trim().is_empty() { ("No description", TEXT_FAINT) } else { (ts.description.as_str(), TEXT_DIM) };
                let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), theme::font(11.0), color);
                job.wrap.max_width = line.width(); job.wrap.max_rows = 1;
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(egui::pos2(line.min.x, line.center().y - galley.size().y * 0.5), galley, color);
            }
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(top.shrink2(egui::vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
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
            // The shader button: shows the set's instance, opens the instance menu.
            let shader = state.session.shader_instance_of(ts.id).name;
            let button = ui.add(egui::Button::new(RichText::new(shader).font(theme::font(11.5)).color(TEXT_DIM)).frame(false).truncate())
                .on_hover_text(tr("Shader instance: click to pick another, make a new one or open Shader Settings"));
            if button.hovered() { ui.painter().rect_stroke(button.rect, 2.0, egui::Stroke::new(1.0, theme::BORDER_STRONG), egui::StrokeKind::Inside); }
            egui::Popup::menu(&button).id(egui::Id::new(("texture_set_shader", ts.id))).show(|ui| {
                ui.set_min_width(160.0);
                crate::panels::shader_settings::instance_menu(state, ui, ts.id, shader_ops);
            });
        });
        if resp.hovered() { *hovered = Some(ts.material_slot); }
        if resp.clicked() && !selected { cmds.push(Command::SelectTextureSet(ts.id)); }
        if resp.double_clicked() { rename = Some((ts.id, ts.name.clone(), true)); }
        resp.context_menu(|ui| {
            if ui.button(tr("Rename")).clicked() { rename = Some((ts.id, ts.name.clone(), true)); ui.close(); }
            let original = state.session.original_set_name(ts.id);
            let reset = ui.add_enabled(original.as_deref().is_some_and(|o| o != ts.name), egui::Button::new("Reset name"));
            let reset = match &original { Some(o) => reset.on_hover_text(format!("Rename to the mesh material name \"{o}\"")), None => reset };
            if reset.clicked() { cmds.push(Command::ResetTextureSetName(ts.id)); ui.close(); }
            if ui.button("Edit Description").clicked() { describe = Some((ts.id, ts.description.clone(), true)); ui.close(); }
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
    ui.data_mut(|d| match describe { Some(r) => { d.insert_temp(describe_id, r); }, None => { d.remove_temp::<(TextureSetId, String, bool)>(describe_id); } });
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

/// The mesh-map slots of a texture set, in Painter's order: the map, the slot's name, and what
/// a map in it is called.
pub const MESH_MAP_SLOTS: [(MeshMapKind, &str, &str); 9] = [
    (MeshMapKind::Normal, "Normal", "Normal Map from Mesh"),
    (MeshMapKind::WorldSpaceNormal, "World space normal", "World Space Normals"),
    (MeshMapKind::Id, "ID", "Color Map from Mesh"),
    (MeshMapKind::AmbientOcclusion, "Ambient occlusion", "Ambient Occlusion Map from Mesh"),
    (MeshMapKind::Curvature, "Curvature", "Curvature Map from Mesh"),
    (MeshMapKind::Position, "Position", "Position"),
    (MeshMapKind::Thickness, "Thickness", "Thickness Map from Mesh"),
    (MeshMapKind::Height, "Height", "Height Map from Mesh"),
    (MeshMapKind::BentNormal, "Bent normals", "Bent Normals Map from Mesh"),
];

/// Text of an empty slot ("Select height map").
pub fn empty_slot_label(title: &str) -> String {
    let name = if title == "ID" { title.to_string() } else { title.to_lowercase() };
    format!("Select {name} map")
}

/// Load an image file into one mesh-map slot of a texture set, replacing the map that was there
/// (the slot's file picker, or an image dropped on the slot). The other slots keep their maps.
pub(crate) fn set_mesh_map_from_file(state: &mut UiState, texture_set: TextureSetId, kind: MeshMapKind, path: &std::path::Path, now: f64) {
    let directx = subcraft_app::mesh_map_import::parse_file_name(path).2;
    let assignment = subcraft_app::mesh_map_import::MeshMapAssignment { texture_set, kind, path: path.to_path_buf(), directx };
    let report = state.session.import_mesh_maps(&[assignment], false);
    let slot = MESH_MAP_SLOTS.iter().find(|s| s.0 == kind).map_or("Mesh", |s| s.1);
    match report.errors.first() {
        Some(e) => state.set_status(format!("{slot} map: {e}"), now),
        None => {
            let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
            state.set_status(format!("{slot} map set from {file}"), now);
            state.recovery.autosaver.request_soon();
        }
    }
}

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
            mesh_maps(state, ui, &mut cmds, now);
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
    let (id, name, current, description) = { let ts = state.session.selected_texture_set(); (ts.id, ts.name.clone(), ts.resolution, ts.description.clone()) };
    // Name: edited in place, committed when the field loses focus.
    let name_id = egui::Id::new(("tss_name", id));
    let mut text: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_else(|| name.clone());
    ui.horizontal(|ui| {
        field_label(ui, "Name");
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 12.0));
        if r.lost_focus() && text != name && !text.trim().is_empty() { cmds.push(Command::RenameTextureSet { texture_set: id, name: text.clone() }); }
        if r.has_focus() { ui.data_mut(|d| d.insert_temp(name_id, text.clone())); } else { ui.data_mut(|d| d.remove_temp::<String>(name_id)); }
    });
    // Description: also edited in place, committed when the field loses focus (one undo step).
    let desc_id = egui::Id::new(("tss_description", id));
    let mut desc: String = ui.data(|d| d.get_temp(desc_id)).unwrap_or_else(|| description.clone());
    ui.horizontal_top(|ui| {
        field_label(ui, "Description");
        let r = ui.add(egui::TextEdit::multiline(&mut desc).hint_text("Description").desired_rows(4).desired_width(ui.available_width() - 12.0))
            .on_hover_text("Shown under the name in the Texture Set List");
        if r.lost_focus() && desc != description { cmds.push(Command::SetTextureSetDescription { texture_set: id, description: desc.clone() }); }
        if r.has_focus() { ui.data_mut(|d| d.insert_temp(desc_id, desc.clone())); } else { ui.data_mut(|d| d.remove_temp::<String>(desc_id)); }
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
    let mut shader_ops = Vec::new();
    ui.horizontal(|ui| {
        field_label(ui, "Shader Instance");
        let current = state.session.shader_instance_of(id).name;
        egui::ComboBox::from_id_salt("tss_shader").width(ui.available_width() - 28.0).selected_text(current).show_ui(ui, |ui| {
            crate::panels::shader_settings::instance_menu(state, ui, id, &mut shader_ops);
        }).response.on_hover_text(tr("The shader instance this texture set is drawn with; several sets can share one"));
    });
    crate::panels::shader_settings::apply(state, shader_ops);
    ui.horizontal(|ui| { crate::viewport_bar::albedo_filter(ui, state); });
    transparency(state, ui, cmds);
}

/// Issue #145: a texture set that starts empty (unpainted texels have alpha 0) and how the
/// viewport draws opacity (Painter's alpha-test / alpha-blending shaders and their threshold and
/// dithering parameters).
fn transparency(state: &UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    let (id, transparent, alpha, has_opacity) = {
        let ts = state.session.selected_texture_set();
        (ts.id, ts.transparent, ts.alpha, ts.has_channel(ChannelKind::Opacity))
    };
    ui.horizontal(|ui| {
        field_label(ui, "Background");
        let mut t = transparent;
        ui.checkbox(&mut t, "Transparent (start empty)").on_hover_text(
            "Unpainted texels have alpha 0 instead of the default material: export Base Color with an alpha preset \
             (\"+ Opacity\" / \"+ Alpha\") to keep them transparent.",
        );
        if t != transparent { cmds.push(Command::SetTransparent { texture_set: id, transparent: t }); }
    });
    let mut a = alpha;
    ui.horizontal(|ui| {
        field_label(ui, "Alpha");
        egui::ComboBox::from_id_salt("tss_alpha_mode").width(ui.available_width() - 28.0).selected_text(a.mode.label()).show_ui(ui, |ui| {
            for m in subcraft_doc::AlphaMode::ALL { ui.selectable_value(&mut a.mode, m, m.label()); }
        }).response.on_hover_text("How the viewport draws Opacity (and the empty parts of a transparent set). Export is not affected.");
    });
    if a.mode != subcraft_doc::AlphaMode::Opaque {
        ui.horizontal(|ui| {
            field_label(ui, "Alpha threshold");
            ui.add_enabled(a.mode == subcraft_doc::AlphaMode::Test && !a.dithering, egui::Slider::new(&mut a.threshold, 0.0..=1.0).fixed_decimals(2))
                .on_disabled_hover_text("Used by alpha test");
        });
        ui.horizontal(|ui| {
            field_label(ui, "");
            ui.checkbox(&mut a.dithering, "Alpha dithering").on_hover_text("Ordered dither instead of a hard cut / blending: depth-correct, no sorting");
        });
        if !has_opacity && !transparent {
            ui.horizontal(|ui| {
                field_label(ui, "");
                ui.label(RichText::new("Add an Opacity channel or make the set transparent to see an effect.").font(theme::font(11.0)).color(TEXT_FAINT));
            });
        }
    }
    if a != alpha { cmds.push(Command::SetAlphaSettings { texture_set: id, alpha: a }); }
}

fn channels(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>) {
    heading(ui, "Channels");
    let (id, defs, names) = { let ts = state.session.selected_texture_set(); (ts.id, ts.channels.clone(), ts.user_channel_names.clone()) };
    let label = |k: ChannelKind| names.get(&k).cloned().unwrap_or_else(|| k.display_name().to_string());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Channels").font(theme::font(11.5)).color(TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui.push_id(add_channel_id(), |ui| crate::widgets::icon_button(ui, icons::PLUS, "Add a channel")).inner;
            egui::Popup::menu(&r).id(add_channel_id().with("popup")).show(|ui| {
                let groups = add_menu_groups(&defs.iter().map(|d| d.kind).collect::<Vec<_>>());
                if groups.iter().all(|(_, kinds)| kinds.is_empty()) { ui.label("Every channel is already in this texture set"); }
                for (title, kinds) in groups {
                    if kinds.is_empty() { continue; }
                    ui.label(RichText::new(title).small().color(TEXT_FAINT));
                    for k in kinds {
                        if ui.button(label(k)).clicked() { cmds.push(Command::AddChannel { texture_set: id, kind: k }); ui.close(); }
                    }
                    ui.separator();
                }
            });
        });
    });
    // (set, channel, text, first frame) of a user channel being renamed.
    let rename_id = egui::Id::new("tss_channel_rename");
    let mut rename: Option<(TextureSetId, ChannelKind, String, bool)> = ui.data(|d| d.get_temp(rename_id));
    let last = defs.len() == 1;
    for d in &defs {
        ui.horizontal(|ui| {
            if let Some((_, _, text, first)) = rename.as_mut().filter(|(ts, k, ..)| *ts == id && *k == d.kind) {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                if *first { r.request_focus(); *first = false; }
                else if r.lost_focus() {
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) { cmds.push(Command::RenameUserChannel { texture_set: id, kind: d.kind, name: text.clone() }); }
                    rename = None;
                }
            } else {
                let r = ui.add(egui::Label::new(RichText::new(label(d.kind)).font(theme::font_strong(12.0)).color(TEXT)).selectable(false).sense(egui::Sense::click()));
                if d.kind.is_user() {
                    let r = r.on_hover_text("Double-click to rename this user channel");
                    if r.double_clicked() { rename = Some((id, d.kind, label(d.kind), true)); }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let x = ui.push_id(("remove_channel", d.kind), |ui| ui.add_enabled(!last, egui::Button::new(RichText::new(icons::X).font(theme::font(12.0))).frame(false))).inner;
                let x = if last { x.on_disabled_hover_text("A texture set keeps at least one channel") }
                    else { x.on_hover_text("Remove this channel. Layers keep what they painted in it: add it back (or undo) to get it again") };
                if x.clicked() { cmds.push(Command::RemoveChannel { texture_set: id, kind: d.kind }); }
                let gear = ui.push_id(("channel_format", d.kind), |ui| ui.add(egui::Button::new(RichText::new(icons::GEAR).font(theme::font(12.0))).frame(false))).inner
                    .on_hover_text("Channel format");
                egui::Popup::menu(&gear).id(egui::Id::new(("channel_format_menu", d.kind))).show(|ui| {
                    for f in CHANNEL_FORMATS {
                        if ui.selectable_label(d.format == f, format_badge(d.kind, f)).clicked() {
                            if f != d.format { cmds.push(Command::SetChannelFormat { texture_set: id, kind: d.kind, format: f }); }
                            ui.close();
                        }
                    }
                });
                ui.label(RichText::new(format_badge(d.kind, d.format)).font(theme::font(11.0)).color(TEXT_DIM));
            });
        });
    }
    ui.data_mut(|d| match rename { Some(r) => { d.insert_temp(rename_id, r); }, None => { d.remove::<(TextureSetId, ChannelKind, String, bool)>(rename_id); } });
    ui.add_space(6.0);
    let current = state.session.selected_texture_set().mixing;
    let picked = mixing_rows(ui, current);
    if picked != current { cmds.push(Command::SetMixing { texture_set: id, mixing: picked }); }
}

/// Formats offered by a channel's settings menu.
pub const CHANNEL_FORMATS: [ChannelFormat; 4] = [ChannelFormat::Bit8, ChannelFormat::Bit16, ChannelFormat::Float16, ChannelFormat::Float32];

/// Id of the add-channel button (tests click it).
pub fn add_channel_id() -> egui::Id { egui::Id::new("tss_add_channel") }

/// The add-channel menu, grouped as in Painter: channels the viewport shader uses, the other standard
/// channels, then the user channels. Channels the set already has are left out.
pub fn add_menu_groups(present: &[ChannelKind]) -> [(&'static str, Vec<ChannelKind>); 3] {
    let missing = |f: &dyn Fn(ChannelKind) -> bool| ChannelKind::ALL.iter().copied().filter(|k| !present.contains(k) && f(*k)).collect::<Vec<_>>();
    let shaded = |k: ChannelKind| subcraft_render::SHADED_CHANNELS.contains(&k);
    [
        ("Supported channels", missing(&|k| !k.is_user() && shaded(k))),
        ("Unsupported channels", missing(&|k| !k.is_user() && !shaded(k))),
        ("User channels", missing(&|k| k.is_user())),
    ]
}

/// Labels of the mixing dropdowns, in Painter's order.
pub const MIXING_LABELS: [&str; 4] = ["Normal mixing", "Height to normal method", "Ambient occlusion mixing", "UV padding"];

/// The four mixing dropdowns under the channel rows. Returns the settings with the user's pick.
fn mixing_rows(ui: &mut Ui, current: MixingSettings) -> MixingSettings {
    fn row<T: Copy + PartialEq>(ui: &mut Ui, label: &str, value: &mut T, all: &[T], name: fn(T) -> &'static str, tip: &str) {
        // Label above the dropdown: the labels are long and the panel can be 300 px narrow.
        ui.add(egui::Label::new(RichText::new(label).font(theme::font(11.5)).color(TEXT_DIM)).selectable(false).truncate());
        egui::ComboBox::from_id_salt(("tss_mixing", label)).width((ui.available_width() - 28.0).max(60.0)).truncate().selected_text(name(*value)).show_ui(ui, |ui| {
            for &v in all { ui.selectable_value(value, v, name(v)); }
        }).response.on_hover_text(tip);
    }
    let mut m = current;
    row(ui, MIXING_LABELS[0], &mut m.normal, &NormalMixing::ALL, NormalMixing::label,
        "Combine adds the detail of the Height channel to the Normal channel; Replace uses the Normal channel alone");
    row(ui, MIXING_LABELS[1], &mut m.height_to_normal, &HeightToNormal::ALL, HeightToNormal::label,
        "How height becomes normals: Smooth looks at the 3x3 neighbourhood, Sharp only at the direct neighbours");
    row(ui, MIXING_LABELS[2], &mut m.ao, &AoMixing::ALL, AoMixing::label,
        "Multiply darkens the painted Ambient Occlusion channel with the baked map; Replace uses the channel alone");
    row(ui, MIXING_LABELS[3], &mut m.uv_padding, &UvPadding::ALL, UvPadding::label,
        "How texels outside the UV islands are filled at export. SubCraft pads from 2D neighbours for both choices for now");
    m
}

fn mesh_maps(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>, now: f64) {
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
    // Every slot is always listed, as in Painter: an empty one is a place to put an image.
    let mut dropped: Option<(MeshMapKind, std::path::PathBuf)> = None;
    for (kind, title, what) in MESH_MAP_SLOTS {
        let thumb = map_thumbnail(state, ui.ctx(), id, kind);
        let desc = if thumb.is_some() { format!("{what} {set_name}") } else { empty_slot_label(title) };
        let (row, action) = map_slot(ui, thumb.as_ref(), title, &desc);
        match action {
            Some(SlotAction::Pick) => {
                let purpose = crate::files::Purpose::MeshMapSlot { texture_set: id, kind };
                crate::files::pick(ui.ctx(), purpose, crate::files::IMAGES, false, Some("Choose an image for this mesh map"));
            }
            Some(SlotAction::Clear) => cmds.push(Command::ClearMeshMap { texture_set: id, kind }),
            Some(SlotAction::Bake) => {
                state.bake.open = true;
                if !state.bake.settings.maps.contains(&kind) { state.bake.settings.maps.push(kind); }
            }
            None => {}
        }
        // An image dragged from Assets, or a file from the system file browser, lands in the slot.
        use crate::panels::shelf::ShelfDrag;
        let over = ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| row.rect.contains(p));
        let file_over = over && ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let texture_over = row.dnd_hover_payload::<ShelfDrag>().is_some_and(|d| matches!(*d, ShelfDrag::Texture(_)));
        if file_over || texture_over {
            ui.painter().rect_stroke(row.rect, 2.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        }
        if over && let Some(path) = ui.ctx().input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())).filter(|p| !p.as_os_str().is_empty()) {
            state.drops_taken = true;
            dropped = Some((kind, path));
        }
        if let Some(d) = row.dnd_release_payload::<ShelfDrag>()
            && let ShelfDrag::Texture(i) = *d
            && let Some(path) = state.textures.items.get(i).map(|t| t.path.clone())
        {
            dropped = Some((kind, path));
        }
    }
    if let Some((kind, path)) = dropped {
        set_mesh_map_from_file(state, id, kind, &path, now);
    }
}

/// What the user asked of a mesh-map slot this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotAction {
    /// Choose an image file for the slot (also replaces the map of a filled slot).
    Pick,
    /// Remove the map.
    Clear,
    /// Open the bake window with this map ticked.
    Bake,
}

/// One mesh-map slot: thumbnail, slot name, and the map's name or the "Select ... map" prompt.
/// Click picks an image, the cross (filled slots) removes the map, right-click opens the menu.
fn map_slot(ui: &mut Ui, thumb: Option<&egui::TextureHandle>, title: &str, desc: &str) -> (egui::Response, Option<SlotAction>) {
    let filled = thumb.is_some();
    let (rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, theme::BG_2);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, if row.hovered() { TEXT_FAINT } else { theme::BORDER }), egui::StrokeKind::Inside);
    let img = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(38.0, 38.0));
    p.rect_filled(img, 0.0, theme::FIELD);
    if let Some(t) = thumb { p.image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE); }
    p.text(egui::pos2(img.right() + 8.0, rect.top() + 14.0), egui::Align2::LEFT_CENTER, title, theme::font_strong(12.0), TEXT);
    let desc_color = if filled { TEXT_DIM } else { TEXT_FAINT };
    let mut job = egui::text::LayoutJob::simple_singleline(desc.to_string(), theme::font(11.0), desc_color);
    job.wrap.max_width = rect.right() - img.right() - 40.0; job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    p.galley(egui::pos2(img.right() + 8.0, rect.top() + 24.0), galley, desc_color);
    let mut action = None;
    let mut over_cross = false;
    if filled {
        let x = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, rect.center().y), egui::vec2(20.0, 20.0));
        let r = ui.interact(x, ui.id().with(("clear_map", title)), egui::Sense::click());
        ui.painter().text(x.center(), egui::Align2::CENTER_CENTER, icons::X, theme::font(12.0), if r.hovered() { TEXT } else { TEXT_DIM });
        over_cross = r.hovered();
        if r.on_hover_text("Remove the map from this slot").clicked() { action = Some(SlotAction::Clear); }
    }
    if action.is_none() && !over_cross && row.clicked() { action = Some(SlotAction::Pick); }
    row.context_menu(|ui| {
        if ui.button(if filled { "Replace image…" } else { "Select image…" }).clicked() { action = Some(SlotAction::Pick); ui.close(); }
        if ui.button("Bake this map…").clicked() { action = Some(SlotAction::Bake); ui.close(); }
        if ui.add_enabled(filled, egui::Button::new("Remove")).clicked() { action = Some(SlotAction::Clear); ui.close(); }
    });
    let row = if over_cross { row } else {
        row.on_hover_text(if filled { "Click to replace the image, or drop one here · right-click for more" } else { "Click to choose an image, or drop one here · right-click for more" })
    };
    ui.add_space(2.0);
    (row, action)
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
        assert!(!t.iter().any(|s| s.starts_with("Normal Map from Mesh")), "an empty slot does not name a map");
        assert!(t.iter().any(|s| s == "Select normal map"), "the empty slots are still listed: {t:?}");
        let tex = map_thumbnail(&state, &ctx, ts, MeshMapKind::Curvature).unwrap();
        assert_eq!(tex.size(), [THUMB, THUMB]);
        state.run(vec![Command::ClearMeshMap { texture_set: ts, kind: MeshMapKind::Curvature }], 0.0);
        assert!(state.session.mesh_maps(ts).is_none());
    }

    #[test]
    fn every_mesh_map_slot_is_listed_before_anything_is_baked() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, settings); }
        let t = texts(&frame(&mut state, &ctx, settings));
        assert_eq!(MESH_MAP_SLOTS.map(|s| s.0), MeshMapKind::ALL, "one slot per map kind");
        for (_, title, _) in MESH_MAP_SLOTS {
            assert!(t.iter().any(|s| s == title), "missing slot {title}: {t:?}");
            assert!(t.iter().any(|s| *s == empty_slot_label(title)), "missing prompt for {title}: {t:?}");
        }
        assert_eq!(empty_slot_label("ID"), "Select ID map");
        assert_eq!(empty_slot_label("Bent normals"), "Select bent normals map");
    }

    #[test]
    fn an_image_file_fills_one_slot_replaces_it_and_keeps_the_other_slots() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ts = state.session.selection().texture_set;
        let mut maps = std::collections::BTreeMap::new();
        maps.insert(MeshMapKind::Curvature, subcraft_image::TiledImage::new(64, 64, 1, [0.7; 4]));
        state.session.set_mesh_maps(ts, maps);
        let dir = std::env::temp_dir().join(format!("subcraft-mesh-map-slot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, grey: f32| {
            let path = dir.join(name);
            subcraft_image::save_image(&path, 16, 16, 1, &[grey; 256], subcraft_image::FileFormat::Png8).unwrap();
            path
        };
        let ao = |state: &UiState| state.session.mesh_maps(ts).and_then(|m| m.get(MeshMapKind::AmbientOcclusion)).map(|img| (img.width(), img.get(32, 32)[0]));
        assert_eq!(ao(&state), None);

        set_mesh_map_from_file(&mut state, ts, MeshMapKind::AmbientOcclusion, &write("first.png", 0.25), 0.0);
        let (w, v) = ao(&state).expect("the slot is filled");
        assert_eq!(w, 64, "resampled to the texture set's size");
        assert!((v - 0.25).abs() < 0.01, "{v}");
        assert!(state.session.mesh_maps(ts).unwrap().get(MeshMapKind::Curvature).is_some(), "the other slots keep their maps");

        // The same slot again: the map is replaced.
        set_mesh_map_from_file(&mut state, ts, MeshMapKind::AmbientOcclusion, &write("second.png", 0.75), 0.0);
        let (_, v) = ao(&state).unwrap();
        assert!((v - 0.75).abs() < 0.01, "{v}");

        // A file that cannot be read leaves the slot as it was.
        set_mesh_map_from_file(&mut state, ts, MeshMapKind::AmbientOcclusion, &dir.join("missing.png"), 0.0);
        let (_, v) = ao(&state).unwrap();
        assert!((v - 0.75).abs() < 0.01, "{v}");

        // The picker's answer goes through the same path.
        crate::files::handle_for_test(&mut state, crate::files::Purpose::MeshMapSlot { texture_set: ts, kind: MeshMapKind::Thickness }, &[write("thick.png", 0.5)]);
        assert!(state.session.mesh_maps(ts).unwrap().get(MeshMapKind::Thickness).is_some());
        let _ = std::fs::remove_dir_all(&dir);
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

    fn topmost(out: &egui::FullOutput, want: &str) -> Option<egui::Pos2> {
        out.shapes.iter().filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == want => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).min_by(|a, b| a.y.total_cmp(&b.y))
    }

    #[test]
    fn header_offers_visibility_focus_and_description_modes() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ts = state.session.selection().texture_set;
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        let out = frame(&mut state, &ctx, list);
        assert!(!texts(&out).iter().any(|s| s == "No description"), "empty descriptions are hidden by default");

        // Visibility menu: the header eye is above the row eye.
        let eye = topmost(&out, icons::EYE).expect("header eye");
        click_frame(&mut state, &ctx, eye);
        let out = frame(&mut state, &ctx, list);
        let labels: Vec<_> = VISIBILITY_MENU.iter().map(|(_, l)| text_pos(&out, l).unwrap_or_else(|| panic!("menu lists {l}"))).collect();
        assert!(labels[0].y < labels[1].y && labels[1].y < labels[2].y);
        click_frame(&mut state, &ctx, labels[1]);
        assert!(state.session.is_set_hidden(ts), "Hide All hides the set");
        let out = frame(&mut state, &ctx, list);
        assert_eq!(topmost(&out, icons::EYE), Some(eye), "the header eye stays put");
        click_frame(&mut state, &ctx, eye);
        let out = frame(&mut state, &ctx, list);
        click_frame(&mut state, &ctx, text_pos(&out, "Show All").expect("menu reopens"));
        assert!(!state.session.is_set_hidden(ts), "Show All shows it again");

        // Focus mode toggles.
        let out = frame(&mut state, &ctx, list);
        let focus = text_pos(&out, icons::CROSSHAIR).expect("focus button");
        click_frame(&mut state, &ctx, focus);
        assert!(state.session.texture_set_focus());
        click_frame(&mut state, &ctx, focus);
        assert!(!state.session.texture_set_focus());

        // Settings menu: the three description modes.
        let out = frame(&mut state, &ctx, list);
        click_frame(&mut state, &ctx, text_pos(&out, icons::GEAR).expect("settings button"));
        let out = frame(&mut state, &ctx, list);
        for mode in DescriptionDisplay::ALL { assert!(text_pos(&out, mode.label()).is_some(), "lists {}", mode.label()); }
        click_frame(&mut state, &ctx, text_pos(&out, "Show All Descriptions").unwrap());
        assert_eq!(state.set_descriptions, DescriptionDisplay::ShowAll);
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        assert!(texts(&frame(&mut state, &ctx, list)).iter().any(|s| s == "No description"), "Show All lists empty descriptions");

        state.run(vec![Command::SetTextureSetDescription { texture_set: ts, description: "Hull plates".into() }], 0.0);
        state.set_descriptions = DescriptionDisplay::HideEmpty;
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        assert!(texts(&frame(&mut state, &ctx, list)).iter().any(|s| s == "Hull plates"));
        state.set_descriptions = DescriptionDisplay::HideAll;
        for _ in 0..2 { frame(&mut state, &ctx, list); }
        assert!(!texts(&frame(&mut state, &ctx, list)).iter().any(|s| s == "Hull plates"));
    }

    #[test]
    fn mixing_dropdowns_are_listed_and_a_pick_is_one_undo_step() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ctx = egui::Context::default(); theme::apply(&ctx);
        for _ in 0..2 { frame(&mut state, &ctx, settings); }
        let t = texts(&frame(&mut state, &ctx, settings));
        for want in MIXING_LABELS.into_iter().chain(["Combine", "Smooth (Sobel)", "Multiply", "3D Space Neighbor"]) {
            assert!(t.iter().any(|s| s == want), "missing {want}: {t:?}");
        }
        let ts = state.session.selection().texture_set;
        let mut m = state.session.selected_texture_set().mixing;
        assert_eq!(m, MixingSettings::default(), "new sets start with Painter's defaults");
        m.normal = NormalMixing::Replace;
        state.run(vec![Command::SetMixing { texture_set: ts, mixing: m }], 0.0);
        assert_eq!(state.session.selected_texture_set().mixing.normal, NormalMixing::Replace);
        state.session.perform(subcraft_app::Action::Undo).unwrap();
        assert_eq!(state.session.selected_texture_set().mixing, MixingSettings::default());
    }

    #[test]
    fn add_menu_is_grouped_and_leaves_out_present_channels() {
        let groups = add_menu_groups(&ChannelKind::METAL_ROUGH);
        assert_eq!(groups.iter().map(|(t, _)| *t).collect::<Vec<_>>(), ["Supported channels", "Unsupported channels", "User channels"]);
        assert!(groups[0].1.contains(&ChannelKind::AmbientOcclusion) && !groups[0].1.contains(&ChannelKind::BaseColor));
        assert!(groups[1].1.contains(&ChannelKind::Specular));
        assert_eq!(groups[2].1.len(), 16);
        let all: Vec<_> = groups.iter().flat_map(|(_, k)| k.clone()).collect();
        assert_eq!(all.len() + ChannelKind::METAL_ROUGH.len(), ChannelKind::ALL.len(), "every missing channel is offered once");
    }

    #[test]
    fn channel_rows_remove_and_change_format_through_commands() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ts = state.session.selection().texture_set;
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 1400.0)));
        let run = |state: &mut UiState, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: screen, events, ..Default::default() }, |ui| settings(state, ui));
            out.textures_delta.clear();
            out
        };
        let click = |state: &mut UiState, at: egui::Pos2| {
            let b = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            for e in [vec![egui::Event::PointerMoved(at)], vec![b(true)], vec![b(false)]] { run(state, e); }
        };
        let row_y = |out: &egui::FullOutput, name: &str| text_pos(out, name).map(|p| p.y);
        for _ in 0..2 { run(&mut state, vec![]); }
        let out = run(&mut state, vec![]);
        // The gear and the cross of the Metallic row.
        let y = row_y(&out, "Metallic").expect("Metallic row");
        let on_row = |icon: &str| out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == icon && (t.pos.y + t.galley.size().y * 0.5 - y).abs() < 6.0 => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        });
        let (gear, cross) = (on_row(icons::GEAR).expect("gear on the row"), on_row(icons::X).expect("cross on the row"));
        click(&mut state, gear);
        let out = run(&mut state, vec![]);
        // L32F: no row shows that badge, so the only match is the menu entry.
        let pick = text_pos(&out, "L32F").expect("format menu lists L32F");
        click(&mut state, pick);
        let def = state.session.selected_texture_set().channels.iter().find(|c| c.kind == ChannelKind::Metallic).copied().unwrap();
        assert_eq!(def.format, ChannelFormat::Float32);
        click(&mut state, cross);
        assert!(!state.session.selected_texture_set().has_channel(ChannelKind::Metallic), "the cross removes the channel");
        state.session.perform(subcraft_app::Action::Undo).unwrap();
        assert!(state.session.selected_texture_set().has_channel(ChannelKind::Metallic));
        let _ = ts;
    }

    #[test]
    fn description_modes_decide_which_rows_show_a_line() {
        assert!(!DescriptionDisplay::HideEmpty.shows("  "));
        assert!(DescriptionDisplay::HideEmpty.shows("Glass"));
        assert!(!DescriptionDisplay::HideAll.shows("Glass"));
        assert!(DescriptionDisplay::ShowAll.shows(""));
        assert_eq!(DescriptionDisplay::default(), DescriptionDisplay::HideEmpty);
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
