//! Preference migration, recent-file controller and all guide pages run without desktop input.
use subcraft_app::Session;
use subcraft_ui::{dialogs, state::UiState, welcome};

#[test]
fn older_preferences_show_welcome_by_default_and_the_choice_roundtrips() {
    let mut prefs: subcraft_ui::recovery::Preferences = serde_json::from_str(r#"{"autosave_minutes":5,"autosave_keep":3}"#).unwrap();
    assert!(prefs.show_welcome); prefs.show_welcome = false;
    let back: subcraft_ui::recovery::Preferences = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
    assert!(!back.show_welcome); assert_eq!(back.autosave_minutes, 5);
}

#[test]
fn recent_controller_opens_the_project_and_failed_open_keeps_current_work() {
    let root = std::env::temp_dir().join(format!("subcraft-welcome-{}", subcraft_core::ObjectId::new()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("crate.subcraft");
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128); session.save(Some(&path)).unwrap();
    let expected = session.document().clone();
    let mut state = UiState::new(Session::sample(), None); state.welcome.open = true;
    state.welcome.pending_recent = Some(path.clone());
    dialogs::run_pending(&mut state, dialogs::Pending::OpenRecent, &egui::Context::default(), 0.0);
    assert_eq!(state.session.document(), &expected); assert!(!state.welcome.open);
    assert_eq!(state.welcome.recent.paths().len(), 1);
    state.welcome.pending_recent = Some(root.join("missing.subcraft"));
    dialogs::run_pending(&mut state, dialogs::Pending::OpenRecent, &egui::Context::default(), 1.0);
    assert_eq!(state.session.document(), &expected); assert!(state.status.as_ref().unwrap().0.contains("Could not open"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn welcome_and_each_guide_page_render_without_editing_the_project() {
    let mut state = UiState::new(Session::sample(), None);
    let doc = state.session.document().clone();
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    for page in 0usize..7 {
        state.welcome.open = page == 0;
        state.welcome.guide = (page > 0).then_some(page.saturating_sub(1));
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))), ..Default::default() }, |ui| {
            assert!(welcome::show(&mut state, ui.ctx()).is_none());
        });
        assert!(!output.shapes.is_empty()); assert_eq!(state.session.document(), &doc);
        output.textures_delta.clear();
    }
}

#[test]
fn narrow_welcome_keeps_startup_and_project_controls_inside_the_window() {
    let mut state = UiState::new(Session::sample(), None); state.welcome.open = true;
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    for frame in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(440.0, 768.0))), ..Default::default() }, |ui| {
            assert!(welcome::show(&mut state, ui.ctx()).is_none());
        });
        output.textures_delta.clear();
        if frame == 0 { continue; } // egui measures an anchored window before its first visible frame.
        for label in ["New from a model…", "Open project…", "Try the sample", "Show this screen on startup"] {
            let text = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == label => Some(t), _ => None,
            }).unwrap_or_else(|| panic!("Welcome control must be visible: {label}"));
            assert!(text.pos.x >= 0.0, "{label} starts offscreen");
            assert!(text.pos.x + text.galley.size().x <= 440.0, "{label} ends offscreen");
        }
    }
}

// Feed egui input directly; this never sends mouse or keyboard input to the desktop.
fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> (egui::FullOutput, Option<welcome::WelcomeAction>) {
    let mut action = None;
    let mut output = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))), ..Default::default() }, |ui| {
        action = welcome::show(state, ui.ctx());
    });
    output.textures_delta.clear(); (output, action)
}

fn click(state: &mut UiState, ctx: &egui::Context, text: &str) -> Option<welcome::WelcomeAction> {
    // Windows settle their position/size after their initial measuring pass.
    for _ in 0..3 { let _ = frame(state, ctx, vec![]); }
    let (output, _) = frame(state, ctx, vec![]);
    let pos = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    }).unwrap_or_else(|| panic!("Missing button: {text}"));
    let event = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let _ = frame(state, ctx, vec![egui::Event::PointerMoved(pos), event(true)]);
    frame(state, ctx, vec![event(false)]).1
}

#[test]
fn welcome_buttons_dispatch_and_guide_navigation_reaches_each_step() {
    let mut state = UiState::new(Session::sample(), None); let doc = state.session.document().clone();
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    state.welcome.open = true;
    assert!(matches!(click(&mut state, &ctx, "Try the sample"), Some(welcome::WelcomeAction::Pending(dialogs::Pending::NewSample))));
    assert!(click(&mut state, &ctx, "Quick start").is_none());
    assert!(!state.welcome.open); assert_eq!(state.welcome.guide, Some(0));
    for index in 0..subcraft_app::launch::QUICK_START.len() {
        let step = &subcraft_app::launch::QUICK_START[index];
        assert!(matches!(click(&mut state, &ctx, step.button), Some(welcome::WelcomeAction::Guide(action)) if action == step.action));
        if index > 0 {
            click(&mut state, &ctx, "Back"); assert_eq!(state.welcome.guide, Some(index - 1));
            click(&mut state, &ctx, "Next"); assert_eq!(state.welcome.guide, Some(index));
        }
        click(&mut state, &ctx, if index == 5 { "Finish" } else { "Next" });
    }
    assert_eq!(state.welcome.guide, None); assert_eq!(state.session.document(), &doc);
}
