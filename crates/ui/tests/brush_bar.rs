//! Real egui input and geometry checks for the responsive top bar.
use egui::{Event, Pos2};
use subcraft_app::{Action, Session};
use subcraft_ui::{
    state::{Tool, UiState},
    viewport_bar,
};

fn setup() -> (UiState, egui::Context) {
    let state = UiState::new(
        Session::new(subcraft_mesh::primitives::cube(2.0), 128),
        None,
    );
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    (state, ctx)
}

fn frame(
    state: &mut UiState,
    ctx: &egui::Context,
    width: f32,
    events: Vec<Event>,
) -> (egui::FullOutput, egui::Rect) {
    let mut bounds = egui::Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            ..Default::default()
        },
        |ui| {
            viewport_bar::show(ui, state);
            bounds = ui.min_rect();
        },
    );
    output.textures_delta.clear();
    (output, bounds)
}

fn position(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == label => {
            Some(t.pos + t.galley.size() * 0.5)
        }
        _ => None,
    })
}

#[test]
fn bar_fits_wide_wrapped_and_tiny_docks_and_hides_for_non_paint_tools() {
    for width in [
        1280.0, 1125.0, 1020.0, 1000.0, 980.0, 700.0, 540.0, 520.0, 480.0, 420.0, 300.0, 200.0,
    ] {
        let (mut state, ctx) = setup();
        state.session.brush.settings.size = 100.0;
        state.session.brush.settings.hardness = 1.0;
        let mut last = None;
        for _ in 0..4 {
            last = Some(frame(&mut state, &ctx, width, vec![]));
        }
        let (output, bounds) = last.unwrap();
        assert!(
            bounds.right() <= width + 0.5,
            "bar overflow at {width}: {bounds:?}"
        );
        let brush = position(&output, subcraft_ui::icons::PAINT_BRUSH)
            .expect("tool icons always reachable");
        let view = position(&output, if width >= 540.0 { "Resolution" } else { "View" }).unwrap();
        if width >= 1000.0 {
            assert!((brush.y - view.y).abs() < 3.0, "inline on wide docks");
        } else {
            assert!(brush.y > view.y + 15.0, "stacked rows on narrow docks");
        }
        if width >= 540.0 {
            assert!(position(&output, "Anti-aliasing").is_some());
            assert!(position(&output, "AA").is_none());
            for label in ["Size", "Flow", "Opacity", "Hardness"] {
                assert!(
                    position(&output, label).is_some(),
                    "missing {label} at {width}"
                );
            }
        }
    }
    let (mut state, ctx) = setup();
    for tool in [Tool::Pen, Tool::Eyedropper] {
        state.tool = tool;
        let (output, _) = frame(&mut state, &ctx, 1125.0, vec![]);
        assert!(position(&output, "Flow").is_none());
        assert!(position(&output, "Resolution").is_some());
    }
}

fn pointer(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn opacity_drag_edits_shared_settings_and_shortcuts_update_the_same_bar_without_history() {
    let (mut state, ctx) = setup();
    state.session.brush.settings.stroke_opacity = 0.73;
    let before = state.session.document().clone();
    let count = state.session.history().entries().len();
    for _ in 0..3 {
        frame(&mut state, &ctx, 1125.0, vec![]);
    }
    let (output, _) = frame(&mut state, &ctx, 1125.0, vec![]);
    let start = position(&output, "73").expect("unboxed percentage");
    frame(
        &mut state,
        &ctx,
        1125.0,
        vec![Event::PointerMoved(start), pointer(start, true)],
    );
    for distance in [8.0, 16.0, 24.0] {
        frame(
            &mut state,
            &ctx,
            1125.0,
            vec![Event::PointerMoved(start + egui::vec2(distance, 0.0))],
        );
    }
    frame(
        &mut state,
        &ctx,
        1125.0,
        vec![pointer(start + egui::vec2(24.0, 0.0), false)],
    );
    assert!(state.session.brush.settings.stroke_opacity > 0.73);
    assert_eq!(
        state
            .session
            .brush_preset("Top bar")
            .settings
            .stroke_opacity,
        state.session.brush.settings.stroke_opacity
    );
    assert_eq!(state.session.document(), &before);
    assert_eq!(state.session.history().entries().len(), count);
    state.session.brush.settings.size = 12.0;
    state.session.perform(Action::BrushLarger).unwrap();
    let (output, _) = frame(&mut state, &ctx, 1125.0, vec![]);
    assert!(
        position(&output, "13.8").is_some(),
        "bar reads live app values"
    );
}

#[test]
fn tiny_brush_menu_exposes_all_four_settings() {
    let (mut state, ctx) = setup();
    for _ in 0..3 {
        frame(&mut state, &ctx, 300.0, vec![]);
    }
    let (output, _) = frame(&mut state, &ctx, 300.0, vec![]);
    let p = position(&output, subcraft_ui::icons::SLIDERS).unwrap();
    frame(
        &mut state,
        &ctx,
        300.0,
        vec![Event::PointerMoved(p), pointer(p, true)],
    );
    frame(&mut state, &ctx, 300.0, vec![pointer(p, false)]);
    let (output, _) = frame(&mut state, &ctx, 300.0, vec![]);
    for text in ["Size", "Flow", "Opacity", "Hardness"] {
        assert!(position(&output, text).is_some(), "menu missing {text}");
    }
}

fn click(state: &mut UiState, ctx: &egui::Context, pos: Pos2) {
    frame(
        state,
        ctx,
        1125.0,
        vec![Event::PointerMoved(pos), pointer(pos, true)],
    );
    frame(state, ctx, 1125.0, vec![pointer(pos, false)]);
}

#[test]
fn icons_select_each_tool_without_touching_document_or_settings() {
    let (mut state, ctx) = setup();
    let doc = state.session.document().clone();
    let settings = state.session.brush.settings;
    for (icon, tool) in [
        (subcraft_ui::icons::ERASER, Tool::Eraser),
        (subcraft_ui::icons::PAINT_BUCKET, Tool::PolygonFill),
        (subcraft_ui::icons::PAINT_BRUSH, Tool::Brush),
    ] {
        for _ in 0..3 {
            frame(&mut state, &ctx, 1125.0, vec![]);
        }
        let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
        click(&mut state, &ctx, position(&out, icon).unwrap());
        assert_eq!(state.tool, tool);
        assert_eq!(state.session.brush.settings, settings);
        assert_eq!(state.session.document(), &doc);
        if tool == Tool::PolygonFill {
            let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
            click(&mut state, &ctx, position(&out, "Flow").unwrap());
            let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
            assert!(
                position(&out, "Paint per stamp").is_none(),
                "Fill disables flow popover"
            );
        }
    }
}

#[test]
fn size_is_scrubbable_without_a_resting_box_or_inline_slider() {
    let (mut state, ctx) = setup();
    state.session.brush.settings.size = 12.5;
    for _ in 0..3 {
        frame(&mut state, &ctx, 1125.0, vec![]);
    }
    let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
    let start = position(&out, "12.5").unwrap();
    for s in &out.shapes {
        if let egui::epaint::Shape::Rect(r) = &s.shape {
            // The full-width toolbar background is allowed; no number-field rectangle.
            assert!(
                !(r.rect.contains(start)
                    && r.rect.width() < 100.0
                    && (r.fill != egui::Color32::TRANSPARENT || r.stroke.width > 0.0)),
                "boxed size: {r:?}"
            );
        }
    }
    frame(
        &mut state,
        &ctx,
        1125.0,
        vec![Event::PointerMoved(start), pointer(start, true)],
    );
    for x in [10.0, 20.0, 30.0] {
        frame(
            &mut state,
            &ctx,
            1125.0,
            vec![Event::PointerMoved(start + egui::vec2(x, 0.0))],
        );
    }
    frame(
        &mut state,
        &ctx,
        1125.0,
        vec![pointer(start + egui::vec2(30.0, 0.0), false)],
    );
    assert!(state.session.brush.settings.size > 12.5);
}

#[test]
fn flow_and_hardness_open_independent_sliders_and_edit_shared_values() {
    for (name, heading, tip) in [
        ("Flow", "Paint per stamp", false),
        ("Hardness", "Tip edge", true),
    ] {
        let (mut state, ctx) = setup();
        state.session.brush.settings.flow = 0.31;
        state.session.brush.settings.hardness = 0.31;
        let doc = state.session.document().clone();
        for _ in 0..3 {
            frame(&mut state, &ctx, 1125.0, vec![]);
        }
        let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
        assert!(position(&out, heading).is_none());
        click(&mut state, &ctx, position(&out, name).unwrap());
        for _ in 0..3 {
            frame(&mut state, &ctx, 1125.0, vec![]);
        }
        let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
        let header = position(&out, heading).expect("slider popover open");
        let track = out
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::epaint::Shape::Rect(r) = &s.shape {
                    (r.rect.top() > header.y && r.rect.width() > 50.0 && r.rect.height() <= 12.0)
                        .then_some(r.rect)
                } else {
                    None
                }
            })
            .reduce(|a, b| a.union(b))
            .expect("popover slider rail");
        let p = egui::pos2(egui::lerp(track.x_range(), 0.8), track.center().y);
        click(&mut state, &ctx, p);
        let changed = if tip {
            state.session.brush.settings.hardness
        } else {
            state.session.brush.settings.flow
        };
        assert!(changed > 0.6, "{name}: {changed}");
        let other = if tip {
            state.session.brush.settings.flow
        } else {
            state.session.brush.settings.hardness
        };
        assert_eq!(other, 0.31);
        let (out, _) = frame(&mut state, &ctx, 1125.0, vec![]);
        assert!(
            position(&out, heading).is_some(),
            "popup remains open after value changes"
        );
        assert_eq!(state.session.document(), &doc);
    }
}
