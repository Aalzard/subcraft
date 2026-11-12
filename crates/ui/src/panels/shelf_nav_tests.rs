//! Assets panel navigation: several types, folders, saved searches, sub-library tabs and the
//! thumbnail sizes, through the real panel.
use super::*;
use crate::panels::shelf_filter::{STARTER, YOURS, folder_path};
use egui::{Pos2, Rect};

fn state() -> UiState {
    UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(1.0), 64), None)
}

fn frame(ctx: &egui::Context, s: &mut UiState, draw: impl FnOnce(&mut UiState, &mut Ui)) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 2400.0))), ..Default::default() }, |ui| draw(s, ui));
    out.textures_delta.clear();
    out
}

/// Texts drawn by the panel, with their positions.
fn texts(out: &egui::FullOutput) -> Vec<(String, Pos2)> {
    out.shapes.iter().filter_map(|s| match &s.shape {
        egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
        _ => None,
    }).collect()
}

fn drawn(s: &mut UiState, draw: fn(&mut UiState, &mut Ui)) -> Vec<(String, Pos2)> {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut out = frame(&ctx, s, draw);
    for _ in 0..2 {
        out = frame(&ctx, s, draw);
    }
    texts(&out)
}

fn has(texts: &[(String, Pos2)], name: &str) -> bool {
    texts.iter().any(|(t, _)| t == name)
}

#[test]
fn ctrl_click_adds_and_removes_types_and_one_always_stays() {
    let mut nav = ShelfNav::new(ShelfTab::Materials);
    nav.click_type(ShelfTab::Environments, true);
    nav.click_type(ShelfTab::Alphas, true);
    assert_eq!(nav.tabs(), [ShelfTab::Materials, ShelfTab::Alphas, ShelfTab::Environments], "Painter's icon order");
    nav.click_type(ShelfTab::Alphas, true);
    assert_eq!(nav.tabs(), [ShelfTab::Materials, ShelfTab::Environments]);
    nav.click_type(ShelfTab::Materials, true);
    assert_eq!(nav.tabs(), [ShelfTab::Environments]);
    nav.click_type(ShelfTab::Environments, true);
    assert_eq!(nav.tabs(), [ShelfTab::Environments], "the last type cannot be deselected");
    nav.click_type(ShelfTab::Brushes, true);
    nav.click_type(ShelfTab::Filters, false);
    assert_eq!(nav.tabs(), [ShelfTab::Filters], "a plain click shows one type");
}

#[test]
fn the_search_is_one_query_that_round_trips() {
    let mut nav = ShelfNav::new(ShelfTab::Materials);
    nav.click_type(ShelfTab::SmartMaterials, true);
    nav.click_path(folder_path(STARTER, Some("Metal")), false);
    nav.filter.query = "rust n:-old".into();
    let query = nav.query();
    assert_eq!(query, "u:basematerial=,smartmaterial= p:/starter_assets/metal/^ rust n:-old");
    let mut back = ShelfNav::new(ShelfTab::Filters);
    back.apply_query(&query);
    assert_eq!(back.tabs(), nav.tabs());
    assert_eq!(back.paths, nav.paths);
    assert_eq!(back.filter.query, "rust n:-old");
    assert!(same_search(&query, &back.query()));
    back.filter.query.push('x');
    assert!(!same_search(&query, &back.query()), "typing more leaves the saved search");
    // A saved query without types keeps the types shown.
    assert!(same_search("rust", "u:basematerial= rust"));
    assert!(!same_search("u:alpha= rust", "u:basematerial= rust"));
}

#[test]
fn assets_are_placed_in_libraries_and_folders() {
    let s = state();
    let materials = items(&s, ShelfTab::Materials);
    assert_eq!(materials.len(), s.library.materials.len());
    assert!(materials.iter().all(|m| m.library == STARTER && m.folder.is_some()), "built-in materials sit in their category folder");
    let alphas = items(&s, ShelfTab::Alphas);
    assert!(alphas.iter().any(|a| a.starter && a.folder.as_deref() == Some("Starter pack")));
    assert!(alphas.iter().all(|a| !a.name.starts_with("Starter: ")), "starter tips are listed once, from the starter pack");
    assert!(items(&s, ShelfTab::Brushes).iter().all(|b| b.folder.as_deref().is_some_and(|f| f.ends_with("presets"))));
    assert!(!items(&s, ShelfTab::Brushes).is_empty() && !alphas.is_empty());
    assert!(CATEGORIES.iter().all(|c| items(&s, c.0).iter().all(|i| i.library == STARTER || i.library == YOURS)));
}

#[test]
fn locations_grey_out_folders_without_assets_of_the_selected_types() {
    let mut s = state();
    s.shelf.open(ShelfTab::Materials, "");
    let metal = s.library.materials[0].category;
    let at = |s: &UiState| locations(s, &Search::of(s)).into_iter().find(|l| l.library == STARTER && l.folder.as_deref() == Some(metal)).unwrap();
    let place = at(&s);
    assert!(place.applicable > 0 && place.assets >= place.applicable);
    s.shelf.open(ShelfTab::Environments, "");
    let place = at(&s);
    assert_eq!(place.applicable, 0, "no environment in a material category");
    assert!(place.assets > 0, "the folder still exists (greyed, not hidden)");
    // The search counts too.
    s.shelf.open(ShelfTab::Materials, "zzzz-no-such-asset!");
    assert_eq!(at(&s).applicable, 0);
}

#[test]
fn ctrl_selected_types_are_drawn_together_under_headings() {
    let mut s = state();
    s.shelf.open(ShelfTab::Materials, "");
    s.shelf.extra_tabs.push(ShelfTab::Environments);
    let t = drawn(&mut s, show);
    assert!(has(&t, &s.library.materials[0].name), "materials are shown");
    assert!(has(&t, &s.library.environments[0].name), "environments are shown with them");
    assert!(has(&t, "Materials") && has(&t, "Environments"), "each type has a heading");
}

#[test]
fn a_selected_folder_limits_the_assets_shown() {
    let mut s = state();
    s.shelf.open(ShelfTab::Materials, "");
    let category = s.library.materials[0].category;
    let other = s.library.materials.iter().find(|m| m.category != category).expect("two categories").name.clone();
    s.shelf.paths = vec![folder_path(STARTER, Some(category)).to_lowercase()];
    let t = drawn(&mut s, show);
    assert!(has(&t, &s.library.materials[0].name));
    assert!(!has(&t, &other), "{other} is in another folder");
    assert!(has(&t, category), "the breadcrumbs name the folder");
    // A library alone: every built-in material.
    s.shelf.paths = vec![folder_path(STARTER, None)];
    assert!(has(&drawn(&mut s, show), &other));
    s.shelf.paths = vec![folder_path(YOURS, None)];
    assert!(!has(&drawn(&mut s, show), &other), "nothing of the user's library on a fresh install");
}

#[test]
fn list_view_draws_one_row_per_asset() {
    let mut s = state();
    s.shelf.open(ShelfTab::Environments, "");
    set_preview_size(&mut s.recovery.prefs, "List");
    assert_eq!(preview_size_name(&s.recovery.prefs), "List");
    let t = drawn(&mut s, show);
    let rows: Vec<Pos2> = s.library.environments.iter().take(4).map(|e| t.iter().find(|(n, _)| *n == e.name).expect("drawn").1).collect();
    assert!(rows.windows(2).all(|w| (w[0].x - w[1].x).abs() < 0.5 && w[1].y >= w[0].y + LIST_ROW - 0.5), "names line up in rows: {rows:?}");
    set_preview_size(&mut s.recovery.prefs, "Large");
    assert_eq!((s.recovery.prefs.shelf_preview_size, s.recovery.prefs.shelf_list_view), (144, false));
    s.recovery.prefs.shelf_preview_size = 100;
    assert_eq!(preview_size_name(&s.recovery.prefs), "Medium", "an older free size reads as the nearest preset");
}

#[test]
fn saved_searches_keep_distinct_names() {
    let mut saved = Vec::new();
    assert_eq!(save_search(&mut saved, "Grunge", "u:texture= grunge".into()), "Grunge");
    assert_eq!(save_search(&mut saved, "grunge", "u:alpha=".into()), "grunge 2");
    assert_eq!(rename_search(&mut saved, 1, "Grunge"), Some(("grunge 2".into(), "Grunge 2".into())));
    assert_eq!(rename_search(&mut saved, 0, "Grunge"), Some(("Grunge".into(), "Grunge".into())), "keeping its own name is fine");
    assert_eq!(rename_search(&mut saved, 5, "x"), None);
}

#[test]
fn saved_searches_survive_in_the_preferences() {
    let mut prefs = crate::recovery::Preferences::default();
    save_search(&mut prefs.shelf_saved_searches, "Envs", "u:environment=".into());
    prefs.shelf_list_view = true;
    let back = crate::recovery::Preferences::from_json_lenient(&serde_json::to_string(&prefs).unwrap());
    assert_eq!(back.shelf_saved_searches, prefs.shelf_saved_searches);
    assert!(back.shelf_list_view);
    let old = crate::recovery::Preferences::from_json_lenient("{\"ui_size_percent\":100}");
    assert!(old.shelf_saved_searches.is_empty() && !old.shelf_list_view, "older settings files load");
}

#[test]
fn a_sub_library_tab_keeps_its_own_search_and_resets_to_it() {
    let mut s = state();
    s.shelf.open(ShelfTab::Materials, "");
    let id = s.shelf.add_sub_library("Envs".into(), "u:environment=".into());
    assert_eq!(s.shelf.open_sub_library, Some(id));
    let mut dock = crate::app::default_layout();
    crate::app::sync_sub_libraries(&mut dock, &mut s.shelf);
    let tab = crate::app::Tab::SubLibrary(id);
    let (at, shelf) = (dock.find_tab(&tab).expect("docked"), dock.find_tab(&crate::app::Tab::Shelf).unwrap());
    assert_eq!((at.surface, at.node), (shelf.surface, shelf.node), "docked beside the Assets panel");
    let t = drawn(&mut s, |s, ui| show_sub_library(s, ui, 1));
    assert!(has(&t, &s.library.environments[0].name), "the tab shows its search");
    assert!(!has(&t, &s.library.materials[0].name));
    assert_eq!(s.shelf.tabs(), [ShelfTab::Materials], "the main panel keeps its own search");
    // Searching inside the tab changes the tab only; Reset brings its query back.
    s.shelf.sub_libraries[0].nav.filter.query = "zzzz".into();
    assert!(!has(&drawn(&mut s, |s, ui| show_sub_library(s, ui, 1)), &s.library.environments[0].name));
    let mut nav = s.shelf.sub_libraries[0].nav.clone();
    nav.apply_query(&s.shelf.sub_libraries[0].query.clone());
    assert!(nav.filter.query.is_empty() && nav.tabs() == [ShelfTab::Environments]);
    // Closing it removes the tab; it has to be made again.
    s.shelf.close_sub_library(id);
    crate::app::sync_sub_libraries(&mut dock, &mut s.shelf);
    assert!(dock.find_tab(&tab).is_none());
    assert!(has(&drawn(&mut s, |s, ui| show_sub_library(s, ui, 1)), "This sub-library was closed. Make a new one from the Assets panel."));
}

#[test]
fn breadcrumbs_and_sections_are_drawn() {
    let mut s = state();
    s.shelf.open(ShelfTab::Materials, "");
    s.shelf.show_paths = true;
    s.shelf.show_saved = true;
    save_search(&mut s.recovery.prefs.shelf_saved_searches, "Metals", "u:basematerial= metal".into());
    let t = drawn(&mut s, show);
    assert!(has(&t, "All libraries"));
    assert!(has(&t, "Filter by path") && has(&t, "Hide non-applicable folders"));
    assert!(t.iter().any(|(n, _)| n.starts_with("Starter assets  (")), "libraries list with their counts");
    assert!(has(&t, "Saved searches") && has(&t, "Metals"));
    // Applying it shows its tag; the tag goes once the search changes.
    let saved = s.recovery.prefs.shelf_saved_searches[0].clone();
    let mut nav = s.shelf.nav();
    nav.apply_query(&saved.query);
    nav.saved = Some(saved.name.clone());
    s.shelf.set_nav(nav);
    let t = drawn(&mut s, show);
    assert!(t.iter().any(|(n, _)| n.starts_with("Metals  ")), "search tag");
    s.shelf.filter.query = "metal rust".into();
    drawn(&mut s, show);
    assert_eq!(s.shelf.saved, None);
}
