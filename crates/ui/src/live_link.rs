//! File > Live link: keep a Unity, Unreal or Godot project's copy of the textures current. After
//! each edit settles (no stroke, compositing finished, a second without changes) the textures
//! are exported into the engine project in the background. The work is `subcraft_app::live_link`.

use std::sync::Arc;
use std::thread::JoinHandle;

use egui::RichText;
use subcraft_app::live_link::{Engine, LiveLinkTarget, SendReport};
use subcraft_mesh::Mesh;

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};

/// Seconds without changes before a send.
pub const SETTLE_SECONDS: f64 = 1.0;

#[derive(Default)]
pub struct LiveLinkUi {
    pub open: bool,
    /// Send automatically after edits.
    pub on: bool,
    job: Option<(Engine, JoinHandle<Result<SendReport, String>>)>,
    /// Session revision of the last send (started).
    sent_revision: Option<u64>,
    /// The mesh the model was last sent for.
    sent_mesh: Option<Arc<Mesh>>,
    /// The revision seen last frame and when it first appeared.
    seen: Option<(u64, f64)>,
    /// A send was asked for by hand (Send now).
    forced: bool,
    pub last: Option<String>,
    pub error: Option<String>,
}

impl LiveLinkUi {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Whether a send should start now: the link is on (or Send now was pressed), the project changed
/// since the last send and has been still for [`SETTLE_SECONDS`].
pub fn due(link: &LiveLinkUi, revision: u64, idle: bool, now: f64) -> bool {
    if link.job.is_some() || !idle {
        return false;
    }
    if link.forced {
        return true;
    }
    link.on && link.sent_revision != Some(revision) && link.seen.is_some_and(|(r, t)| r == revision && now - t >= SETTLE_SECONDS)
}

/// Call every frame: finishes a running send and starts the next one when due.
pub fn tick(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if let Some((engine, job)) = &state.live_link.job
        && job.is_finished()
    {
        let engine = *engine;
        let (_, job) = state.live_link.job.take().expect("checked above");
        match job.join().unwrap_or_else(|_| Err("the send crashed".into())) {
            Ok(report) => {
                let line = format!("{} ({})", report.summary(engine), chrono_free_time(now));
                log::info!("live link: {} -> {}", report.summary(engine), report.dir.display());
                state.live_link.last = Some(line);
                state.live_link.error = None;
            }
            Err(e) => {
                log::warn!("live link: {e}");
                state.live_link.error = Some(e.clone());
                state.set_status(format!("Live link: {e}"), now);
                state.live_link.on = false;
            }
        }
    }
    let link = &mut state.live_link;
    if !link.on && !link.forced {
        return;
    }
    let Some(target) = state.session.live_link_target().cloned() else {
        link.on = false;
        link.forced = false;
        return;
    };
    let revision = state.session.revision();
    if link.seen.is_none_or(|(r, _)| r != revision) {
        link.seen = Some((revision, now));
    }
    let idle = !state.session.is_stroking() && !state.session.composites_pending();
    if !due(link, revision, idle, now) {
        if link.on {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        return;
    }
    let mesh = state.session.scene().mesh.clone();
    let with_model = link.sent_mesh.as_ref().is_none_or(|m| !Arc::ptr_eq(m, &mesh));
    let snapshot = state.session.live_link_snapshot(&target, with_model);
    link.sent_revision = Some(revision);
    link.sent_mesh = Some(mesh);
    link.forced = false;
    link.job = Some((target.engine, std::thread::spawn(move || snapshot.send())));
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// Seconds since start as `m:ss`, enough to tell sends apart without a clock dependency.
fn chrono_free_time(now: f64) -> String {
    let s = now.max(0.0) as u64;
    format!("{}:{:02} after start", s / 60, s % 60)
}

/// Pick an engine project folder and make it the target.
fn choose(state: &mut UiState, dir: &std::path::Path) {
    match LiveLinkTarget::for_folder(dir) {
        Some(target) => {
            state.live_link.error = None;
            state.live_link.sent_revision = None;
            state.live_link.sent_mesh = None;
            state.session.set_live_link_target(Some(target));
        }
        None => state.live_link.error = Some(format!("{} is not inside a Unity, Unreal or Godot project", dir.display())),
    }
}

fn engine_note(engine: Engine) -> &'static str {
    match engine {
        Engine::Godot => crate::i18n::tr("Godot imports the files when its window gets focus. Each texture set also gets a ready StandardMaterial3D (.tres) next to the textures: drag it onto the mesh."),
        Engine::Unity => crate::i18n::tr("Unity imports the files when its window gets focus. Maps are for URP Lit: Base Map, Metallic Map (smoothness in alpha), Normal Map."),
        Engine::Unreal => crate::i18n::tr("Unreal imports new and changed files in Content when Editor Preferences > Loading & Saving > Auto Reimport is on (the default). Maps: Base Color, Normal (DirectX), packed Occlusion / Roughness / Metallic."),
    }
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    tick(state, ctx, now);
    if !state.live_link.open {
        return;
    }
    let mut open = true;
    let target = state.session.live_link_target().cloned();
    let mut pick = false;
    let mut forget = false;
    crate::widgets::window(ctx, RichText::new(crate::i18n::tr("Live link")).strong()).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        ui.label(RichText::new(crate::i18n::tr("Send this project's textures to a game engine project and keep them up to date while you paint.")).color(TEXT_DIM()));
        ui.add_space(6.0);
        match &target {
            Some(t) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t.engine.label()).strong());
                    ui.label(RichText::new(&t.project).color(TEXT_DIM()));
                });
                ui.label(RichText::new(engine_note(t.engine)).color(TEXT_DIM()).small());
            }
            None => {
                ui.label(crate::i18n::tr("No engine project chosen yet."));
            }
        }
        ui.horizontal(|ui| {
            if ui.button(if target.is_some() { crate::i18n::tr("Change project...") } else { crate::i18n::tr("Choose engine project...") }).clicked() {
                pick = true;
            }
            if target.is_some() && ui.button(crate::i18n::tr("Forget")).on_hover_text(crate::i18n::tr("Stop linking this project to the engine project")).clicked() {
                forget = true;
            }
        });
        ui.add_space(6.0);
        let link = &mut state.live_link;
        ui.add_enabled_ui(target.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut link.on, crate::i18n::tr("Send automatically after edits"));
                if ui.add_enabled(!link.busy(), egui::Button::new(crate::i18n::tr("Send now"))).clicked() {
                    link.forced = true;
                }
                if link.busy() {
                    ui.spinner();
                }
            });
        });
        if let Some(last) = &link.last {
            ui.label(RichText::new(last).color(TEXT_DIM()));
        }
        if let Some(e) = &link.error {
            ui.label(RichText::new(e).color(theme::WARNING()));
        }
    });
    if pick && let Some(dir) = rfd::FileDialog::new().pick_folder() {
        choose(state, &dir);
    }
    if forget {
        state.session.set_live_link_target(None);
        state.live_link.on = false;
    }
    if !open {
        state.live_link.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Session;

    #[test]
    fn a_send_waits_for_the_painting_to_settle_and_happens_once_per_change() {
        let mut link = LiveLinkUi { on: true, ..Default::default() };
        link.seen = Some((5, 10.0));
        assert!(!due(&link, 5, true, 10.5), "too soon after the change");
        assert!(!due(&link, 5, false, 12.0), "a stroke or compositing is running");
        assert!(due(&link, 5, true, 11.0));
        link.sent_revision = Some(5);
        assert!(!due(&link, 5, true, 20.0), "nothing new since the last send");
        link.on = false;
        link.seen = Some((6, 21.0));
        assert!(!due(&link, 6, true, 30.0), "off: no automatic sends");
        link.forced = true;
        assert!(due(&link, 6, true, 21.0), "Send now does not wait");
    }

    #[test]
    fn send_now_writes_into_a_chosen_godot_project() {
        let root = std::env::temp_dir().join(format!("ui-live-link-{}", subcraft_core::ObjectId::new()));
        std::fs::create_dir_all(root.join("levels")).unwrap();
        std::fs::write(root.join("project.godot"), "config_version=5\n").unwrap();
        let mut session = Session::new(subcraft_mesh::primitives::cube(1.0), 128);
        session.update_composites();
        let mut state = UiState::new(session, None);
        choose(&mut state, &root.join("levels"));
        assert_eq!(state.session.live_link_target().map(|t| t.engine), Some(Engine::Godot));
        choose(&mut state, &std::env::temp_dir());
        assert!(state.live_link.error.is_some(), "a folder outside any engine project is refused");
        state.live_link.forced = true;
        let ctx = egui::Context::default();
        tick(&mut state, &ctx, 1.0);
        let (_, job) = state.live_link.job.take().expect("the send started");
        let report = job.join().unwrap().unwrap();
        assert!(report.model.is_some() && report.textures > 0 && report.materials.len() == 1);
        assert!(report.dir.starts_with(root.join("subcraft")));
        std::fs::remove_dir_all(root).ok();
    }
}
