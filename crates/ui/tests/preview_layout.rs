//! Shelf wrapping, scrolling and independent preview-size preferences at real dock widths.
use egui::{Event, Pos2, Rect};
use subcraft_app::{Command, Session};
use subcraft_ui::{panels::{self, shelf::ShelfTab}, recovery::Preferences, state::UiState, theme};

fn state() -> UiState { UiState::new(Session::new(subcraft_mesh::primitives::cube(1.0), 64), None) }

fn frame(ctx: &egui::Context, s: &mut UiState, shelf: bool, width: f32, events: Vec<Event>) -> (egui::FullOutput, Rect) {
    let mut bounds = Rect::NOTHING;
    let mut out = ctx.run_ui(egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 600.0))), events, ..Default::default()
    }, |ui| { if shelf { panels::shelf::show(s, ui); } else { panels::layers::show(s, ui); } bounds = ui.min_rect(); });
    out.textures_delta.clear(); (out, bounds)
}

fn tiles(out: &egui::FullOutput, size: f32) -> Vec<Rect> {
    out.shapes.iter().filter_map(|s| match &s.shape {
        egui::Shape::Rect(r) if (r.rect.width() - size).abs() < 0.01 && (r.rect.height() - size).abs() < 0.01 && r.stroke.width > 0.0 => Some(r.rect),
        _ => None,
    }).collect()
}

#[test]
fn shelf_wraps_all_categories_downward_without_horizontal_overflow() {
    for width in [240.0, 320.0, 700.0, 1100.0] {
        for size in [48, 88, 160] {
            for tab in [ShelfTab::Textures, ShelfTab::Materials, ShelfTab::SmartMaterials, ShelfTab::SmartMasks, ShelfTab::Brushes, ShelfTab::Environments] {
                let ctx = egui::Context::default(); theme::apply(&ctx);
                let mut s = state(); s.shelf.tab = tab; s.recovery.prefs.shelf_preview_size = size;
                let mut result = frame(&ctx, &mut s, true, width, vec![]);
                for _ in 0..2 { result = frame(&ctx, &mut s, true, width, vec![]); }
                let (out, bounds) = result;
                assert!(bounds.right() <= width + 0.5, "panel grew horizontally: {tab:?}, {width}, {size}: {bounds:?}");
                assert!(bounds.bottom() <= 600.5, "footer pushed below panel: {tab:?}, {width}, {size}");
                let tiles = tiles(&out, size as f32);
                assert!(!tiles.is_empty(), "expected assets in {tab:?}");
                for rect in &tiles { assert!(rect.left() >= 0.0 && rect.right() <= width, "tile off right edge at {width}, {size}: {rect:?}"); }
                if tiles.len() * (size as usize + 10) > width as usize {
                    assert!(tiles.iter().any(|r| r.top() > tiles[0].top()), "items must continue on a new row");
                }
            }
        }
    }
}

#[test]
fn footer_sliders_resize_previews_independently_without_document_edits() {
    for shelf in [true, false] {
      for width in [280.0, 520.0] {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut s = state(); s.shelf.tab = ShelfTab::Textures;
        let layer = s.session.selection().layer.unwrap(); s.session.execute(Command::AddMask { layer, white: true }).unwrap();
        let before = s.session.document().clone(); let history = s.session.history().entries().len();
        let mut out = frame(&ctx, &mut s, shelf, width, vec![]).0;
        for _ in 0..2 { out = frame(&ctx, &mut s, shelf, width, vec![]).0; }
        let pos = out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Circle(c) if c.center.y > 540.0 => Some(c.center + egui::vec2(25.0, 0.0)), _ => None
        }).expect("size slider stays beneath the scroll area");
        for pressed in [true, false] {
            frame(&ctx, &mut s, shelf, width, vec![Event::PointerMoved(pos), Event::PointerButton {pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default()}]);
        }
        out = frame(&ctx, &mut s, shelf, width, vec![]).0;
        if shelf {
            assert!(s.recovery.prefs.shelf_preview_size > 88); assert_eq!(s.recovery.prefs.layer_preview_size, 36);
            assert!(!tiles(&out, s.recovery.prefs.shelf_preview_size as f32).is_empty());
        } else {
            assert!(s.recovery.prefs.layer_preview_size > 36); assert_eq!(s.recovery.prefs.shelf_preview_size, 88);
            let thumbs: Vec<_> = out.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.rect.width() > 36.0 && r.rect.width() <= 64.0 && (r.rect.width()-r.rect.height()).abs() < 0.01 => Some(r.rect), _ => None
            }).collect();
            assert!(thumbs.len() >= 2);
            assert!(thumbs.iter().all(|r| r.right() <= width), "larger layer previews must fit narrow panels");
        }
        assert_eq!(s.session.document(), &before); assert_eq!(s.session.history().entries().len(), history);
    }
    }
}

#[test]
fn preview_preferences_load_old_defaults_and_roundtrip_independently() {
    let mut prefs: Preferences = serde_json::from_str("{\"ui_size_percent\":100}").unwrap();
    assert_eq!((prefs.shelf_preview_size, prefs.layer_preview_size), (88,36));
    prefs.shelf_preview_size = 144; prefs.layer_preview_size = 56;
    assert_eq!(serde_json::from_str::<Preferences>(&serde_json::to_string(&prefs).unwrap()).unwrap(), prefs);
}

#[test]
fn shelf_scroll_reaches_last_asset_while_size_footer_stays_visible() {
    let ctx = egui::Context::default(); theme::apply(&ctx);
    let mut s = state(); s.shelf.tab = ShelfTab::Textures;
    for _ in 0..3 { frame(&ctx, &mut s, true, 320.0, vec![]); }
    let before = frame(&ctx, &mut s, true, 320.0, vec![]).0;
    let last_before = *tiles(&before, 88.0).last().unwrap();
    assert!(last_before.bottom() > 600.0, "fixture needs vertical scrolling");
    let pointer = Pos2::new(160.0, 350.0);
    frame(&ctx, &mut s, true, 320.0, vec![Event::PointerMoved(pointer), Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -10000.0), phase: egui::TouchPhase::Move, modifiers: Default::default()
    }]);
    for _ in 0..20 { frame(&ctx, &mut s, true, 320.0, vec![]); }
    let (out, bounds) = frame(&ctx, &mut s, true, 320.0, vec![]);
    let last = *tiles(&out, 88.0).last().unwrap();
    assert!(last.bottom() < 560.0 && last.bottom() < last_before.bottom(), "last asset is reachable: {last:?}");
    assert!(bounds.bottom() <= 600.5);
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Preview size" && s.clip_rect.contains(t.pos))));
}
