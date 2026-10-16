//! Preferences must persist a backend before the next GPU/window is constructed.
use subcraft_app::graphics::{GraphicsApi, GraphicsSettings};
use subcraft_ui::graphics::{self, GraphicsUi};

fn frame(g: &mut GraphicsUi, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() }, |ui| { graphics::controls(g, ui); });
    out.textures_delta.clear(); out
}
fn click(g: &mut GraphicsUi, ctx: &egui::Context, label: &str) {
    for _ in 0..3 { frame(g, ctx, vec![]); }
    let out = frame(g, ctx, vec![]);
    let pos = out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == label => Some(t.pos + t.galley.size() * 0.5), _ => None,
    }).unwrap_or_else(|| panic!("Missing graphics control: {label}"));
    for pressed in [true, false] { frame(g, ctx, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]); }
}
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn selecting_opengl_in_the_real_combo_persists_and_requests_restart() {
    let root = std::env::temp_dir().join(format!("subcraft-graphics-ui-{}", subcraft_core::ObjectId::new()));
    let path = root.join("graphics.json");
    let mut g = GraphicsUi { path: Some(path.clone()), active: "DirectX 12".into(), ..Default::default() };
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    click(&mut g, &ctx, "Automatic"); click(&mut g, &ctx, "OpenGL");
    assert_eq!(g.selected, GraphicsApi::OpenGl); assert!(g.restart_required());
    assert_eq!(GraphicsSettings::load(&path).unwrap().api, GraphicsApi::OpenGl);
    assert_eq!(g.active, "DirectX 12", "selection must not misreport the still-running device");
    let out = frame(&mut g, &ctx, vec![]);
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.text().contains("Restart required"))));
    g.select(GraphicsApi::Automatic); assert!(!g.restart_required());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn failed_save_keeps_the_running_selection_and_reports_the_error() {
    let mut g = GraphicsUi::default(); g.select(GraphicsApi::OpenGl);
    assert_eq!(g.selected, GraphicsApi::Automatic); assert!(!g.restart_required()); assert!(g.error.is_some());
}
