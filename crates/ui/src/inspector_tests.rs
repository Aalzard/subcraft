//! Exercise the actual dock, layer rows and inspectors with pointer input.
use super::*;
use egui::{Event, Pos2, Rect};
use subcraft_core::LayerId;

fn setup() -> (SubcraftApp, egui::Context, [LayerId; 3]) {
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let paint = session.selection().layer.unwrap();
    let mut ids = [paint; 3];
    for (index, (name, color)) in [
        ("Blue fill", ChannelValue::rgb(0.0, 0.0, 1.0)),
        ("Red fill", ChannelValue::rgb(1.0, 0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    {
        session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        let id = session.selection().layer.unwrap();
        ids[index + 1] = id;
        session
            .execute(Command::Rename {
                layer: id,
                name: name.into(),
            })
            .unwrap();
        session
            .execute(Command::SetFillChannel {
                layer: id,
                channel: ChannelKind::BaseColor,
                fill: FillChannel::new(FillSource::uniform(color)),
            })
            .unwrap();
    }
    session.execute(Command::SelectLayer(Some(paint))).unwrap();
    session.execute(Command::Seal).unwrap();
    let mut dock = default_layout();
    dock.set_active_tab(dock.find_tab(&Tab::Brush).unwrap())
        .unwrap();
    let app = SubcraftApp {
        inspector_selection: Some(InspectorSelection::current(&session)),
        state: UiState::new(session, None),
        dock,
        frames: 0,
        screenshot: None,
        screenshot_requested: false,
        pending_now: None,
        active_tabs: HashSet::new(),
        test_crash: false,
        profile: None,
    };
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (app, ctx, ids)
}

fn frame(app: &mut SubcraftApp, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1100.0))),
            events,
            ..Default::default()
        },
        |ui| app.workspace(ui),
    );
    out.textures_delta.clear();
    out
}

fn settle(app: &mut SubcraftApp, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 {
        frame(app, ctx, vec![]);
    }
    frame(app, ctx, vec![])
}

fn text(out: &egui::FullOutput, label: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

fn click_text(app: &mut SubcraftApp, ctx: &egui::Context, label: &str) {
    let out = settle(app, ctx);
    let pos = text(&out, label).unwrap_or_else(|| panic!("missing visible control: {label}"));
    click_at(app, ctx, pos);
}

fn click_at(app: &mut SubcraftApp, ctx: &egui::Context, pos: Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

fn active(app: &SubcraftApp, tab: Tab) -> bool {
    let path = app.dock.find_tab(&tab).unwrap();
    app.dock[path.surface][path.node].get_leaf().unwrap().active == path.tab
}

#[test]
fn clicking_paint_fill_and_another_fill_reveals_the_selected_layers_controls() {
    let (mut app, ctx, [paint, blue, red]) = setup();
    let before = app.state.session.document().clone();
    let history = app.state.session.history().entries().len();
    assert!(active(&app, Tab::Brush));
    click_text(&mut app, &ctx, "Red fill");
    assert_eq!(app.state.session.selection().layer, Some(red));
    assert!(active(&app, Tab::Properties));
    let out = settle(&mut app, &ctx);
    assert!(text(&out, "PROJECTION").is_some());
    assert!(text(&out, "Paint whole material…").is_none());
    click_text(&mut app, &ctx, "Blue fill");
    assert_eq!(app.state.session.selection().layer, Some(blue));
    assert!(active(&app, Tab::Properties));
    assert_eq!(app.state.rename.as_ref().map(|(id, _)| *id), Some(blue));
    click_text(&mut app, &ctx, "Layer 1");
    assert_eq!(app.state.session.selection().layer, Some(paint));
    assert!(active(&app, Tab::Brush));
    let out = settle(&mut app, &ctx);
    assert!(text(&out, "PAINT TARGET").is_some());
    assert!(text(&out, "PROJECTION").is_none());
    assert_eq!(app.state.session.document(), &before);
    assert_eq!(app.state.session.history().entries().len(), history);
}

fn assert_color(app: &mut SubcraftApp, expected: [f32; 3]) {
    app.state.session.update_composites();
    let image = app
        .state
        .session
        .composite(app.state.session.selection().texture_set)
        .unwrap()
        .channel(ChannelKind::BaseColor)
        .unwrap();
    for y in 0..64 {
        for x in 0..64 {
            let pixel = image.get(x, y);
            for i in 0..3 {
                assert!(
                    (pixel[i] - expected[i]).abs() < 0.001,
                    "pixel ({x},{y}): {pixel:?}"
                );
            }
        }
    }
}

#[test]
fn fill_controls_edit_only_the_selected_fill_with_numeric_output_and_one_undo() {
    let (mut app, ctx, [paint, blue, red]) = setup();
    let other_fill = app
        .state
        .session
        .selected_texture_set()
        .find(blue)
        .unwrap()
        .clone();
    let paint_layer = app
        .state
        .session
        .selected_texture_set()
        .find(paint)
        .unwrap()
        .clone();
    click_text(&mut app, &ctx, "Red fill");
    let brush = app.state.session.brush.material.clone();
    assert_color(&mut app, [1.0, 0.0, 0.0]);
    let history = app.state.session.history().entries().len();
    let out = settle(&mut app, &ctx);
    let path = app.dock.find_tab(&Tab::Properties).unwrap();
    let bounds = app.dock[path.surface][path.node]
        .get_leaf()
        .unwrap()
        .viewport;
    let toggle = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == "color" && bounds.contains(t.pos) => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("selected fill's Base Color toggle");
    click_at(&mut app, &ctx, toggle);
    assert_eq!(app.state.session.selection().layer, Some(red));
    assert_color(&mut app, [0.0, 0.0, 1.0]);
    assert_eq!(app.state.session.history().entries().len(), history + 1);
    assert_eq!(
        app.state.session.selected_texture_set().find(blue).unwrap(),
        &other_fill
    );
    assert_eq!(
        app.state
            .session
            .selected_texture_set()
            .find(paint)
            .unwrap(),
        &paint_layer
    );
    assert_eq!(app.state.session.brush.material, brush);
    app.state.session.execute(Command::Undo).unwrap();
    assert_color(&mut app, [1.0, 0.0, 0.0]);
    app.state.session.execute(Command::Redo).unwrap();
    assert_color(&mut app, [0.0, 0.0, 1.0]);
}

#[test]
fn manual_tabs_persist_until_selection_changes_in_a_rearranged_dock() {
    let (mut app, _ctx, [paint, blue, _red]) = setup();
    app.dock = DockState::new(vec![Tab::Brush, Tab::Viewport, Tab::Properties]);
    app.state
        .session
        .execute(Command::SelectLayer(Some(blue)))
        .unwrap();
    assert!(app.follow_layer_selection());
    assert!(active(&app, Tab::Properties));
    app.dock
        .set_active_tab(app.dock.find_tab(&Tab::Brush).unwrap())
        .unwrap();
    for _ in 0..3 {
        assert!(!app.follow_layer_selection());
    }
    assert!(
        active(&app, Tab::Brush),
        "explicit Brush tab remains available for mask painting"
    );
    app.state
        .session
        .execute(Command::SelectLayer(Some(paint)))
        .unwrap();
    app.follow_layer_selection();
    app.dock
        .set_active_tab(app.dock.find_tab(&Tab::Properties).unwrap())
        .unwrap();
    assert!(!app.follow_layer_selection());
    assert!(
        active(&app, Tab::Properties),
        "paint-layer mask/part properties remain accessible"
    );
    app.state
        .session
        .execute(Command::SelectLayer(None))
        .unwrap();
    app.follow_layer_selection();
    assert!(active(&app, Tab::Properties));
    app.state
        .session
        .execute(Command::AddLayer(NewLayer::Folder))
        .unwrap();
    app.follow_layer_selection();
    assert!(active(&app, Tab::Properties));
    app.state
        .session
        .execute(Command::DeleteLayer(
            app.state.session.selection().layer.unwrap(),
        ))
        .unwrap();
    app.follow_layer_selection();
    app.state
        .set_session(Session::new(subcraft_mesh::primitives::cube(1.0), 64));
    app.follow_layer_selection();
    assert!(active(&app, Tab::Brush));
}

fn thumbnail(app: &mut SubcraftApp, ctx: &egui::Context, name: &str, mask: bool) -> Pos2 {
    let out = settle(app, ctx);
    let row = text(&out, name).unwrap();
    let mut squares: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - 26.0).abs() < 0.01
                    && (r.rect.height() - 26.0).abs() < 0.01
                    && r.stroke.width > 0.0
                    && (r.rect.center().y - row.y).abs() < 5.0 =>
            {
                Some(r.rect.center())
            }
            _ => None,
        })
        .collect();
    squares.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        squares.len(),
        2,
        "content and mask are distinct square controls"
    );
    squares[usize::from(mask)]
}

fn click_modified(
    app: &mut SubcraftApp,
    ctx: &egui::Context,
    pos: Pos2,
    modifiers: egui::Modifiers,
) {
    for pressed in [true, false] {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1100.0))),
                events: vec![
                    Event::ModifiersChanged(modifiers),
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers,
                    },
                ],
                ..Default::default()
            },
            |ui| app.workspace(ui),
        );
        out.textures_delta.clear();
    }
    frame(app, ctx, vec![Event::ModifiersChanged(Default::default())]);
}

#[test]
fn adjacent_thumbnails_select_distinct_domains_and_paint_masks_open_their_own_inspector() {
    let (mut app, ctx, [paint, _, red]) = setup();
    for layer in [paint, red] {
        app.state
            .session
            .execute(Command::AddMask { layer, white: true })
            .unwrap();
    }
    let before = app.state.session.document().clone();
    let history = app.state.session.history().entries().len();
    for (name, layer) in [("Red fill", red), ("Layer 1", paint)] {
        let mask = thumbnail(&mut app, &ctx, name, true);
        click_at(&mut app, &ctx, mask);
        assert_eq!(app.state.session.selection().layer, Some(layer));
        assert_eq!(
            app.state.session.layer_target(),
            subcraft_app::LayerTarget::Mask
        );
        assert!(active(&app, Tab::Properties));
        let out = settle(&mut app, &ctx);
        assert!(text(&out, "Paint value").is_some());
        assert!(text(&out, "PROJECTION").is_none());
        let content = thumbnail(&mut app, &ctx, name, false);
        click_at(&mut app, &ctx, content);
        assert_eq!(
            app.state.session.layer_target(),
            subcraft_app::LayerTarget::Content
        );
        assert!(active(
            &app,
            if layer == paint {
                Tab::Brush
            } else {
                Tab::Properties
            }
        ));
    }
    assert_eq!(app.state.session.document(), &before);
    assert_eq!(app.state.session.history().entries().len(), history);
}

#[test]
fn mask_thumbnail_shortcuts_and_properties_dispatch_undoable_mask_actions() {
    let (mut app, ctx, [paint, _, red]) = setup();
    app.state
        .session
        .execute(Command::AddMask {
            layer: red,
            white: true,
        })
        .unwrap();
    let before = app.state.session.history().entries().len();
    let at = thumbnail(&mut app, &ctx, "Red fill", true);
    click_modified(
        &mut app,
        &ctx,
        at,
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert!(
        !app.state
            .session
            .selected_texture_set()
            .find(red)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .enabled
    );
    assert_eq!(app.state.session.selection().layer, Some(paint));
    assert_eq!(app.state.session.history().entries().len(), before + 1);
    let at = thumbnail(&mut app, &ctx, "Red fill", true);
    click_modified(
        &mut app,
        &ctx,
        at,
        egui::Modifiers {
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(app.state.layer_previews.inspect, Some(red));
    assert_eq!(
        app.state.session.layer_target(),
        subcraft_app::LayerTarget::Mask
    );
    click_text(&mut app, &ctx, "Copy");
    assert!(app.state.session.can_paste_mask());
    let history = app.state.session.history().entries().len();
    click_text(&mut app, &ctx, "Invert");
    assert_eq!(app.state.session.history().entries().len(), history + 1);
    let mask = app
        .state
        .session
        .selected_layer()
        .unwrap()
        .mask
        .as_ref()
        .unwrap();
    assert_eq!(
        mask.base.get(0, 0)[0],
        1.0,
        "inversion must preserve authored pixels"
    );
    assert_eq!(mask.effects.len(), 1);
    click_text(&mut app, &ctx, "Remove mask");
    assert!(app.state.session.selected_layer().unwrap().mask.is_none());
    assert_eq!(
        app.state.session.layer_target(),
        subcraft_app::LayerTarget::Content
    );
    app.state.session.execute(Command::Undo).unwrap();
    assert_eq!(
        app.state
            .session
            .selected_layer()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .effects
            .len(),
        1
    );
}

#[test]
fn effect_rows_add_select_edit_and_route_paint_effects_to_the_brush() {
    let (mut app, ctx, [_, _, red]) = setup();
    click_text(&mut app, &ctx, "Red fill");
    assert!(active(&app, Tab::Properties));
    let history = app.state.session.history().entries().len();
    let out = settle(&mut app, &ctx);
    assert!(text(&out, "Content effects").is_some(), "the selected layer shows its content stack");
    click_text(&mut app, &ctx, &format!("{} Add", icons::PLUS));
    click_text(&mut app, &ctx, "Levels");
    let layer = app.state.session.selected_layer().unwrap().clone();
    assert_eq!(layer.id, red);
    assert_eq!(layer.content_effects.len(), 1);
    let levels = layer.content_effects[0].id;
    assert_eq!(app.state.session.selected_effect(), Some(levels));
    assert_eq!(app.state.session.history().entries().len(), history + 1);
    // The effect's own Properties replace the fill material controls.
    assert!(active(&app, Tab::Properties));
    let out = settle(&mut app, &ctx);
    assert!(text(&out, "BLENDING PER CHANNEL").is_some());
    assert!(text(&out, "PROJECTION").is_none());
    assert!(text(&out, "Levels (Base Color)").is_some(), "nested row under the layer");
    // Per-channel opacity sliders and their values fit inside the inspector (not past the edge).
    let values = out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "1.00")).count();
    let inside = out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "1.00" && t.pos.x + t.galley.size().x <= 1600.0 && s.clip_rect.contains(t.pos))).count();
    assert!(values >= 4 && inside == values, "{inside} of {values} opacity values visible");
    click_text(&mut app, &ctx, "Visible");
    assert!(!app.state.session.selected_layer().unwrap().content_effects[0].visible);
    assert_eq!(app.state.session.history().entries().len(), history + 2);
    // Back to the layer, then select the effect again from its row.
    click_text(&mut app, &ctx, "Back to layer");
    assert_eq!(app.state.session.selected_effect(), None);
    assert!(text(&settle(&mut app, &ctx), "PROJECTION").is_some());
    click_text(&mut app, &ctx, "Levels (Base Color)");
    assert_eq!(app.state.session.selected_effect(), Some(levels));
    // A Paint effect takes the brush, even on a fill layer.
    click_text(&mut app, &ctx, &format!("{} Add", icons::PLUS));
    click_text(&mut app, &ctx, "Paint");
    let paint = app.state.session.selected_layer().unwrap().content_effects[1].id;
    assert_eq!(app.state.session.brush.target, subcraft_paint::PaintTarget::ContentEffect(paint));
    assert!(active(&app, Tab::Brush));
    assert_eq!(app.state.session.history().entries().len(), history + 3);
}
