//! Real egui drag events, without moving the owner's desktop pointer.
use egui::{Event, Pos2, Rect};
use subcraft_app::{Command, LayerTarget, NewLayer, Session, effect_stack::EffectAddress};
use subcraft_core::{ChannelKind, EffectId};
use subcraft_doc::{ContentEffectKind, Levels, MaskEffectKind};
use subcraft_ui::{
    panels::{layers, shelf::ShelfDrag},
    state::UiState,
    theme,
};
fn fixture(mask: bool, count: usize) -> (UiState, egui::Context, Vec<EffectId>) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    if mask {
        s.execute(Command::AddMask {
            layer,
            white: false,
        })
        .unwrap();
    }
    let mut ids = Vec::new();
    for i in 0..count {
        s.execute(if mask {
            Command::AddMaskEffect {
                layer,
                kind: MaskEffectKind::Levels(Levels::default()),
            }
        } else {
            Command::AddContentEffect {
                layer,
                kind: ContentEffectKind::Levels {
                    channel: ChannelKind::BaseColor,
                    levels: Levels::default(),
                },
            }
        })
        .unwrap();
        let id = s.selected_effect().unwrap();
        ids.push(id);
        if mask {
            let mut effect = s
                .selected_layer()
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .effects
                .last()
                .unwrap()
                .as_ref()
                .clone();
            effect.name = format!("Step {i}");
            s.execute(Command::SetMaskEffect { layer, effect }).unwrap();
        } else {
            let mut effect = s
                .selected_layer()
                .unwrap()
                .content_effects
                .last()
                .unwrap()
                .as_ref()
                .clone();
            effect.name = format!("Step {i}");
            s.execute(Command::SetContentEffect { layer, effect })
                .unwrap();
        }
    }
    s.execute(Command::SelectLayerTarget {
        layer,
        target: if mask {
            LayerTarget::Mask
        } else {
            LayerTarget::Content
        },
    })
    .unwrap();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (UiState::new(s, None), ctx, ids)
}
fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(340.0, 500.0))),
            events,
            ..Default::default()
        },
        |ui| layers::show(state, ui),
    );
    out.textures_delta.clear();
    out
}
fn layout(state: &mut UiState, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 {
        frame(state, ctx, vec![]);
    }
    frame(state, ctx, vec![])
}
fn label(out: &egui::FullOutput, name: &str) -> Rect {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}
fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}
fn begin(state: &mut UiState, ctx: &egui::Context, pos: Pos2) {
    frame(
        state,
        ctx,
        vec![Event::PointerMoved(pos), button(pos, true)],
    );
    frame(
        state,
        ctx,
        vec![Event::PointerMoved(pos + egui::vec2(8.0, 0.0))],
    );
    assert!(egui::DragAndDrop::has_payload_of_type::<EffectAddress>(ctx));
}
fn order(state: &UiState, mask: bool) -> Vec<EffectId> {
    let layer = state.session.selected_layer().unwrap();
    if mask {
        layer
            .mask
            .as_ref()
            .unwrap()
            .effects
            .iter()
            .map(|e| e.id)
            .collect()
    } else {
        layer.content_effects.iter().map(|e| e.id).collect()
    }
}
#[test]
fn drag_names_both_directions_and_domains_selects_the_moved_effect_with_one_undo() {
    for mask in [false, true] {
        let (mut state, ctx, ids) = fixture(mask, 3);
        let original = state.session.document().clone();
        let history = state.session.history().entries().len();
        let out = layout(&mut state, &ctx);
        let start = label(&out, "Step 0").center();
        let end = label(&out, "Step 2").center() - egui::vec2(0.0, 8.0);
        begin(&mut state, &ctx, start);
        let hover = frame(&mut state, &ctx, vec![Event::PointerMoved(end)]);
        assert_eq!(state.session.document(), &original);
        assert_eq!(state.session.history().entries().len(), history);
        assert!(hover.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::LineSegment {stroke,..} if stroke.width==2.0 && stroke.color==theme::ACCENT)),"insertion line must be visible");
        frame(&mut state, &ctx, vec![button(end, false)]);
        assert_eq!(order(&state, mask), [ids[1], ids[2], ids[0]]);
        assert_eq!(state.session.selected_effect(), Some(ids[0]));
        assert_eq!(state.session.history().entries().len(), history + 1);
        state.session.execute(Command::Undo).unwrap();
        assert_eq!(state.session.document(), &original);
        let out = layout(&mut state, &ctx);
        let start = label(&out, "Step 2").center();
        let end = label(&out, "Step 0").center() + egui::vec2(0.0, 8.0);
        begin(&mut state, &ctx, start);
        frame(&mut state, &ctx, vec![Event::PointerMoved(end)]);
        frame(&mut state, &ctx, vec![button(end, false)]);
        assert_eq!(order(&state, mask), [ids[2], ids[0], ids[1]]);
    }
}
#[test]
fn drag_thumbnail_reorders_without_becoming_a_click_or_moving_the_layer() {
    let (mut state, ctx, ids) = fixture(true, 3);
    let out = layout(&mut state, &ctx);
    let name = label(&out, "Step 0");
    let start = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r)
                if r.stroke.width > 0.0
                    && r.rect.width() == 20.0
                    && r.rect.height() == 20.0
                    && (r.rect.center().y - name.center().y).abs() < 2.0 =>
            {
                Some(r.rect.center())
            }
            _ => None,
        })
        .unwrap();
    let layer_order: Vec<_> = state
        .session
        .selected_texture_set()
        .layers
        .iter()
        .map(|l| l.id)
        .collect();
    let end = label(&out, "Step 2").center() - egui::vec2(0.0, 8.0);
    begin(&mut state, &ctx, start);
    frame(&mut state, &ctx, vec![Event::PointerMoved(end)]);
    frame(&mut state, &ctx, vec![button(end, false)]);
    assert_eq!(order(&state, true), [ids[1], ids[2], ids[0]]);
    assert_eq!(state.session.selected_effect(), Some(ids[0]));
    assert_eq!(state.session.layer_target(), LayerTarget::Mask);
    assert_eq!(
        layer_order,
        state
            .session
            .selected_texture_set()
            .layers
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>()
    );
}
#[test]
fn escape_and_outside_release_cancel_without_document_or_history_changes() {
    for escape in [false, true] {
        let (mut state, ctx, _) = fixture(false, 3);
        let before = state.session.document().clone();
        let history = state.session.history().entries().len();
        let out = layout(&mut state, &ctx);
        begin(&mut state, &ctx, label(&out, "Step 0").center());
        let end = if escape {
            label(&out, "Step 2").center() - egui::vec2(0.0, 8.0)
        } else {
            egui::pos2(700.0, 650.0)
        };
        frame(&mut state, &ctx, vec![Event::PointerMoved(end)]);
        if escape {
            frame(
                &mut state,
                &ctx,
                vec![Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            assert!(!egui::DragAndDrop::has_any_payload(&ctx));
        }
        frame(&mut state, &ctx, vec![button(end, false)]);
        assert_eq!(state.session.document(), &before);
        assert_eq!(state.session.history().entries().len(), history);
    }
}
#[test]
fn self_and_adjacent_noop_drops_do_not_create_history() {
    for target in ["Step 0", "Step 1"] {
        let (mut state, ctx, _) = fixture(false, 3);
        let before = state.session.document().clone();
        let history = state.session.history().entries().len();
        let out = layout(&mut state, &ctx);
        begin(&mut state, &ctx, label(&out, "Step 0").center());
        let end = label(&out, target).center() + egui::vec2(0.0, 8.0);
        frame(&mut state, &ctx, vec![Event::PointerMoved(end)]);
        frame(&mut state, &ctx, vec![button(end, false)]);
        assert_eq!(state.session.document(), &before);
        assert_eq!(state.session.history().entries().len(), history);
    }
}
#[test]
fn effect_targets_do_not_consume_shelf_payloads_or_accept_foreign_domains() {
    let (mut state, ctx, ids) = fixture(false, 3);
    let out = layout(&mut state, &ctx);
    let pos = label(&out, "Step 2").center() - egui::vec2(0.0, 8.0);
    let before = state.session.document().clone();
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    let mut retained = false;
    let mut result = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(340.0, 500.0))),
            events: vec![Event::PointerMoved(pos), button(pos, false)],
            ..Default::default()
        },
        |ui| {
            layers::show(&mut state, ui);
            retained = egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(&ctx);
        },
    );
    result.textures_delta.clear();
    assert!(retained);
    assert_eq!(state.session.document(), &before);
    let source = EffectAddress {
        texture_set: state.session.selection().texture_set,
        layer: state.session.selection().layer.unwrap(),
        domain: LayerTarget::Mask,
        effect: ids[0],
    };
    egui::DragAndDrop::set_payload(&ctx, source);
    frame(
        &mut state,
        &ctx,
        vec![Event::PointerMoved(pos), button(pos, false)],
    );
    assert_eq!(state.session.document(), &before);
}
#[test]
fn holding_an_effect_near_the_list_edge_scrolls_without_editing() {
    let (mut state, ctx, ids) = fixture(false, 24);
    let out = layout(&mut state, &ctx);
    let top = label(&out, "Step 23");
    let clip = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Step 23" => Some(s.clip_rect),
            _ => None,
        })
        .unwrap();
    let source = EffectAddress {
        texture_set: state.session.selection().texture_set,
        layer: state.session.selection().layer.unwrap(),
        domain: LayerTarget::Content,
        effect: ids[23],
    };
    egui::DragAndDrop::set_payload(&ctx, source);
    let before = state.session.document().clone();
    let pos = egui::pos2(250.0, clip.bottom() - 2.0);
    for _ in 0..40 {
        frame(&mut state, &ctx, vec![Event::PointerMoved(pos)]);
    }
    let out = frame(&mut state, &ctx, vec![]);
    let first = out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == "Step 23" => Some(t.pos.y),
        _ => None,
    });
    assert!(first.is_none_or(|y| y < top.top() - 10.0));
    assert_eq!(state.session.document(), &before);
    egui::DragAndDrop::clear_payload(&ctx);
}
