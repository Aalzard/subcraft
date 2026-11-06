//! Text tool panel: type text, pick a font, then add it as an editable decal layer, use it as the
//! brush tip, or stamp it projected on the model. All logic lives in `subcraft_app::text`.

use egui::Ui;
use glam::Vec2;
use subcraft_core::LayerId;
use subcraft_doc::TextDecal;
use subcraft_text::{Align, BUNDLED_FONTS, Direction, FontRef, SystemFontInfo};

use crate::state::UiState;
use crate::widgets::{Edit, caption, channel_value, hint, primary_button, prop_row, section, segmented, slider_row};

/// What the next click on the model does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Armed {
    /// Move the selected text layer to the clicked surface point.
    PlaceDecal,
    /// Stamp the draft text, projected from the camera, onto the selected paint layer.
    StampProjected,
}

/// Which text window is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// Compose new text, then add it as a layer and place it.
    New,
    /// Edit the selected text layer (double-click it in the Layers panel).
    Edit,
}

pub struct TextUi {
    /// Text window (opened from the Layers panel, Layer menu, Ctrl+Shift+T, or a double-click).
    pub window: Option<Window>,
    /// Draft of the New window (kept separate from the Brush panel's draft).
    compose: Option<TextDecal>,
    /// Text being composed (before it becomes a layer).
    pub draft: Option<TextDecal>,
    /// Working copy of the selected text layer; written back when an edit is finished.
    pub edit: Option<TextDecal>,
    edit_base: Option<TextDecal>,
    edit_for: Option<LayerId>,
    pub armed: Option<Armed>,
    /// Installed fonts, scanned on demand (scanning can take a moment).
    pub system: Option<Vec<SystemFontInfo>>,
    pub filter: String,
    /// Projected stamp size (percent of the model diagonal) and rotation.
    pub stamp_size: f32,
    pub stamp_angle: f32,
}

impl Default for TextUi {
    fn default() -> Self {
        Self { window: None, compose: None, draft: None, edit: None, edit_base: None, edit_for: None, armed: None, system: None, filter: String::new(), stamp_size: 25.0, stamp_angle: 0.0 }
    }
}

impl TextUi {
    /// Open the "new text" window with a fresh, empty draft.
    pub fn open_new(&mut self) {
        self.window = Some(Window::New);
        self.compose = None;
    }
    /// Open the editor for the selected text layer.
    pub fn open_edit(&mut self) {
        self.window = Some(Window::Edit);
    }
}

fn font_label(font: &FontRef, embedded: &[(subcraft_core::ResourceId, String)]) -> String {
    match font {
        FontRef::Bundled { name } => name.clone(),
        FontRef::System { family, bold, italic } => format!("{family}{}{}", if *bold { " Bold" } else { "" }, if *italic { " Italic" } else { "" }),
        FontRef::Embedded { resource } => embedded.iter().find(|(id, _)| id == resource).map_or_else(|| "Missing font".into(), |(_, n)| n.clone()),
    }
}

/// Controls shared by the draft and the selected layer. Returns `(changed, finished)`.
fn controls(ui: &mut Ui, d: &mut TextDecal, text_ui: &mut TextUi, embedded: &[(subcraft_core::ResourceId, String)], placed: bool, focus: bool) -> Edit {
    let mut edit = Edit::default();
    let r = ui.add(egui::TextEdit::multiline(&mut d.style.text).desired_rows(2).desired_width(f32::INFINITY).hint_text("Type text…"));
    if focus { r.request_focus(); }
    edit = edit.or(Edit::from(&r));

    prop_row(ui, "Font", |ui| {
        egui::ComboBox::from_id_salt("text_font").selected_text(font_label(&d.font, embedded)).show_ui(ui, |ui| {
            ui.label(caption("Bundled"));
            for (name, _) in BUNDLED_FONTS {
                let f = FontRef::Bundled { name: (*name).into() };
                if ui.selectable_label(d.font == f, *name).clicked() { d.font = f; edit = edit.or(Edit { changed: true, done: true }); }
            }
            if !embedded.is_empty() {
                ui.separator();
                for (id, name) in embedded {
                    let f = FontRef::Embedded { resource: *id };
                    if ui.selectable_label(d.font == f, name).clicked() { d.font = f; edit = edit.or(Edit { changed: true, done: true }); }
                }
            }
            ui.separator();
            if let Some(list) = &text_ui.system {
                ui.add(egui::TextEdit::singleline(&mut text_ui.filter).hint_text("Search installed fonts"));
                let needle = text_ui.filter.to_lowercase();
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for info in list.iter().filter(|i| needle.is_empty() || i.family.to_lowercase().contains(&needle)).take(400) {
                        let f = FontRef::System { family: info.family.clone(), bold: info.bold, italic: info.italic };
                        let label = font_label(&f, embedded);
                        if ui.selectable_label(d.font == f, label).clicked() { d.font = f; edit = edit.or(Edit { changed: true, done: true }); }
                    }
                });
            } else if ui.button("Load installed fonts…").clicked() {
                text_ui.system = Some(subcraft_app::text::font_library().system_fonts());
            }
        });
    });
    prop_row(ui, "", |ui| {
        if ui.button(format!("{}  Choose font file…", crate::icons::IMAGE)).on_hover_text("A .ttf / .otf file; it is embedded in the project").clicked() {
            crate::files::pick(ui.ctx(), crate::files::Purpose::Font, crate::files::FONTS, false, None);
        }
    });

    let mut size = d.size * 100.0;
    let r = slider_row(ui, "Size", egui::Slider::new(&mut size, TextDecal::MIN_SIZE * 100.0..=60.0).logarithmic(true).suffix("% width"));
    if r.changed() { d.size = size / 100.0; }
    edit = edit.or(Edit::from(&r));
    let r = slider_row(ui, "Letter spacing", egui::Slider::new(&mut d.style.letter_spacing, -0.2..=1.0).suffix(" em"));
    edit = edit.or(Edit::from(&r));
    let r = slider_row(ui, "Line spacing", egui::Slider::new(&mut d.style.line_spacing, 0.5..=3.0));
    edit = edit.or(Edit::from(&r));
    prop_row(ui, "Align", |ui| {
        if segmented(ui, &mut d.style.align, &[(Align::Left, "Left"), (Align::Center, "Center"), (Align::Right, "Right")]) { edit = edit.or(Edit { changed: true, done: true }); }
    });
    prop_row(ui, "Direction", |ui| {
        if segmented(ui, &mut d.style.direction, &[(Direction::Auto, "Auto"), (Direction::Ltr, "LTR"), (Direction::Rtl, "RTL")]) { edit = edit.or(Edit { changed: true, done: true }); }
    });
    if placed {
        let mut deg = d.rotation.to_degrees();
        let r = slider_row(ui, "Rotation", egui::Slider::new(&mut deg, -180.0..=180.0).suffix("°"));
        if r.changed() { d.rotation = deg.to_radians(); }
        edit = edit.or(Edit::from(&r));
        let mut c = d.center;
        let rx = slider_row(ui, "Position X", egui::Slider::new(&mut c.x, 0.0..=1.0));
        let ry = slider_row(ui, "Position Y", egui::Slider::new(&mut c.y, 0.0..=1.0));
        d.center = c;
        edit = edit.or(Edit::from(&rx)).or(Edit::from(&ry));
    }
    let r = slider_row(ui, "Opacity", egui::Slider::new(&mut d.opacity, 0.0..=1.0));
    edit = edit.or(Edit::from(&r));
    for (kind, value) in d.values.iter_mut() {
        prop_row(ui, kind.display_name(), |ui| { edit = edit.or(channel_value(ui, *kind, value)); });
    }
    edit
}

/// Keep the working copy of the selected text layer in step with the document.
fn sync_edit(state: &mut UiState) {
    let selected = state.session.selected_text().cloned();
    let layer_id = state.session.selected_layer().map(|l| l.id);
    match (&selected, layer_id) {
        (Some(sel), Some(id)) => {
            if state.text.edit_for != Some(id) || state.text.edit_base.as_ref() != Some(sel) {
                state.text.edit = Some(sel.clone());
                state.text.edit_base = Some(sel.clone());
                state.text.edit_for = Some(id);
            }
        }
        _ => {
            state.text.edit = None;
            state.text.edit_base = None;
            state.text.edit_for = None;
        }
    }
}

/// Controls of the selected text layer; edits are written back when finished. Returns false when
/// no text layer is selected.
fn edit_body(state: &mut UiState, ui: &mut Ui, now: f64, embedded: &[(subcraft_core::ResourceId, String)]) -> bool {
    let Some(mut d) = state.text.edit.take() else { return false };
    ui.label(caption("Editing the selected text layer"));
    let e = controls(ui, &mut d, &mut state.text, embedded, true, false);
    if e.done && state.text.edit_base.as_ref() != Some(&d) {
        if let Err(err) = state.session.update_selected_text(d.clone()) { state.set_status(format!("Text: {err}"), now); }
        state.gpu_dirty = true;
    }
    state.text.edit = Some(d);
    if ui.button("Place by clicking the model").clicked() { state.text.armed = Some(Armed::PlaceDecal); }
    hint(ui, "Changes apply when you release a slider or leave the text box. Undo (Ctrl+Z) steps back one edit.");
    true
}

/// Add `d` as a text layer; `place` then arms "click the model to place it".
fn add_layer(state: &mut UiState, d: &TextDecal, place: bool, now: f64) -> bool {
    let mut at = d.clone();
    at.center = Vec2::splat(0.5);
    match state.session.add_text_layer(at) {
        Ok(()) => {
            state.gpu_dirty = true;
            if place {
                state.text.armed = Some(Armed::PlaceDecal);
                state.set_status("Text layer added: click the model to place it (Esc keeps it centered). Double-click the layer to edit it later", now);
            } else {
                state.set_status("Text layer added: double-click it in Layers to edit, or use Place by clicking the model", now);
            }
            true
        }
        Err(e) => { state.set_status(format!("Text: {e}"), now); false }
    }
}

/// Width of the floating text window in points (the dialog can never stretch past it).
pub const WINDOW_WIDTH: f32 = 460.0;

/// The floating text window: "new text" (type, font, size, colour, alignment, then place) or the
/// editor of the selected text layer.
pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(mode) = state.text.window else { return };
    sync_edit(state);
    if mode == Window::Edit && state.text.edit.is_none() { state.text.window = None; return; }
    let embedded = state.session.embedded_fonts();
    let mut open = true;
    let mut close = false;
    let title = if mode == Window::New { "New text" } else { "Edit text" };
    let screen = ctx.content_rect();
    let width = WINDOW_WIDTH.min((screen.width() - 32.0).max(280.0));
    egui::Window::new(title).id(egui::Id::new("text_window")).open(&mut open).collapsible(false).resizable(false)
        .default_width(width).max_width(width)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0))
        .show(ctx, |ui| {
        // Fixed content width: sliders and the text box fill the dialog, never the screen.
        ui.set_width(width - 24.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        match mode {
            Window::New => {
                let first = state.text.compose.is_none();
                let mut d = state.text.compose.take().unwrap_or_else(|| state.session.new_text_decal(""));
                controls(ui, &mut d, &mut state.text, &embedded, false, first);
                ui.add_space(4.0);
                let ready = !d.style.text.trim().is_empty();
                ui.horizontal(|ui| {
                    let place = ui.add_enabled(ready, egui::Button::new(egui::RichText::new("Add and place on model").strong())).on_hover_text("Adds the text layer, then click the model to put it where you want it");
                    if place.clicked() && add_layer(state, &d, true, now) { close = true; }
                    if ui.add_enabled(ready, egui::Button::new("Add centered")).on_hover_text("Adds the text layer in the middle of the texture; move it later with Position X/Y").clicked() && add_layer(state, &d, false, now) { close = true; }
                    if ui.button("Cancel").clicked() { close = true; }
                });
                hint(ui, "Colour comes from the Base Color row below (the brush colour by default). Double-click a text layer in Layers to edit it later.");
                state.text.compose = Some(d);
            }
            Window::Edit => { edit_body(state, ui, now, &embedded); }
        }
    });
    if !open || close { state.text.window = None; state.text.compose = None; }
}

pub fn show(state: &mut UiState, ui: &mut Ui, now: f64) {
    let embedded = state.session.embedded_fonts();
    sync_edit(state);

    section(ui, "text", "Text", false, |ui| {
        if !edit_body(state, ui, now, &embedded) {
            let mut d = state.text.draft.take().unwrap_or_else(|| state.session.new_text_decal("Text"));
            // New text uses the brush colour, so a changed brush material is picked up.
            let e = controls(ui, &mut d, &mut state.text, &embedded, false, false);
            let _ = e;
            ui.add_space(4.0);
            if primary_button(ui, "Add text layer (decal)").on_hover_text("An editable layer you can move, rotate and re-render at any resolution").clicked() {
                add_layer(state, &d, true, now);
            }
            ui.horizontal(|ui| {
                if ui.button("Use as brush tip").on_hover_text("Strokes then paint the text along the path").clicked() {
                    match state.session.set_text_brush_tip(&d.font, &d.style) {
                        Ok(()) => state.set_status("Text is now the brush tip (Brush panel › tip shows its name)", now),
                        Err(e) => state.set_status(format!("Text: {e}"), now),
                    }
                }
                if ui.button("Stamp on model…").on_hover_text("Click the model to project the text from the camera onto the selected paint layer").clicked() {
                    state.text.armed = Some(Armed::StampProjected);
                }
            });
            let mut t = state.text.stamp_size;
            let mut a = state.text.stamp_angle;
            slider_row(ui, "Stamp size", egui::Slider::new(&mut t, 1.0..=100.0).logarithmic(true).suffix("%"));
            slider_row(ui, "Stamp angle", egui::Slider::new(&mut a, -180.0..=180.0).suffix("°"));
            state.text.stamp_size = t;
            state.text.stamp_angle = a;
            hint(ui, "Decal layers live in UV space (they do not cross UV seams); a projected stamp is painted pixels and does.");
            state.text.draft = Some(d);
        }
        if let Some(armed) = state.text.armed {
            ui.label(caption(match armed { Armed::PlaceDecal => "Click the model to place the text · Esc cancels", Armed::StampProjected => "Click the model to stamp the text · Esc cancels" }));
        }
    });
}
