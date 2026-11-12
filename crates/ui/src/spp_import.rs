//! Opening a Painter project in the background: the window stays responsive, the project appears
//! as soon as the model and the flattened textures are read, and the rebuilt layer stack is
//! installed when it is done (see `subcraft_app::SppImportJob`). Progress and Cancel are in the
//! status bar.

use std::path::{Path, PathBuf};

use subcraft_app::{SppImportJob, SppPoll};

use crate::state::UiState;

pub struct SppImportUi {
    job: SppImportJob,
    name: String,
}

impl SppImportUi {
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// `Opening Cube.spp: Rebuilding the Painter stack (120 of 374 layers)`, with the share done.
    pub fn progress(&self) -> (String, Option<f32>) {
        let control = self.job.control();
        let what = if self.job.is_rebuilding() { "Rebuilding layers of" } else { "Opening" };
        (format!("{what} {}: {}", self.name, control.stage()), control.fraction())
    }
}

/// What an import is: a rebuild in SubCraft, not the Painter file opened as it is.
pub const RECONSTRUCTION_NOTICE: &str = "Reconstructed from the .spp: layers, masks, mesh and baked maps were rebuilt in SubCraft. Painter-only effects and Substance materials may be approximated or skipped. The original file is not changed.";

/// Shown once after a Painter import finished, until dismissed.
pub fn reconstruction_notice(state: &mut UiState, ctx: &egui::Context) {
    let Some(name) = state.spp_notice.clone() else { return };
    let mut open = true;
    let mut ok = false;
    crate::widgets::window(ctx, format!("{name} imported")).id(egui::Id::new("spp_reconstruction_notice")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(RECONSTRUCTION_NOTICE);
        ui.add_space(8.0);
        ok = ui.button("OK").clicked();
    });
    if ok || !open {
        state.spp_notice = None;
    }
}

pub fn is_spp(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp"))
}

/// Start importing `path`, replacing a running import.
pub fn start(state: &mut UiState, path: &Path) {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    state.spp_import = Some(SppImportUi { job: SppImportJob::start(path), name });
}

/// Hand finished results to the session; call once per frame.
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    loop {
        let Some(import) = state.spp_import.as_mut() else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let path: PathBuf = import.job.path.clone();
        let name = import.name.clone();
        match import.job.poll(&mut state.session) {
            SppPoll::Idle => return,
            SppPoll::Opened(session, notes) => {
                // `set_session` cancels any running import; this one continues with the stack rebuild.
                let running = state.spp_import.take();
                state.set_session(*session);
                state.spp_import = running;
                state.welcome.remember(&path);
                let status = match notes.first() {
                    Some(first) if notes.len() > 1 => format!("Opened {name}; rebuilding its layers in the background. {first}"),
                    _ => format!("Opened {name}; rebuilding its layers in the background"),
                };
                state.set_status(status, now);
            }
            SppPoll::StackInstalled(notes) => {
                let summary = notes.iter().find(|n| n.contains("Rebuilt stack")).map_or_else(|| "The Painter layer stack is ready".to_string(), |n| n.trim().to_string());
                log::info!("Painter import: {summary}");
                state.set_status(format!("{name}: {summary}"), now);
            }
            SppPoll::Discarded => {
                state.spp_import = None;
                return;
            }
            SppPoll::Finished(result) => {
                let opened = import.job.is_rebuilding();
                state.spp_import = None;
                match result {
                    Ok(()) => state.spp_notice = Some(name.clone()),
                    Err(e) if e == "cancelled" && opened => state.set_status(format!("Rebuilding the layers of {name} was cancelled; the flattened Painter result stays."), now),
                    Err(e) if e == "cancelled" => state.set_status(format!("Opening {name} was cancelled"), now),
                    Err(e) => state.set_status(format!("Could not open {name}: {e}"), now),
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod notice_tests {
    use super::*;

    /// The owner's wording: an import is a reconstruction, not the Painter file opened as it is.
    #[test]
    fn a_finished_import_shows_the_reconstruction_notice_until_ok() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        let ctx = egui::Context::default();
        let run = |state: &mut UiState, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))), ..Default::default() }, |ui| reconstruction_notice(state, ui.ctx()));
            // No renderer here: drop the font atlas uploads (epaint panics on unapplied deltas).
            out.textures_delta.clear();
            out
        };
        let texts = |out: &egui::FullOutput| out.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5)), _ => None }).collect::<Vec<_>>();
        assert!(texts(&run(&mut state, vec![])).is_empty(), "nothing before an import finished");
        state.spp_notice = Some("Robot.spp".into());
        let mut out = run(&mut state, vec![]);
        for _ in 0..2 { out = run(&mut state, vec![]); }
        let shown = texts(&out);
        let all: String = shown.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" ");
        assert!(all.contains("Reconstructed from the .spp") && all.contains("The original file is not changed"), "{all}");
        assert!(all.contains("Robot.spp"), "{all}");
        let ok = shown.iter().find(|(t, _)| t == "OK").map(|(_, p)| *p).expect("an OK button");
        run(&mut state, vec![egui::Event::PointerMoved(ok), egui::Event::PointerButton { pos: ok, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        run(&mut state, vec![egui::Event::PointerButton { pos: ok, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        assert!(state.spp_notice.is_none(), "OK dismisses it");
    }
}
