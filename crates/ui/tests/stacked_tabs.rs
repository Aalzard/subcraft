//! Issue #637: with `TabBarStyle::wrap` (Preferences > Stack panel tabs on more rows) the tabs
//! that don't fit a panel's width wrap onto more rows, so every tab label stays visible and
//! clickable without scrolling. Off: one row that scrolls (the old behaviour).
use egui::{Pos2, Rect, vec2};
use egui_dock::{DockArea, DockState, TabViewer};

const TABS: [&str; 7] = ["LAYERS", "PROPERTIES", "TEXTURE SET LIST", "HISTORY", "SHADER SETTINGS", "DISPLAY SETTINGS", "LOG"];

struct Viewer {
    /// Tab button rect (full) and visible part, per tab title, collected each frame.
    buttons: Vec<(String, Rect, Rect)>,
    /// Top of the panel content (just below the tab bar).
    body_top: f32,
}

impl TabViewer for Viewer {
    type Tab = String;
    fn id(&mut self, tab: &mut String) -> egui::Id { egui::Id::new(tab.as_str()) }
    fn title(&mut self, tab: &mut String) -> egui::WidgetText { egui::RichText::new(tab.as_str()).font(subcraft_ui::theme::font_strong(11.5)).into() }
    fn ui(&mut self, ui: &mut egui::Ui, _tab: &mut String) { self.body_top = ui.max_rect().top(); ui.label("content"); }
    fn on_tab_button(&mut self, tab: &mut String, response: &egui::Response) {
        self.buttons.push((tab.clone(), response.rect, response.interact_rect));
    }
}

/// One frame of a single panel holding all `TABS`, `width` points wide.
fn frame(ctx: &egui::Context, dock: &mut DockState<String>, wrap: bool, width: f32, events: Vec<egui::Event>) -> Viewer {
    let mut viewer = Viewer { buttons: Vec::new(), body_top: f32::NAN };
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width, 500.0));
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
        let mut style = subcraft_ui::theme::dock_style(ui.style());
        style.tab_bar.wrap = wrap;
        DockArea::new(dock).style(style).show_close_buttons(true).show_leaf_close_all_buttons(false).show_leaf_collapse_buttons(false).show_inside(ui, &mut viewer);
    });
    out.textures_delta.clear();
    viewer
}

fn setup() -> (egui::Context, DockState<String>) {
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    (ctx, DockState::new(TABS.iter().map(|t| t.to_string()).collect()))
}

fn rows(viewer: &Viewer) -> Vec<f32> {
    let mut tops: Vec<f32> = viewer.buttons.iter().map(|(_, r, _)| r.top()).collect();
    tops.sort_by(f32::total_cmp);
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    tops
}

#[test]
fn narrow_panel_wraps_every_tab_into_view() {
    let (ctx, mut dock) = setup();
    let width = 320.0;
    let mut v = frame(&ctx, &mut dock, true, width, vec![]);
    for _ in 0..2 { v = frame(&ctx, &mut dock, true, width, vec![]); }
    assert_eq!(v.buttons.len(), TABS.len());
    let rows = rows(&v);
    assert!(rows.len() >= 3, "7 tabs in 320 pt should take 3+ rows, got {rows:?}");
    let bar = 32.0; // theme tab bar height
    for (i, top) in rows.iter().enumerate() {
        assert!((top - (rows[0] + bar * i as f32)).abs() < 0.5, "rows stack one bar height apart: {rows:?}");
    }
    for (title, rect, visible) in &v.buttons {
        assert!(rect.right() <= width + 0.5 && rect.left() >= -0.5, "{title} {rect:?} sticks out of the panel");
        assert_eq!(rect, visible, "{title} is clipped: every tab is fully visible");
    }
    // Tab order is kept, row by row.
    let mut order = v.buttons.clone();
    order.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()).then(a.1.left().total_cmp(&b.1.left())));
    assert_eq!(order.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), TABS);
    // The content starts below the last row.
    assert!((v.body_top - (rows[0] + bar * rows.len() as f32)).abs() < 1.0, "body at {} rows {rows:?}", v.body_top);

    // A tab on the last row is clickable and becomes the active one.
    let (title, rect, _) = v.buttons.iter().find(|b| b.1.top() == *rows.last().unwrap()).unwrap().clone();
    let at = rect.center();
    frame(&ctx, &mut dock, true, width, vec![egui::Event::PointerMoved(at)]);
    frame(&ctx, &mut dock, true, width, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    frame(&ctx, &mut dock, true, width, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    let (_, active) = dock.find_active_focused().expect("a focused leaf");
    assert_eq!(*active, title);
}

#[test]
fn wide_panel_keeps_one_row_and_wrap_off_scrolls() {
    let (ctx, mut dock) = setup();
    // Everything fits: one row, same as before.
    let v = frame(&ctx, &mut dock, true, 2000.0, vec![]);
    assert_eq!(rows(&v).len(), 1);
    // Wrap off in a narrow panel: one row, tabs past the edge are clipped (scroll to reach).
    let v = frame(&ctx, &mut dock, false, 320.0, vec![]);
    assert_eq!(rows(&v).len(), 1);
    assert!(v.buttons.iter().any(|(_, r, vis)| r != vis), "without wrap some tabs are cut off");
}

#[test]
fn stacked_tabs_is_on_by_default() {
    assert!(subcraft_ui::recovery::Preferences::default().stacked_tabs);
}
