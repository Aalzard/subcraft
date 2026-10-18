//! UI zoom is one saved display preference, with no effect on the project or brush state.
use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::{
    recovery::{self, Preferences},
    state::UiState,
    theme,
};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0))),
            ..Default::default()
        },
        |ui| {
            state.recovery.prefs.apply_ui_size(ui.ctx());
            recovery::preferences_window(state, ui.ctx());
        },
    );
    out.textures_delta.clear();
    out
}
fn pos(out: &egui::FullOutput, text: &str) -> Pos2 {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {text}"))
}
fn click(state: &mut UiState, ctx: &egui::Context, p: Pos2) {
    for pressed in [true, false] {
        frame(
            state,
            ctx,
            vec![
                Event::PointerMoved(p),
                Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

#[test]
fn legacy_preferences_default_to_95_and_invalid_sizes_clamp_before_zooming() {
    let mut prefs: Preferences = serde_json::from_str(r#"{"autosave_minutes":8}"#).unwrap();
    assert_eq!(prefs.ui_size_percent, 95);
    let ctx = egui::Context::default();
    for (input, clamped) in [(0, 75), (10000, 150), (95, 95)] {
        prefs.ui_size_percent = input;
        prefs.apply_ui_size(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        assert_eq!(prefs.ui_size_percent, clamped);
        assert!((ctx.zoom_factor() - clamped as f32 / 100.0).abs() < 0.001);
        let restored: Preferences =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert_eq!(restored, prefs);
    }
}

#[test]
fn first_preference_scales_the_context_live_and_reset_restores_the_smaller_default() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.recovery.show_prefs = true;
    let doc = state.session.document().clone();
    let settings = state.session.brush.settings;
    let history = state.session.history().entries().len();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    for _ in 0..5 {
        frame(&mut state, &ctx, vec![]);
    }
    let out = frame(&mut state, &ctx, vec![]);
    let ui_size = pos(&out, "UI size");
    let graphics = pos(&out, "Graphics");
    assert!(ui_size.y < graphics.y);
    assert!((ctx.zoom_factor() - 0.95).abs() < 0.001);
    let rail = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r)
                if r.rect.top() >= ui_size.y - 8.0
                    && r.rect.bottom() < graphics.y
                    && r.rect.width() > 50.0
                    && r.rect.height() < 12.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .reduce(|a, b| a.union(b))
        .expect("UI size slider");
    click(
        &mut state,
        &ctx,
        egui::pos2(egui::lerp(rail.x_range(), 0.8), rail.center().y),
    );
    for _ in 0..5 {
        frame(&mut state, &ctx, vec![]);
    }
    assert!(state.recovery.prefs.ui_size_percent > 105);
    assert!(
        (ctx.zoom_factor() - state.recovery.prefs.ui_size_percent as f32 / 100.0).abs() < 0.001
    );
    let out = frame(&mut state, &ctx, vec![]);
    click(&mut state, &ctx, pos(&out, "Reset"));
    for _ in 0..5 {
        frame(&mut state, &ctx, vec![]);
    }
    assert_eq!(state.recovery.prefs.ui_size_percent, 95);
    assert!((ctx.zoom_factor() - 0.95).abs() < 0.001);
    assert_eq!(state.session.document(), &doc);
    assert_eq!(state.session.brush.settings, settings);
    assert_eq!(state.session.history().entries().len(), history);
}

#[test]
fn scene_dabs_write_the_same_texels_at_small_default_and_large_ui_sizes() {
    use subcraft_core::{ChannelKind, ChannelValue};
    let mut reference: Option<Vec<[f32; 4]>> = None;
    for percent in [75, 95, 150] {
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        state.recovery.prefs.ui_size_percent = percent;
        state.session.brush.settings.size = 14.0;
        state
            .session
            .brush
            .material
            .set(ChannelKind::BaseColor, ChannelValue::rgb(0.7, 0.15, 0.05));
        let history = state.session.history().entries().len();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let draw = |state: &mut UiState, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0))),
                    ..Default::default()
                },
                |ui| {
                    state.recovery.prefs.apply_ui_size(ui.ctx());
                    subcraft_ui::viewport::show(state, ui);
                },
            );
            out.textures_delta.clear();
            out
        };
        for _ in 0..5 {
            draw(&mut state, vec![]);
        }
        let out = draw(&mut state, vec![]);
        // Use scene geometry, not pixel-snapped text, for a precise camera-center input.
        let center = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == theme::BG_0 => Some(r.rect),
                _ => None,
            })
            .max_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap()
            .center();
        for pressed in [true, false] {
            draw(
                &mut state,
                vec![
                    Event::PointerMoved(center),
                    Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert_eq!(
            state.session.history().entries().len(),
            history + 1,
            "one undo step at {percent}%"
        );
        let subcraft_doc::LayerContent::Paint(paint) =
            &state.session.selected_layer().unwrap().content
        else {
            panic!("paint layer")
        };
        let image = &paint.channels[&ChannelKind::BaseColor];
        let pixels = (0..64)
            .flat_map(|y| {
                (0..64).map(move |x| {
                    let p = image.get(x, y);
                    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
                })
            })
            .collect::<Vec<_>>();
        assert!(pixels.iter().map(|p| p[3]).sum::<f32>() > 2.0);
        if let Some(before) = &reference {
            let error = before
                .iter()
                .zip(&pixels)
                .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
                .fold(0.0f32, f32::max);
            assert!(
                error <= 0.005,
                "UI zoom changed brush texels at {percent}%: {error}"
            );
        } else {
            reference = Some(pixels);
        }
    }
}
