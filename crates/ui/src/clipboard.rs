//! Layer / effect clipboard in the UI: panels and shortcuts queue [`Request`]s, [`process`]
//! runs them once per frame. The clips live in [`UiState`], not in the session, so they survive
//! opening another project.

use std::sync::Arc;

use subcraft_app::{Command, EffectClip, LayerClip, LayerTarget};
use subcraft_core::{EffectId, LayerId};

use crate::state::UiState;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Copy (or cut) the multi-selection, else the selected layer.
    CopyLayers { cut: bool },
    PasteLayers,
    CopyEffect { layer: LayerId, target: LayerTarget, effect: EffectId },
    PasteEffects { layer: LayerId },
}

#[derive(Default)]
pub struct Clips {
    pub layers: Option<Arc<LayerClip>>,
    pub effects: Option<Arc<EffectClip>>,
}

fn queue_id() -> egui::Id {
    egui::Id::new("clipboard_requests")
}

/// Queue a request from anywhere that has the egui context.
pub fn request(ctx: &egui::Context, r: Request) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Request>>(queue_id()).push(r));
    ctx.request_repaint();
}

/// Ctrl+C / X / V arrive as egui copy / cut / paste events, not key presses.
pub fn shortcut_requests(ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events: Vec<Request> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Copy => Some(Request::CopyLayers { cut: false }),
                egui::Event::Cut => Some(Request::CopyLayers { cut: true }),
                egui::Event::Paste(_) => Some(Request::PasteLayers),
                _ => None,
            })
            .collect()
    });
    for r in events {
        request(ctx, r);
    }
}

pub fn process(state: &mut UiState, ctx: &egui::Context, now: f64) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("effect_clip_present"), state.clips.effects.is_some()));
    let requests = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Vec<Request>>(queue_id())));
    for r in requests {
        let before = state.clips.layers.clone();
        run(state, r, now);
        if let Some(text) = clipboard_marker(before.as_ref(), state.clips.layers.as_ref()) {
            ctx.copy_text(text);
        }
    }
}

/// Text put on the system clipboard when layers are copied or cut. The window layer only turns
/// Ctrl+V into a paste event when the system clipboard holds text, so with it empty (or holding an
/// image) Ctrl+V did nothing; with the marker there, the next Ctrl+V pastes the layers.
fn clipboard_marker(before: Option<&Arc<LayerClip>>, after: Option<&Arc<LayerClip>>) -> Option<String> {
    let after = after?;
    if before.is_some_and(|b| Arc::ptr_eq(b, after)) {
        return None;
    }
    Some(format!("SubCraft: {} layer(s) copied", after.layers.len()))
}

fn run(state: &mut UiState, r: Request, now: f64) {
    match r {
        Request::CopyLayers { cut } => {
            let targets = state.multi.targets(state.session.selection().layer);
            match state.session.copy_layers(&targets) {
                Ok(clip) => {
                    let n = clip.layers.len();
                    state.clips.layers = Some(Arc::new(clip));
                    state.set_status(format!("{} {n} layer(s)", if cut { "Cut" } else { "Copied" }), now);
                    if cut {
                        state.run(vec![Command::DeleteLayers(targets)], now);
                        state.multi.clear();
                    }
                }
                Err(e) => state.set_status(e.to_string(), now),
            }
        }
        Request::PasteLayers => match state.clips.layers.clone() {
            Some(clip) => {
                state.run(vec![Command::PasteLayers(clip)], now);
                state.multi.clear();
            }
            None => state.set_status("Nothing copied yet: copy layers first", now),
        },
        Request::CopyEffect { layer, target, effect } => match state.session.copy_effects(layer, target, &[effect]) {
            Ok(clip) => {
                state.clips.effects = Some(Arc::new(clip));
                state.set_status("Copied effect", now);
            }
            Err(e) => state.set_status(e.to_string(), now),
        },
        Request::PasteEffects { layer } => match state.clips.effects.clone() {
            Some(clip) => state.run(vec![Command::PasteEffects { layer, clip }], now),
            None => state.set_status("Nothing copied yet: copy an effect first", now),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::{NewLayer, Session};

    fn state_with_two_layers() -> (UiState, LayerId, LayerId) {
        let mut s = UiState::new(Session::sample(), None);
        s.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        let a = s.session.selection().layer.unwrap();
        s.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        let b = s.session.selection().layer.unwrap();
        (s, a, b)
    }

    #[test]
    fn copying_layers_puts_text_on_the_system_clipboard_so_ctrl_v_reaches_the_app() {
        let (mut s, a, _) = state_with_two_layers();
        s.session.execute(Command::SelectLayer(Some(a))).unwrap();
        let ctx = egui::Context::default();
        let copied = |out: &egui::FullOutput| out.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.starts_with("SubCraft: 1 layer")));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            request(ui.ctx(), Request::CopyLayers { cut: false });
            process(&mut s, ui.ctx(), 0.0);
        });
        out.textures_delta.clear();
        assert!(copied(&out), "copy writes the marker: {:?}", out.platform_output.commands);
        // Pasting doesn't touch the system clipboard.
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            request(ui.ctx(), Request::PasteLayers);
            process(&mut s, ui.ctx(), 0.0);
        });
        out.textures_delta.clear();
        assert!(!copied(&out));
    }

    #[test]
    fn clips_survive_replacing_the_session() {
        let (mut s, a, _) = state_with_two_layers();
        s.session.execute(Command::SelectLayer(Some(a))).unwrap();
        run(&mut s, Request::CopyLayers { cut: false }, 0.0);
        let before = s.session.selected_texture_set().layers.len();
        s.set_session(Session::sample());
        let other = s.session.selected_texture_set().layers.len();
        run(&mut s, Request::PasteLayers, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), other + 1);
        assert!(before > 0);
    }

    #[test]
    fn cut_removes_and_paste_without_a_copy_reports_instead_of_failing() {
        let (mut s, a, b) = state_with_two_layers();
        run(&mut s, Request::PasteLayers, 0.0);
        let count = s.session.selected_texture_set().layers.len();
        assert_eq!(s.session.selected_texture_set().layers.len(), count, "nothing pasted");
        s.session.execute(Command::SelectLayer(Some(b))).unwrap();
        run(&mut s, Request::CopyLayers { cut: true }, 0.0);
        assert!(s.session.selected_texture_set().find(b).is_none());
        assert!(s.session.selected_texture_set().find(a).is_some());
        run(&mut s, Request::PasteLayers, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), count);
    }
}
