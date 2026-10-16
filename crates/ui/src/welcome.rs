//! Welcome hub and six-step guide. File/project operations use existing app/dialog controllers.
use std::path::PathBuf;
use egui::RichText;
use subcraft_app::launch::{GuideAction, RecentFiles, RecentItem, QUICK_START};
use crate::{dialogs::Pending, state::UiState, theme, widgets::{primary_button, secondary_button}};

pub const RECENTS_KEY: &str = "subcraft_recent_v1";
#[derive(Default)]
pub struct WelcomeUi {
    pub open: bool, pub guide: Option<usize>, pub whats_new: bool, pub about: bool,
    pub recent: RecentFiles, pub items: Vec<RecentItem>, pub pending_recent: Option<PathBuf>,
}

impl WelcomeUi {
    pub fn refresh(&mut self) { self.items = self.recent.items(); }
    pub fn remember(&mut self, path: &std::path::Path) {
        if cfg!(target_arch = "wasm32") { return; }
        self.recent.remember(path); self.refresh();
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WelcomeAction { Pending(Pending), Guide(GuideAction) }

pub fn show(state: &mut UiState, ctx: &egui::Context) -> Option<WelcomeAction> {
    let mut action = None;
    if state.welcome.open && state.recovery.crashed.is_empty() && state.bug_report.crashes.is_empty() {
        let mut open = true;
        egui::Window::new("Welcome to SubCraft").open(&mut open).collapsible(false).resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(560.0_f32.min((ctx.content_rect().width() - 32.0).max(220.0)));
            ui.heading("Start texturing");
            ui.label(RichText::new("Open a model, build materials and export PBR textures.").color(theme::TEXT_DIM));
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                if primary_button(ui, "New from a model…").on_hover_text("OBJ, GLB/glTF or FBX with UVs").clicked() { action = Some(WelcomeAction::Pending(Pending::OpenMesh)); }
                if secondary_button(ui, "Open project…").on_hover_text("Open a saved .subcraft project").clicked() { action = Some(WelcomeAction::Pending(Pending::OpenProject)); }
                if secondary_button(ui, "Try the sample").on_hover_text("Open the built-in crate with UVs").clicked() { action = Some(WelcomeAction::Pending(Pending::NewSample)); }
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if secondary_button(ui, "Quick start").on_hover_text("Six steps from a model to exported textures").clicked() { state.welcome.open = false; state.welcome.guide = Some(0); }
                if secondary_button(ui, "What's new").on_hover_text("See the latest features in this build").clicked() { state.welcome.whats_new = true; }
                if secondary_button(ui, "Continue in workspace").clicked() { state.welcome.open = false; }
            });
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.add_space(14.0); ui.separator(); ui.label(RichText::new("Recent files").strong());
                let mut remove = None;
                if state.welcome.items.is_empty() { ui.label(RichText::new("Projects and models you open or save appear here.").small().color(theme::TEXT_FAINT)); }
                egui::ScrollArea::vertical().id_salt("welcome-recents").max_height(160.0).min_scrolled_height(0.0).show(ui, |ui| {
                    for item in &state.welcome.items {
                        ui.horizontal(|ui| {
                            let label = if item.available { item.name.clone() } else { format!("{} (missing)", item.name) };
                            if ui.add_enabled(item.available, egui::Button::new(label)).on_hover_text(item.path.display().to_string()).clicked() {
                                state.welcome.pending_recent = Some(item.path.clone()); action = Some(WelcomeAction::Pending(Pending::OpenRecent));
                            }
                            if ui.small_button("Remove").clicked() { remove = Some(item.path.clone()); }
                        });
                    }
                });
                if let Some(path) = remove { state.welcome.recent.remove(&path); state.welcome.refresh(); }
            }
            ui.add_space(14.0); ui.separator();
            ui.checkbox(&mut state.recovery.prefs.show_welcome, "Show this screen on startup");
            ui.label(RichText::new("You can reopen it from Help at any time.").small().color(theme::TEXT_FAINT));
            ui.add_space(8.0); ui.separator();
            ui.label(RichText::new(subcraft_app::launch::INDEPENDENCE_NOTICE).small().color(theme::TEXT_DIM));
        });
        state.welcome.open &= open;
    }
    if let Some(index) = state.welcome.guide {
        let index = index.min(QUICK_START.len() - 1); let step = &QUICK_START[index]; let mut open = true;
        egui::Window::new("Quick start").open(&mut open).collapsible(false).resizable(false)
            .default_width(320.0).default_pos(egui::pos2(950.0, 120.0)).show(ctx, |ui| {
            ui.label(RichText::new(format!("Step {} of {}", index + 1, QUICK_START.len())).small().color(theme::ACCENT));
            ui.heading(step.title); ui.add_space(6.0); ui.label(step.text); ui.add_space(10.0);
            if primary_button(ui, step.button).clicked() { action = Some(WelcomeAction::Guide(step.action)); }
            ui.add_space(10.0); ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(index > 0, egui::Button::new("Back")).clicked() { state.welcome.guide = Some(index - 1); }
                if secondary_button(ui, if index + 1 == QUICK_START.len() { "Finish" } else { "Next" }).clicked() {
                    state.welcome.guide = if index + 1 == QUICK_START.len() { None } else { Some(index + 1) };
                }
            });
        });
        if !open { state.welcome.guide = None; }
    }
    if state.welcome.about {
        egui::Window::new("About SubCraft").open(&mut state.welcome.about).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.set_width(440.0_f32.min((ctx.content_rect().width() - 48.0).max(220.0)));
            ui.heading(format!("SubCraft {}", env!("CARGO_PKG_VERSION")));
            ui.label("An open-source 3D texture painting application.");
            ui.add_space(12.0);
            ui.label(subcraft_app::launch::INDEPENDENCE_NOTICE);
            ui.add_space(12.0);
            ui.label(RichText::new("Licensed under MIT or Apache-2.0.").color(theme::TEXT_DIM));
            ui.hyperlink_to("SubCraft on GitHub", "https://github.com/Aalzard/subcraft");
        });
    }
    if state.welcome.whats_new {
        egui::Window::new("What's new").open(&mut state.welcome.whats_new).collapsible(false).default_width(420.0).show(ctx, |ui| {
            ui.heading(format!("SubCraft {}",env!("CARGO_PKG_VERSION")));
            for text in ["Import textures… turns a texture's maps into one material, ambient occlusion included.", "3D View toolbar: resolution, anti-aliasing, orthographic camera and single-channel views.", "Texture Sets → Resolution: paint at a lower size for speed, raise it before exporting.", "Blend mode and opacity right in each layer row.", "The brush keeps painting where it overlaps a model's edge; no more orange hover tint.", "SubCraft tells you when a new update is out (Edit → Preferences to turn it off).", "Projects saved in this version need SubCraft 0.1.5 or newer; older projects still open."] { ui.label(text); ui.add_space(6.0); }
            ui.hyperlink_to("Full changelog", "https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md");
        });
    }
    action
}
