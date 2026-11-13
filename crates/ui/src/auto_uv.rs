//! Mesh > Auto UV unwrap: generate a new UV layout for a model without UVs or with bad ones and
//! move the painted work onto it. The work is `subcraft_app::auto_uv` (background job).

use egui::RichText;
use subcraft_app::{AutoUvOutput, MeshReloadJob};
use subcraft_mesh::unwrap::{PackQuality, UnwrapSettings, UvReport};

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::{caption, hint, primary_button, secondary_button};

#[derive(Default)]
pub struct AutoUvUi {
    pub open: bool,
    pub settings: UnwrapSettings,
    pub job: Option<MeshReloadJob<AutoUvOutput>>,
    /// Quality of the current layout, measured when the window opens.
    pub current: Option<UvReport>,
    pub error: Option<String>,
    /// Scripted runs (`--auto-uv`): unwrap as soon as the window opens.
    pub auto: bool,
}

impl AutoUvUi {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

pub fn open(state: &mut UiState) {
    let a = &mut state.auto_uv;
    if !a.open {
        a.open = true;
        a.error = None;
        a.current = Some(state.session.uv_report());
    }
    if state.auto_uv.auto {
        state.auto_uv.auto = false;
        start(state);
    }
}

fn start(state: &mut UiState) {
    if state.session.is_stroking() {
        state.auto_uv.error = Some("finish the stroke first".into());
        return;
    }
    state.auto_uv.error = None;
    state.auto_uv.job = Some(state.session.start_auto_uv(state.auto_uv.settings));
}

/// Poll the background unwrap (call every frame).
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(job) = &mut state.auto_uv.job else { return };
    ctx.request_repaint();
    let Some(result) = job.poll() else { return };
    state.auto_uv.job = None;
    match result.and_then(|out| state.session.finish_auto_uv(out)) {
        Ok(summary) => {
            state.mesh_reloaded();
            state.set_status(summary, now);
            state.auto_uv.open = false;
        }
        Err(e) => state.auto_uv.error = Some(e),
    }
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    poll(state, ctx, now);
    if !state.auto_uv.open {
        return;
    }
    let mut open = true;
    let mut run = false;
    let mut cancel = false;
    egui::Window::new(RichText::new("Auto UV Unwrap").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
        let a = &mut state.auto_uv;
        let busy = a.busy();
        ui.label(RichText::new("Cut the model into islands, flatten them and pack them into 0..1. Painted layers move onto the new layout.").color(TEXT_DIM()));
        ui.add_space(6.0);
        if let Some(r) = &a.current {
            let text = if r.usable() { format!("Current UVs: usable ({})", r.summary()) } else { format!("Current UVs: {}", r.summary()) };
            ui.label(RichText::new(text).color(if r.usable() { TEXT_DIM() } else { theme::WARNING() }));
            ui.add_space(6.0);
        }
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("auto_uv_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(caption("Seam angle"));
                ui.add(egui::Slider::new(&mut a.settings.angle_threshold_deg, 20.0..=120.0).suffix("°").integer())
                    .on_hover_text("Largest bend inside one island. Smaller angles cut more seams and distort less.");
                ui.end_row();
                ui.label(caption("Island margin"));
                ui.add(egui::Slider::new(&mut a.settings.margin_texels, 0.0..=32.0).suffix(" px").integer())
                    .on_hover_text("Gap between islands, in texels of the texture size below.");
                ui.end_row();
                ui.label(caption("Texture size"));
                egui::ComboBox::from_id_salt("auto_uv_res").selected_text(a.settings.resolution.to_string()).show_ui(ui, |ui| {
                    for r in [256u32, 512, 1024, 2048, 4096] {
                        ui.selectable_value(&mut a.settings.resolution, r, r.to_string());
                    }
                });
                ui.end_row();
                ui.label(caption("Pack quality"));
                egui::ComboBox::from_id_salt("auto_uv_quality").selected_text(a.settings.quality.label()).show_ui(ui, |ui| {
                    for q in PackQuality::ALL {
                        ui.selectable_value(&mut a.settings.quality, q, q.label());
                    }
                });
                ui.end_row();
                ui.label(caption("Texel density"));
                ui.checkbox(&mut a.settings.balance_density, "Balance across islands");
                ui.end_row();
            });
        });
        ui.add_space(6.0);
        hint(ui, "This cannot be undone: the undo history is cleared and baked mesh maps are marked for rebaking. Save first to keep this version.");
        ui.add_space(8.0);
        if let Some(job) = &a.job {
            ui.add(egui::ProgressBar::new(job.progress().fraction()).text(job.progress().stage()));
            if secondary_button(ui, "Cancel").clicked() {
                cancel = true;
            }
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, "Unwrap").clicked() {
                    run = true;
                }
            });
        }
        if let Some(e) = &a.error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).color(theme::WARNING()));
        }
    });
    if cancel && let Some(job) = state.auto_uv.job.take() {
        job.cancel();
    }
    if run {
        start(state);
    }
    if !open {
        state.auto_uv.open = false;
        state.auto_uv.job = None;
    }
    let _ = now;
}
