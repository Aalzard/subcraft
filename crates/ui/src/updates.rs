//! Update controls only dispatch to app; no package/project logic in widgets.
use crate::state::UiState;
#[cfg(not(target_arch = "wasm32"))]
use subcraft_app::updater::Phase;

pub fn can_restart(state: &UiState) -> bool {
    state.bake.job.is_none() && !state.pen.is_editing() && !state.pen.is_pending() && !state.asset_preview.is_applying()
}

pub fn button(state: &mut UiState, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(updater) = &mut state.updater {
        let text = match &updater.phase { Phase::Checking => "Checking…", Phase::Downloading { .. } => "Downloading…", Phase::Preparing | Phase::Exiting => "Restarting…", Phase::Available(_) | Phase::Prepared { .. } => "Update", _ => "Check / update" };
        let response = ui.add_enabled(!updater.busy(), egui::Button::new(egui::RichText::new(format!("{}  {text}", crate::icons::DOWNLOAD)).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(35, 128, 67)));
        state.update_button = Some(response.rect);
        if response.on_hover_text("Check the latest release, install it and restart. Your current project is preserved.").clicked() {
            state.update_notice = false;
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
        if state.update_notice {
            match startup_notice(&updater.phase) {
                Notice::Waiting => {}
                Notice::Nothing => state.update_notice = false,
                Notice::Show(version) => {
                    if !updater.open && let Some(anchor) = state.update_button {
                        match callout(ctx, anchor, &version) {
                            Some(true) => { updater.open = true; updater.auto_install = true; state.update_notice = false; }
                            Some(false) => state.update_notice = false,
                            None => {}
                        }
                    }
                }
            }
        }
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

/// What the startup update callout does for the updater's current phase.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq)]
pub enum Notice {
    /// The check is still running.
    Waiting,
    /// Up to date, offline, or the user is already updating: nothing to announce.
    Nothing,
    /// This version can be installed.
    Show(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn startup_notice(phase: &Phase) -> Notice {
    match phase {
        Phase::Checking => Notice::Waiting,
        Phase::Available(release) | Phase::Prepared { release, .. } => Notice::Show(release.version.clone()),
        _ => Notice::Nothing,
    }
}

/// Small window under the update button with an arrow pointing at it. Returns `Some(true)` for
/// "Update now", `Some(false)` for "Later".
#[cfg(not(target_arch = "wasm32"))]
fn callout(ctx: &egui::Context, anchor: egui::Rect, version: &str) -> Option<bool> {
    use crate::theme::{self, ACCENT, BG_2, TEXT, TEXT_DIM};
    use egui::{Order, RichText, Stroke};
    const WIDTH: f32 = 280.0;
    let screen = ctx.content_rect();
    let x = (anchor.center().x - WIDTH * 0.5).clamp(screen.left() + 8.0, (screen.right() - WIDTH - 8.0).max(screen.left() + 8.0));
    let top = anchor.bottom() + 10.0;
    let mut choice = None;
    let area = egui::Area::new(egui::Id::new("update_callout")).order(Order::Foreground).fixed_pos(egui::pos2(x, top)).show(ctx, |ui| {
        egui::Frame::new().fill(BG_2).stroke(Stroke::new(1.0, ACCENT)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(WIDTH - 26.0);
            ui.label(RichText::new(format!("{}  Version {version} is available", crate::icons::DOWNLOAD)).font(theme::font_strong(13.0)).color(TEXT));
            ui.add_space(2.0);
            ui.label(RichText::new("Click Update to install it. SubCraft restarts and keeps your project.").color(TEXT_DIM));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if crate::widgets::primary_button(ui, "Update now").clicked() {
                    choice = Some(true);
                }
                if crate::widgets::secondary_button(ui, "Later").clicked() {
                    choice = Some(false);
                }
            });
        });
    });
    // Arrow from the window's top edge up to the button.
    let tip = egui::pos2(anchor.center().x, anchor.bottom() + 2.0);
    let base = area.response.rect.top() + 0.5;
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, egui::Id::new("update_callout_arrow")));
    painter.add(egui::Shape::convex_polygon(
        vec![tip, egui::pos2(tip.x + 8.0, base), egui::pos2(tip.x - 8.0, base)],
        BG_2,
        Stroke::new(1.0, ACCENT),
    ));
    choice
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use subcraft_app::updater::{Asset, PackageKind, Release};

    fn release() -> Release {
        let asset = Asset { name: "SubCraft-9.9.9-setup.exe".into(), browser_download_url: String::new(), size: 1, digest: String::new() };
        Release { version: "9.9.9".into(), notes: String::new(), kind: PackageKind::WindowsPortable, asset }
    }

    #[test]
    fn callout_waits_for_the_check_then_shows_only_a_newer_release() {
        assert_eq!(startup_notice(&Phase::Checking), Notice::Waiting);
        assert_eq!(startup_notice(&Phase::Available(release())), Notice::Show("9.9.9".into()));
        assert_eq!(startup_notice(&Phase::Prepared { release: release(), package: Default::default() }), Notice::Show("9.9.9".into()));
        for quiet in [Phase::Current, Phase::Error("offline".into()), Phase::Idle, Phase::Downloading { bytes: 0, total: 1 }] {
            assert_eq!(startup_notice(&quiet), Notice::Nothing, "{quiet:?}");
        }
    }
}
