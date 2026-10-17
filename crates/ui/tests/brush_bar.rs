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
        1280.0, 1125.0, 980.0, 700.0, 540.0, 480.0, 420.0, 300.0, 200.0,
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
        let brush = position(&output, "Brush").expect("tool menu always reachable");
        let view = position(&output, if width >= 540.0 { "Resolution" } else { "View" }).unwrap();
        if width >= 1125.0 {
            assert!((brush.y - view.y).abs() < 3.0, "inline on wide docks");
        } else {
            assert!(brush.y > view.y + 15.0, "stacked rows on narrow docks");
        }
        if width >= 540.0 {
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
    let start = position(&output, "73").expect("percentage drag box");
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
    let p = position(&output, "Brush").unwrap();
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
