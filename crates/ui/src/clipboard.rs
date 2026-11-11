//! Layer / effect clipboard in the UI: panels and shortcuts queue [`Request`]s, [`process`]
//! runs them once per frame. The clips live in [`UiState`], not in the session, so they survive
//! opening another project.

use std::sync::Arc;

use subcraft_app::{Command, EffectClip, LayerClip, LayerTarget};
use subcraft_core::{EffectId, LayerId};

use crate::state::UiState;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Ctrl+C / Ctrl+X: the selected effect(s) when an effect is selected, else the layer(s).
    Copy { cut: bool },
    /// Ctrl+V: whatever was copied last, effects or layers.
    Paste,
    /// Copy (or cut) the multi-selection, else the selected layer.
    CopyLayers { cut: bool },
    PasteLayers,
    /// Copy (or cut) effects of one of a layer's stacks.
    CopyEffects { layer: LayerId, target: LayerTarget, effects: Vec<EffectId>, cut: bool },
    /// Paste the copied effects into this layer, above its selected effect.
    PasteEffects { layer: LayerId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Layers,
    Effects,
}

#[derive(Default)]
pub struct Clips {
    pub layers: Option<Arc<LayerClip>>,
    pub effects: Option<Arc<EffectClip>>,
    /// Which of the two was copied last: Ctrl+V pastes that one.
    pub newest: Option<ClipKind>,
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
                egui::Event::Copy => Some(Request::Copy { cut: false }),
                egui::Event::Cut => Some(Request::Copy { cut: true }),
                egui::Event::Paste(_) => Some(Request::Paste),
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
        let effects_before = state.clips.effects.clone();
        run(state, r, now);
        if let Some(text) = clipboard_marker(before.as_ref(), state.clips.layers.as_ref()) {
            ctx.copy_text(text);
        } else if let Some(text) = changed_clip(effects_before.as_ref(), state.clips.effects.as_ref()).map(|c| format!("SubCraft: {} effect(s) copied", c.content.len() + c.mask.len())) {
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

/// The clip a request just stored, if it stored one. Copied effects need the same marker text
/// as layers, or Ctrl+V would never arrive.
fn changed_clip<'a, T>(before: Option<&Arc<T>>, after: Option<&'a Arc<T>>) -> Option<&'a Arc<T>> {
    after.filter(|a| !before.is_some_and(|b| Arc::ptr_eq(b, a)))
}

fn run(state: &mut UiState, r: Request, now: f64) {
    match r {
        Request::Copy { cut } => {
            let session = &state.session;
            let next = match (session.selection().layer, session.selected_effect()) {
                (Some(layer), Some(effect)) => {
                    let target = session.layer_target();
                    Request::CopyEffects { layer, target, effects: state.effect_multi.targets((layer, target), Some(effect)), cut }
                }
                _ => Request::CopyLayers { cut },
            };
            run(state, next, now);
        }
        Request::Paste => {
            let next = match (state.clips.newest, state.session.selection().layer) {
                (Some(ClipKind::Effects), Some(layer)) => Request::PasteEffects { layer },
                _ => Request::PasteLayers,
            };
            run(state, next, now);
        }
        Request::CopyLayers { cut } => {
            let targets = state.multi.targets(state.session.selection().layer);
            match state.session.copy_layers(&targets) {
                Ok(clip) => {
                    let n = clip.layers.len();
                    state.clips.layers = Some(Arc::new(clip));
                    state.clips.newest = Some(ClipKind::Layers);
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
        Request::CopyEffects { layer, target, effects, cut } => match state.session.copy_effects(layer, target, &effects) {
            Ok(clip) => {
                let n = clip.content.len() + clip.mask.len();
                state.clips.effects = Some(Arc::new(clip));
                state.clips.newest = Some(ClipKind::Effects);
                state.set_status(format!("{} {n} effect(s)", if cut { "Cut" } else { "Copied" }), now);
                if cut {
                    state.run(vec![Command::RemoveEffects { layer, target, effects }], now);
                    state.effect_multi.clear();
                }
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

    fn add_levels(s: &mut UiState, layer: LayerId) -> EffectId {
        let kind = subcraft_doc::ContentEffectKind::Levels { channel: subcraft_core::ChannelKind::BaseColor, levels: Default::default() };
        s.session.execute(Command::AddContentEffect { layer, kind }).unwrap();
        s.session.selected_effect().unwrap()
    }

    fn content(s: &UiState, layer: LayerId) -> Vec<EffectId> {
        s.session.selected_texture_set().find(layer).unwrap().content_effects.iter().map(|e| e.id).collect()
    }

    #[test]
    fn the_copy_keys_take_the_selected_effects_and_paste_follows_the_newest_clip() {
        let (mut s, a, b) = state_with_two_layers();
        let layers = s.session.selected_texture_set().layers.len();
        s.session.execute(Command::SelectLayer(Some(a))).unwrap();
        let (e0, e1, e2) = (add_levels(&mut s, a), add_levels(&mut s, a), add_levels(&mut s, a));

        // Ctrl+C with e2 selected and e0 Ctrl-clicked: both effects, not the layer.
        let stack = (a, LayerTarget::Content);
        let primary = s.effect_multi.click(stack, &[e0, e1, e2], Some(e2), e0, true, false);
        s.session.execute(Command::SelectEffect { layer: a, target: LayerTarget::Content, effect: primary }).unwrap();
        run(&mut s, Request::Copy { cut: false }, 0.0);
        assert!(s.clips.layers.is_none());
        assert_eq!(s.clips.effects.as_ref().unwrap().content.len(), 2);
        assert_eq!(s.clips.newest, Some(ClipKind::Effects));

        // Ctrl+V on another layer pastes the effects there, in stack order.
        s.session.execute(Command::SelectLayer(Some(b))).unwrap();
        run(&mut s, Request::Paste, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), layers, "no layer pasted");
        let pasted = s.session.selected_texture_set().find(b).unwrap().content_effects.clone();
        assert_eq!(pasted.len(), 2);
        assert!(pasted.iter().all(|e| ![e0, e1, e2].contains(&e.id)));

        // Ctrl+X removes what it copied, in one undo step.
        s.session.execute(Command::SelectEffect { layer: a, target: LayerTarget::Content, effect: Some(e1) }).unwrap();
        s.effect_multi.clear();
        run(&mut s, Request::Copy { cut: true }, 0.0);
        assert_eq!(content(&s, a), [e0, e2]);
        assert_eq!(s.clips.effects.as_ref().unwrap().content.len(), 1);
        s.session.execute(Command::Undo).unwrap();
        assert_eq!(content(&s, a), [e0, e1, e2]);

        // Paste goes right above the selected effect.
        s.session.execute(Command::SelectEffect { layer: a, target: LayerTarget::Content, effect: Some(e0) }).unwrap();
        run(&mut s, Request::Paste, 0.0);
        let now = content(&s, a);
        assert_eq!((now.len(), now[0], now[2], now[3]), (4, e0, e1, e2));

        // No effect selected: the keys mean layers again, and the newest clip is then the layer.
        s.session.execute(Command::SelectLayer(Some(b))).unwrap();
        run(&mut s, Request::Copy { cut: false }, 0.0);
        assert_eq!(s.clips.newest, Some(ClipKind::Layers));
        run(&mut s, Request::Paste, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), layers + 1);
    }
}
