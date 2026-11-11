//! Presets from the tool Properties (right-click menu, Presets block) and the Shelf's preset
//! context menu actions. Real egui pointer events; no desktop mouse.
use std::path::PathBuf;

use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_app::assets::{UserAssetKind, UserLibrary};
use subcraft_app::brush_preset::{Favorites, PresetKind, PresetTool, ShelfBrush};
use subcraft_ui::panels::brush;
use subcraft_ui::panels::shelf::{PresetAction, apply_preset_action, preset_favorite_key, preset_order};
use subcraft_ui::state::{Tool, UiState};

fn state(name: &str) -> (PathBuf, UiState) {
    let dir = std::env::temp_dir().join(format!("subcraft-preset-menus-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    s.user_library = Some(UserLibrary::new(dir.clone()));
    (dir, s)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 2600.0))), ..Default::default() },
        |ui| brush::show(state, ui),
    );
    output.textures_delta.clear();
    output
}

fn texts(output: &egui::FullOutput) -> Vec<(String, Pos2)> {
    output.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5)),
        _ => None,
    }).collect()
}

fn button(pos: Pos2, which: egui::PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button: which, pressed, modifiers: egui::Modifiers::NONE }
}

fn click(state: &mut UiState, ctx: &egui::Context, pos: Pos2, which: egui::PointerButton) {
    frame(state, ctx, vec![Event::PointerMoved(pos), button(pos, which, true)]);
    frame(state, ctx, vec![button(pos, which, false)]);
    frame(state, ctx, vec![]);
}

fn names(state: &UiState) -> Vec<String> {
    state.brush_presets.iter().filter(|p| p.user_file.is_some()).map(|p| p.name.clone()).collect()
}

#[test]
fn right_click_in_the_tool_properties_offers_the_three_presets_and_creates_one() {
    let (dir, mut s) = state("menu");
    s.tool = Tool::Eraser;
    s.session.brush.settings.size = 21.0;
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 { frame(&mut s, &ctx, vec![]); }
    let out = frame(&mut s, &ctx, vec![]);
    assert!(!texts(&out).iter().any(|(t, _)| t == "Create tool preset"), "the menu is closed until asked for");
    // Empty space of the panel: its left margin, level with a section title (the section header
    // itself spans the full width and takes clicks).
    let brush_title = texts(&out).iter().filter(|(t, _)| t == "ALPHA").map(|(_, p)| *p).next().expect("Alpha section");
    let empty = egui::pos2(3.0, brush_title.y);
    click(&mut s, &ctx, empty, egui::PointerButton::Secondary);
    let out = frame(&mut s, &ctx, vec![]);
    let menu = texts(&out);
    let entry = |label: &str| menu.iter().filter(|(t, _)| t == label).map(|(_, p)| *p).last().unwrap_or_else(|| panic!("no menu entry: {label}"));
    // Painter's order: tool, material, brush.
    let (tool, material, brush_entry) = (entry("Create tool preset"), entry("Create material preset"), entry("Create brush preset"));
    assert!(tool.y < material.y && material.y < brush_entry.y);
    click(&mut s, &ctx, tool, egui::PointerButton::Primary);
    let made = s.brush_presets.iter().find(|p| p.user_file.is_some()).expect("a preset was created in the library");
    assert_eq!((made.kind, made.tool), (PresetKind::Tool, Some(PresetTool::Eraser)), "a tool preset records the tool in use");
    assert_eq!(made.settings.size, 21.0);
    assert_eq!(s.user_library.as_ref().unwrap().list(UserAssetKind::BrushPreset).len(), 1);
    let out = frame(&mut s, &ctx, vec![]);
    assert!(!texts(&out).iter().any(|(t, _)| t == "Create material preset"), "the menu closed after the pick");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn right_click_on_a_control_does_not_swallow_its_left_click() {
    let (dir, mut s) = state("controls");
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 { frame(&mut s, &ctx, vec![]); }
    let out = frame(&mut s, &ctx, vec![]);
    let follow = texts(&out).iter().find(|(t, _)| t == "Follow Path").map(|(_, p)| *p).expect("Follow Path checkbox");
    assert!(!s.session.brush.settings.follow_path);
    click(&mut s, &ctx, follow, egui::PointerButton::Primary);
    assert!(s.session.brush.settings.follow_path, "controls on top of the menu background still work");
    assert!(names(&s).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn create_uses_the_typed_name_and_each_kind_stores_its_part() {
    let (dir, mut s) = state("kinds");
    s.preset_name = "  Rusty edge ".into();
    brush::create_preset(&mut s, PresetKind::Brush, 1.0);
    assert_eq!(names(&s), ["Rusty edge"]);
    assert!(s.preset_name.is_empty(), "the name field is cleared for the next one");
    // Without a typed name: the brush's name, or "Material".
    brush::create_preset(&mut s, PresetKind::Material, 2.0);
    brush::create_preset(&mut s, PresetKind::Tool, 3.0);
    assert_eq!(names(&s), ["Rusty edge", "Material", "Round"]);
    let kinds: Vec<_> = s.brush_presets.iter().filter(|p| p.user_file.is_some()).map(|p| (p.kind, p.tool)).collect();
    assert_eq!(kinds, [(PresetKind::Brush, None), (PresetKind::Material, None), (PresetKind::Tool, Some(PresetTool::Paint))]);
    // No library: nothing is created and the status says why.
    s.user_library = None;
    brush::create_preset(&mut s, PresetKind::Tool, 4.0);
    assert_eq!(names(&s).len(), 3);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn selecting_a_tool_preset_switches_to_its_tool_and_a_material_preset_keeps_the_tool() {
    let (dir, mut s) = state("select");
    s.tool = Tool::Smudge;
    brush::create_preset(&mut s, PresetKind::Tool, 1.0);
    brush::create_preset(&mut s, PresetKind::Material, 1.0);
    brush::create_preset(&mut s, PresetKind::Brush, 1.0);
    let user: Vec<_> = s.brush_presets.iter().filter(|p| p.user_file.is_some()).cloned().collect();
    let (tool, material, brush_preset) = (&user[0], &user[1], &user[2]);

    s.tool = Tool::Brush;
    s.select_shelf_brush(ShelfBrush::Preset(tool), 2.0);
    assert_eq!(s.tool, Tool::Smudge, "the tool preset brings its tool");
    s.tool = Tool::Clone;
    s.select_shelf_brush(ShelfBrush::Preset(material), 3.0);
    assert_eq!(s.tool, Tool::Clone, "a material preset serves the painting tool in use");
    s.tool = Tool::Eyedropper;
    s.select_shelf_brush(ShelfBrush::Preset(material), 3.5);
    assert_eq!(s.tool, Tool::Brush, "from a tool that does not paint, the brush is selected");
    s.tool = Tool::PolygonFill;
    s.select_shelf_brush(ShelfBrush::Preset(brush_preset), 4.0);
    assert_eq!(s.tool, Tool::Brush, "a brush preset behaves as before: it selects the brush");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn shelf_menu_actions_update_rename_duplicate_favorite_and_delete() {
    let (dir, mut s) = state("actions");
    let lib = s.user_library.clone().unwrap();
    s.preset_name = "Mine".into();
    brush::create_preset(&mut s, PresetKind::Tool, 1.0);
    let index = s.brush_presets.iter().position(|p| p.name == "Mine").unwrap();
    let file = s.brush_presets[index].user_file.clone().unwrap();

    // Update from current tool: same file, new values.
    s.session.brush.settings.size = 77.0;
    s.tool = Tool::Clone;
    apply_preset_action(&mut s, index, PresetAction::Update, 2.0);
    assert_eq!((s.brush_presets[index].settings.size, s.brush_presets[index].tool), (77.0, Some(PresetTool::Clone)));
    assert_eq!(s.brush_presets[index].user_file.as_ref(), Some(&file));

    // Favorite: saved in the library, listed first.
    apply_preset_action(&mut s, index, PresetAction::Favorite, 3.0);
    assert!(s.favorites.contains(&preset_favorite_key("Mine")));
    assert!(Favorites::load(&lib).contains(&preset_favorite_key("Mine")));
    assert_eq!(preset_order(&s.brush_presets, &s.favorites)[0], index, "favorites come first");
    assert_eq!(preset_order(&s.brush_presets, &s.favorites).len(), s.brush_presets.len());

    // Rename keeps the favorite mark and the Properties title of the selected preset.
    let preset = s.brush_presets[index].clone();
    s.select_shelf_brush(ShelfBrush::Preset(&preset), 3.5);
    apply_preset_action(&mut s, index, PresetAction::Rename("Yours".into()), 4.0);
    assert_eq!(s.brush_presets[index].name, "Yours");
    assert!(!file.exists() && s.brush_presets[index].user_file.as_ref().unwrap().exists());
    assert!(s.favorites.contains(&preset_favorite_key("Yours")) && !s.favorites.contains(&preset_favorite_key("Mine")));
    assert!(s.is_selected_shelf_brush("Yours"));

    // Duplicate lands right after the original; a built-in can be duplicated too.
    apply_preset_action(&mut s, index, PresetAction::Duplicate, 5.0);
    assert_eq!(s.brush_presets[index + 1].name, "Yours copy");
    let builtin = s.brush_presets.iter().position(|p| p.user_file.is_none()).expect("built-in presets");
    let builtin_name = s.brush_presets[builtin].name.clone();
    apply_preset_action(&mut s, builtin, PresetAction::Duplicate, 5.5);
    assert_eq!(s.brush_presets[builtin + 1].name, format!("{builtin_name} copy"));
    assert!(s.brush_presets[builtin + 1].user_file.is_some());
    // Built-ins refuse what needs a file; nothing changes.
    let count = s.brush_presets.len();
    apply_preset_action(&mut s, builtin, PresetAction::Delete, 6.0);
    apply_preset_action(&mut s, builtin, PresetAction::Rename("Hacked".into()), 6.0);
    apply_preset_action(&mut s, builtin, PresetAction::Update, 6.0);
    assert_eq!((s.brush_presets.len(), s.brush_presets[builtin].name.clone()), (count, builtin_name));

    // Delete removes the file, the entry and the favorite mark.
    let index = s.brush_presets.iter().position(|p| p.name == "Yours").unwrap();
    let file = s.brush_presets[index].user_file.clone().unwrap();
    apply_preset_action(&mut s, index, PresetAction::Delete, 7.0);
    assert!(!file.exists() && !s.brush_presets.iter().any(|p| p.name == "Yours"));
    assert!(!s.favorites.contains(&preset_favorite_key("Yours")));
    assert!(Favorites::load(&lib).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
