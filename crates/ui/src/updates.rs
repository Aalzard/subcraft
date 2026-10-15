//! Update controls only dispatch to app; no package/project logic in widgets.
use crate::state::UiState;
#[cfg(not(target_arch = "wasm32"))]
use subcraft_app::updater::Phase;

pub fn can_restart(state: &UiState) -> bool {
    state.bake.job.is_none() && !state.pen.is_editing() && !state.pen.is_pending()
}

pub fn button(state: &mut UiState, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(updater) = &mut state.updater {
        let text = match &updater.phase { Phase::Checking => "Checking…", Phase::Downloading { .. } => "Downloading…", Phase::Preparing | Phase::Exiting => "Restarting…", Phase::Available(_) | Phase::Prepared { .. } => "Update", _ => "Check / update" };
        let response = ui.add_enabled(!updater.busy(), egui::Button::new(egui::RichText::new(format!("{}  {text}", crate::icons::DOWNLOAD)).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(35, 128, 67)));
        if response.on_hover_text("Check the latest release, install it and restart. Your current project is preserved.").clicked() {
            updater.open = true; updater.auto_install = true;
            if !matches!(updater.phase, Phase::Available(_) | Phase::Prepared { .. }) { updater.request_update(); }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (state, ui); // The web app is deployed by GitHub Pages, not a desktop installer.
}
pub fn tick(state: &mut UiState, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let can_restart = can_restart(state);
        let Some(updater) = &mut state.updater else { return }; updater.poll();
        if updater.auto_install {
            match updater.phase.clone() {
                Phase::Available(release) => updater.download(release),
                Phase::Prepared { .. } if can_restart => { updater.install(&mut state.session); state.painting = false; },
                _ => {},
            }
        }
        if matches!(updater.phase, Phase::Exiting) { state.allow_close = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        if updater.busy() || updater.auto_install && matches!(updater.phase, Phase::Prepared { .. }) { ctx.request_repaint_after(std::time::Duration::from_millis(100)); }
        if !updater.open { return; }
        let mut open = true;
        let modal = matches!(updater.phase, Phase::Preparing | Phase::Exiting);
        let contents = |ui: &mut egui::Ui| {
            ui.set_width(440.0); ui.label(format!("Installed version: {}", env!("CARGO_PKG_VERSION"))); ui.add_space(8.0);
            match &updater.phase {
                Phase::Idle => { ui.label("Use the green button to check and update."); },
                Phase::Checking => { ui.spinner(); ui.label("Checking GitHub releases…"); },
                Phase::Current => { ui.label("You're up to date."); },
                Phase::Available(release) => { ui.heading(format!("Version {} is available", release.version)); egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| { ui.label(&release.notes); }); if ui.button("Update and restart").clicked() { updater.auto_install = true; } },
                Phase::Downloading { bytes, total } => { ui.label("Downloading update… You can keep working."); ui.add(egui::ProgressBar::new(*bytes as f32 / (*total).max(1) as f32).text(format!("{:.1} / {:.1} MiB", *bytes as f32 / 1048576.0, *total as f32 / 1048576.0))); if ui.button("Skip installation").clicked() { updater.auto_install = false; } },
                Phase::Prepared { .. } => { ui.label(if !can_restart { "Waiting for your current work to finish…" } else { "Download verified. Ready to update and restart." }); if ui.button("Update and restart").clicked() { updater.auto_install = true; } },
                Phase::Preparing => { ui.spinner(); ui.label("Preserving your project and preparing the restart…"); },
                Phase::Exiting => { ui.label("Restarting SubCraft…"); },
                Phase::Error(error) => { ui.label(egui::RichText::new(error).color(crate::theme::TEXT)); if ui.button("Try again").clicked() { updater.check(); } },
            }
        };
        if modal { egui::Modal::new(egui::Id::new("update-restart")).show(ctx, contents); }
        else { egui::Window::new("SubCraft updates").open(&mut open).collapsible(false).resizable(false).show(ctx, contents); }
        updater.open &= open;
        if !open { updater.auto_install = false; }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (state, ctx);
}
