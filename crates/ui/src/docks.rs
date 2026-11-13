//! Painter's Docks toolbar and view undocking: closed panels get a button in the header; a click
//! pops the panel up as a floating window beside the button, a second click closes it, and
//! dragging the window by its tab docks it like any other panel. The 3D and 2D views can float
//! in their own window too (View menu). All of it is plain `egui_dock` state, saved with the layout.

use egui::{Pos2, Ui, Vec2};
use egui_dock::{DockState, SurfaceIndex};

use crate::app::Tab;
use crate::i18n::tr;

/// Panels the Docks toolbar offers, in its order (the 3D and 2D views are never closed).
pub const DOCKABLE: [Tab; 10] = [
    Tab::DisplaySettings,
    Tab::ShaderSettings,
    Tab::Log,
    Tab::History,
    Tab::Properties,
    Tab::Layers,
    Tab::TextureSetList,
    Tab::TextureSetSettings,
    Tab::Shelf,
    Tab::Parts,
];

/// Narrowest header that shows the Docks toolbar: below it the buttons would squeeze out the
/// project name (the `long_header_titles_cannot_paint_over_menus_or_actions` test at 1200 px).
pub const TOOLBAR_MIN_WIDTH: f32 = 1400.0;

/// Size of a panel popped up from the Docks toolbar.
pub const POPUP_SIZE: Vec2 = Vec2::new(360.0, 560.0);
/// Size of a 3D or 2D view floated with View > Undock.
pub const UNDOCKED_VIEW_SIZE: Vec2 = Vec2::new(900.0, 640.0);

/// Whether a panel can be closed (and so appear on the Docks toolbar).
pub fn closeable(tab: Tab) -> bool {
    DOCKABLE.contains(&tab)
}

/// Dockable panels that are not open anywhere.
pub fn closed(dock: &DockState<Tab>) -> Vec<Tab> {
    DOCKABLE.into_iter().filter(|t| dock.find_tab(t).is_none()).collect()
}

/// Which surface (main area or a floating window) shows `tab`.
pub fn surface_of(dock: &DockState<Tab>, tab: Tab) -> Option<SurfaceIndex> {
    dock.find_tab(&tab).map(|p| p.surface)
}

/// Open `tab` alone in a floating window at `at` (its top-left corner) of `size`.
pub fn float(dock: &mut DockState<Tab>, tab: Tab, at: Pos2, size: Vec2) -> SurfaceIndex {
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.remove_tab(path);
    }
    let surface = dock.add_window(vec![tab]);
    if let Some(window) = dock.get_window_state_mut(surface) {
        window.set_position(at).set_size(size);
    }
    surface
}

/// Close `tab` wherever it is (an emptied floating window goes with it).
pub fn close(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.remove_tab(path);
    }
}

/// Put a floating `tab` back into the main area: the 2D view behind the 3D view (or the other
/// way round), anything else beside Properties, or the first pane when that is gone.
pub fn redock(dock: &mut DockState<Tab>, tab: Tab) {
    if surface_of(dock, tab).is_none_or(|s| s.is_main()) {
        return;
    }
    close(dock, tab);
    let partner = match tab {
        Tab::Viewport => Tab::UvView,
        Tab::UvView => Tab::Viewport,
        _ => Tab::Properties,
    };
    match dock.find_tab(&partner).filter(|p| p.surface.is_main()) {
        Some(host) => dock[host.surface][host.node].append_tab(tab),
        None => dock.push_to_first_leaf(tab),
    }
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.set_active_tab(path);
    }
}

/// View > Undock: float the 3D or 2D view, or dock it back when it already floats.
pub fn toggle_undock(dock: &mut DockState<Tab>, tab: Tab, at: Pos2) {
    match surface_of(dock, tab) {
        Some(s) if !s.is_main() => redock(dock, tab),
        Some(_) => {
            float(dock, tab, at, UNDOCKED_VIEW_SIZE);
        }
        None => {}
    }
}

/// Show `tab`: reopen it beside Properties (or in the first pane) when it was closed, then bring
/// it to the front.
pub fn open(dock: &mut DockState<Tab>, tab: Tab) {
    if dock.find_tab(&tab).is_none() {
        let host = if matches!(tab, Tab::Viewport | Tab::UvView) { None } else { dock.find_tab(&Tab::Properties).filter(|p| p.surface.is_main()) };
        match host {
            Some(host) => dock[host.surface][host.node].append_tab(tab),
            None => dock.push_to_first_leaf(tab),
        }
    }
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.set_active_tab(path);
    }
}

/// Dock every floating panel back into the main area (a restored layout's windows may be off
/// screen) and drop the emptied windows.
pub fn settle_floating(dock: &mut DockState<Tab>) {
    let floating: Vec<Tab> = dock.iter_all_tabs().filter(|(p, _)| !p.surface.is_main()).map(|(_, t)| *t).collect();
    for tab in floating {
        redock(dock, tab);
    }
    // Removing from the end pops each window (and the empty placeholders egui_dock leaves for
    // windows closed out of order), so only the main area is left.
    let windows: Vec<SurfaceIndex> = (1..dock.iter_surfaces().count()).map(SurfaceIndex).collect();
    for s in windows.into_iter().rev() {
        dock.remove_surface(s);
    }
}

/// Whether `tab` currently floats in its own window.
pub fn is_floating(dock: &DockState<Tab>, tab: Tab) -> bool {
    surface_of(dock, tab).is_some_and(|s| !s.is_main())
}

/// The Docks toolbar: one button per closed panel, plus the panel it last popped up while that
/// still floats on its own. `popped` remembers that panel between frames.
pub fn toolbar(ui: &mut Ui, dock: &mut DockState<Tab>, popped: &mut Option<Tab>) {
    // A popped panel dragged into the layout (or closed) is no longer the toolbar's.
    if popped.is_some_and(|t| !is_floating(dock, t)) {
        *popped = None;
    }
    let mut shown = closed(dock);
    if let Some(t) = *popped {
        shown.push(t);
        shown.sort_by_key(|t| DOCKABLE.iter().position(|d| d == t));
    }
    // Right-to-left layout: iterate backwards so the buttons read in DOCKABLE order.
    for tab in shown.into_iter().rev() {
        let open = *popped == Some(tab);
        let response = ui.selectable_label(open, tab.icon()).on_hover_text(format!("{} · {}", tab.title(), tr("click to show, click again to close; drag its tab to dock it")));
        if response.clicked() {
            if open {
                close(dock, tab);
                *popped = None;
            } else {
                if let Some(previous) = popped.take() {
                    close(dock, previous);
                }
                // Below the button, kept on screen.
                let screen = ui.ctx().content_rect();
                let x = (response.rect.right() - POPUP_SIZE.x).clamp(screen.left(), (screen.right() - POPUP_SIZE.x).max(screen.left()));
                float(dock, tab, Pos2::new(x, response.rect.bottom() + 4.0), POPUP_SIZE);
                *popped = Some(tab);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::default_layout;

    #[test]
    fn the_toolbar_lists_closed_panels_and_pops_them_up_and_away() {
        let mut dock = default_layout();
        assert_eq!(closed(&dock), vec![Tab::DisplaySettings, Tab::ShaderSettings, Tab::Log], "the three optional panels start closed");
        // Pop it up: it floats alone in a new window.
        let s = float(&mut dock, Tab::DisplaySettings, Pos2::new(100.0, 50.0), POPUP_SIZE);
        assert!(!s.is_main() && is_floating(&dock, Tab::DisplaySettings));
        assert_eq!(closed(&dock), vec![Tab::ShaderSettings, Tab::Log]);
        // Closing it again removes the emptied window and brings its button back.
        let surfaces = dock.surfaces_count();
        close(&mut dock, Tab::DisplaySettings);
        assert_eq!(closed(&dock), vec![Tab::DisplaySettings, Tab::ShaderSettings, Tab::Log]);
        assert!(dock.surfaces_count() <= surfaces);
        // Any core panel can be closed and comes back through the toolbar.
        close(&mut dock, Tab::History);
        assert_eq!(closed(&dock), vec![Tab::DisplaySettings, Tab::ShaderSettings, Tab::Log, Tab::History]);
        float(&mut dock, Tab::History, Pos2::ZERO, POPUP_SIZE);
        redock(&mut dock, Tab::History);
        assert_eq!(surface_of(&dock, Tab::History), Some(SurfaceIndex::main()), "docked beside Properties");
        assert!(!closeable(Tab::Viewport) && !closeable(Tab::UvView) && closeable(Tab::Properties));
        // The Window > Views menu reopens a closed panel instead of doing nothing.
        close(&mut dock, Tab::Parts);
        open(&mut dock, Tab::Parts);
        assert_eq!(surface_of(&dock, Tab::Parts), Some(SurfaceIndex::main()));
    }

    #[test]
    fn the_views_undock_into_a_window_and_dock_back_behind_each_other() {
        let mut dock = default_layout();
        toggle_undock(&mut dock, Tab::Viewport, Pos2::new(40.0, 40.0));
        assert!(is_floating(&dock, Tab::Viewport));
        assert!(!is_floating(&dock, Tab::UvView), "the 2D view stays in the layout");
        toggle_undock(&mut dock, Tab::Viewport, Pos2::ZERO);
        assert!(!is_floating(&dock, Tab::Viewport));
        let (view, uv) = (dock.find_tab(&Tab::Viewport).unwrap(), dock.find_tab(&Tab::UvView).unwrap());
        assert_eq!((view.surface, view.node), (uv.surface, uv.node), "back in the 2D view's pane");
        // Both views can float at once; each docks back to the other's pane or the first one.
        toggle_undock(&mut dock, Tab::UvView, Pos2::ZERO);
        toggle_undock(&mut dock, Tab::Viewport, Pos2::ZERO);
        assert!(is_floating(&dock, Tab::UvView) && is_floating(&dock, Tab::Viewport));
        redock(&mut dock, Tab::Viewport);
        redock(&mut dock, Tab::UvView);
        assert!(!is_floating(&dock, Tab::Viewport) && !is_floating(&dock, Tab::UvView));
    }
}
