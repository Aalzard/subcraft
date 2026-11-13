//! Painter parity for effect-stack interactions: insert above the selection, multi-effect
//! remove / duplicate / nudge, rename, drops across layers and Alt-drag copies.
use subcraft_app::effect_stack::{DropSide, EffectAddress, EffectDrop};
use subcraft_app::{Action, Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, EffectId, LayerId};
use subcraft_doc::{ContentEffectKind, Levels, MaskEffectKind};

fn session() -> Session {
    Session::new(subcraft_mesh::primitives::cube(2.0), 64)
}

fn levels() -> ContentEffectKind {
    ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels: Levels::default() }
}

fn add_layer(s: &mut Session, kind: NewLayer) -> LayerId {
    s.execute(Command::AddLayer(kind)).unwrap();
    s.selection().layer.unwrap()
}

/// Add `n` content effects to `layer`, each on top of the last. Returns them bottom to top.
fn add_content(s: &mut Session, layer: LayerId, n: usize) -> Vec<EffectId> {
    s.execute(Command::SelectEffect { layer, target: LayerTarget::Content, effect: None }).unwrap();
    (0..n)
        .map(|_| {
            s.execute(Command::AddContentEffect { layer, kind: levels() }).unwrap();
            s.selected_effect().unwrap()
        })
        .collect()
}

fn add_mask(s: &mut Session, layer: LayerId, n: usize) -> Vec<EffectId> {
    if s.selected_texture_set().find(layer).unwrap().mask.is_none() {
        s.execute(Command::AddMask { layer, white: true }).unwrap();
    }
    s.execute(Command::SelectEffect { layer, target: LayerTarget::Mask, effect: None }).unwrap();
    (0..n)
        .map(|_| {
            s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
            s.selected_effect().unwrap()
        })
        .collect()
}

fn content(s: &Session, layer: LayerId) -> Vec<EffectId> {
    s.selected_texture_set().find(layer).unwrap().content_effects.iter().map(|e| e.id).collect()
}

fn mask(s: &Session, layer: LayerId) -> Vec<EffectId> {
    s.selected_texture_set().find(layer).unwrap().mask.as_ref().map(|m| m.effects.iter().map(|e| e.id).collect()).unwrap_or_default()
}

fn steps(s: &Session) -> usize {
    s.history().entries().len()
}

fn address(s: &Session, layer: LayerId, domain: LayerTarget, effect: EffectId) -> EffectAddress {
    EffectAddress { texture_set: s.selection().texture_set, layer, domain, effect }
}

#[test]
fn a_new_effect_goes_right_above_the_selected_one_else_on_top() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let e = add_content(&mut s, l, 3);
    assert_eq!(content(&s, l), e);

    // Select the bottom one: the next effect lands between it and the middle one.
    s.execute(Command::SelectEffect { layer: l, target: LayerTarget::Content, effect: Some(e[0]) }).unwrap();
    s.execute(Command::AddContentEffect { layer: l, kind: levels() }).unwrap();
    let new = s.selected_effect().unwrap();
    assert_eq!(content(&s, l), [e[0], new, e[1], e[2]]);
    // No effect selected: on top.
    s.execute(Command::SelectEffect { layer: l, target: LayerTarget::Content, effect: None }).unwrap();
    s.execute(Command::AddContentEffect { layer: l, kind: levels() }).unwrap();
    assert_eq!(content(&s, l).last().copied(), s.selected_effect());

    // The mask stack behaves the same, and a selected content effect does not place mask effects.
    let m = add_mask(&mut s, l, 2);
    s.execute(Command::SelectEffect { layer: l, target: LayerTarget::Mask, effect: Some(m[0]) }).unwrap();
    s.execute(Command::AddMaskEffect { layer: l, kind: MaskEffectKind::Invert }).unwrap();
    let new_mask = s.selected_effect().unwrap();
    assert_eq!(mask(&s, l), [m[0], new_mask, m[1]]);
    s.execute(Command::SelectEffect { layer: l, target: LayerTarget::Content, effect: Some(e[0]) }).unwrap();
    s.execute(Command::AddMaskEffect { layer: l, kind: MaskEffectKind::Invert }).unwrap();
    assert_eq!(mask(&s, l).last().copied(), s.selected_effect());
}

#[test]
fn pasted_effects_go_above_the_selected_effect_of_the_matching_stack() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let e = add_content(&mut s, a, 3);
    let clip = std::sync::Arc::new(s.copy_effects(a, LayerTarget::Content, &[e[2], e[1]]).unwrap());

    s.execute(Command::SelectEffect { layer: a, target: LayerTarget::Content, effect: Some(e[0]) }).unwrap();
    let before = steps(&s);
    s.execute(Command::PasteEffects { layer: a, clip: clip.clone() }).unwrap();
    assert_eq!(steps(&s), before + 1);
    let now = content(&s, a);
    assert_eq!(now.len(), 5);
    assert_eq!((now[0], now[3], now[4]), (e[0], e[1], e[2]), "the two pasted effects sit right above the selected one");
    assert!(!e.contains(&now[1]) && !e.contains(&now[2]), "pasted effects get new ids");

    // Another layer, nothing selected in it: on top.
    let b = add_layer(&mut s, NewLayer::Paint);
    let own = add_content(&mut s, b, 1);
    s.execute(Command::SelectEffect { layer: b, target: LayerTarget::Content, effect: None }).unwrap();
    s.execute(Command::PasteEffects { layer: b, clip }).unwrap();
    let now = content(&s, b);
    assert_eq!((now.len(), now[0]), (3, own[0]));
}

#[test]
fn several_effects_are_removed_duplicated_and_nudged_in_one_undo_step() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let e = add_content(&mut s, l, 5);
    let target = LayerTarget::Content;

    // Alt+Up on two effects: both move one step and keep their order.
    let before = steps(&s);
    s.execute(Command::NudgeEffects { layer: l, target, effects: vec![e[2], e[1]], up: true }).unwrap();
    assert_eq!(content(&s, l), [e[0], e[3], e[1], e[2], e[4]]);
    assert_eq!(steps(&s), before + 1);
    s.execute(Command::NudgeEffects { layer: l, target, effects: vec![e[1], e[2]], up: false }).unwrap();
    assert_eq!(content(&s, l), e);
    // At the end of the stack nothing happens and no undo step is added.
    let before = steps(&s);
    s.execute(Command::NudgeEffects { layer: l, target, effects: vec![e[4]], up: true }).unwrap();
    s.execute(Command::NudgeEffects { layer: l, target, effects: vec![e[0]], up: false }).unwrap();
    assert_eq!((content(&s, l), steps(&s)), (e.clone(), before));
    assert!(s.execute(Command::NudgeEffects { layer: l, target, effects: vec![EffectId::new()], up: true }).is_err());

    // Ctrl+D: each copy sits right above its original; the top copy is selected.
    s.execute(Command::DuplicateEffects { layer: l, target, effects: vec![e[3], e[0]] }).unwrap();
    let now = content(&s, l);
    assert_eq!(now.len(), 7);
    assert_eq!((now[0], now[2], now[3], now[4], now[6]), (e[0], e[1], e[2], e[3], e[4]));
    assert_eq!(s.selected_effect(), Some(now[5]));
    let name = |s: &Session, id| s.selected_texture_set().find(l).unwrap().content_effects.iter().find(|e| e.id == id).unwrap().name.clone();
    assert_eq!(name(&s, now[5]), format!("{} copy", name(&s, e[3])));
    assert_eq!(steps(&s), before + 1);
    s.execute(Command::Undo).unwrap();
    assert_eq!(content(&s, l), e);

    // Delete: all at once, one step back restores them where they were.
    s.execute(Command::RemoveEffects { layer: l, target, effects: vec![e[4], e[1], e[2]] }).unwrap();
    assert_eq!(content(&s, l), [e[0], e[3]]);
    assert_eq!(s.selected_effect(), None, "a removed effect is no longer selected");
    s.execute(Command::Undo).unwrap();
    assert_eq!(content(&s, l), e);
    let doc = s.document().clone();
    assert!(s.execute(Command::RemoveEffects { layer: l, target, effects: vec![EffectId::new()] }).is_err());
    assert!(s.execute(Command::RemoveEffects { layer: l, target: LayerTarget::Mask, effects: vec![e[0]] }).is_err(), "no mask");
    assert_eq!(s.document(), &doc);

    // The mask stack takes the same commands.
    let m = add_mask(&mut s, l, 3);
    s.execute(Command::NudgeEffects { layer: l, target: LayerTarget::Mask, effects: vec![m[0]], up: true }).unwrap();
    assert_eq!(mask(&s, l), [m[1], m[0], m[2]]);
    s.execute(Command::RemoveEffects { layer: l, target: LayerTarget::Mask, effects: vec![m[0], m[2]] }).unwrap();
    assert_eq!(mask(&s, l), [m[1]]);
    s.execute(Command::DuplicateEffects { layer: l, target: LayerTarget::Mask, effects: vec![m[1]] }).unwrap();
    assert_eq!(mask(&s, l).len(), 2);
    assert_eq!(content(&s, l), e, "the content stack is untouched");
}

#[test]
fn delete_and_ctrl_d_act_on_the_selected_effect_before_the_layer() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let e = add_content(&mut s, l, 2);
    s.execute(Command::SelectEffect { layer: l, target: LayerTarget::Content, effect: Some(e[0]) }).unwrap();

    assert!(s.perform(Action::DuplicateLayer).unwrap());
    assert_eq!(s.selected_texture_set().layers.len(), 1, "the layer is not duplicated");
    let now = content(&s, l);
    assert_eq!((now.len(), now[0], now[2]), (3, e[0], e[1]));
    assert_eq!(s.selected_effect(), Some(now[1]));

    assert!(s.perform(Action::DeleteLayer).unwrap());
    assert_eq!(content(&s, l), e);
    assert!(s.selected_texture_set().find(l).is_some(), "the layer stays");
    // No effect selected any more: now the keys mean the layer.
    assert_eq!(s.selected_effect(), None);
    assert!(s.perform(Action::DuplicateLayer).unwrap());
    assert_eq!(s.selected_texture_set().layers.len(), 2);
    assert!(s.perform(Action::DeleteLayer).unwrap());
    assert_eq!(s.selected_texture_set().layers.len(), 1);
}

/// QA 2026-11: a NaN alpha threshold survived clamp, was saved as null and the project no longer
/// opened.
#[test]
fn a_nan_alpha_threshold_is_refused() {
    let mut s = session();
    let texture_set = s.selection().texture_set;
    let doc = s.document().clone();
    let alpha = subcraft_doc::AlphaSettings { threshold: f32::NAN, ..Default::default() };
    assert!(s.execute(Command::SetAlphaSettings { texture_set, alpha }).is_err());
    assert_eq!(s.document(), &doc);
}

#[test]
fn effects_are_renamed_in_either_stack() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let e = add_content(&mut s, l, 1);
    let m = add_mask(&mut s, l, 1);
    s.execute(Command::RenameEffect { layer: l, target: LayerTarget::Content, effect: e[0], name: "  Rust tint ".into() }).unwrap();
    s.execute(Command::RenameEffect { layer: l, target: LayerTarget::Mask, effect: m[0], name: "Flip".into() }).unwrap();
    let layer = s.selected_texture_set().find(l).unwrap();
    assert_eq!(layer.content_effects[0].name, "Rust tint");
    assert_eq!(layer.mask.as_ref().unwrap().effects[0].name, "Flip");

    // QA 2026-11: the same name (after trimming) added an empty "Rename Effect" step.
    let entries = s.history().entries().len();
    s.execute(Command::RenameEffect { layer: l, target: LayerTarget::Content, effect: e[0], name: "Rust tint ".into() }).unwrap();
    assert_eq!(s.history().entries().len(), entries, "renaming to the same name adds no undo step");

    let doc = s.document().clone();
    assert!(s.execute(Command::RenameEffect { layer: l, target: LayerTarget::Content, effect: e[0], name: "   ".into() }).is_err());
    assert!(s.execute(Command::RenameEffect { layer: l, target: LayerTarget::Content, effect: m[0], name: "x".into() }).is_err(), "wrong stack");
    assert_eq!(s.document(), &doc);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().find(l).unwrap().mask.as_ref().unwrap().effects[0].name, "Invert");
}

#[test]
fn dropping_several_effects_reorders_or_copies_them_inside_their_stack() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let e = add_content(&mut s, l, 5);
    let domain = LayerTarget::Content;

    // Drag e1 with e3 also selected, drop above e4: both go, order kept, the grabbed one selected.
    let before = steps(&s);
    s.execute(Command::DropEffects {
        source: address(&s, l, domain, e[1]),
        effects: vec![e[3], e[1]],
        to: EffectDrop::Beside { effect: e[4], side: DropSide::Above },
        copy: false,
    })
    .unwrap();
    assert_eq!(content(&s, l), [e[0], e[2], e[4], e[1], e[3]]);
    assert_eq!((steps(&s), s.selected_effect()), (before + 1, Some(e[1])));
    s.execute(Command::Undo).unwrap();

    // The grabbed effect always goes, even when the extra list does not name it.
    s.execute(Command::DropEffects { source: address(&s, l, domain, e[4]), effects: vec![], to: EffectDrop::Beside { effect: e[0], side: DropSide::Below }, copy: false }).unwrap();
    assert_eq!(content(&s, l), [e[4], e[0], e[1], e[2], e[3]]);
    s.execute(Command::Undo).unwrap();

    // Unchanged, self-anchored and stale drops add no undo step.
    let before = steps(&s);
    for (source, to) in [
        (address(&s, l, domain, e[1]), EffectDrop::Beside { effect: e[2], side: DropSide::Below }),
        (address(&s, l, domain, e[1]), EffectDrop::Beside { effect: e[1], side: DropSide::Above }),
        (address(&s, l, domain, e[1]), EffectDrop::Beside { effect: EffectId::new(), side: DropSide::Above }),
        (address(&s, l, domain, EffectId::new()), EffectDrop::Beside { effect: e[2], side: DropSide::Above }),
        (address(&s, LayerId::new(), domain, e[1]), EffectDrop::Beside { effect: e[2], side: DropSide::Above }),
        (address(&s, l, LayerTarget::Mask, e[1]), EffectDrop::Beside { effect: e[2], side: DropSide::Above }),
        (address(&s, l, domain, e[4]), EffectDrop::Layer(l)),
        (address(&s, l, domain, e[1]), EffectDrop::Layer(LayerId::new())),
    ] {
        s.execute(Command::DropEffects { source, effects: vec![], to, copy: false }).unwrap();
    }
    assert_eq!((content(&s, l), steps(&s)), (e.clone(), before));

    // Dropped on its own layer row: to the top of the stack.
    s.execute(Command::DropEffects { source: address(&s, l, domain, e[0]), effects: vec![e[1]], to: EffectDrop::Layer(l), copy: false }).unwrap();
    assert_eq!(content(&s, l), [e[2], e[3], e[4], e[0], e[1]]);
    s.execute(Command::Undo).unwrap();

    // Alt-drag: the originals stay, copies land at the drop position, the grabbed one's copy is selected.
    s.execute(Command::DropEffects {
        source: address(&s, l, domain, e[2]),
        effects: vec![e[0]],
        to: EffectDrop::Beside { effect: e[2], side: DropSide::Above },
        copy: true,
    })
    .unwrap();
    let now = content(&s, l);
    assert_eq!(now.len(), 7);
    assert_eq!((now[0], now[1], now[2], now[5], now[6]), (e[0], e[1], e[2], e[3], e[4]));
    assert!(!e.contains(&now[3]) && !e.contains(&now[4]));
    assert_eq!(s.selected_effect(), Some(now[4]), "the copy of the grabbed effect");
    s.execute(Command::Undo).unwrap();
    assert_eq!(content(&s, l), e, "one undo step removes both copies");
}

#[test]
fn effects_move_or_copy_to_another_layer_and_mask_effects_create_the_mask() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let b = add_layer(&mut s, NewLayer::Fill);
    let folder = add_layer(&mut s, NewLayer::Folder);
    let ea = add_content(&mut s, a, 3);
    let eb = add_content(&mut s, b, 1);

    // Move two content effects from a onto b's row: on top of b's stack, in order, b selected.
    let before = steps(&s);
    s.execute(Command::DropEffects { source: address(&s, a, LayerTarget::Content, ea[2]), effects: vec![ea[0]], to: EffectDrop::Layer(b), copy: false }).unwrap();
    assert_eq!(content(&s, a), [ea[1]]);
    assert_eq!(content(&s, b), [eb[0], ea[0], ea[2]]);
    assert_eq!((s.selection().layer, s.selected_effect(), s.layer_target()), (Some(b), Some(ea[2]), LayerTarget::Content));
    assert_eq!(steps(&s), before + 1, "one undo step for both layers");
    s.execute(Command::Undo).unwrap();
    assert_eq!((content(&s, a), content(&s, b)), (ea.clone(), eb.clone()));

    // Alt: a copy goes over, the source keeps its effect.
    s.execute(Command::DropEffects { source: address(&s, a, LayerTarget::Content, ea[1]), effects: vec![], to: EffectDrop::Layer(b), copy: true }).unwrap();
    assert_eq!(content(&s, a), ea);
    let now = content(&s, b);
    assert_eq!((now.len(), now[0]), (2, eb[0]));
    assert_eq!(s.selected_effect(), Some(now[1]));
    assert!(!ea.contains(&now[1]));

    // Adjustments may land on a folder; a paint effect may not, and nothing changes.
    s.execute(Command::DropEffects { source: address(&s, a, LayerTarget::Content, ea[0]), effects: vec![], to: EffectDrop::Layer(folder), copy: false }).unwrap();
    assert_eq!(content(&s, folder), [ea[0]]);
    s.execute(Command::SelectEffect { layer: a, target: LayerTarget::Content, effect: None }).unwrap();
    s.execute(Command::AddContentEffect { layer: a, kind: ContentEffectKind::Paint { channels: Default::default() } }).unwrap();
    let paint = s.selected_effect().unwrap();
    let doc = s.document().clone();
    assert!(s.execute(Command::DropEffects { source: address(&s, a, LayerTarget::Content, paint), effects: vec![ea[1]], to: EffectDrop::Layer(folder), copy: false }).is_err());
    assert_eq!(s.document(), &doc);

    // Mask effects dropped on a layer without a mask give it one and switch to its mask stack.
    let m = add_mask(&mut s, a, 2);
    assert!(s.selected_texture_set().find(b).unwrap().mask.is_none());
    s.execute(Command::DropEffects { source: address(&s, a, LayerTarget::Mask, m[0]), effects: vec![m[1]], to: EffectDrop::Layer(b), copy: false }).unwrap();
    assert_eq!(mask(&s, b), m);
    assert!(mask(&s, a).is_empty() && s.selected_texture_set().find(a).unwrap().mask.is_some(), "the source keeps its (now empty) mask");
    assert_eq!((s.selection().layer, s.selected_effect(), s.layer_target()), (Some(b), Some(m[0]), LayerTarget::Mask));
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(b).unwrap().mask.is_none());
    assert_eq!(mask(&s, a), m);
}
