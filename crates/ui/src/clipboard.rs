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
    /// Painter's "Paste as instance": instances of the copied layers above the selection.
    PasteAsInstance,
    /// Copy (or cut) effects of one of a layer's stacks.
    CopyEffects { layer: LayerId, target: LayerTarget, effects: Vec<EffectId>, cut: bool },
    /// Paste the copied effects into this layer, above its selected effect.
    PasteEffects { layer: LayerId },
    /// Ctrl+Shift+C: copy the content (paint or fill, and content effects) of this layer, or of
    /// the selected one.
    CopyContent { layer: Option<LayerId> },
    /// Ctrl+Shift+V: replace the content of this layer, or of the selected one, with the copy.
    PasteContent { layer: Option<LayerId> },
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
    /// Copied layer content (Ctrl+Shift+C); separate from the layer clip, so Ctrl+V is unaffected.
    pub content: Option<Arc<LayerClip>>,
    /// Copied blending options: blend mode and opacity per channel.
    pub blending: Option<std::collections::BTreeMap<subcraft_core::ChannelKind, subcraft_doc::ChannelBlend>>,
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
        // Ctrl+Shift+C / Ctrl+Shift+V mean the layer's content (Painter). Shift alone is not
        // enough: on Windows Shift+Insert is the plain paste key and arrives as the same event.
        let content = i.modifiers.shift && i.modifiers.command;
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Copy if content => Some(Request::CopyContent { layer: None }),
                egui::Event::Paste(_) if content => Some(Request::PasteContent { layer: None }),
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
        let content_before = state.clips.content.clone();
        run(state, r, now);
        if let Some(text) = clipboard_marker(before.as_ref(), state.clips.layers.as_ref()) {
            ctx.copy_text(text);
        } else if let Some(text) = changed_clip(effects_before.as_ref(), state.clips.effects.as_ref()).map(|c| format!("SubCraft: {} effect(s) copied", c.content.len() + c.mask.len())) {
            ctx.copy_text(text);
        } else if changed_clip(content_before.as_ref(), state.clips.content.as_ref()).is_some() {
            ctx.copy_text("SubCraft: layer content copied".to_string());
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
        Request::CopyContent { layer } => {
            let Some(layer) = layer.or(state.session.selection().layer) else { return state.set_status("Select a layer to copy its content", now) };
            if state.session.selected_texture_set().find(layer).is_some_and(|l| l.is_folder()) {
                return state.set_status("A folder has no content to copy", now);
            }
            match state.session.copy_layers(&[layer]) {
                Ok(clip) => {
                    state.clips.content = Some(Arc::new(clip));
                    state.set_status("Copied layer content", now);
                }
                Err(e) => state.set_status(e.to_string(), now),
            }
        }
        Request::PasteContent { layer } => match (state.clips.content.clone(), layer.or(state.session.selection().layer)) {
            (Some(clip), Some(layer)) => state.run(vec![Command::PasteLayerContent { layer, clip }], now),
            (None, _) => state.set_status("Nothing copied yet: copy a layer's content first (Ctrl+Shift+C)", now),
            (_, None) => state.set_status("Select the layer to paste the content into", now),
        },
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
        Request::PasteAsInstance => match state.clips.layers.clone() {
            Some(clip) => {
                state.run(vec![Command::PasteAsInstance(clip.layers.iter().map(|l| l.id).collect())], now);
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

    /// QA 2026-11: on Windows Shift+Insert (the plain paste key) arrives as a Paste event with
    /// Shift held and was taken for Painter's Ctrl+Shift+V (paste into the layer's content).
    #[test]
    fn shift_insert_pastes_layers_and_ctrl_shift_v_pastes_content() {
        let pasted = |modifiers: egui::Modifiers| {
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput { events: vec![egui::Event::ModifiersChanged(modifiers), egui::Event::Paste("x".into())], ..Default::default() }, |ui| shortcut_requests(ui.ctx()));
            out.textures_delta.clear();
            ctx.data(|d| d.get_temp::<Vec<Request>>(queue_id())).unwrap_or_default()
        };
        assert!(matches!(pasted(egui::Modifiers::SHIFT).as_slice(), [Request::Paste]), "Shift+Insert");
        assert!(matches!(pasted(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT).as_slice(), [Request::PasteContent { .. }]), "Ctrl+Shift+V");
        assert!(matches!(pasted(egui::Modifiers::COMMAND).as_slice(), [Request::Paste]), "Ctrl+V");
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

    #[test]
    fn layer_content_is_copied_into_an_existing_layer_without_touching_its_name_mask_or_blending() {
        let (mut s, a, b) = state_with_two_layers();
        let e = add_levels(&mut s, a);
        s.session.execute(Command::AddMask { layer: b, white: false }).unwrap();
        s.session.execute(Command::SetBlendOpacity { layer: b, channel: subcraft_core::ChannelKind::BaseColor, opacity: 0.25 }).unwrap();
        let layers = s.session.selected_texture_set().layers.len();

        // Paste before any copy only reports.
        run(&mut s, Request::PasteContent { layer: Some(b) }, 0.0);
        assert!(s.session.selected_texture_set().find(b).unwrap().content_effects.is_empty());

        run(&mut s, Request::CopyContent { layer: Some(a) }, 0.0);
        assert!(s.clips.layers.is_none() && s.clips.newest.is_none(), "the layer clipboard is untouched");
        let history = s.session.history().entries().len();
        s.session.execute(Command::SelectLayer(Some(b))).unwrap();
        run(&mut s, Request::PasteContent { layer: None }, 0.0);
        let ts = s.session.selected_texture_set();
        let (from, to) = (ts.find(a).unwrap(), ts.find(b).unwrap());
        assert_eq!(ts.layers.len(), layers, "no layer is added");
        assert_eq!(to.kind(), from.kind(), "the paint layer became a fill like the source");
        assert_eq!(to.content, from.content);
        assert_eq!(to.content_effects.len(), 1);
        assert_ne!(to.content_effects[0].id, e, "pasted effects get their own ids");
        assert_eq!(to.content_effects[0].kind, from.content_effects[0].kind);
        assert!(to.mask.is_some() && to.name != from.name);
        assert_eq!(to.blend_for(subcraft_core::ChannelKind::BaseColor).opacity, 0.25);
        assert_eq!(s.session.history().entries().len(), history + 1);
        s.session.execute(Command::Undo).unwrap();
        assert!(s.session.selected_texture_set().find(b).unwrap().content_effects.is_empty());

        // Folders neither give nor take content.
        s.session.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
        let folder = s.session.selection().layer.unwrap();
        let before = s.session.document().clone();
        run(&mut s, Request::PasteContent { layer: Some(folder) }, 0.0);
        assert_eq!(s.session.document(), &before);
        let clip = s.clips.content.clone();
        run(&mut s, Request::CopyContent { layer: Some(folder) }, 0.0);
        assert!(match (&clip, &s.clips.content) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), _ => false }, "the content clip is kept");
    }
}
