//! The eframe application: menu bar, dockable panels, status bar and keyboard shortcuts.

use crate::i18n::tr;
use std::path::PathBuf;

use std::collections::HashSet;

use egui::{RichText, Ui};
use egui_dock::{DockArea, DockState, NodeIndex};
use serde::{Deserialize, Serialize};
use subcraft_app::{Action, Command, Key, NewLayer, Session, Shortcut};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, LayerContent, MaskEffectKind, ProceduralKind, ProceduralParams};

use crate::dialogs::{self, Pending};
use crate::orbit::OrbitCamera;
use crate::state::{Tool, UiState};
use crate::widgets::{icon_button, primary_button, secondary_button, tool_button};
use crate::{icons, panels, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Viewport,
    UvView,
    Layers,
    Properties,
    Brush,
    /// Painter's TEXTURE SET LIST panel (one row per texture set).
    TextureSetList,
    /// Painter's TEXTURE SET SETTINGS panel (general properties, channels, mesh maps).
    TextureSetSettings,
    Parts,
    History,
    Shelf,
    /// Painter's DISPLAY SETTINGS panel. Optional: closed until View > Display settings opens it.
    DisplaySettings,
    /// An Assets sub-library: one search in a tab of its own (made from the Assets panel; never
    /// saved with the layout, Painter makes them again too).
    SubLibrary(u32),
    /// Painter's SHADER SETTINGS panel. Optional: closed until the Window menu or a texture
    /// set's shader menu opens it.
    ShaderSettings,
    /// Painter's LOG panel (General / Assets messages). Optional: closed until Window > Views
    /// opens it.
    Log,
}

impl Tab {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Tab::Viewport => tr("3D View"),
            Tab::UvView => tr("2D View"),
            Tab::Layers => tr("Layers"),
            Tab::Properties => tr("Properties"),
            Tab::Brush => tr("Brush"),
            Tab::TextureSetList => tr("Texture Set List"),
            Tab::TextureSetSettings => tr("Texture Set Settings"),
            Tab::Parts => tr("Parts"),
            Tab::History => tr("History"),
            Tab::Shelf => tr("Assets"),
            Tab::DisplaySettings => tr("Display Settings"),
            Tab::SubLibrary(_) => tr("Assets"),
            Tab::ShaderSettings => tr("Shader Settings"),
            Tab::Log => tr("Log"),
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Tab::Viewport => icons::CUBE,
            Tab::UvView => icons::GRID,
            Tab::Layers => icons::STACK,
            Tab::Properties => icons::SLIDERS,
            Tab::Brush => icons::PAINT_BRUSH,
            Tab::TextureSetList | Tab::TextureSetSettings => icons::CUBE_WIRE,
            Tab::Parts => icons::CUBE_WIRE,
            Tab::History => icons::HISTORY,
            Tab::Shelf | Tab::SubLibrary(_) => icons::SQUARES,
            Tab::DisplaySettings => icons::SUN,
            Tab::ShaderSettings => icons::SPARKLE,
            Tab::Log => icons::INFO,
        }
    }
}

/// Command-line options.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    /// Open Preferences (also useful for isolated renderer smoke tests).
    pub preferences: bool,
    pub graphics_startup: Option<crate::graphics::GraphicsUi>,
    /// Open the welcome hub even when taking a screenshot.
    pub welcome: bool,
    /// Open a step of the in-app guide (zero-based).
    pub guide_step: Option<usize>,

    /// Open a workspace snapshot preserved during an in-app update.
    pub resume_update: Option<PathBuf>,
    /// Check releases on startup (scripted verification / Help).
    pub check_updates: bool,
    #[cfg(feature = "update-fixture")]
    pub auto_update_fixture: bool,
    /// Project (`.subcraft`) or mesh to open instead of the bundled default model.
    pub mesh: Option<PathBuf>,
    /// Build a small demo layer stack (fills, masks and painted strokes) on start.
    pub demo: bool,
    /// Save a screenshot of the window to this path after a few frames, then quit.
    pub screenshot: Option<PathBuf>,
    /// Start with the 2D (UV) view in front.
    pub view_2d: bool,
    /// Start with Brush settings in front (also useful for native screenshot verification).
    pub brush_panel: bool,
    /// Start with the Display Settings panel open, scrolled to this group when it names one
    /// (`environment`, `camera`, `post`, `render`, `visualization`).
    pub display_settings: Option<String>,
    /// Start with a narrow vertical texture shelf (native layout validation).
    pub shelf_panel: bool,
    /// Bring Texture Set Settings in front of Layers (scripted captures).
    pub texture_set_settings: bool,
    /// Open the selected layer's first mask generator for native validation.
    pub generator_panel: bool,
    /// Select this Shelf brush (tip or preset name) shortly before a scripted screenshot, the
    /// same way a Shelf click does, to capture the viewport acknowledgement natively.
    pub select_brush: Option<String>,
    /// Run a scripted Clone or Smudge demonstration ("clone" / "smudge") for native captures.
    pub capture_tool: Option<String>,
    /// Open this Shelf tab (textures, materials, smart-materials, smart-masks, brushes, environments).
    pub shelf_tab: Option<String>,
    /// Import these smart material files / folders at startup (scripted captures and tests).
    pub import_smart: Vec<PathBuf>,
    /// Open the Export Textures window (scripted captures).
    pub export_window: bool,
    /// Open the New Project window with this model loaded (scripted captures).
    pub new_project_model: Option<PathBuf>,
    /// High-poly model for the same window.
    pub new_project_high: Option<PathBuf>,
    /// Give Properties the whole right column (scripted captures of its long blocks).
    pub properties_panel: bool,
    /// Export template to select (index into the built-in list) when the window opens.
    pub export_template: Option<usize>,
    /// Export preset to select by name or id (built-in or from the user library).
    pub export_preset: Option<String>,
    /// Painter's `--export-path`: the Export window's default output folder (a project's own
    /// saved export folder still wins when it has one).
    pub export_path: Option<PathBuf>,
    /// Open the Render quality window (scripted captures).
    pub render_window: bool,
    /// Render a still or a turntable from the loaded project, then quit (scripted export).
    pub render_job: Option<crate::render_export::Job>,
    /// Start in Rendering mode; a scripted screenshot waits for this many samples per pixel.
    pub path_trace: Option<u32>,
    /// Look overrides for scripted renders, e.g. `tone=filmic,bloom=0.8,ao=0.7,exposure=1.2`.
    pub quality: Option<String>,
    pub effect_panel: bool,
    pub symmetry_axes: Option<[bool; 3]>,
    /// Fixed camera for scripted screenshots (`--camera yaw,pitch,zoom[,x,y,z]`).
    pub camera: Option<crate::orbit::CameraView>,
    /// Radial symmetry for scripted runs: axis (0..3) and copy count.
    pub radial_symmetry: Option<(u8, u32)>,
    /// Start a real high-to-low bake and show its dialog (also usable with --screenshot).
    pub bake_high_poly: Option<PathBuf>,
    pub bake_size: Option<u32>,
    /// Bake part matching for this run (stored in the project settings like the dialog choice).
    pub bake_match: Option<subcraft_core::BakeMatchMode>,
    /// High-poly placement for this run: comma list of `auto`, `center`, `origin`, `offset=x:y:z`,
    /// `scale=s`, `rot=x:y:z` (scripted screenshots; stored in the project settings).
    pub bake_place: Option<String>,
    /// Inspect the real high/low geometry before baking, without starting the bake.
    pub bake_preview_only: bool,
    /// Bake every mesh map of the low poly onto itself at start (scripted captures of the Mesh Maps rows).
    pub bake_low: bool,
    /// Select the layer of this name after the demo stack is built (scripted captures).
    pub select_layer: Option<String>,
    /// Pin a completed map for reproducible native screenshot validation.
    pub bake_preview_map: Option<subcraft_core::MeshMapKind>,
    /// Start with the 3D view showing this baked mesh map (the mesh map viewer).
    pub mesh_map: Option<subcraft_core::MeshMapKind>,
    /// Light the viewport with the environment of this name.
    pub environment: Option<String>,
    /// Reload the project's mesh from this model on start (File > Reload mesh, scripted).
    pub reload_mesh: Option<PathBuf>,
    /// Mode for `reload_mesh`; `None` opens the window with the suggested mode for review.
    pub reload_mode: Option<subcraft_app::ReloadMode>,
    /// Open Auto UV unwrap and run it with the default settings on start (scripted).
    pub auto_uv: bool,
    /// Recover the newest crashed session without asking.
    pub recover: bool,
    /// Autosave interval instead of the preference (testing).
    pub autosave_interval: Option<std::time::Duration>,
    /// Panic a moment after start (tests the crash report).
    pub test_crash: bool,
    /// Ignore the saved dock layout and window geometry for this run.
    pub reset_layout: bool,
    /// Linux: run under XWayland / X11 instead of Wayland (`--x11`, `SUBCRAFT_X11=1`; pen
    /// tablets on compositors without pointer emulation, issue #82).
    pub force_x11: bool,
    /// Initial inner window size in points (scripted layout checks).
    pub window_size: Option<[f32; 2]>,
    /// Force the display scale (pixels per point) for scripted layout checks.
    pub scale: Option<f32>,
    /// Measure frames while a generator value changes every frame, write stats, quit.
    pub profile_edits: Option<(PathBuf, Option<u32>)>,
    /// Measure frames while the 3D view is orbited, hovered and painted with scripted input, write stats, quit.
    pub profile_view: Option<PathBuf>,
    /// Already-baked mesh maps: listed in the New Project window with `new_project_model`,
    /// otherwise in the "Import mesh maps" window for the open project (scripted captures).
    pub mesh_map_files: Vec<PathBuf>,
    /// Bake window: show the ray-miss heatmap.
    pub bake_misses: bool,
    /// Bake window: cage front / rear (fractions of the diagonal).
    pub bake_cage: Option<[f32; 2]>,
    /// Bake curvature from the normal map.
    pub curvature_from_normal: bool,
    /// Show the ID color editor with a color selected and its eyedropper on (scripted captures).
    pub id_colors: bool,
}

impl LaunchOptions {
    /// The Export window's folder at launch: `--export-path` when given, else `default`.
    pub fn export_folder(&self, default: &str) -> String {
        self.export_path.as_ref().map_or_else(|| default.to_string(), |p| p.display().to_string())
    }

    fn shows_welcome(&self, enabled: bool, recovered: bool) -> bool {
        self.resume_update.is_none() && (self.welcome || (enabled && self.mesh.is_none()
            && !self.demo && self.screenshot.is_none() && !recovered))
    }
}

/// Share of the centre the 3D view takes when the 3D and 2D views sit side by side (Painter opens
/// on 3D | 2D with the 2D view the narrower one).
pub const VIEW_SPLIT_3D: f32 = 0.62;

/// Substance Painter's arrangement: 3D | 2D in the centre (3D about 62%, 2D the rest; the divider
/// drags and its ratio is saved with the layout), ASSETS | HISTORY (| PARTS) on the left, and on
/// the right TEXTURE SET LIST, then LAYERS | TEXTURE SET SETTINGS, then PROPERTIES. Nothing below
/// the centre. A layout the user already saved is loaded instead (same storage key).
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let surface = dock.main_surface_mut();
    // Assets ~18% like Painter; the right column keeps 24% so Properties rows and chips fit at 1600 px.
    let [center, right] = surface.split_right(NodeIndex::root(), 0.76, vec![Tab::TextureSetList]);
    // Brush settings live inside the context-sensitive Properties panel (no separate tab).
    let [_, stack] = surface.split_below(right, 0.10, vec![Tab::Layers, Tab::TextureSetSettings]);
    surface.split_below(stack, 0.42, vec![Tab::Properties]);
    surface.split_left(center, 0.235, vec![Tab::Shelf, Tab::History, Tab::Parts]);
    let view = dock.find_tab(&Tab::Viewport).expect("the 3D view is in the layout");
    dock.main_surface_mut().split_right(view.node, VIEW_SPLIT_3D, vec![Tab::UvView]);
    for visible in [Tab::Shelf, Tab::Layers, Tab::Viewport] {
        if let Some(path) = dock.find_tab(&visible) { let _ = dock.set_active_tab(path); }
    }
    dock
}

/// The layout before Brush settings were folded into Properties: a separate Brush tab beside it.
/// Saved layouts like this still load; the Brush tab is removed when they are restored.
pub fn legacy_layout() -> DockState<Tab> {
    let mut dock = default_layout();
    let path = dock.find_tab(&Tab::Properties).expect("default layout has Properties");
    dock.set_focused_node_and_surface(path.node_path());
    dock.push_to_focused_leaf(Tab::Brush);
    dock
}

/// Old saved layouts have a Brush tab: drop it, Properties now shows the brush settings.
fn migrate_layout(dock: &mut DockState<Tab>) {
    if let Some(path) = dock.find_tab(&Tab::Brush) {
        let _ = dock.remove_tab(path);
    }
    dock.retain_tabs(|t| !matches!(t, Tab::SubLibrary(_)));
}

/// Keep the dock's sub-library tabs in step with the Assets panel: dock a new one beside the
/// Assets tab and bring it forward; drop the tab of one that was closed.
pub fn sync_sub_libraries(dock: &mut DockState<Tab>, shelf: &mut panels::shelf::ShelfUi) {
    dock.retain_tabs(|t| match t {
        Tab::SubLibrary(id) => shelf.sub_library_name(*id).is_some(),
        _ => true,
    });
    if let Some(id) = shelf.open_sub_library.take() {
        let tab = Tab::SubLibrary(id);
        if dock.find_tab(&tab).is_none() {
            match dock.find_tab(&Tab::Shelf) {
                Some(host) => dock[host.surface][host.node].append_tab(tab),
                None => dock.push_to_focused_leaf(tab),
            }
        }
        if let Some(path) = dock.find_tab(&tab) {
            let _ = dock.set_active_tab(path);
        }
    }
}

/// View > Display settings: add the panel as a tab beside Properties and bring it forward, or
/// close it when it is already open.
pub fn toggle_display_settings(dock: &mut DockState<Tab>) {
    if let Some(path) = dock.find_tab(&Tab::DisplaySettings) {
        let _ = dock.remove_tab(path);
        return;
    }
    open_display_settings(dock);
}

/// Open the Display Settings panel (if closed) and show it.
pub fn open_display_settings(dock: &mut DockState<Tab>) {
    if dock.find_tab(&Tab::DisplaySettings).is_none() {
        match dock.find_tab(&Tab::Properties) {
            Some(host) => dock[host.surface][host.node].append_tab(Tab::DisplaySettings),
            None => dock.push_to_focused_leaf(Tab::DisplaySettings),
        }
    }
    if let Some(path) = dock.find_tab(&Tab::DisplaySettings) {
        let _ = dock.set_active_tab(path);
    }
}

/// Open the Shader Settings panel beside Properties (if closed) and show it.
pub fn open_shader_settings(dock: &mut DockState<Tab>) {
    if dock.find_tab(&Tab::ShaderSettings).is_none() {
        match dock.find_tab(&Tab::Properties) {
            Some(host) => dock[host.surface][host.node].append_tab(Tab::ShaderSettings),
            None => dock.push_to_focused_leaf(Tab::ShaderSettings),
        }
    }
    if let Some(path) = dock.find_tab(&Tab::ShaderSettings) {
        let _ = dock.set_active_tab(path);
    }
}

/// Window > Shader settings: open the panel, or close it when it is open.
pub fn toggle_shader_settings(dock: &mut DockState<Tab>) {
    match dock.find_tab(&Tab::ShaderSettings) {
        Some(path) => {
            let _ = dock.remove_tab(path);
        }
        None => open_shader_settings(dock),
    }
}

/// Which views F frames: the one under the pointer, or both (3D, 2D) when it is over neither.
/// The path being edited: its UV rectangle (centre, size) and the box around its anchors on the
/// model (None when none of them lands on the surface).
fn path_bounds(state: &UiState) -> Option<((glam::Vec2, glam::Vec2), Option<subcraft_mesh::Aabb>)> {
    let path = state.pen.preview(&state.session)?;
    let uvs: Vec<glam::Vec2> = path.anchors.iter().map(|a| a.position).collect();
    let (lo, hi) = uvs.iter().fold((glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN)), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    if uvs.is_empty() { return None; }
    let world: Vec<glam::Vec3> = uvs.iter().filter_map(|&uv| state.session.pen_uv_position(uv)).collect();
    let world = (!world.is_empty()).then(|| subcraft_mesh::Aabb::from_points(world));
    Some((((lo + hi) * 0.5, hi - lo), world))
}

pub fn frame_targets(hovered_3d: bool, hovered_2d: bool) -> (bool, bool) {
    match (hovered_3d, hovered_2d) {
        (false, false) => (true, true),
        other => other,
    }
}

/// Whether the 2D view sits beside the 3D view (its own pane) instead of behind it as a tab.
pub fn is_side_by_side(dock: &DockState<Tab>) -> bool {
    match (dock.find_tab(&Tab::Viewport), dock.find_tab(&Tab::UvView)) {
        (Some(a), Some(b)) => (a.surface, a.node) != (b.surface, b.node),
        _ => false,
    }
}

/// View > Side-by-side 3D | 2D: put the 2D view in its own pane to the right of the 3D view
/// ([`VIEW_SPLIT_3D`]; the divider then drags horizontally like every dock divider and the ratio is saved
/// with the layout), or tab it behind the 3D view again.
pub fn toggle_side_by_side(dock: &mut DockState<Tab>) {
    let (Some(_), Some(uv)) = (dock.find_tab(&Tab::Viewport), dock.find_tab(&Tab::UvView)) else { return };
    if is_side_by_side(dock) {
        let _ = dock.remove_tab(uv);
        let Some(view) = dock.find_tab(&Tab::Viewport) else { return };
        dock[view.surface][view.node].append_tab(Tab::UvView);
        if let Some(path) = dock.find_tab(&Tab::UvView) { let _ = dock.set_active_tab(path); }
        if let Some(path) = dock.find_tab(&Tab::Viewport) { let _ = dock.set_active_tab(path); }
    } else {
        let _ = dock.remove_tab(uv);
        let Some(view) = dock.find_tab(&Tab::Viewport) else { return };
        dock.main_surface_mut().split_right(view.node, VIEW_SPLIT_3D, vec![Tab::UvView]);
    }
}

/// The share the first of the two side-by-side views has in `dock`, when they are split directly
/// against each other (the ratio the user dragged the divider to).
fn view_split(dock: &DockState<Tab>) -> Option<f32> {
    let (view, uv) = (dock.find_tab(&Tab::Viewport)?, dock.find_tab(&Tab::UvView)?);
    if view.surface != uv.surface { return None; }
    let parent = view.node.parent()?;
    if uv.node.parent() != Some(parent) { return None; }
    match &dock[view.surface][parent] {
        egui_dock::Node::Horizontal(split) => Some(split.fraction),
        _ => None,
    }
}

/// Painter's view layouts (F1..F4 and the Viewport menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewLayout {
    /// 3D and 2D views side by side.
    Both,
    Only3d,
    Only2d,
    /// Swap the two panes, or bring the other view to the front when they share a pane.
    Swap,
}

fn front_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(path) = dock.find_tab(&tab) { let _ = dock.set_active_tab(path); }
}

/// Whether `tab` is the one shown in its pane.
pub fn is_front(dock: &DockState<Tab>, tab: Tab) -> bool {
    dock.find_tab(&tab).is_some_and(|path| dock[path.surface][path.node].get_leaf().is_some_and(|leaf| leaf.active == path.tab))
}

impl ViewLayout {
    /// The layouts offered by the Viewport Mode dropdown, in Painter's order (F1..F4).
    pub const MENU: [ViewLayout; 4] = [ViewLayout::Both, ViewLayout::Only3d, ViewLayout::Only2d, ViewLayout::Swap];

    pub fn label(self) -> &'static str {
        match self {
            ViewLayout::Both => tr("3D | 2D"),
            ViewLayout::Only3d => tr("3D only"),
            ViewLayout::Only2d => tr("2D only"),
            ViewLayout::Swap => tr("Swap 3D and 2D"),
        }
    }
}

/// The layout the docks show: both views side by side, or the one in front of the other.
pub fn current_view_layout(dock: &DockState<Tab>) -> ViewLayout {
    if is_side_by_side(dock) {
        ViewLayout::Both
    } else if is_front(dock, Tab::UvView) {
        ViewLayout::Only2d
    } else {
        ViewLayout::Only3d
    }
}

pub fn apply_view_layout(dock: &mut DockState<Tab>, layout: ViewLayout) {
    match layout {
        ViewLayout::Both => if !is_side_by_side(dock) { toggle_side_by_side(dock) },
        ViewLayout::Only3d | ViewLayout::Only2d => {
            if is_side_by_side(dock) { toggle_side_by_side(dock); }
            front_tab(dock, if layout == ViewLayout::Only3d { Tab::Viewport } else { Tab::UvView });
        }
        ViewLayout::Swap => {
            let (Some(view), Some(uv)) = (dock.find_tab(&Tab::Viewport), dock.find_tab(&Tab::UvView)) else { return };
            if !is_side_by_side(dock) {
                front_tab(dock, if is_front(dock, Tab::Viewport) { Tab::UvView } else { Tab::Viewport });
                return;
            }
            // Exchange the two views where they are: side by side the split and its ratio stay;
            // with one undocked (Painter 7.4) the floating window now shows the other view.
            // Any other tab sharing a pane with a view stays where it was.
            for (path, tab) in [(view, Tab::UvView), (uv, Tab::Viewport)] {
                if let Some(slot) = dock[path.surface][path.node].get_leaf_mut().and_then(|leaf| leaf.tabs.get_mut(path.tab.0)) {
                    *slot = tab;
                }
            }
        }
    }
}

/// The dock shown while the panels are hidden (Tab): only the 3D and 2D views, arranged as in `dock`.
pub fn views_only(dock: &DockState<Tab>) -> DockState<Tab> {
    if is_side_by_side(dock) {
        // The pane order follows the full layout (F4 may have swapped them).
        let uv_first = match (dock.find_tab(&Tab::Viewport), dock.find_tab(&Tab::UvView)) {
            (Some(view), Some(uv)) => uv.node.0 < view.node.0,
            _ => false,
        };
        let (first, second) = if uv_first { (Tab::UvView, Tab::Viewport) } else { (Tab::Viewport, Tab::UvView) };
        let mut views = DockState::new(vec![first]);
        let split = view_split(dock).unwrap_or(VIEW_SPLIT_3D);
        views.main_surface_mut().split_right(NodeIndex::root(), split, vec![second]);
        views
    } else {
        let mut views = DockState::new(vec![Tab::Viewport, Tab::UvView]);
        front_tab(&mut views, if is_front(dock, Tab::UvView) { Tab::UvView } else { Tab::Viewport });
        views
    }
}

// Apply the requested shelf-left / utility-tabs-bottom arrangement once, then retain edits.
const DOCK_KEY: &str = "subcraft_dock_v6";
/// Side of the header logo in points (a small mark, never a splash).
const BRAND_MARK_SIZE: f32 = 22.0;

/// A restored dock layout is only used when it still holds every panel exactly once, has no
/// floating windows (they can be left off-screen after a monitor change) and no split is
/// squeezed to nothing. Anything else falls back to the default layout.
pub fn layout_is_valid(dock: &DockState<Tab>) -> bool {
    use egui_dock::Node;
    let mut seen = std::collections::HashMap::<Tab, usize>::new();
    for (_, tab) in dock.iter_all_tabs() { *seen.entry(*tab).or_default() += 1; }
    // The 3D and 2D views are always there. Every other panel can be closed (it comes back from
    // the Docks toolbar) but appears at most once; Brush is the legacy tab of old layouts.
    let required = [Tab::Viewport, Tab::UvView];
    if !required.iter().all(|t| seen.get(t) == Some(&1)) || seen.values().any(|&n| n > 1) { return false; }
    if seen.keys().any(|t| !required.contains(t) && !crate::docks::closeable(*t) && *t != Tab::Brush) { return false; }
    if dock.iter_surfaces().count() != 1 { return false; }
    dock.iter_all_nodes().all(|(_, node)| match node {
        Node::Horizontal(split) | Node::Vertical(split) => split.fraction.is_finite() && (0.04..=0.96).contains(&split.fraction),
        _ => true,
    })
}

/// Parse `1280x720`.
pub fn parse_size(text: &str) -> Option<[f32; 2]> {
    let (w, h) = text.split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?);
    (w >= 320.0 && h >= 240.0 && w <= 16384.0 && h <= 16384.0).then_some([w, h])
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn f_frames_the_view_under_the_pointer_or_both() {
        assert_eq!(frame_targets(true, false), (true, false));
        // The Viewport Mode dropdown reads the layout back from the docks.
        let mut dock = DockState::new(vec![Tab::Viewport, Tab::UvView]);
        apply_view_layout(&mut dock, ViewLayout::Both);
        assert_eq!(current_view_layout(&dock), ViewLayout::Both);
        apply_view_layout(&mut dock, ViewLayout::Only2d);
        assert_eq!(current_view_layout(&dock), ViewLayout::Only2d);
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert_eq!(current_view_layout(&dock), ViewLayout::Only3d, "swapping tabbed views brings the 3D view to the front");
        assert_eq!(ViewLayout::MENU.len(), 4);
    }

    #[test]
    fn swap_exchanges_a_floating_view_with_the_docked_one() {
        let mut dock = default_layout();
        crate::docks::toggle_undock(&mut dock, Tab::UvView, egui::pos2(40.0, 40.0));
        assert!(crate::docks::is_floating(&dock, Tab::UvView) && !crate::docks::is_floating(&dock, Tab::Viewport));
        let docked_at = dock.find_tab(&Tab::Viewport).unwrap();
        let others: Vec<Tab> = dock.iter_all_tabs().map(|(_, t)| *t).filter(|t| !matches!(t, Tab::Viewport | Tab::UvView)).collect();
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert!(crate::docks::is_floating(&dock, Tab::Viewport), "the 3D view now floats");
        assert_eq!(dock.find_tab(&Tab::UvView), Some(docked_at), "the 2D view takes the 3D view's place");
        let after: Vec<Tab> = dock.iter_all_tabs().map(|(_, t)| *t).filter(|t| !matches!(t, Tab::Viewport | Tab::UvView)).collect();
        assert_eq!(others, after, "every other panel stays where it was");
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert!(crate::docks::is_floating(&dock, Tab::UvView));
        assert_eq!(dock.find_tab(&Tab::Viewport), Some(docked_at), "swapping twice restores the layout");
        // Side by side, the swap still exchanges the panes.
        let mut side = DockState::new(vec![Tab::Viewport, Tab::UvView]);
        apply_view_layout(&mut side, ViewLayout::Both);
        let (v, u) = (side.find_tab(&Tab::Viewport).unwrap(), side.find_tab(&Tab::UvView).unwrap());
        apply_view_layout(&mut side, ViewLayout::Swap);
        assert_eq!((side.find_tab(&Tab::Viewport), side.find_tab(&Tab::UvView)), (Some(u), Some(v)));
        assert_eq!(frame_targets(false, true), (false, true));
        assert_eq!(frame_targets(false, false), (true, true), "over a panel: both views");
        // QA 2026-11: a view no longer drawn kept its last hover, and F framed it instead of the
        // visible one. Each frame starts with neither hovered; the drawn views set their own.
        let mut state = crate::state::UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        state.hovered_3d = true;
        state.hovered_2d = true;
        state.gpu_changed = true;
        state.begin_frame();
        assert!(!state.hovered_3d && !state.hovered_2d);
        assert!(!state.gpu_changed, "uploads are counted per frame");
    }
    #[test]
    fn default_layout_is_valid_and_broken_ones_are_rejected() {
        assert!(layout_is_valid(&default_layout()));
        let mut missing = default_layout();
        let path = missing.find_tab(&Tab::Viewport).unwrap();
        let _ = missing.remove_tab(path);
        assert!(!layout_is_valid(&missing), "the 3D view is never missing");
        let mut closed = default_layout();
        let path = closed.find_tab(&Tab::Shelf).unwrap();
        let _ = closed.remove_tab(path);
        assert!(layout_is_valid(&closed), "a panel closed with its X (Docks toolbar) is a valid layout");
        let mut squeezed = default_layout();
        for (_, node) in squeezed.iter_all_nodes_mut() {
            if let egui_dock::Node::Horizontal(s) | egui_dock::Node::Vertical(s) = node { s.fraction = 0.0; }
        }
        assert!(!layout_is_valid(&squeezed));
        let mut floating = default_layout();
        floating.add_window(vec![]);
        assert!(!layout_is_valid(&floating));
        // A saved layout with a floating panel loads with the panel docked.
        let mut popped = default_layout();
        crate::docks::float(&mut popped, Tab::History, egui::Pos2::ZERO, crate::docks::POPUP_SIZE);
        crate::docks::settle_floating(&mut popped);
        assert!(layout_is_valid(&popped) && popped.find_tab(&Tab::History).is_some_and(|p| p.surface.is_main()));
        let mut doubled = default_layout();
        doubled.push_to_focused_leaf(Tab::Viewport);
        assert!(!layout_is_valid(&doubled));
    }
    #[test]
    fn default_layout_matches_painter_with_nothing_below_the_centre() {
        let dock = default_layout();
        let node = |t: Tab| { let p = dock.find_tab(&t).unwrap(); (p.surface, p.node) };
        let visible = |tab: Tab| { let p = dock.find_tab(&tab).unwrap(); dock[p.surface][p.node].get_leaf().unwrap().active == p.tab };
        // 3D | 2D side by side, the 3D view on the left and wider (about 62 / 38).
        assert!(is_side_by_side(&dock) && current_view_layout(&dock) == ViewLayout::Both);
        assert!(visible(Tab::Viewport) && visible(Tab::UvView), "both views show");
        assert!(node(Tab::Viewport).1.is_left() && node(Tab::UvView).1.is_right(), "3D left, 2D right");
        let split = view_split(&dock).expect("the views are split against each other");
        assert!((split - VIEW_SPLIT_3D).abs() < 1e-6 && (0.6..=0.65).contains(&split), "3D about 62%: {split}");
        assert!(1.0 - split < split, "the 2D view is the narrower one");
        assert!(matches!(dock.main_surface()[node(Tab::Viewport).1.parent().unwrap()], egui_dock::Node::Horizontal(_)), "split left | right");
        assert_eq!(node(Tab::Shelf), node(Tab::History), "History sits beside Assets");
        assert_eq!(node(Tab::Layers), node(Tab::TextureSetSettings), "Layers | Texture Set Settings");
        for t in [Tab::TextureSetList, Tab::Properties, Tab::Layers, Tab::Shelf] { assert_ne!(node(t), node(Tab::Viewport)); }
        assert_ne!(node(Tab::TextureSetList), node(Tab::Layers));
        assert_ne!(node(Tab::Layers), node(Tab::Properties));
        // The centre leaf is the only thing in its column: its parent split is horizontal only.
        let leaves = dock.main_surface().iter().filter(|n| n.is_leaf()).count();
        assert_eq!(leaves, 6, "3D, 2D, assets group, list, layers group, properties");
        // Vertical order on the right: list above layers above properties (by split nesting).
        use egui_dock::Node;
        let (list, layers, props) = (node(Tab::TextureSetList).1, node(Tab::Layers).1, node(Tab::Properties).1);
        let vertical_parent = |n: NodeIndex| n.parent().map(|p| matches!(dock.main_surface()[p], Node::Vertical(_))).unwrap_or(false);
        assert!(vertical_parent(list) && vertical_parent(layers) && vertical_parent(props));
        assert!(!dock.iter_all_tabs().any(|(_, t)| *t == Tab::Brush));
        // Proportions of Painter's right column: short list, Layers + Settings about a third,
        // Properties the rest (about half or more).
        let share = |n: NodeIndex| match dock.main_surface()[n.parent().unwrap()] { Node::Vertical(ref v) | Node::Horizontal(ref v) => if n.is_left() { v.fraction } else { 1.0 - v.fraction }, _ => unreachable!() };
        let list_share = share(list);
        let stack_parent = layers.parent().unwrap();
        let props_share = share(props);
        let layers_share = (1.0 - list_share) * share(layers);
        assert!(list_share <= 0.15, "texture set list is short: {list_share}");
        assert!((0.28..=0.40).contains(&layers_share), "layers + settings about a third: {layers_share}");
        assert!(props_share * (1.0 - list_share) >= 0.5, "properties gets half or more");
        let _ = stack_parent;
    }
    #[test]
    fn brush_settings_live_in_properties_and_old_layouts_with_a_brush_tab_still_load() {
        assert!(default_layout().find_tab(&Tab::Brush).is_none(), "no separate Brush tab by default");
        let mut old = legacy_layout();
        assert!(old.find_tab(&Tab::Brush).is_some() && layout_is_valid(&old), "a saved layout with the Brush tab is still valid");
        let mut restored = old.clone();
        migrate_layout(&mut restored);
        assert!(restored.find_tab(&Tab::Brush).is_none() && layout_is_valid(&restored));
        assert!(restored.find_tab(&Tab::Properties).is_some());
        let mut doubled = legacy_layout();
        doubled.push_to_focused_leaf(Tab::Brush);
        assert!(!layout_is_valid(&doubled), "two Brush tabs are rejected");
        migrate_layout(&mut old);
        assert!(layout_is_valid(&old));
    }
    #[test]
    fn view_menu_request_pins_brush_settings_in_properties_until_the_selection_changes() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false, popped_dock: None,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None, view_profile: None,
        };
        app.state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        app.follow_layer_selection();
        assert!(!panels::properties::shows_brush(&app.state), "a fill layer shows its own properties");
        app.state.open_brush_editor = true;
        app.open_requested_brush_editor();
        assert!(panels::properties::shows_brush(&app.state));
        assert_eq!(app.dock.find_active_focused().map(|(_, t)| *t), Some(Tab::Properties));
        app.state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        app.follow_layer_selection();
        assert!(!app.state.brush_pinned && !panels::properties::shows_brush(&app.state));
    }
    #[test]
    fn side_by_side_splits_2d_beside_3d_with_a_saved_ratio_and_toggles_back() {
        let mut dock = default_layout();
        assert!(is_side_by_side(&dock), "the default layout is 3D | 2D");
        toggle_side_by_side(&mut dock);
        assert!(!is_side_by_side(&dock) && layout_is_valid(&dock), "toggled to one pane");
        toggle_side_by_side(&mut dock);
        assert!(is_side_by_side(&dock) && layout_is_valid(&dock), "a valid layout with both views in their own panes");
        let view = dock.find_tab(&Tab::Viewport).unwrap();
        let uv = dock.find_tab(&Tab::UvView).unwrap();
        // The panes are the two sides of a horizontal split, 3D the wider one.
        let parent = view.node.parent().unwrap();
        assert_eq!(uv.node.parent(), Some(parent));
        match &dock[view.surface][parent] {
            egui_dock::Node::Horizontal(split) => assert!((split.fraction - VIEW_SPLIT_3D).abs() < 1e-6),
            other => panic!("expected a horizontal split, got {:?}", other.is_parent()),
        }
        // Dragging the divider (a different fraction) survives a save and load, so the ratio is kept.
        if let egui_dock::Node::Horizontal(split) = &mut dock[view.surface][parent] { split.fraction = 0.3; }
        // The ratio lives in the dock state that is saved and restored as a whole (DOCK_KEY), so a dragged divider is kept.
        assert!(layout_is_valid(&dock) && is_side_by_side(&dock));
        assert!((view_split(&dock).unwrap() - 0.3).abs() < 1e-6, "the dragged split is read back");
        // Hiding the panels (Tab) keeps the dragged split too.
        assert!((view_split(&views_only(&dock)).unwrap() - 0.3).abs() < 1e-6);
        toggle_side_by_side(&mut dock);
        assert!(!is_side_by_side(&dock) && layout_is_valid(&dock));
        let v = dock.find_tab(&Tab::Viewport).unwrap();
        assert!(dock[v.surface][v.node].get_leaf().unwrap().active == v.tab, "3D visible again");
    }
    #[test]
    fn painter_view_layouts_f1_to_f4() {
        let mut dock = default_layout();
        assert_eq!(current_view_layout(&dock), ViewLayout::Both, "Painter opens on 3D | 2D");
        apply_view_layout(&mut dock, ViewLayout::Only3d);
        assert!(!is_side_by_side(&dock) && is_front(&dock, Tab::Viewport) && !is_front(&dock, Tab::UvView));
        // F3: 2D only, F2: 3D only (the other view stays a tab behind it).
        apply_view_layout(&mut dock, ViewLayout::Only2d);
        assert!(!is_side_by_side(&dock) && is_front(&dock, Tab::UvView));
        apply_view_layout(&mut dock, ViewLayout::Only3d);
        assert!(is_front(&dock, Tab::Viewport));
        // F4 in one pane brings the other view forward.
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert!(is_front(&dock, Tab::UvView));
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert!(is_front(&dock, Tab::Viewport));
        // F1: both, the 3D view on the left.
        apply_view_layout(&mut dock, ViewLayout::Both);
        assert!(is_side_by_side(&dock) && layout_is_valid(&dock));
        let nodes = |dock: &DockState<Tab>| (dock.find_tab(&Tab::Viewport).unwrap().node, dock.find_tab(&Tab::UvView).unwrap().node);
        let (view, uv) = nodes(&dock);
        apply_view_layout(&mut dock, ViewLayout::Both);
        assert_eq!(nodes(&dock), (view, uv), "F1 twice changes nothing");
        // F4 side by side exchanges the panes and keeps the split.
        apply_view_layout(&mut dock, ViewLayout::Swap);
        assert_eq!(nodes(&dock), (uv, view));
        assert!(is_side_by_side(&dock) && layout_is_valid(&dock));
        // The views-only layout (Tab) follows the full one.
        let hidden = views_only(&dock);
        assert!(is_side_by_side(&hidden) && hidden.iter_all_tabs().count() == 2);
        assert!(hidden.find_tab(&Tab::UvView).unwrap().node.0 < hidden.find_tab(&Tab::Viewport).unwrap().node.0, "the swapped order is kept");
        // F2 from side by side returns to one pane with the 3D view in front.
        apply_view_layout(&mut dock, ViewLayout::Only3d);
        assert!(!is_side_by_side(&dock) && is_front(&dock, Tab::Viewport) && layout_is_valid(&dock));
        apply_view_layout(&mut dock, ViewLayout::Only2d);
        let hidden = views_only(&dock);
        assert!(!is_side_by_side(&hidden) && is_front(&hidden, Tab::UvView) && hidden.iter_all_tabs().count() == 2);
    }

    #[test]
    fn tab_is_a_shortcut_unless_a_text_field_has_the_keyboard() {
        let key = |key, pressed, modifiers| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        let none = egui::Modifiers::NONE;
        let mut events = vec![key(egui::Key::Tab, true, none), key(egui::Key::A, true, none), key(egui::Key::Tab, false, none)];
        assert!(take_tab_press(&mut events, false));
        assert_eq!(events.len(), 1, "egui never sees the Tab, so the focus does not move: {events:?}");
        // While typing, Tab stays egui's.
        let mut events = vec![key(egui::Key::Tab, true, none)];
        assert!(!take_tab_press(&mut events, true));
        assert_eq!(events.len(), 1);
        // Ctrl+Tab / Shift+Tab are not the shortcut.
        let mut events = vec![key(egui::Key::Tab, true, egui::Modifiers::SHIFT)];
        assert!(!take_tab_press(&mut events, false));
        assert_eq!(events.len(), 1);
        assert_eq!(map_key(egui::Key::Tab), Some(Key::Tab));
    }

    #[test]
    fn window_size_parsing() {
        assert_eq!(parse_size("1280x720"), Some([1280.0, 720.0]));
        assert_eq!(parse_size("10x10"), None);
        assert_eq!(parse_size("abc"), None);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct InspectorSelection(subcraft_app::Selection, Option<subcraft_doc::LayerKind>, subcraft_app::LayerTarget, Option<subcraft_core::EffectId>, bool);

impl InspectorSelection {
    fn current(session: &Session) -> Self {
        let paints_effect = matches!(session.brush.target, subcraft_paint::PaintTarget::ContentEffect(_));
        Self(session.selection(), session.selected_layer().map(|layer| layer.kind()), session.layer_target(), session.selected_effect(), paints_effect)
    }
}

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod inspector_tests;

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn restored_updates_and_existing_work_open_directly_in_the_workspace() {
        let mut options = LaunchOptions::default();
        assert!(options.shows_welcome(true, false)); assert!(!options.shows_welcome(false, false));
        assert!(!options.shows_welcome(true, true));
        options.mesh = Some("project.subcraft".into()); assert!(!options.shows_welcome(true, false));
        options.mesh = None; options.demo = true; assert!(!options.shows_welcome(true, false));
        options.demo = false; options.screenshot = Some("capture.png".into()); assert!(!options.shows_welcome(true, false));
        options.welcome = true; assert!(options.shows_welcome(false, false));
        options.resume_update = Some("job.json".into());
        assert!(!options.shows_welcome(true, false)); assert!(!options.shows_welcome(true, true));
    }

    #[test]
    fn recent_open_waits_for_unsaved_changes_before_replacing_a_session() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false, popped_dock: None,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None, view_profile: None,
        };
        app.request(Pending::OpenRecent, 0.0);
        assert_eq!(app.pending_now.take(), Some(Pending::OpenRecent));
        assert_eq!(app.state.confirm, None);
        app.state.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        let doc = app.state.session.document().clone();
        app.request(Pending::OpenRecent, 0.0);
        assert_eq!(app.pending_now, None);
        assert_eq!(app.state.confirm, Some(Pending::OpenRecent));
        assert_eq!(app.state.session.document(), &doc);
    }

    #[test]
    fn shelf_double_click_request_brings_the_brush_settings_forward_once() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None),
            dock: legacy_layout(), frames: 0, screenshot: None, screenshot_requested: false,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, popped_dock: None, profile: None, view_profile: None,
        };
        let brush = app.dock.find_tab(&Tab::Brush).unwrap();
        let properties = app.dock.find_tab(&Tab::Properties).unwrap();
        app.dock.set_active_tab(properties).unwrap();
        let active = |app: &mut SubcraftApp| app.dock.find_active_focused().map(|(_, tab)| *tab);
        app.open_requested_brush_editor();
        assert_ne!(active(&mut app), Some(Tab::Brush), "no request, no change");
        app.state.open_brush_editor = true;
        app.open_requested_brush_editor();
        assert_eq!(active(&mut app), Some(Tab::Brush));
        assert!(!app.state.open_brush_editor, "consumed");
        assert_eq!(Some(brush.node_path()), app.dock.focused_leaf());
    }

    #[test]
    fn edit_menu_opens_settings_without_project_edits() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false, popped_dock: None,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None, view_profile: None,
        };
        let doc = app.state.session.document().clone();
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let draw = |app: &mut SubcraftApp, events| {
            let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))), ..Default::default() }, |ui| {
                egui::Panel::top("file-menu-test").exact_size(44.0).show(ui, |ui| app.header_bar(ui));
            });
            out.textures_delta.clear(); out
        };
        let find = |out: &egui::FullOutput, text: &str| out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().ends_with(text) => Some(t.pos + t.galley.size() * 0.5), _ => None,
        }).unwrap_or_else(|| panic!("missing {text}"));
        for label in ["Edit", "Settings…"] {
            for _ in 0..3 { draw(&mut app, vec![]); }
            let out = draw(&mut app, vec![]); let p = find(&out, label);
            for pressed in [true, false] {
                draw(&mut app, vec![egui::Event::PointerMoved(p), egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
            }
        }
        assert!(app.state.recovery.show_prefs);
        assert_eq!(app.state.session.document(), &doc);
    }

    #[test]
    fn long_header_titles_cannot_paint_over_menus_or_actions() {
        let mut mesh = subcraft_mesh::primitives::cube(2.0);
        mesh.name = "A very long multi-part model with a name that exceeds the header gap".into();
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(mesh, 128), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false, popped_dock: None,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None, view_profile: None,
        };
        let title = dialogs::title(&app.state).trim_start_matches("SubCraft — ").to_string();
        for width in [460.0, 1200.0] {
            let ctx = egui::Context::default(); theme::apply(&ctx);
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0))), ..Default::default() }, |ui| {
                egui::Panel::top("test-header").exact_size(44.0).show(ui, |ui| app.header_bar(ui));
            });
            output.textures_delta.clear();
            let texts: Vec<_> = output.shapes.iter().filter_map(|s| {
                if let egui::epaint::Shape::Text(t) = &s.shape { Some((s.clip_rect, t)) } else { None }
            }).collect();
            let project = texts.iter().find(|(_, t)| t.galley.job.text == title);
            if width == 460.0 { assert!(project.is_none(), "A narrow header must leave room for its controls"); }
            else {
                let (clip, _) = project.expect("A wide header should show the project name");
                let (_, help) = texts.iter().find(|(_, t)| t.galley.job.text == "Help").unwrap();
                let (_, export) = texts.iter().find(|(_, t)| t.galley.job.text.contains("Export")).unwrap();
                assert!(clip.left() > help.pos.x + help.galley.size().x);
                assert!(clip.right() < export.pos.x);
            }
        }
    }

    #[test]
    fn save_and_open_wait_for_pen_jobs_and_then_use_the_finished_document() {
        let file = std::env::temp_dir().join(format!("subcraft-pen-save-{}.subcraft", subcraft_core::ObjectId::new()));
        let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128); session.save(Some(&file)).unwrap();
        let mut app = SubcraftApp {
            state: UiState::new(session, None), dock: default_layout(), frames: 0,
            screenshot: None, screenshot_requested: false, pending_now: None,
            active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, popped_dock: None, profile: None, view_profile: None,
        };
        app.state.pen.begin(&app.state.session, glam::Vec2::splat(0.4), None).unwrap();
        app.perform(Action::Save, 0.0); assert!(app.state.pen_after.is_some());
        assert_eq!(Session::open_project(&file).unwrap().selected_layer().unwrap().kind(), subcraft_doc::LayerKind::Paint);
        while app.state.pen.poll(&mut app.state.session, 16384).unwrap() {}
        app.flush_pen_after(0.1);
        assert_eq!(Session::open_project(&file).unwrap().document(), app.state.session.document());
        assert!(!app.state.session.is_modified());
        app.state.pen.begin(&app.state.session, glam::Vec2::splat(0.6), None).unwrap();
        app.request(Pending::OpenProject, 0.2); assert_eq!(app.state.confirm, None); assert!(app.pending_now.is_none());
        while app.state.pen.poll(&mut app.state.session, 16384).unwrap() {}
        app.flush_pen_after(0.3); assert_eq!(app.state.confirm, Some(Pending::OpenProject));
        std::fs::remove_file(file).unwrap();
    }
}

pub struct SubcraftApp {
    state: UiState,
    dock: DockState<Tab>,
    frames: u64,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    /// Action approved this frame, run after the UI pass.
    pending_now: Option<Pending>,
    /// Tabs whose body was shown last frame (drawn with the accent marker).
    active_tabs: HashSet<Tab>,
    /// Follow selection changes once, leaving explicit tab choices alone between selections.
    inspector_selection: Option<InspectorSelection>,
    test_crash: bool,
    /// The panel the Docks toolbar last popped up (closed again by a second click).
    popped_dock: Option<Tab>,
    profile: Option<crate::edit_profile::EditProfile>,
    view_profile: Option<crate::view_profile::ViewProfile>,
}

impl SubcraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: LaunchOptions) -> Self {
        theme::apply(&cc.egui_ctx);
        if let Some(scale) = options.scale { cc.egui_ctx.set_pixels_per_point(scale); }
        let mut status = None;
        let is_project = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(subcraft_app::PROJECT_EXTENSION));
        let resumed = options.resume_update.as_ref().and_then(|path| match subcraft_app::updater::resume_session(path) {
            Ok(session) => Some(session), Err(e) => { status = Some(format!("Could not restore update snapshot: {e}")); None }
        });
        let session = if let Some(session) = resumed { session } else { match &options.mesh {
            Some(path) if is_project(path) => match Session::open_project(path) {
                Ok(s) => s,
                Err(e) => {
                    status = Some(format!("Could not open {}: {e}. Showing the sample.", path.display()));
                    Session::default_model()
                }
            },
            // A Painter project opens in the background (started below): show the sample until then.
            Some(path) if crate::spp_import::is_spp(path) => Session::default_model(),
            Some(path) => match Session::open_mesh(path, subcraft_app::DEFAULT_RESOLUTION) {
                Ok((s, warnings)) => {
                    if !warnings.is_empty() {
                        status = Some(format!("Imported with {} warning(s): {}", warnings.len(), warnings[0]));
                    }
                    s
                }
                Err(e) => {
                    status = Some(format!("Could not open {}: {e}. Showing the sample.", path.display()));
                    Session::default_model()
                }
            },
            None => Session::default_model(),
        }};
        let restore_failed = status.is_some();
        let mut state = UiState::new(session, cc.wgpu_render_state.clone())
            .with_user_library(subcraft_app::assets::UserLibrary::default_root().map(subcraft_app::assets::UserLibrary::new));
        let opened_from_arg = !status.as_ref().is_some_and(|s| s.starts_with("Could not open"));
        if let Some(s) = status {
            state.set_status(s, 0.0);
        }
        {
            if !restore_failed && let Some(path) = &options.resume_update {
                let result = std::fs::read_to_string(path.with_file_name("result.txt")).unwrap_or_else(|_| "Your workspace was restored after an update.".into());
                state.set_status(result.clone(), 0.0);
                if result.starts_with("Update failed") && let Some(updater) = &mut state.updater { updater.open = true; updater.phase = subcraft_app::updater::Phase::Error(result); }
            }
        }
        if !restore_failed && options.resume_update.is_none() && let Some(path) = options.mesh.as_ref().filter(|p| crate::spp_import::is_spp(p)) {
            crate::spp_import::start(&mut state, path);
        }
        crate::bug_report::record_gpu(cc.wgpu_render_state.as_ref());
        // From here on a lost graphics device ends in a message and a safe-mode restart, never
        // in a panic of the renderer (issue #119).
        crate::device_lost::install(&cc.egui_ctx, options.screenshot.is_none());
        state.bug_report = crate::bug_report::BugReportUi::load();
        if let Some(graphics) = options.graphics_startup.clone() {
            let active = state.graphics.active.clone(); state.graphics = graphics; state.graphics.active = active;
        }
        let prefs = cc.storage.and_then(|s| s.get_string(crate::recovery::PREFS_KEY)).map(|j| crate::recovery::Preferences::from_json_lenient(&j)).unwrap_or_default();
        crate::recovery::apply_cache_dir(&prefs);
        state.recovery = crate::recovery::RecoveryUi::start(&state.session, prefs, options.autosave_interval);
        // Settings > Libraries: extra library folders, and which library is the default.
        let (extra, default) = (state.recovery.prefs.extra_libraries.clone(), state.recovery.prefs.default_library.clone());
        if !extra.is_empty() || default != crate::panels::shelf_filter::YOURS {
            state.set_libraries(&extra, &default);
        }
        // The "SubCraft Materials" pack folder, if one is set.
        state.reload_materials_pack();
        if subcraft_app::safe_mode::active() == Some(&subcraft_app::safe_mode::Reason::GpuLostLastRun) {
            // The GPU reset last run: keep the heavy GPU work off from now on (saved with the
            // preferences) so the next normal start does not reset it again (issue #119).
            state.recovery.prefs.use_gpu_compositing = false;
            state.recovery.prefs.use_gpu_baking = false;
        }
        state.recovery.prefs.apply_ui_size(&cc.egui_ctx);
        theme::set(&cc.egui_ctx, state.recovery.prefs.theme);
        crate::i18n::sync(&cc.egui_ctx, state.recovery.prefs.language);
        subcraft_compositor::gpu::set_gpu_compositing_enabled(state.recovery.prefs.use_gpu_compositing && !subcraft_app::safe_mode::is_active());
        state.recovery.show_prefs = options.preferences;
        if options.bake_low && options.texture_set_settings { cc.egui_ctx.data_mut(|d| d.insert_temp(egui::Id::new("capture_jump_maps"), true)); }
        if options.bake_low && options.bake_high_poly.is_none() {
            if let Some(size) = options.bake_size { state.bake.settings.size = size; }
            else if let Some(r) = state.session.document().texture_sets.iter().map(|t| t.resolution).max() { state.bake.settings.size = r.clamp(256, 4096); }
            let texture_sets = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
            state.bake.job = Some(state.session.start_bake(subcraft_app::BakeRequest { settings: state.bake.settings.clone(), high_poly: None, texture_sets }));
        }
        state.bake.miss_heatmap = options.bake_misses;
        if let Some([front, rear]) = options.bake_cage { state.bake.settings.cage_front = front; state.bake.settings.cage_rear = rear; }
        state.bake.settings.curvature_from_normal |= options.curvature_from_normal;
        if options.id_colors { cc.egui_ctx.data_mut(|d| d.insert_temp(egui::Id::new("capture_id_colors"), true)); }
        if let Some(high) = &options.bake_high_poly {
            state.bake.open = true;
            state.bake.high_poly = high.display().to_string();
            if let Some(size) = options.bake_size { state.bake.settings.size = size; }
            if let Some(mode) = options.bake_match {
                let mut project = state.session.settings().clone();
                project.bake_match.mode = mode;
                state.session.set_settings(project);
            }
            if let Some(spec) = &options.bake_place {
                let mut project = state.session.settings().clone();
                match crate::placement::parse_place_spec(spec, &state.session.scene().mesh, high) {
                    Ok(p) => project.bake_placement = p,
                    Err(e) => log::error!("--bake-place: {e}"),
                }
                state.session.set_settings(project);
            }
            state.bake.settings.name_match = state.session.settings().bake_match.clone();
            state.bake.settings.placement = state.session.settings().bake_placement.clone();
            let texture_sets = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
            if !options.bake_preview_only { state.bake.job = Some(state.session.start_bake(subcraft_app::BakeRequest {
                settings: state.bake.settings.clone(), high_poly: Some(high.clone()), texture_sets,
            })); }
            if let Some(kind) = options.bake_preview_map { state.bake.displayed_map = Some(kind); state.bake.follow_maps = false; }
        }
        if options.mesh_map.is_some() { state.recovery.prefs.viewport.mesh_map = options.mesh_map; }
        {
            // Background release check; a newer version gets a callout at the update button.
            // Not for scripted screenshots or right after an update restart.
            let automatic = state.recovery.prefs.check_updates_on_start && options.screenshot.is_none() && options.resume_update.is_none();
            if (options.check_updates || automatic) && let Some(updater) = &mut state.updater {
                updater.check();
                // The automatic check is silent: no window, only the callout if there's an update.
                updater.open = options.check_updates;
                state.update_notice = true;
            }
        }
        {
            state.welcome.recent = cc.storage.and_then(|s| s.get_string(crate::welcome::RECENTS_KEY))
                .and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
            state.welcome.refresh();
            if opened_from_arg && let Some(path) = &options.mesh { state.welcome.remember(path); }
        }
        if options.recover && !state.recovery.crashed.is_empty() {
            let c = state.recovery.crashed.remove(0);
            crate::recovery::recover(&mut state, c, 0.0);
        }
        // `--profile-edits` profiles the demo unless a project was given (`SUBCRAFT_PROFILE_GENERATOR`
        // then picks which generator's Level is dragged).
        if options.demo || (options.profile_edits.is_some() && options.mesh.is_none()) {
            build_demo(&mut state);
        }
        if let Some(name) = &options.select_layer {
            let mut found = None;
            state.session.selected_texture_set().walk(|l, _| if l.name == *name { found = Some(l.id); });
            if let Some(id) = found { let _ = state.session.execute(Command::SelectLayer(Some(id))); }
        }
        if let Some(resolution) = options.profile_edits.as_ref().and_then(|p| p.1) {
            let _ = state.session.execute(Command::SetResolution { texture_set: None, resolution });
        }
        state.welcome.open = options.shows_welcome(state.recovery.prefs.show_welcome, state.session.is_recovered());
        if let Some(view) = options.camera {
            state.camera_view = Some(view);
            state.camera = crate::orbit::OrbitCamera::framing_with(&state.session.scene().mesh.bounds, Some(&view));
        }
        if let Some(symmetry) = options.symmetry_axes {
            let stroke = subcraft_paint::StrokeOptions { symmetry, ..state.session.brush.options };
            let _ = state.session.execute(Command::SetStrokeOptions(stroke));
        }
        if let Some((axis, count)) = options.radial_symmetry {
            let radial = subcraft_paint::RadialSymmetry { enabled: true, axis, count, ..state.session.brush.options.radial };
            let stroke = subcraft_paint::StrokeOptions { radial, ..state.session.brush.options };
            let _ = state.session.execute(Command::SetStrokeOptions(stroke));
        }
        if options.effect_panel && let Some(layer) = state.session.selected_layer() {
            let target = layer.content_effects.last().map(|e| (layer.id, subcraft_app::LayerTarget::Content, e.id))
                .or_else(|| layer.mask.as_ref().and_then(|m| m.effects.last()).map(|e| (layer.id, subcraft_app::LayerTarget::Mask, e.id)));
            if let Some((layer, target, effect)) = target { let _ = state.session.execute(Command::SelectEffect { layer, target, effect: Some(effect) }); }
        }
        if options.generator_panel && let Some(layer) = state.session.selected_layer() {
            let target = layer.mask.as_ref().and_then(|m| m.effects.iter().find(|e| matches!(e.kind, subcraft_doc::MaskEffectKind::Generator(_)))).map(|e| (layer.id, e.id));
            if let Some((layer, effect)) = target { let _ = state.session.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Mask, effect: Some(effect) }); }
        }
        state.capture_select_brush = options.select_brush.clone();
        state.capture_tool = options.capture_tool.clone();
        if let Some(samples) = options.path_trace {
            state.recovery.prefs.viewport.path_tracing = true;
            state.pt_capture_samples = Some(samples);
        }
        if let Some(spec) = options.quality.as_deref() {
            let prefs = &mut state.recovery.prefs.viewport;
            for part in spec.split(',') {
                let Some((key, value)) = part.split_once('=') else { continue };
                // Finite only: `inf` / `nan` parse, and a NaN look setting kept the project "modified"
                // forever and was saved as `null`.
                let number = value.parse::<f32>().ok().filter(|v| v.is_finite());
                match (key.trim(), number) {
                    ("bloom", Some(v)) => prefs.bloom = v,
                    ("threshold", Some(v)) => prefs.bloom_threshold = v,
                    ("ao", Some(v)) => prefs.ssao = v,
                    ("ao-radius", Some(v)) => prefs.ssao_radius = v,
                    ("exposure", Some(v)) => prefs.exposure = v,
                    ("glow", Some(v)) => prefs.emission_glow = v,
                    ("glow-radius", Some(v)) => prefs.glow_radius = v,
                    ("dof", Some(v)) => prefs.dof = v > 0.0,
                    ("dof-amount", Some(v)) => { prefs.dof = true; prefs.dof_amount = v }
                    ("dof-focus", Some(v)) => prefs.dof_focus = v,
                    ("dof-range", Some(v)) => prefs.dof_range = v,
                    ("glare", Some(v)) => prefs.glare = v,
                    ("glare-length", Some(v)) => prefs.glare_length = v,
                    ("glare-threshold", Some(v)) => prefs.glare_threshold = v,
                    ("glare-remap", Some(v)) => prefs.glare_remap = v,
                    ("post", Some(v)) => prefs.post_effects = v > 0.0,
                    ("falloff", Some(v)) => prefs.bloom_falloff = v,
                    ("gamma", Some(v)) => prefs.gamma = v,
                    ("factor", Some(v)) => prefs.mapping_factor = v,
                    ("flare", Some(v)) => { prefs.flare_on = v > 0.0; prefs.flare_amount = v }
                    ("aberration", Some(v)) => { prefs.aberration_on = v > 0.0; prefs.aberration_amount = v }
                    ("vignette", Some(v)) => { prefs.vignette_on = v > 0.0; prefs.vignette_amount = v }
                    ("sharpen", Some(v)) => { prefs.sharpen_on = v > 0.0; prefs.sharpen_amount = v }
                    ("grain", Some(v)) => { prefs.grain_on = v > 0.0; prefs.grain_amount = v }
                    ("grain-colored", Some(v)) => prefs.grain_type = if v > 0.0 { subcraft_doc::GrainType::Colored } else { subcraft_doc::GrainType::Monochromatic },
                    ("saturation", Some(v)) => { prefs.color_correction_on = true; prefs.cc_saturation = v }
                    ("contrast", Some(v)) => { prefs.color_correction_on = true; prefs.cc_contrast = v }
                    ("brightness", Some(v)) => { prefs.color_correction_on = true; prefs.cc_brightness = v }
                    ("sepia", Some(v)) => { prefs.color_correction_on = true; prefs.cc_sepia = v }
                    ("temperature", Some(v)) => { prefs.color_correction_on = true; prefs.cc_temperature = v }
                    ("env-opacity", Some(v)) => prefs.env_opacity = v,
                    ("env-blur", Some(v)) => prefs.env_blur = v,
                    ("env-ev", Some(v)) => prefs.env_exposure = v,
                    ("fov", Some(v)) => crate::panels::display_settings::set_fov(&mut state.camera, v),
                    ("glare-mode", _) => {
                        let wanted = value.trim().to_ascii_lowercase();
                        if let Some(m) = subcraft_render::GlareMode::ALL.into_iter().find(|m| format!("{m:?}").to_ascii_lowercase() == wanted || m.label().to_ascii_lowercase().replace(' ', "-").contains(&wanted.replace(' ', "-"))) { prefs.glare_mode = m; }
                    }
                    ("tone", _) => {
                        let wanted = value.trim().to_ascii_lowercase();
                        if let Some(t) = subcraft_render::ToneMap::ALL.into_iter().find(|t| t.label().to_ascii_lowercase().starts_with(&wanted)) { prefs.tone_map = t; }
                    }
                    _ => {}
                }
            }
        }
        state.pending_render_job = options.render_job.clone();
        if let Some(model) = &options.reload_mesh {
            state.mesh_reload.path = model.display().to_string();
            state.mesh_reload.auto = options.reload_mode;
            crate::mesh_reload::open(&mut state);
            state.welcome.open = false;
        }
        if options.auto_uv {
            state.auto_uv.auto = true;
            crate::auto_uv::open(&mut state);
            state.welcome.open = false;
        }
        if let Some(model) = &options.new_project_model {
            state.new_project.show_window(&state.recovery.prefs.new_project.clone());
            state.new_project.load_model(model);
            if let Some(high) = &options.new_project_high { state.new_project.load_high_poly(high); }
            if !options.mesh_map_files.is_empty() { state.new_project.add_mesh_maps(&options.mesh_map_files); }
            state.welcome.open = false;
        } else if !options.mesh_map_files.is_empty() {
            let sets = state.session.mesh_map_set_keys();
            state.mesh_map_import.start(&options.mesh_map_files, &sets);
            state.welcome.open = false;
        }
        state.export.folder = options.export_folder(&state.export.folder);
        // `--export-path` is only the default: a project's saved folder still wins (#322).
        state.export.default_folder = state.export.folder.clone();
        state.export.open = options.export_window || options.export_template.is_some() || options.export_preset.is_some();
        if let Some(i) = options.export_template { let all = subcraft_export::ExportTemplate::builtin(); state.export.preset = all[i.min(all.len() - 1)].name.clone(); }
        if let Some(name) = &options.export_preset {
            let presets = state.export.presets(state.user_library.as_ref());
            let found = presets.all().find(|p| p.name == *name || p.key() == name).map(|p| p.key().to_string());
            if let Some(key) = found { state.export.preset = key; }
            state.export.tab = crate::dialogs::ExportTab::OutputTemplates;
        }
        state.render_window = options.render_window;
        if !options.import_smart.is_empty() {
            crate::files::import_smart_materials(&mut state, &options.import_smart, 0.0);
        }
        if let Some(tab) = options.shelf_tab.as_deref() {
            use crate::panels::shelf::ShelfTab;
            state.shelf.tab = match tab {
                "normal-maps" => ShelfTab::NormalMaps,
                "materials" => ShelfTab::Materials,
                "smart-materials" => ShelfTab::SmartMaterials,
                "smart-masks" => ShelfTab::SmartMasks,
                "filters" => ShelfTab::Filters,
                "brushes" => ShelfTab::Brushes,
                "alphas" => ShelfTab::Alphas,
                "environments" => ShelfTab::Environments,
                _ => ShelfTab::Textures,
            };
        }
        state.welcome.guide = options.guide_step.map(|s| s.min(subcraft_app::launch::QUICK_START.len() - 1));
        if state.welcome.guide.is_some() { state.welcome.open = false; }

        #[cfg(feature = "update-fixture")]
        if options.auto_update_fixture && let Some(updater) = &mut state.updater { updater.request_update(); }
        let dock = cc
            .storage
            .and_then(|s| s.get_string(DOCK_KEY))
            .and_then(|j| serde_json::from_str(&j).ok())
            // Floating panels come back docked: a window could be off screen after a monitor change.
            .map(|mut d: DockState<Tab>| { crate::docks::settle_floating(&mut d); d })
            .filter(|d: &DockState<Tab>| options.screenshot.is_none() && !options.reset_layout && {
                let ok = layout_is_valid(d);
                if !ok { log::warn!("Saved panel layout is invalid; using the default layout"); }
                ok
            })
            .unwrap_or_else(default_layout);
        let mut dock = dock;
        migrate_layout(&mut dock);
        if options.properties_panel && let Some(path) = dock.find_tab(&Tab::Layers) { let _ = dock.remove_tab(path); }
        if options.texture_set_settings && let Some(path) = dock.find_tab(&Tab::TextureSetSettings) { let _ = dock.set_active_tab(path); }
        if options.shelf_panel {
            dock = default_layout();
            state.shelf.tab = crate::panels::shelf::ShelfTab::Textures;
            if let Some(path) = dock.find_tab(&Tab::Shelf) { let _ = dock.set_active_tab(path); }
        }
        if let Some(name) = options.environment.as_deref() {
            match state.library.environments.iter().position(|e| e.name == name) {
                Some(i) => state.env.index = i,
                None => log::warn!("no environment named {name}"),
            }
        }
        if options.view_2d
            && let Some(path) = dock.find_tab(&Tab::UvView)
        {
            let _ = dock.set_active_tab(path);
        }
        if let Some(group) = options.display_settings.as_deref() {
            open_display_settings(&mut dock);
            let wanted = group.trim().to_ascii_lowercase();
            state.display_jump = panels::display_settings::Section::ALL.into_iter().find(|s| !wanted.is_empty() && s.short().to_ascii_lowercase().starts_with(&wanted));
        } else if options.brush_panel && let Some(path) = dock.find_tab(&Tab::Brush) {
            let _ = dock.set_active_tab(path);
        } else if state.session.selected_layer().is_some_and(|layer| layer.kind() != subcraft_doc::LayerKind::Paint)
            && let Some(path) = dock.find_tab(&Tab::Properties)
        {
            let _ = dock.set_active_tab(path);
        }
        let mut state = state;
        if options.screenshot.is_none() {
            state.intro = Some(crate::intro::Intro::default());
        }
        Self {
            inspector_selection: Some(InspectorSelection::current(&state.session)),
            state,
            dock,
            frames: 0,
            screenshot: options.screenshot,
            screenshot_requested: false,
            pending_now: None,
            active_tabs: HashSet::new(),
            test_crash: options.test_crash, popped_dock: None,
            profile: options.profile_edits.map(|(out, res)| crate::edit_profile::EditProfile::new(out, res)),
            view_profile: options.profile_view.map(crate::view_profile::ViewProfile::new),
        }
    }

    fn follow_layer_selection(&mut self) -> bool {
        let selection = InspectorSelection::current(&self.state.session);
        if self.inspector_selection.replace(selection) == Some(selection) {
            return false;
        }
        // Paint layers and content Paint effects open the brush; everything else its Properties.
        let paint_content = selection.1 == Some(subcraft_doc::LayerKind::Paint) && selection.2 == subcraft_app::LayerTarget::Content && selection.3.is_none();
        // A pinned Brush view (View menu, tool click) lasts until the selection changes again.
        self.state.brush_pinned = false;
        let tab = if (paint_content || selection.4) && self.dock.find_tab(&Tab::Brush).is_some() { Tab::Brush } else { Tab::Properties };
        self.dock.find_tab(&tab).is_some_and(|path| self.dock.set_active_tab(path).is_ok())
    }

    /// A Shelf double-click asks for the Brush settings: bring that tab forward and focus it.
    fn open_requested_brush_editor(&mut self) {
        if !std::mem::take(&mut self.state.open_brush_editor) {
            return;
        }
        // Legacy layouts have their own Brush tab; otherwise Properties switches to the brush settings.
        let tab = if self.dock.find_tab(&Tab::Brush).is_some() { Tab::Brush } else { self.state.brush_pinned = true; Tab::Properties };
        if let Some(path) = self.dock.find_tab(&tab) {
            let _ = self.dock.set_active_tab(path);
            self.dock.set_focused_node_and_surface(path.node_path());
        }
    }

    /// Resolve selection before drawing (shortcuts/imports) and after drawing (layer clicks).
    fn workspace(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.state.properties_hosts_brush = self.dock.find_tab(&Tab::Brush).is_none();
        self.follow_layer_selection();
        self.open_requested_brush_editor();
        sync_sub_libraries(&mut self.dock, &mut self.state.shelf);
        let style = theme::dock_style(ui.style());
        // Tab and Baking mode show the views alone (Baking mode adds its own panels around them).
        let hidden = self.state.hide_ui || self.state.bake.open;
        let mut viewer = Viewer { state: &mut self.state, was_active: &self.active_tabs, active: HashSet::new() };
        // Tab hides the panels: only the views are laid out, in the arrangement of the full layout.
        let mut views = hidden.then(|| views_only(&self.dock));
        egui::Frame::new().fill(theme::BG_0()).inner_margin(egui::Margin { left: 3, right: 0, top: 3, bottom: 0 }).show(ui, |ui| {
            let dock = views.as_mut().unwrap_or(&mut self.dock);
            DockArea::new(dock).style(style).show_close_buttons(true).show_leaf_close_all_buttons(false).show_leaf_collapse_buttons(false).show_inside(ui, &mut viewer);
        });
        // A view tab clicked while the panels are hidden is the front one when they come back.
        if let Some(views) = &views && !is_side_by_side(&self.dock) {
            front_tab(&mut self.dock, if is_front(views, Tab::UvView) { Tab::UvView } else { Tab::Viewport });
        }
        let active = std::mem::take(&mut viewer.active);
        if active != self.active_tabs {
            self.active_tabs = active;
            ctx.request_repaint();
        }
        if self.follow_layer_selection() {
            ctx.request_repaint();
        }
        // The panels' mask requests (Alt+click on a mask thumbnail, the mask menu).
        crate::mask_view::handle_requests(&mut self.state, &ctx, ctx.input(|i| i.time));
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.state.recovery.capture.is_some() {
            return; // Settings > Shortcuts takes the next key (and the Tab press) itself.
        }
        if ctx.egui_wants_keyboard_input() || self.state.modal_open() {
            self.state.tab_pressed = false;
            return;
        }
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => map_key(*key).map(|k| Shortcut {
                        ctrl: modifiers.command,
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        key: k,
                    }),
                    _ => None,
                })
                .collect()
        });
        let keymap = self.state.recovery.prefs.keys();
        let mut pressed = pressed;
        if std::mem::take(&mut self.state.tab_pressed) {
            pressed.push(Shortcut::key(Key::Tab));
        }
        for s in pressed {
            if let Some(a) = keymap.action_for(s) {
                self.perform(a, ctx.input(|i| i.time));
            }
        }
    }

    /// The 2D view shows what the 3D view was just switched to (C / Shift+C / M / B / Shift+B
    /// act on both views, as in Painter).
    fn sync_uv_source(&mut self) {
        let prefs = &self.state.recovery.prefs.viewport;
        self.state.uv.source = crate::panels::uv_view::source_for(prefs.mesh_map, prefs.shown_channel());
    }

    fn perform(&mut self, action: Action, now: f64) {
        if self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
            if action == Action::Undo { if self.state.asset_preview.is_applying() {self.state.asset_preview.cancel_apply();} else {self.state.pen.undo_pending(&self.state.session);} return; }
            if matches!(action, Action::Save | Action::SaveAs | Action::Export) {
                if let Err(e) = self.state.pen.release(&self.state.session) { self.state.set_status(e.to_string(), now); }
                if self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
                    self.state.pen_after = Some(crate::pen::After::Action(action)); return;
                }
            }
        }
        // With an effect selected, Delete and Ctrl+D act on the selected effect(s) (Painter).
        if matches!(action, Action::DeleteLayer | Action::DuplicateLayer)
            && crate::panels::effects::selection_action(&mut self.state, action == Action::DeleteLayer, now)
        {
            return;
        }
        // Delete / Duplicate act on every selected layer when several are selected.
        if self.state.multi.is_multi() && matches!(action, Action::DeleteLayer | Action::DuplicateLayer | Action::NewFolder) {
            let ids = self.state.multi.ids.clone();
            let cmd = match action {
                Action::DeleteLayer => subcraft_app::Command::DeleteLayers(ids),
                Action::DuplicateLayer => subcraft_app::Command::DuplicateLayers(ids),
                // Ctrl+G groups the whole selection into one folder (Painter).
                _ => subcraft_app::Command::GroupLayers(ids),
            };
            self.state.run(vec![cmd], now);
            if action == Action::NewFolder { self.state.multi.clear(); }
            return;
        }
        match action {
            Action::SelectAllLayers => {
                let mut rows = Vec::new();
                self.state.session.selected_texture_set().walk(|l, _| rows.push(l.id));
                rows.reverse();
                self.state.multi.select_all(&rows);
            }
            Action::ResetCamera => {
                // Painter's F: frame the view under the cursor, or both when it is over neither.
                let (three, two) = frame_targets(self.state.hovered_3d, self.state.hovered_2d);
                // While editing a path, F focuses on it instead (Painter 11.0).
                let path = if self.state.tool == Tool::Pen { path_bounds(&self.state) } else { None };
                if three {
                    match path.and_then(|(_, world)| world) {
                        Some(world) => self.state.camera.focus_on(&world),
                        None => self.state.camera = OrbitCamera::framing(&self.state.session.scene().mesh.bounds),
                    }
                    self.state.orbit_pivot = None;
                }
                if two {
                    match path {
                        Some((uv_box, _)) => self.state.uv.frame = Some(uv_box),
                        None => {
                            self.state.uv.zoom = 1.0;
                            self.state.uv.rotation = 0.0;
                            self.state.uv.offset = egui::Vec2::ZERO;
                        }
                    }
                }
            }
            Action::HideUi => self.state.hide_ui = !self.state.hide_ui,
            Action::PaintingMode => {
                self.state.bake.open = false;
                self.state.recovery.prefs.viewport.path_tracing = false;
            }
            Action::CloseProject => self.request(Pending::NewSample, now),
            Action::View3dAnd2d => apply_view_layout(&mut self.dock, ViewLayout::Both),
            Action::View3dOnly => apply_view_layout(&mut self.dock, ViewLayout::Only3d),
            Action::View2dOnly => apply_view_layout(&mut self.dock, ViewLayout::Only2d),
            Action::SwapViews => apply_view_layout(&mut self.dock, ViewLayout::Swap),
            Action::NextMeshMap | Action::PrevMeshMap => {
                let prefs = &mut self.state.recovery.prefs.viewport;
                if action == Action::NextMeshMap { crate::viewport_bar::cycle_mesh_map(prefs) } else { crate::viewport_bar::cycle_mesh_map_back(prefs) }
                self.sync_uv_source();
            }
            Action::InvertMaskGray => {
                match self.state.session.perform(action) {
                    Ok(_) => {
                        let value = self.state.session.brush.material.mask_value;
                        self.state.set_status(format!("Mask brush value: {value:.2} ({})", if value >= 0.5 { "white: reveals" } else { "black: hides" }), now);
                    }
                    Err(e) => self.state.set_status(e.to_string(), now),
                }
            }
            Action::IsolateTextureSet => {
                match self.state.session.perform(action) {
                    Ok(_) => {
                        let session = &self.state.session;
                        let isolated = session.document().texture_sets.iter().any(|t| session.is_set_hidden(t.id));
                        let name = session.selected_texture_set().name.clone();
                        self.state.set_status(if isolated { format!("Showing only texture set {name} (Alt+Q shows all)") } else { "Showing every texture set".to_string() }, now);
                    }
                    Err(e) => self.state.set_status(e.to_string(), now),
                }
            }
            Action::BrushTool => self.state.tool = Tool::Brush,
            Action::EraserTool => self.state.tool = Tool::Eraser,
            Action::PolygonFillTool => self.state.tool = Tool::PolygonFill,
            Action::SmudgeTool => self.state.tool = Tool::Smudge,
            Action::ProjectionTool => self.state.tool = Tool::Projection,
            Action::CloneTool => self.state.tool = Tool::Clone,
            Action::EyedropperTool => self.state.tool = Tool::Eyedropper,
            Action::NextCamera => crate::view_extras::cycle_camera(&mut self.state, 1),
            Action::PrevCamera => crate::view_extras::cycle_camera(&mut self.state, -1),
            Action::PerspectiveCamera => self.state.recovery.prefs.viewport.orthographic = false,
            Action::OrthographicCamera => self.state.recovery.prefs.viewport.orthographic = true,
            Action::NextChannel | Action::PrevChannel => {
                // Painter's C / Shift+C: the material, then each channel of the selected texture set.
                let channels: Vec<_> = self.state.session.selected_texture_set().channel_kinds().collect();
                self.state.recovery.prefs.viewport.cycle_channel(&channels, action == Action::NextChannel);
                self.sync_uv_source();
            }
            Action::DisplayMaterial => {
                self.state.recovery.prefs.viewport.show_material();
                self.sync_uv_source();
            }
            Action::NewProject => self.request(Pending::OpenMesh, now),
            Action::Quit => self.request(Pending::Close, now),
            Action::PenTool => { self.state.tool = Tool::Pen; self.state.picking_parts = false; }
            Action::NewTextLayer => self.state.text.open_new(),
            Action::Save => {
                dialogs::save(&mut self.state, false, now);
            }
            Action::SaveAs => {
                dialogs::save(&mut self.state, true, now);
            }
            Action::OpenProject => self.request(Pending::OpenProject, now),
            Action::Bake => self.state.bake.open = true,
            Action::ReimportMesh => crate::mesh_reload::open_reimport(&mut self.state),
            Action::TogglePathTracing => {
                let prefs = &mut self.state.recovery.prefs.viewport;
                prefs.path_tracing = !prefs.path_tracing;
            }
            Action::Export => {
                self.state.export.open = true;
                self.state.export.view_2d_only = false;
                self.state.export.settings.height_to_normal = self.state.height_scale;
            }
            Action::HideExcludedGeometry => {
                let session = &mut self.state.session;
                session.toggle_hide_excluded();
                let msg = match (session.hide_excluded(), session.has_excluded_geometry()) {
                    (true, true) => "Hiding the geometry this layer's geometry mask leaves out (Alt+H shows it again)",
                    (true, false) => "Excluded geometry will be hidden once the layer has a geometry mask (Alt+H turns this off)",
                    (false, _) => "Showing all geometry",
                };
                self.state.set_status(msg, now);
            }
            Action::QuickMaskToggle => {
                self.state.session.toggle_quick_mask();
                let msg = match self.state.session.quick_mask() {
                    Some(q) if q.editing => "Quick mask: paint where strokes may land (U applies it, Y clears it)",
                    Some(_) => "Quick mask applied: strokes land only on its painted areas (U edits, I inverts, Y clears)",
                    None => "Quick mask removed (nothing was painted)",
                };
                self.state.set_status(msg, now);
            }
            Action::QuickMaskClear => {
                if self.state.session.quick_mask().is_some() {
                    self.state.session.clear_quick_mask();
                    self.state.set_status("Quick mask cleared: strokes paint everywhere again", now);
                }
            }
            // Without a quick mask, I belongs to the 3D view (isolate the part under the cursor).
            Action::QuickMaskInvert => {
                if self.state.session.invert_quick_mask() {
                    self.state.set_status("Quick mask inverted", now);
                }
            }
            _ => {
                if let Err(e) = self.state.session.perform(action) {
                    self.state.set_status(e.to_string(), now);
                }
            }
        }
    }

    /// Top bar: brand, menus, project name, then undo/redo and the main actions on the right.
    fn header_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let km = &self.state.recovery.prefs.keys();
        let bar = ui.max_rect();
        let frameless = crate::window_frame::frameless();
        // Empty parts of the header move the window (registered first so widgets stay on top).
        if frameless {
            crate::window_frame::title_bar_background(ui, bar);
        }
        // Right side first so the left side can't push it out of the bar.
        let right_edge = ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if frameless {
                // Minimize / maximize / close flush with the window's right edge, beside Export.
                let gap = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 0.0;
                crate::window_frame::caption_buttons(ui);
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_space(8.0);
            }
            if primary_button(ui, format!("{}  Export", icons::EXPORT)).on_hover_text(tip(km, "Export textures", Action::Export)).clicked() {
                self.perform(Action::Export, now);
            }
            if secondary_button(ui, format!("{}  Bake", icons::MAGIC_WAND)).on_hover_text(tip(km, "Bake mesh maps", Action::Bake)).clicked() {
                self.perform(Action::Bake, now);
            }
            // Painter's Docks toolbar: a button per closed panel, in headers wide enough to keep
            // the project name (narrower windows reopen panels from Window > Views).
            if bar.width() >= crate::docks::TOOLBAR_MIN_WIDTH {
                ui.add_space(6.0);
                crate::docks::toolbar(ui, &mut self.dock, &mut self.popped_dock);
            }
            crate::updates::button(&mut self.state, ui);
            if icon_button(ui, icons::SAVE, &tip(km, "Save", Action::Save)).clicked() {
                self.perform(Action::Save, now);
            }
            ui.add_space(6.0);
            let h = self.state.session.history();
            let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
            if ui.add_enabled_ui(can_redo, |ui| icon_button(ui, icons::REDO, &tip(km, "Redo", Action::Redo))).inner.clicked() {
                self.perform(Action::Redo, now);
            }
            if ui.add_enabled_ui(can_undo, |ui| icon_button(ui, icons::UNDO, &tip(km, "Undo", Action::Undo))).inner.clicked() {
                self.perform(Action::Undo, now);
            }
            ui.cursor().right()
        }).inner;
        let left_edge = ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            // Brand mark.
            // The only logo in the interface: the small mark beside the name.
            let (r, _) = ui.allocate_exact_size(egui::vec2(BRAND_MARK_SIZE, BRAND_MARK_SIZE), egui::Sense::hover());
            let mark = theme::brand_mark(ui.ctx());
            ui.painter().image(mark.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            ui.label(RichText::new(tr("SubCraft")).font(theme::font_strong(14.0)).color(theme::TEXT()));
            ui.add_space(10.0);
            self.menu(ui)
        }).inner;
        // Keep long project names out of the menus/actions.
        let title_rect = egui::Rect::from_min_max(egui::pos2(left_edge + 8.0, bar.top()), egui::pos2(right_edge - 8.0, bar.bottom()));
        if title_rect.width() >= 80.0 {
            let title = dialogs::title(&self.state);
            let name = title.trim_start_matches("SubCraft — ");
            let center = egui::pos2(bar.center().x.clamp(title_rect.left(), title_rect.right()), bar.center().y);
            ui.painter().with_clip_rect(title_rect.intersect(ui.clip_rect())).text(center, egui::Align2::CENTER_CENTER, name, theme::font(12.5), theme::TEXT_FAINT());
        }
    }

    /// Vertical tool strip left of the dock area.
    fn tool_rail(&mut self, ui: &mut Ui) {
        let km = &self.state.recovery.prefs.keys();
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            let tool = &mut self.state.tool;
            for (t, icon, tip) in [
                (Tool::Brush, icons::PAINT_BRUSH, tip(km, "Brush", Action::BrushTool)),
                (Tool::Eraser, icons::ERASER, tip(km, "Eraser", Action::EraserTool)),
                (Tool::PolygonFill, icons::PAINT_BUCKET, tip(km, "Polygon fill", Action::PolygonFillTool)),
                (Tool::Clone, icons::COPY, crate::keys::native_string(format!("{}: Ctrl+click or hold V and click the model to set the source, then paint to copy it", tip(km, "Clone", Action::CloneTool)))),
                (Tool::Smudge, icons::DROP, tip(km, "Smudge: drag the paint under the brush along the stroke", Action::SmudgeTool)),
                (Tool::Projection, icons::IMAGE, tip(km, "Projection: paint the brush material cast from the view and tiled over it", Action::ProjectionTool)),
                (Tool::Eyedropper, icons::CURSOR, tip(km, "Eyedropper (Shift+click picks material)", Action::EyedropperTool)),
                (Tool::Pen, icons::CURVE, tip(km, "Pen paths: click points, drag for curves; edit anchors and handles in 2D View", Action::PenTool)),
            ] {
                if tool_button(ui, icon, *tool == t, &tip).clicked() {
                    *tool = t;
                    if t == Tool::Pen { self.state.picking_parts = false; }
                }
            }
            ui.add_space(6.0);
            rail_divider(ui);
            ui.add_space(6.0);
            let target = &mut self.state.session.brush.target;
            let on_mask = target.is_mask();
            if ui.add_enabled_ui(*tool != Tool::Pen, |ui| tool_button(ui, icons::MASK, on_mask, "Paint the layer's mask instead of its content (Brush tool)")).inner.clicked() {
                *target = if on_mask { subcraft_paint::PaintTarget::Content } else { subcraft_paint::PaintTarget::MaskBase };
            }
            // Painter's quick mask: a temporary selection painted with the usual tools.
            let quick = self.state.session.quick_mask().map(|q| q.editing);
            let quick_tip = match quick {
                Some(true) => tip(km, "Quick mask: editing (strokes paint the mask; click or press the key to apply it)", Action::QuickMaskToggle),
                Some(false) => tip(km, "Quick mask: on, strokes land only on its painted areas (click to edit it; I inverts, Y clears)", Action::QuickMaskToggle),
                None => tip(km, "Quick mask: paint a temporary selection, then paint only inside it", Action::QuickMaskToggle),
            };
            if tool_button(ui, icons::SELECTION, quick.is_some(), &quick_tip).clicked() {
                self.perform(Action::QuickMaskToggle, ui.input(|i| i.time));
            }
            ui.add_space(6.0);
            rail_divider(ui);
            ui.add_space(6.0);
            // The 2D view beside the 3D view, or back to the 3D view alone (Painter's F1 / F2).
            let both = is_side_by_side(&self.dock);
            let view_tip = if both { tip(km, "Hide the 2D view (3D view only)", Action::View3dOnly) } else { tip(km, "Show the 2D view (UV space) beside the 3D view", Action::View3dAnd2d) };
            if tool_button(ui, icons::GRID, both, &view_tip).clicked() {
                self.perform(if both { Action::View3dOnly } else { Action::View3dAnd2d }, ui.input(|i| i.time));
            }
            if tool_button(ui, icons::CAMERA, false, &tip(km, "Reset camera", Action::ResetCamera)).clicked() {
                self.perform(Action::ResetCamera, ui.input(|i| i.time));
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if tool_button(ui, icons::KEYBOARD, self.state.show_shortcuts, "Keyboard shortcuts").clicked() {
                    self.state.show_shortcuts = !self.state.show_shortcuts;
                }
            });
        });
    }

    fn menu(&mut self, ui: &mut Ui) -> f32 {
        let now = ui.input(|i| i.time);
        let km = &self.state.recovery.prefs.keys();
        egui::MenuBar::new().ui(ui, |ui| {
            let item = |ui: &mut Ui, icon: &str, label: &str, sc: &str| ui.add(egui::Button::new(format!("{icon}   {label}")).shortcut_text(sc)).clicked();
            // Same top menus as Painter: File, Edit, Mode, Window, Viewport, Plugins, Help.
            ui.menu_button(tr("File"), |ui| {
                ui.set_min_width(240.0);
                if item(ui, icons::CUBE, tr("New…"), &sc(km, Action::NewProject)) {
                    ui.close();
                    self.request(Pending::OpenMesh, now);
                }
                if item(ui, icons::FOLDER_OPEN, tr("Open…"), &sc(km, Action::OpenProject)) {
                    ui.close();
                    self.request(Pending::OpenProject, now);
                }
                if item(ui, icons::DOWNLOAD, tr("Import Substance Painter project (reconstruction)…"), "") {
                    ui.close();
                    self.request(Pending::ImportPainter, now);
                }
                ui.menu_button(format!("{}   {}", icons::HISTORY, tr("Recent files")), |ui| {
                    ui.set_min_width(240.0);
                    let mut open = None;
                    if self.state.welcome.items.is_empty() {
                        ui.weak(tr("Projects and models you open or save appear here."));
                    }
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for recent in &self.state.welcome.items {
                            let label = if recent.available { recent.name.clone() } else { format!("{} (missing)", recent.name) };
                            if ui.add_enabled(recent.available, egui::Button::new(label)).on_hover_text(recent.path.display().to_string()).clicked() {
                                open = Some(recent.path.clone());
                            }
                        }
                    });
                    if let Some(path) = open {
                        ui.close();
                        self.state.welcome.pending_recent = Some(path);
                        self.request(Pending::OpenRecent, now);
                    }
                });
                if item(ui, icons::FILE_PLUS, tr("Open sample"), "") {
                    self.request(Pending::NewSample, now);
                    ui.close();
                }
                if item(ui, icons::X, tr("Close"), &sc(km, Action::CloseProject)) {
                    ui.close();
                    self.perform(Action::CloseProject, now);
                }
                ui.separator();
                if item(ui, icons::SAVE, tr("Save"), &sc(km, Action::Save)) {
                    ui.close();
                    self.perform(Action::Save, now);
                }
                if item(ui, icons::SAVE, tr("Save as…"), &sc(km, Action::SaveAs)) {
                    ui.close();
                    self.perform(Action::SaveAs, now);
                }
                if item(ui, icons::SAVE, tr("Save as copy…"), "") {
                    ui.close();
                    dialogs::save_copy(&mut self.state, now);
                }
                ui.separator();
                ui.menu_button(format!("{}   {}", icons::IMAGE, tr("Import resources")), |ui| {
                    ui.set_min_width(220.0);
                    if item(ui, icons::PLUS, tr("Import resources…"), "") {
                        ui.close();
                        self.state.import.open = true;
                    }
                    if item(ui, icons::IMAGE, tr("Import texture set…"), "") {
                        ui.close();
                        crate::panels::layers::import_texture_set_dialog(ui.ctx());
                    }
                    if item(ui, icons::SPARKLE, tr("Import smart material(s)…"), "") {
                        ui.close();
                        crate::panels::shelf::import_smart_dialog(ui.ctx());
                    }
                    if item(ui, icons::FOLDER_OPEN, tr("Import smart material folder…"), "") {
                        ui.close();
                        crate::panels::shelf::import_smart_folder_dialog(ui.ctx());
                    }
                });
                if item(ui, icons::EXPORT, tr("Export textures…"), &sc(km, Action::Export)) {
                    ui.close();
                    self.perform(Action::Export, now);
                }
                if item(ui, icons::EXPORT, tr("Export 2D view…"), "") {
                    ui.close();
                    self.perform(Action::Export, now);
                    self.state.export.view_2d_only = true;
                }
                if item(ui, icons::EXPORT, tr("Export mesh…"), "") {
                    ui.close();
                    let m = &mut self.state.export_mesh;
                    m.open = true;
                    m.folder = self.state.export.folder.clone();
                    m.displacement.scale = self.state.height_scale;
                }
                if item(ui, icons::CAMERA, tr("Render image / turntable…"), "") {
                    ui.close();
                    self.state.render_window = true;
                }
                // Painter's Send to lists other applications; here it is the PhotoCraft round trip.
                ui.menu_button(format!("{}   {}", icons::EXPORT, tr("Send to")), |ui| {
                    ui.set_min_width(220.0);
                    if self.state.photocraft.is_some() {
                        if item(ui, icons::X, tr("Stop editing in PhotoCraft"), "") {
                            self.state.stop_photocraft(now);
                            ui.close();
                        }
                    } else if item(ui, icons::EXPORT, tr("Edit in PhotoCraft…"), "") {
                        self.state.start_photocraft(now);
                        ui.close();
                    }
                });
                ui.separator();
                if item(ui, icons::X, tr("Quit"), &sc(km, Action::Quit)) {
                    self.request(Pending::Close, now);
                    ui.close();
                }
            });
            ui.menu_button(tr("Edit"), |ui| {
                ui.set_min_width(240.0);
                let h = self.state.session.history();
                let undo = h.undo_label().map(|l| format!("Undo {l}")).unwrap_or("Undo".into());
                let redo = h.redo_label().map(|l| format!("Redo {l}")).unwrap_or("Redo".into());
                let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
                if ui.add_enabled(can_undo, egui::Button::new(format!("{}   {undo}", icons::UNDO)).shortcut_text(sc(km, Action::Undo))).clicked() {
                    self.perform(Action::Undo, now);
                }
                if ui.add_enabled(can_redo, egui::Button::new(format!("{}   {redo}", icons::REDO)).shortcut_text(sc(km, Action::Redo))).clicked() {
                    self.perform(Action::Redo, now);
                }
                ui.separator();
                // Painter keeps the layer commands in the layer stack's right-click menu; they are
                // also here so every one of them shows its key.
                ui.menu_button(format!("{}   {}", icons::STACK, tr("Layers")), |ui| {
                    ui.set_min_width(240.0);
                    for (a, icon) in [
                        (Action::NewPaintLayer, icons::PAINT_BRUSH),
                        (Action::NewFillLayer, icons::PAINT_BUCKET),
                        (Action::NewFolder, icons::FOLDER),
                        (Action::NewTextLayer, "Aa"),
                        (Action::DuplicateLayer, icons::COPY),
                        (Action::DeleteLayer, icons::TRASH),
                        (Action::SelectAllLayers, icons::STACK),
                    ] {
                        let info = subcraft_app::actions::info(a);
                        let key = sc(km, a);
                        if item(ui, icon, info.label, &key) {
                            self.perform(a, now);
                            ui.close();
                        }
                    }
                    ui.separator();
                    let have_clip = self.state.clips.layers.is_some();
                    if item(ui, icons::COPY, tr("Copy Layer(s)"), crate::keys::native("Ctrl+C")) {
                        crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: false });
                        ui.close();
                    }
                    if item(ui, icons::COPY, tr("Cut Layer(s)"), crate::keys::native("Ctrl+X")) {
                        crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: true });
                        ui.close();
                    }
                    if have_clip && item(ui, icons::COPY, tr("Paste Layer(s)"), crate::keys::native("Ctrl+V")) {
                        crate::clipboard::request(ui.ctx(), crate::clipboard::Request::PasteLayers);
                        ui.close();
                    }
                });
                ui.separator();
                if item(ui, icons::MAGIC_WAND, tr("Bake mesh maps…"), &sc(km, Action::Bake)) {
                    ui.close();
                    self.perform(Action::Bake, now);
                }
                // Painter: Edit > Project configuration (the mesh, import settings, UV tiles) and
                // Reimport mesh (Ctrl+Shift+R).
                if item(ui, icons::CUBE_WIRE, tr("Project configuration…"), "") {
                    ui.close();
                    crate::mesh_reload::open(&mut self.state);
                }
                if item(ui, icons::CUBE_WIRE, tr("Reimport mesh…"), &sc(km, Action::ReimportMesh)) {
                    ui.close();
                    self.perform(Action::ReimportMesh, now);
                }
                if item(ui, icons::CUBE_WIRE, tr("Auto UV unwrap…"), "") {
                    ui.close();
                    crate::auto_uv::open(&mut self.state);
                }
                ui.separator();
                if item(ui, icons::SLIDERS, tr("Settings…"), "") {
                    self.state.recovery.show_prefs = true;
                    ui.close();
                }
            });
            ui.menu_button(tr("Mode"), |ui| {
                ui.set_min_width(240.0);
                let baking = self.state.bake.open || self.state.bake.job.is_some();
                let rendering = self.state.recovery.prefs.viewport.path_tracing && !baking;
                let mode = |ui: &mut Ui, on: bool, icon: &str, label: &str, key: &str| ui.add(egui::Button::new(format!("{icon}   {label}")).selected(on).shortcut_text(key)).clicked();
                if mode(ui, !baking && !rendering, icons::PAINT_BRUSH, tr("Painting"), &sc(km, Action::PaintingMode)) {
                    self.perform(Action::PaintingMode, now);
                    ui.close();
                }
                if mode(ui, baking, icons::MAGIC_WAND, tr("Bake mesh maps"), &crate::keys::native_string(Shortcut::key(Key::F8).label())) {
                    self.perform(Action::Bake, now);
                    ui.close();
                }
                if mode(ui, rendering, icons::SUN, tr("Rendering (path tracing)"), &sc(km, Action::TogglePathTracing)) {
                    if !rendering {
                        self.state.bake.open = false;
                        self.state.recovery.prefs.viewport.path_tracing = true;
                    }
                    ui.close();
                }
            });
            ui.menu_button(tr("Window"), |ui| {
                ui.set_min_width(240.0);
                ui.menu_button(format!("{}   {}", icons::SQUARES, tr("Views")), |ui| {
                    ui.set_min_width(220.0);
                    for tab in [Tab::Shelf, Tab::Layers, Tab::Properties, Tab::TextureSetList, Tab::TextureSetSettings, Tab::History, Tab::Log, Tab::Parts, Tab::Viewport, Tab::UvView] {
                        if ui.add(egui::Button::new(format!("{}   {}", tab.icon(), tab.title())).selected(is_front(&self.dock, tab))).clicked() {
                            crate::docks::open(&mut self.dock, tab);
                            self.state.hide_ui = false;
                            ui.close();
                        }
                    }
                });
                ui.menu_button(format!("{}   {}", icons::SLIDERS, tr("Toolbars")), |ui| {
                    ui.set_min_width(200.0);
                    ui.checkbox(&mut self.state.recovery.prefs.show_tools_toolbar, tr("Tools"))
                        .on_hover_text("The tool buttons along the left edge of the window.");
                    ui.checkbox(&mut self.state.recovery.prefs.show_status_bar, tr("Status bar"));
                });
                if item(ui, icons::SUN, if self.dock.find_tab(&Tab::DisplaySettings).is_some() { tr("Close Display settings") } else { tr("Display settings") }, "") {
                    toggle_display_settings(&mut self.dock);
                    ui.close();
                }
                if item(ui, icons::SPARKLE, if self.dock.find_tab(&Tab::ShaderSettings).is_some() { tr("Close Shader settings") } else { tr("Shader settings") }, "") {
                    toggle_shader_settings(&mut self.dock);
                    ui.close();
                }
                // Painter 7.4: the 3D or 2D view in its own window (here a floating window).
                let at = ui.ctx().content_rect().center() - crate::docks::UNDOCKED_VIEW_SIZE * 0.5;
                for (tab, undock, dock_back) in [(Tab::Viewport, tr("Undock 3D view"), tr("Dock 3D view")), (Tab::UvView, tr("Undock 2D view"), tr("Dock 2D view"))] {
                    let floating = crate::docks::is_floating(&self.dock, tab);
                    if item(ui, icons::SQUARES, if floating { dock_back } else { undock }, "") {
                        crate::docks::toggle_undock(&mut self.dock, tab, at);
                        ui.close();
                    }
                }
                if item(ui, icons::PAINT_BRUSH, tr("Brush settings"), "") {
                    self.state.open_brush_editor = true;
                    self.state.hide_ui = false;
                    ui.close();
                }
                ui.separator();
                if ui.add(egui::Button::new(format!("{}   {}", icons::SQUARES, tr("Hide UI"))).selected(self.state.hide_ui).shortcut_text(sc(km, Action::HideUi))).clicked() {
                    self.perform(Action::HideUi, now);
                    ui.close();
                }
                if item(ui, icons::SQUARES, tr("Reset UI"), "") {
                    self.dock = default_layout();
                    self.state.hide_ui = false;
                    ui.close();
                }
            });
            ui.menu_button(tr("Viewport"), |ui| {
                ui.set_min_width(260.0);
                // Painter's entries, in Painter's order.
                if item(ui, icons::SQUARES, tr("Display material"), &sc(km, Action::DisplayMaterial)) { self.perform(Action::DisplayMaterial, now); ui.close(); }
                if item(ui, icons::SQUARES, tr("Display next channel"), &sc(km, Action::NextChannel)) { self.perform(Action::NextChannel, now); }
                if item(ui, icons::SQUARES, tr("Display previous channel"), &sc(km, Action::PrevChannel)) { self.perform(Action::PrevChannel, now); }
                if item(ui, icons::SQUARES, tr("Display next mesh map"), &sc(km, Action::NextMeshMap)) { self.perform(Action::NextMeshMap, now); }
                if item(ui, icons::SQUARES, tr("Display previous mesh map"), &sc(km, Action::PrevMeshMap)) { self.perform(Action::PrevMeshMap, now); }
                if item(ui, icons::CAMERA, tr("Show entire mesh"), &sc(km, Action::ResetCamera)) {
                    self.perform(Action::ResetCamera, now);
                    ui.close();
                }
                ui.separator();
                let side = is_side_by_side(&self.dock);
                let view = |ui: &mut Ui, on: bool, label: &str, key: &str| ui.add(egui::Button::new(format!("{}   {label}", icons::SQUARES)).selected(on).shortcut_text(key)).clicked();
                for (a, on, label) in [
                    (Action::View3dAnd2d, side, tr("3D | 2D")),
                    (Action::View3dOnly, !side && is_front(&self.dock, Tab::Viewport), tr("3D only")),
                    (Action::View2dOnly, !side && is_front(&self.dock, Tab::UvView), tr("2D only")),
                    (Action::SwapViews, false, tr("Swap 3D and 2D")),
                ] {
                    if view(ui, on, label, &sc(km, a)) {
                        self.perform(a, now);
                        ui.close();
                    }
                }
                ui.separator();
                let ortho = self.state.recovery.prefs.viewport.orthographic;
                if ui.add(egui::Button::new(format!("{}   {}", icons::CUBE, tr("Perspective"))).selected(!ortho).shortcut_text(sc(km, Action::PerspectiveCamera))).clicked() { self.perform(Action::PerspectiveCamera, now); ui.close(); }
                if ui.add(egui::Button::new(format!("{}   {}", icons::CUBE_WIRE, tr("Orthographic"))).selected(ortho).shortcut_text(sc(km, Action::OrthographicCamera))).clicked() { self.perform(Action::OrthographicCamera, now); ui.close(); }
                if item(ui, icons::CUBE_WIRE, tr("Isolate the texture set"), &sc(km, Action::IsolateTextureSet)) { self.perform(Action::IsolateTextureSet, now); ui.close(); }
                ui.separator();
                if item(ui, icons::SUN, tr("Render quality…"), "") {
                    self.state.render_window = true;
                    ui.close();
                }
                ui.checkbox(&mut self.state.recovery.prefs.viewport.hints, "Viewport hints")
                    .on_hover_text("The list of mouse and key actions in the bottom-left corner of the 3D view.");
                ui.horizontal(|ui| {
                    ui.label(tr("Height bump"));
                    ui.add(egui::Slider::new(&mut self.state.height_scale, 0.0..=0.1));
                });
            });
            // Painter lists its JavaScript and Python plugins here.
            ui.menu_button(tr("Plugins"), |ui| {
                ui.set_min_width(240.0);
                ui.weak(tr("No plugins: SubCraft has no scripting API yet."));
            });
            ui.menu_button(tr("Help"), |ui| {
                ui.set_min_width(240.0);
                if item(ui, icons::CUBE, tr("Welcome screen"), "") { self.state.welcome.open = true; ui.close(); }
                if item(ui, icons::CURSOR, tr("Quick start"), "") { self.state.welcome.guide = Some(0); self.state.welcome.open = false; ui.close(); }
                if item(ui, icons::INFO, tr("What's new"), "") { self.state.welcome.whats_new = true; ui.close(); }
                if item(ui, icons::INFO, tr("About SubCraft…"), "") { self.state.welcome.about = true; ui.close(); }
                ui.separator();

                if item(ui, icons::DOWNLOAD, tr("Check for updates…"), "") { if let Some(updater) = &mut self.state.updater { updater.check(); } ui.close(); }
                if item(ui, icons::KEYBOARD, tr("Keyboard shortcuts"), "") {
                    self.state.show_shortcuts = true;
                    ui.close();
                }
                ui.separator();
                if item(ui, icons::INFO, tr("Report a bug…"), "") {
                    self.state.bug_report.open = true;
                    ui.close();
                }
                if let Some(log) = subcraft_app::diagnostics::log_file()
                    && item(ui, icons::FOLDER_OPEN, tr("Show log files"), "")
                {
                    crate::panels::shelf::show_in_folder(&log);
                    ui.close();
                }
                if item(ui, icons::DOWNLOAD, tr("Export log…"), "") {
                    crate::panels::log::export_log(&mut self.state, now);
                    ui.close();
                }
            });
            ui.cursor().left()
        }).inner
    }

    /// Run `action` now, or after the unsaved-changes prompt.
    fn request(&mut self, action: Pending, now: f64) {
        if self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
            if let Err(e) = self.state.pen.release(&self.state.session) { self.state.set_status(e.to_string(), now); }
            if self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
                self.state.pen_after = Some(crate::pen::After::Request(action)); return;
            }
        }
        let busy = self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying();
        if subcraft_app::unsaved::guard(self.state.session.is_modified(), busy, self.screenshot.is_some()) == subcraft_app::unsaved::Guard::AskToSave {
            self.state.confirm = Some(action);
        } else {
            self.pending_now = Some(action);
        }
        let _ = now;
    }

    fn flush_pen_after(&mut self, now: f64) {
        if !self.state.pen.is_editing() && !self.state.pen.is_pending() && !self.state.asset_preview.is_applying()
            && let Some(after) = self.state.pen_after.take()
        {
            match after {
                crate::pen::After::Action(action) => self.perform(action, now),
                crate::pen::After::Request(action) => self.request(action, now),
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let (icon, tool) = match self.state.tool {
                Tool::Brush => (icons::PAINT_BRUSH, "Brush"),
                Tool::Eraser => (icons::ERASER, "Eraser"),
                Tool::PolygonFill => (icons::PAINT_BUCKET, "Polygon fill"),
                Tool::Clone => (icons::COPY, "Clone"),
                Tool::Smudge => (icons::DROP, "Smudge"),
                Tool::Projection => (icons::IMAGE, "Projection"),
                Tool::Eyedropper => (icons::CURSOR, "Eyedropper"),
                Tool::Pen => (icons::CURVE, "Pen paths"),
            };
            let small = |t: String, c| RichText::new(t).font(theme::font(11.5)).color(c);
            ui.label(small(format!("{icon}  {tool}"), theme::ACCENT()));
            let size = if self.state.tool == Tool::Pen {
                self.state.pen.preview(&self.state.session).map_or_else(|| "Click to start a path".into(), |p| format!("Width {:.2}% UV", p.radius * 200.0))
            } else { format!("Size {:.1}%", self.state.session.brush.settings.size) };
            ui.label(small(size, theme::TEXT_DIM()));
            if let Some(l) = self.state.session.selected_layer() {
                ui.add(egui::Label::new(small(format!("{}  {} / {}", icons::STACK, self.state.session.selected_texture_set().name, l.name), theme::TEXT_DIM())).truncate());
            }
            if let Some(import) = &self.state.spp_import {
                let (text, fraction) = import.progress();
                ui.add(egui::Label::new(small(format!("{}  {text}", icons::INFO), theme::ACCENT())).truncate());
                ui.add(egui::ProgressBar::new(fraction.unwrap_or(0.0)).desired_width(120.0).animate(fraction.is_none()));
                if ui.small_button(tr("Cancel")).clicked() {
                    import.cancel();
                }
            }
            if let Some((msg, t)) = &self.state.status
                && now - t < 6.0
            {
                ui.add(egui::Label::new(small(format!("{}  {msg}", icons::INFO), theme::WARNING())).truncate()).on_hover_text(msg.as_str());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(small(format!("{:.1} ms", self.state.frame_time_ms), theme::TEXT_FAINT()));
                let rec = &self.state.recovery;
                if rec.autosaver.is_saving() {
                    ui.label(small(format!("{}  Autosaving…", icons::SAVE), theme::TEXT_FAINT()));
                } else if let Some(t) = rec.last_saved {
                    let mins = t.elapsed().map_or(0, |d| d.as_secs() / 60);
                    let when = if mins == 0 { "just now".to_string() } else { format!("{mins} min ago") };
                    ui.label(small(format!("{}  Autosaved {when}", icons::SAVE), theme::TEXT_FAINT()))
                        .on_hover_text(rec.autosaver.live().map(|l| format!("Recovery copies: {}", l.dir().display())).unwrap_or_default());
                }
                let mesh = &self.state.session.scene().mesh;
                ui.label(small(format!("{} tris", mesh.triangle_count()), theme::TEXT_FAINT()));
            });
        });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        // "Show all keyboard shortcuts…" buttons.
        if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_shortcuts_window"))).unwrap_or(false) {
            self.state.show_shortcuts = true;
        }
        crate::shortcuts_window::show(&mut self.state, ctx);
        if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_settings_shortcuts"))).unwrap_or(false) {
            self.state.recovery.show_prefs = true;
            self.state.recovery.prefs_tab = crate::recovery::SettingsTab::Shortcuts;
        }
    }

    /// A scripted Rendering-mode capture waits for its samples (or for the mode to give up).
    fn path_trace_capture_ready(&self) -> bool {
        let Some(wanted) = self.state.pt_capture_samples else { return true };
        if !self.state.recovery.prefs.viewport.path_tracing {
            return true;
        }
        let Some(s) = self.state.pt_status else { return false };
        let wanted = if s.max_samples == 0 { wanted } else { wanted.min(s.max_samples) };
        let ready = s.samples >= wanted;
        if ready && !self.screenshot_requested {
            log::info!("path tracing: {} samples per pixel, {:.1} samples/s at {}x{} view pixels", s.samples, s.samples_per_second, self.state.last_view_size[0], self.state.last_view_size[1]);
        }
        ready
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        // A pinned map can be captured during a real bake, after its GPU upload. This follows
        // actual worker completion, never a simulated timer or a placeholder image.
        let pinned_ready = !self.state.bake.follow_maps && self.state.bake.displayed_map.is_some_and(|kind| {
            let ts = self.state.session.selection().texture_set;
            self.state.session.mesh_maps(ts).and_then(|m| m.get(kind)).is_some_and(|image|
                self.state.uploaded_maps.get(&(ts, kind)).is_some_and(|gpu| gpu.same_storage(image)))
        });
        if !self.screenshot_requested && self.frames >= 20 && !self.state.mesh_reload.busy() && (pinned_ready || self.state.bake.job.is_none() && self.state.spp_import.is_none() && !self.state.session.composites_pending())
            && !self.state.bake.preview_pending() && (!self.state.asset_preview.is_applying() || self.state.capture_during_load) && (self.state.bake.preview_mesh.is_none() || self.state.bake.preview_uploaded)
            && self.path_trace_capture_ready()
        {
            if let Some(job) = self.state.pending_render_job.take() {
                match crate::render_export::run(&mut self.state, &job) {
                    Ok(message) => log::info!("{message}"),
                    Err(e) => log::error!("scripted render failed: {e}"),
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.screenshot_requested = true;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let data: Vec<f32> = img.pixels.iter().flat_map(|c| c.to_array()).map(|b| b as f32 / 255.0).collect();
            match subcraft_image::save_image(&path, w as u32, h as u32, 4, &data, subcraft_image::FileFormat::Png8) {
                Ok(()) => log::info!("screenshot saved to {}", path.display()),
                Err(e) => log::error!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn rail_divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, theme::BORDER_STRONG()));
}

/// Take a plain Tab press out of the input when no text field has the keyboard: Tab is a
/// shortcut (Hide UI), and left in the events egui would move the focus to a widget instead, which
/// then swallows every later shortcut. Returns whether Tab was pressed. While typing, Tab is egui's.
pub(crate) fn take_tab_press(events: &mut Vec<egui::Event>, typing: bool) -> bool {
    if typing {
        return false;
    }
    let plain_tab = |e: &egui::Event| matches!(e, egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if !modifiers.any());
    let pressed = events.iter().any(|e| plain_tab(e) && matches!(e, egui::Event::Key { pressed: true, repeat: false, .. }));
    events.retain(|e| !plain_tab(e));
    pressed
}

/// Menu / tooltip text with the action's first binding (the user's rebinds included).
fn sc(km: &subcraft_app::Keymap, a: Action) -> String {
    crate::keys::native_string(km.label(a))
}

/// `"Brush (B)"`; just the label when the action has no key.
fn tip(km: &subcraft_app::Keymap, label: &str, a: Action) -> String {
    let s = sc(km, a);
    if s.is_empty() { label.to_string() } else { format!("{label} ({s})") }
}

/// egui key to keymap key, for every key the table can name.
pub(crate) fn map_key(k: egui::Key) -> Option<Key> {
    use egui::Key as E;
    const LETTERS: [(E, Key); 26] = [
        (E::A, Key::A), (E::B, Key::B), (E::C, Key::C), (E::D, Key::D), (E::E, Key::E), (E::F, Key::F), (E::G, Key::G), (E::H, Key::H), (E::I, Key::I),
        (E::J, Key::J), (E::K, Key::K), (E::L, Key::L), (E::M, Key::M), (E::N, Key::N), (E::O, Key::O), (E::P, Key::P), (E::Q, Key::Q), (E::R, Key::R),
        (E::S, Key::S), (E::T, Key::T), (E::U, Key::U), (E::V, Key::V), (E::W, Key::W), (E::X, Key::X), (E::Y, Key::Y), (E::Z, Key::Z),
    ];
    const DIGITS: [(E, Key); 10] = [
        (E::Num0, Key::Num0), (E::Num1, Key::Num1), (E::Num2, Key::Num2), (E::Num3, Key::Num3), (E::Num4, Key::Num4),
        (E::Num5, Key::Num5), (E::Num6, Key::Num6), (E::Num7, Key::Num7), (E::Num8, Key::Num8), (E::Num9, Key::Num9),
    ];
    const FKEYS: [(E, Key); 12] = [
        (E::F1, Key::F1), (E::F2, Key::F2), (E::F3, Key::F3), (E::F4, Key::F4), (E::F5, Key::F5), (E::F6, Key::F6),
        (E::F7, Key::F7), (E::F8, Key::F8), (E::F9, Key::F9), (E::F10, Key::F10), (E::F11, Key::F11), (E::F12, Key::F12),
    ];
    if let Some((_, m)) = LETTERS.iter().chain(&DIGITS).chain(&FKEYS).find(|(e, _)| *e == k) {
        return Some(*m);
    }
    Some(match k {
        E::Delete => Key::Delete,
        E::Tab => Key::Tab,
        E::Space => Key::Space,
        E::Escape => Key::Escape,
        E::Backspace => Key::Backspace,
        E::Enter => Key::Enter,
        E::Plus => Key::Plus,
        E::Minus => Key::Minus,
        E::OpenBracket => Key::OpenBracket,
        E::CloseBracket => Key::CloseBracket,
        E::Home => Key::Home,
        _ => return None,
    })
}

struct Viewer<'a> {
    state: &'a mut UiState,
    /// Tabs shown last frame (marked) and the set being collected this frame.
    was_active: &'a HashSet<Tab>,
    active: HashSet<Tab>,
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        if matches!(tab, Tab::Viewport | Tab::UvView) {
            return RichText::new(format!("{}   {}", tab.icon(), tab.title())).font(theme::font_strong(12.0)).into();
        }
        // Painter titles its panels in capitals, without icons; Properties names the layer kind.
        let mut title = tab.title().to_uppercase();
        if let Tab::SubLibrary(id) = tab {
            title = self.state.shelf.sub_library_name(*id).unwrap_or(tab.title()).to_uppercase();
        }
        if *tab == Tab::Properties {
            let kind = self.state.session.selected_layer().map(|l| match l.kind() {
                subcraft_doc::LayerKind::Paint => "PAINT",
                subcraft_doc::LayerKind::Path => "PATH",
                subcraft_doc::LayerKind::Text => "TEXT",
                subcraft_doc::LayerKind::Fill => "FILL",
                subcraft_doc::LayerKind::Folder => "FOLDER",
            });
            if let Some(kind) = kind { title = format!("{title} - {kind}"); }
        }
        RichText::new(title).font(theme::font_strong(11.5)).into()
    }

    fn on_tab_button(&mut self, tab: &mut Tab, response: &egui::Response) {
        if self.was_active.contains(tab) {
            let r = response.rect;
            let painter = response.ctx.layer_painter(response.layer_id);
            painter.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 2.0)), 0.0, theme::ACCENT());
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        self.active.insert(*tab);
        match tab {
            Tab::Viewport => crate::edit_profile::time_tab("  tab 3D", || crate::viewport::show(self.state, ui)),
            Tab::UvView => crate::edit_profile::time_tab("  tab 2D", || panels::uv_view::show(self.state, ui)),
            Tab::Layers => crate::edit_profile::time_tab("  tab Layers", || panels::layers::show(self.state, ui)),
            Tab::Properties => crate::edit_profile::time_tab("  tab Properties", || panels::properties::show_docked(self.state, ui)),
            Tab::Brush => crate::edit_profile::time_tab("  tab Brush", || panels::brush::show(self.state, ui)),
            Tab::TextureSetList => crate::edit_profile::time_tab("  tab Texture set list", || panels::texture_sets::list(self.state, ui)),
            Tab::TextureSetSettings => crate::edit_profile::time_tab("  tab Texture set settings", || panels::texture_sets::settings(self.state, ui)),
            Tab::Parts => crate::edit_profile::time_tab("  tab Parts", || panels::parts::show(self.state, ui)),
            Tab::History => crate::edit_profile::time_tab("  tab History", || panels::history::show(self.state, ui)),
            Tab::Shelf => crate::edit_profile::time_tab("  tab Shelf", || panels::shelf::show(self.state, ui)),
            Tab::DisplaySettings => crate::edit_profile::time_tab("  tab Display settings", || panels::display_settings::show(self.state, ui)),
            Tab::SubLibrary(id) => crate::edit_profile::time_tab("  tab Sub-library", || panels::shelf::show_sub_library(self.state, ui, *id)),
            Tab::ShaderSettings => crate::edit_profile::time_tab("  tab Shader settings", || panels::shader_settings::show(self.state, ui)),
            Tab::Log => crate::edit_profile::time_tab("  tab Log", || panels::log::show(self.state, ui)),
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        // Every panel but the 3D and 2D views closes with its tab's X and comes back from the
        // Docks toolbar or Window > Views; a sub-library closes for good.
        crate::docks::closeable(*tab) || matches!(tab, Tab::SubLibrary(_))
    }

    fn on_close(&mut self, tab: &mut Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        if let Tab::SubLibrary(id) = tab {
            self.state.shelf.close_sub_library(*id);
        }
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        // These panels own bounded vertical lists and pinned preview-size footers. The dock's
        // extra scroll area would let content widen itself and scroll the footer offscreen.
        if matches!(tab, Tab::Shelf | Tab::SubLibrary(_) | Tab::Layers | Tab::TextureSetList) { [false, false] } else { [true, true] }
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport | Tab::UvView)
    }
}

impl SubcraftApp {
    /// A section of `ui()` ended (frame profilers).
    fn mark(&mut self, label: &'static str) {
        if let Some(p) = &mut self.profile { p.mark(label); }
        if let Some(p) = &mut self.view_profile { p.mark(label); }
    }
}

/// End of the frame: close the undo step of what the widgets did on a pointer release or an Enter
/// press. The Seal at the top of the frame runs before the widgets, so a click's own command (a
/// checkbox, a dropdown item, the last value of a slider drag, a typed value) stayed open and
/// merged with the next drag of the same control: one Undo reverted both.
pub(crate) fn seal_after_widgets(session: &mut Session, ctx: &egui::Context) {
    if ctx.input(|i| i.pointer.any_released() || i.key_pressed(egui::Key::Enter)) {
        let _ = session.execute(Command::Seal);
    }
}

impl eframe::App for SubcraftApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if take_tab_press(&mut raw_input.events, _ctx.egui_wants_keyboard_input()) {
            self.state.tab_pressed = true;
        }
        self.state.tablet.drain_into(_ctx, raw_input);
        let log = crate::pen_input::input_log_enabled();
        if log {
            for e in raw_input.events.iter().filter_map(crate::pen_input::describe) {
                log::info!(target: "subcraft::input", "in: {e}");
            }
        }
        if let Some(p) = &mut self.view_profile { p.inject(&self.state, &mut raw_input.events); }
        let before = self.state.pen_input.synthesized;
        self.state.pen_input.fix(&mut raw_input.events);
        if log && self.state.pen_input.synthesized != before {
            log::info!(target: "subcraft::input", "pen fixer added pointer events for a touch contact egui-winit did not translate");
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.state.tablet.start_once(_frame, ui.ctx());
        // A lost graphics device: draw nothing more (the renderer would panic on it).
        if crate::device_lost::frame_gate(&mut self.state) == crate::device_lost::Gate::Stop { return; }
        let ctx = ui.ctx().clone();
        if crate::bug_report::stop_after_device_loss(&mut self.state, subcraft_render::caps::device_lost()) {
            return;
        }
        if let Some(p) = &mut self.profile {
            p.ui_begin();
        }
        if let Some(p) = &mut self.view_profile {
            p.ui_begin(&mut self.state, &ctx);
        }
        self.state.recovery.prefs.apply_ui_size(&ctx);
        // Settings > General > Theme applies on the next frame.
        theme::follow(&ctx, self.state.recovery.prefs.theme);
        // The contextual toolbar's Viewport Mode dropdown asks; the docks live here.
        if let Some(layout) = self.state.view_layout_request.take() {
            apply_view_layout(&mut self.dock, layout);
        }
        self.state.view_layout = current_view_layout(&self.dock);
        // Preferences holds the language picker: its native names need the CJK fonts even in English.
        crate::i18n::sync_fonts(&ctx, self.state.recovery.prefs.language, self.state.recovery.show_prefs);
        self.frames += 1;
        if self.frames == 12 && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("capture_jump_maps"))).unwrap_or(false) { ctx.data_mut(|d| d.insert_temp(egui::Id::new("tss_jump"), 2u8)); }
        if self.frames == 5 { subcraft_app::startup_probe::step(subcraft_app::startup_probe::COMPLETE_MARKER); }
        if self.frames >= 12 && self.state.bake.job.is_none() && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("capture_id_colors"))).unwrap_or(false) {
            // Scripted capture of the ID color editor: once an ID map exists, select its second
            // most common color (the first is usually the background) and arm the eyedropper.
            let ts = self.state.session.selected_texture_set().id;
            if let Some(p) = self.state.session.id_palette(ts) && let Some(e) = p.get(1).or(p.first()) {
                self.state.id_palette_selected = Some(e.color);
                self.state.id_palette_new = [0.95, 0.72, 0.15];
                self.state.picking_id_palette = true;
                ctx.data_mut(|d| { d.remove_temp::<bool>(egui::Id::new("capture_id_colors")); d.insert_temp(egui::Id::new("tss_jump"), 3u8); });
            }
        }
        if self.frames == 15 && let Some(name) = self.state.capture_select_brush.take() {
            let now = ctx.input(|i| i.time);
            let preset = self.state.brush_presets.iter().find(|p| p.name == name).cloned();
            let tip = self.state.library.alphas.iter().find(|a| a.name == name).cloned();
            match (preset, tip) {
                (Some(p), _) => self.state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&p), now),
                (None, Some(a)) => self.state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&a), now),
                (None, None) => self.state.set_status(format!("No Shelf brush named {name}"), now),
            }
        }
        if self.frames == 16 && let Some(which) = self.state.capture_tool.take() {
            if which == "language-picker" {
                // Scripted capture: Preferences with the language dropdown open (CJK native names).
                self.state.recovery.show_prefs = true;
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("capture_language_picker"), true));
            } else if which == "settings-shortcuts" {
                // Scripted capture: Settings > Shortcuts (the saved preferences are not touched).
                self.state.recovery.prefs_tab = crate::recovery::SettingsTab::Shortcuts;
                self.state.recovery.show_prefs = true;
            } else {
                scripted_tool_capture(&mut self.state, &which, ctx.input(|i| i.time));
            }
        }
        self.state.view_props_swallow = crate::view_props::show(&ctx, &mut self.state);
        crate::intro::show(&ctx, &mut self.state.intro);
        crate::pen::keyboard(&mut self.state, &ctx, ctx.input(|i| i.time));
        self.shortcuts(&ctx);
        self.state.session.default_uv_scale = self.state.recovery.prefs.default_uv_scale;
        if ctx.input(|i| i.pointer.any_released()) {
            let _ = self.state.session.execute(Command::Seal);
        }
        self.mark("input+shortcuts");
        if let Some(p) = &mut self.profile {
            p.before_sync(&mut self.state);
            ctx.request_repaint();
        }
        self.state.begin_frame();
        self.state.sync_gpu();
        if self.state.session.composites_pending() {
            ctx.request_repaint();
        }
        if let Some(due) = crate::mask_view::refresh_due(&self.state) {
            ctx.request_repaint_after(due);
        }
        if self.profile.as_mut().is_some_and(|p| p.after_sync(&self.state)) {
            self.profile = None;
            self.state.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.state.poll_library();
        self.state.poll_photocraft(ctx.input(|i| i.time));
        if self.state.photocraft.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(subcraft_app::photocraft::POLL_INTERVAL));
        }
        self.state.layer_previews.poll(&ctx);
        self.state.textures.poll(&ctx);
        self.mark("sync+polls");
        if self.state.env_loader.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        // A texture set's shader menu asked for the Shader Settings panel.
        if std::mem::take(&mut self.state.open_shader_settings) {
            open_shader_settings(&mut self.dock);
        }
        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(12, 0));
        egui::Panel::top("header").exact_size(44.0).show_separator_line(false)
            .frame(egui::Frame::new().fill(theme::BG_2()).inner_margin(egui::Margin { left: 12, right: if crate::window_frame::frameless() { 0 } else { 12 }, top: 0, bottom: 0 }))
            .show(ui, |ui| self.header_bar(ui));
        // Tab (Hide UI) leaves the menu bar and the views.
        let (rail, status) = (self.state.recovery.prefs.show_tools_toolbar, self.state.recovery.prefs.show_status_bar);
        if !self.state.hide_ui && status {
            egui::Panel::bottom("status").exact_size(26.0).show_separator_line(false).frame(bar(theme::BG_0())).show(ui, |ui| self.status_bar(ui));
        }
        if !self.state.hide_ui && rail && !self.state.bake.open {
            egui::Panel::left("tools")
                .exact_size(48.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme::BG_2()).inner_margin(egui::Margin::symmetric(4, 0)))
                .show(ui, |ui| self.tool_rail(ui));
        }
        self.mark("bars");
        dialogs::bake_mode(&mut self.state, ui, ctx.input(|i| i.time));
        self.workspace(ui);
        self.mark("workspace (docked panels)");
        if let Some(layer) = ctx.data_mut(|data| data.remove_temp::<subcraft_core::LayerId>(egui::Id::new("inspect_mask"))) {
            self.state.layer_previews.inspect = Some(layer);
        }
        crate::layer_previews::inspection_window(&mut self.state, &ctx);
        self.shortcuts_window(&ctx);
        let now = ctx.input(|i| i.time);
        crate::asset_preview::show(&mut self.state,&ctx);
        crate::render_export::window(&mut self.state, &ctx);
        panels::display_settings::sync_project(&mut self.state);
        crate::view_state::sync(&mut self.state);
        crate::new_project::window(&mut self.state, &ctx);
        crate::mesh_map_import::window(&mut self.state, &ctx, now);
        crate::render_export::handle_request(&mut self.state, ctx.input(|i| i.time));
        crate::pen::finish_frame(&mut self.state, &ctx, now);
        self.flush_pen_after(now);
        self.mark("windows+pen");
        crate::recovery::tick(&mut self.state, &ctx, now);
        self.mark("recovery tick");
        crate::recovery::preferences_window(&mut self.state, &ctx);
        crate::recovery::prompt(&mut self.state, &ctx, now);
        crate::text_panel::window(&mut self.state, &ctx, now);
        crate::bug_report::report_window(&mut self.state, &ctx);
        crate::bug_report::crash_window(&mut self.state, &ctx);
        crate::bug_report::safe_mode_window(&mut self.state, &ctx);
        crate::spp_import::reconstruction_notice(&mut self.state, &ctx);
        if let Some(action) = crate::welcome::show(&mut self.state, &ctx) {
            match action {
                crate::welcome::WelcomeAction::Pending(pending) => self.request(pending, now),
                crate::welcome::WelcomeAction::Guide(action) => match action {
                    subcraft_app::launch::GuideAction::OpenModel => self.request(Pending::OpenMesh, now),
                    subcraft_app::launch::GuideAction::Bake => self.state.bake.open = true,
                    subcraft_app::launch::GuideAction::SmartMaterial => {
                        self.state.shelf.tab = panels::shelf::ShelfTab::SmartMaterials;
                        if let Some(path) = self.dock.find_tab(&Tab::Shelf) { let _ = self.dock.set_active_tab(path); }
                    }
                    subcraft_app::launch::GuideAction::PaintParts => {
                        if let Some(path) = self.dock.find_tab(&Tab::Parts) { let _ = self.dock.set_active_tab(path); }
                        self.state.tool = Tool::Brush;
                    }
                    subcraft_app::launch::GuideAction::Save => self.perform(Action::Save, now),
                    subcraft_app::launch::GuideAction::Export => self.state.export.open = true,
                },
            }
            ctx.request_repaint();
        }

        crate::updates::tick(&mut self.state, &ctx);
        self.mark("dialogs+welcome");
        if self.test_crash && self.frames > 30 {
            panic!("test crash requested with --test-crash");
        }
        dialogs::export_window(&mut self.state, &ctx, now);
        crate::layer_export::layer_export_window(&mut self.state, &ctx, now);
        crate::export_mesh::export_mesh_window(&mut self.state, &ctx, now);
        crate::mesh_reload::window(&mut self.state, &ctx, now);
        crate::auto_uv::window(&mut self.state, &ctx, now);
        dialogs::poll_bake(&mut self.state, &ctx, now);
        panels::id_colors::sync_pick_view(&mut self.state);
        crate::spp_import::poll(&mut self.state, &ctx, now);
        crate::files::poll(&mut self.state, now);
        crate::files::import_report_window(&mut self.state, &ctx);
        crate::import_resources::window(&mut self.state, &ctx);
        crate::clipboard::shortcut_requests(&ctx);
        crate::clipboard::process(&mut self.state, &ctx, now);
        // Files dropped anywhere else on the window (the Shelf handles its own).
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !std::mem::take(&mut self.state.drops_taken) {
            for drop in crate::files::window_drops(&dropped) {
                match drop {
                    crate::files::WindowDrop::SmartMaterial(p) => crate::files::import_painter_smart_material(&mut self.state, &p, now),
                    crate::files::WindowDrop::Model(p) => { self.state.dropped_model = Some(p); self.request(Pending::OpenMesh, now); }
                    crate::files::WindowDrop::Project(p) => { self.state.welcome.pending_recent = Some(p); self.request(Pending::OpenRecent, now); }
                    crate::files::WindowDrop::PainterProject(p) => { self.state.dropped_model = Some(p); self.request(Pending::ImportPainter, now); }
                }
            }
        }
        if let Some(action) = dialogs::unsaved_prompt(&mut self.state, &ctx, now) {
            self.pending_now = Some(action);
        }
        // Closing with unsaved work asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.allow_close && self.screenshot.is_none()
            && (self.state.session.is_modified() || self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Close, now);
        }
        if let Some(action) = self.pending_now.take() {
            if action == Pending::Close {
                self.state.allow_close = true;
            }
            dialogs::run_pending(&mut self.state, action, &ctx, now);
        }
        seal_after_widgets(&mut self.state.session, &ctx);
        let title = dialogs::title(&self.state);
        if title != self.state.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.state.last_title = title;
        }
        if crate::window_frame::frameless() { crate::window_frame::resize_edges(&ctx); }
        self.handle_screenshot(&ctx);
        self.mark("rest (title, close)");
        if let Some(p) = &mut self.profile {
            p.ui_end();
        }
        if self.view_profile.as_mut().is_some_and(|p| p.ui_end()) {
            self.view_profile = None;
            self.state.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.screenshot.is_some() {
            return; // scripted runs never overwrite the user's layout
        }
        // Sub-library tabs belong to this session only.
        if let Ok(j) = serde_json::to_string(&self.dock.filter_tabs(|t| !matches!(t, Tab::SubLibrary(_)))) {
            storage.set_string(DOCK_KEY, j);
        }
        if let Ok(j) = serde_json::to_string(&self.state.recovery.prefs) {
            storage.set_string(crate::recovery::PREFS_KEY, j);
        }
        if let Ok(j) = serde_json::to_string(&self.state.welcome.recent) { storage.set_string(crate::welcome::RECENTS_KEY, j); }
    }

    /// Clean exit: the recovery folder of this session is no longer needed.
    fn on_exit(&mut self) {
        self.state.tablet.shutdown();
        self.state.recovery.autosaver.finish_clean();
    }
}

/// Demo stack on the default model: procedural base, an upward-facing "moss" fill masked by a
/// directional generator, and a few painted strokes.
/// Native-capture helper: the same session calls the viewport makes, driven by a script instead of
/// the mouse (no input is sent to the desktop).
fn scripted_tool_capture(state: &mut UiState, which: &str, now: f64) {
    use glam::Vec2;
    use subcraft_paint::StrokeMode;
    let cam = state.camera.paint_camera([900, 650]);
    let s = &mut state.session;
    s.brush.settings.size = 7.0;
    s.brush.settings.hardness = 0.8;
    let stroke = |s: &mut Session, mode: StrokeMode, pts: &[(f32, f32)]| {
        s.brush.mode = mode;
        if s.begin_stroke().is_err() { return; }
        for &(x, y) in pts { let _ = s.stroke_to(&cam, Vec2::new(x, y), false); }
        s.end_stroke();
    };
    let line = |x0: f32, y0: f32, x1: f32, y1: f32| (0..=24).map(|i| { let t = i as f32 / 24.0; (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t) }).collect::<Vec<_>>();
    let set = |s: &mut Session, c: [f32; 3]| s.brush.material = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(c[0], c[1], c[2]))]);
    match which {
        "smudge" => {
            state.tool = Tool::Smudge;
            for (i, c) in [[0.85, 0.1, 0.05], [0.1, 0.25, 0.9], [0.95, 0.8, 0.1]].into_iter().enumerate() {
                set(s, c);
                stroke(s, StrokeMode::Paint, &line(300.0, 250.0 + 45.0 * i as f32, 560.0, 250.0 + 45.0 * i as f32));
            }
            stroke(s, StrokeMode::Smudge, &line(380.0, 215.0, 410.0, 420.0));
            stroke(s, StrokeMode::Smudge, &line(470.0, 215.0, 440.0, 420.0));
        }
        "scatter" => {
            // Scatter presets: dust, drips and flakes in three colours.
            let presets = subcraft_app::brush_builtin::builtin_brush_presets();
            let tips = subcraft_app::assets::alphas::builtin();
            for (i, (name, c)) in [("Dust Scatter", [0.9, 0.75, 0.3]), ("Drips", [0.1, 0.35, 0.9]), ("Rust Flakes", [0.75, 0.25, 0.08])].into_iter().enumerate() {
                if let Some(p) = presets.iter().find(|p| p.name == name) { s.apply_brush_preset(p, &tips); }
                s.brush.settings.size = 7.0;
                set(s, c);
                stroke(s, StrokeMode::Paint, &line(300.0, 250.0 + 70.0 * i as f32, 600.0, 250.0 + 70.0 * i as f32));
            }
        }
        "stabilizer" => {
            // The same shaky cursor path three times: raw, smoothed, and smoothed with a rope.
            let shaky = |y: f32| (0..=60).map(|i| (300.0 + 5.0 * i as f32, y + if i % 2 == 0 { 9.0 } else { -9.0 })).collect::<Vec<_>>();
            set(s, [0.1, 0.1, 0.1]);
            s.brush.settings.size = 3.0;
            s.brush.settings.hardness = 1.0;
            for (i, (smoothing, rope)) in [(0.0, 0.0), (0.8, 0.0), (0.7, 16.0)].into_iter().enumerate() {
                s.brush.options.smoothing = smoothing;
                s.brush.options.lazy_distance_px = rope;
                s.brush.options.catch_up = true;
                stroke(s, StrokeMode::Paint, &shaky(250.0 + 60.0 * i as f32));
            }
        }
        "normal-brush-select" | "normal-brush-select-loading" => {
            // The same call a click on a Starter pack normal map makes (loads the image off-thread).
            use subcraft_assets::starter::{ENTRIES, Kind};
            if let Some(i) = ENTRIES.iter().position(|e| e.kind == Kind::Normal && e.name == "Bolted Armor") {
                let _ = s.execute(Command::AddLayer(NewLayer::Paint));
                state.capture_during_load = which == "normal-brush-select-loading";
                let UiState { asset_preview, session, .. } = state;
                asset_preview.select_starter(session, i, Some(subcraft_paint::StampKind::Normal));
                state.tool = Tool::Brush;
                state.shelf.tab = crate::panels::shelf::ShelfTab::NormalMaps;
                state.show_selected_brush(None);
            }
        }
        "starter-alpha-select" => {
            // A click on a Starter pack alpha in Shelf › Brushes (thumbnail tip at once, full image after).
            use subcraft_assets::starter::{ENTRIES, Kind};
            if let Some(i) = ENTRIES.iter().position(|e| e.kind == Kind::Alpha) {
                let UiState { asset_preview, session, .. } = state;
                asset_preview.select_starter(session, i, None);
                state.tool = Tool::Brush;
                state.shelf.tab = crate::panels::shelf::ShelfTab::Brushes;
                state.show_selected_brush(None);
            }
        }
        "normal-brush-panel" => {
            // End state of a click on a Starter pack normal map (the load itself is `normal-brush-select`).
            use subcraft_assets::starter::{ENTRIES, Kind};
            let _ = s.execute(Command::AddLayer(NewLayer::Paint));
            state.tool = Tool::Brush;
            if let Some(i) = ENTRIES.iter().position(|e| e.kind == Kind::Normal && e.name == "Bolted Armor")
                && let Some(bytes) = subcraft_assets::starter::encoded(i)
                && let Ok(bitmap) = subcraft_image::io::decode_bitmap(bytes, Some("starter.png"))
            {
                let _ = s.set_brush_stamp("Bolted Armor", subcraft_paint::StampKind::Normal, bitmap);
                s.brush.settings.size = 16.0;
            }
        }
        "normal-brush" | "normal-brush-channel" => {
            // Stamp starter normal maps onto the model with a paint layer selected (screws, dents, plates).
            use subcraft_assets::starter::{ENTRIES, Kind};
            let _ = s.execute(Command::AddLayer(NewLayer::Paint));
            state.tool = Tool::Brush;
            if which == "normal-brush-channel" { state.active_channel = ChannelKind::Normal; }
            let jobs: [(&str, f32, f32, f32, &[(f32, f32)]); 4] = [
                ("Bolted Armor", 26.0, 0.0, 2.0, &[(400.0, 270.0)]),
                ("Bullet Holes Dents", 22.0, 35.0, 2.5, &[(500.0, 290.0), (450.0, 360.0)]),
                ("Cratered Surface", 24.0, -20.0, 2.0, &[(380.0, 380.0), (520.0, 400.0)]),
                ("Diamond Plate", 16.0, 0.0, 2.0, &[(450.0, 215.0)]),
            ];
            for (name, size, angle, strength, points) in jobs {
                let Some(i) = ENTRIES.iter().position(|e| e.kind == Kind::Normal && e.name == name) else { continue };
                let Some(bytes) = subcraft_assets::starter::encoded(i) else { continue };
                let Ok(bitmap) = subcraft_image::io::decode_bitmap(bytes, Some("starter.png")) else { continue };
                if s.set_brush_stamp(name, subcraft_paint::StampKind::Normal, bitmap).is_err() { continue }
                s.brush.settings.size = size;
                s.brush.settings.angle_deg = angle;
                if let Some(st) = s.brush.material.stamp.as_mut() { st.strength = strength; }
                for &(x, y) in points { stroke(s, StrokeMode::Paint, &[(x, y)]); }
            }
            // A stamped line: spacing 1 repeats the same map along the stroke.
            s.brush.settings.size = 12.0;
            stroke(s, StrokeMode::Paint, &line(330.0, 330.0, 570.0, 330.0));
        }
        "fill-box" | "fill-modes" => {
            // Polygon fill: boxes (visible-only red, x-ray blue) or one click per fill mode.
            use subcraft_paint::PolygonFillMode as M;
            state.tool = Tool::PolygonFill;
            if which == "fill-box" {
                set(s, [0.85, 0.12, 0.08]);
                let _ = s.polygon_fill_box(&cam, Vec2::new(300.0, 200.0), Vec2::new(450.0, 470.0), M::Polygon, true);
                set(s, [0.1, 0.3, 0.95]);
                let _ = s.polygon_fill_box(&cam, Vec2::new(470.0, 200.0), Vec2::new(620.0, 470.0), M::Polygon, false);
                // Show the far side: the red box stayed in front, the blue one went through.
                state.camera.orbit(330.0, 0.0);
            } else {
                for (mode, c, at) in [
                    (M::Triangle, [0.9, 0.15, 0.1], (340.0, 260.0)),
                    (M::Polygon, [0.95, 0.8, 0.1], (520.0, 250.0)),
                    (M::MeshElement, [0.15, 0.7, 0.25], (450.0, 420.0)),
                    (M::UvIsland, [0.15, 0.35, 0.95], (430.0, 330.0)),
                ] {
                    set(s, c);
                    let _ = s.polygon_fill(&cam, Vec2::new(at.0, at.1), mode);
                }
            }
            state.fill_mode = M::Polygon;
        }
        "view-props-brush" => {
            // The right-click Properties popup over the 3D view, brush settings showing.
            state.tool = Tool::Brush;
            crate::view_props::open(state, egui::pos2(820.0, 330.0), crate::view_props::View::View3d);
        }
        "view-props-uv" => {
            state.tool = Tool::PolygonFill;
            crate::view_props::open(state, egui::pos2(900.0, 300.0), crate::view_props::View::View2d);
        }
        w if w.starts_with("curvature") => {
            // Curvature generator on an orange fill: `curvature-both|edges|cavities|sizes[-props]`.
            use subcraft_doc::{CurvatureDetail, CurvatureDetailMode, FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
            let ts = s.document().texture_sets[0].clone();
            let settings = subcraft_bake::BakeSettings { size: 1024, maps: vec![subcraft_core::MeshMapKind::Curvature], ..Default::default() };
            let mut job = s.start_bake(subcraft_app::BakeRequest { settings, high_poly: None, texture_sets: vec![(ts.id, ts.material_slot)] });
            while job.poll(s).is_none() { std::thread::sleep(std::time::Duration::from_millis(20)); }
            let _ = s.execute(Command::AddLayer(NewLayer::Fill));
            let Some(layer) = s.selection().layer else { return };
            let _ = s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.45, 0.05))) });
            let _ = s.execute(Command::AddMask { layer, white: false });
            let mut g = Generator::new(GeneratorKind::MeshMap(subcraft_core::MeshMapKind::Curvature));
            let mode = if w.contains("edges") || w.contains("sizes") { CurvatureDetailMode::Edges } else if w.contains("cavities") { CurvatureDetailMode::Cavities } else { CurvatureDetailMode::Both };
            let (medium, coarse) = if w.contains("sizes") { (1.5, 1.5) } else { (0.0, 0.0) };
            g.curvature = (mode != CurvatureDetailMode::Both || medium > 0.0).then_some(CurvatureDetail { mode, fine: if w.contains("sizes") { 0.6 } else { 1.0 }, medium, coarse });
            let _ = s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) });
            let effect = s.selected_layer().and_then(|l| l.mask.as_ref()).and_then(|m| m.effects.last()).map(|e| e.id);
            let _ = s.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Mask, effect });
            state.tool = Tool::Brush;
            if w.ends_with("-props") {
                crate::view_props::open(state, egui::pos2(820.0, 260.0), crate::view_props::View::View3d);
            }
        }
        "clone" => {
            state.tool = Tool::Clone;
            set(s, [0.85, 0.1, 0.05]);
            stroke(s, StrokeMode::Paint, &line(300.0, 270.0, 380.0, 270.0));
            set(s, [0.1, 0.25, 0.9]);
            stroke(s, StrokeMode::Paint, &line(300.0, 310.0, 380.0, 340.0));
            set(s, [0.95, 0.8, 0.1]);
            stroke(s, StrokeMode::Paint, &line(300.0, 300.0, 380.0, 300.0));
            if s.set_clone_source(&cam, Vec2::new(340.0, 305.0)) {
                stroke(s, StrokeMode::Clone, &line(470.0, 260.0, 560.0, 260.0));
                stroke(s, StrokeMode::Clone, &line(470.0, 330.0, 560.0, 350.0));
            }
        }
        "projection" => {
            // A colourful 8 x 8 checker cast from the view onto whatever the strokes cross.
            let px: Vec<u8> = (0..64).flat_map(|i| { let (x, y) = (i % 8, i / 8); if (x + y) % 2 == 0 { [235, 70 + 20 * y as u8, 60] } else { [40 + 25 * x as u8, 120, 230] } }).collect();
            if let Err(e) = s.set_brush_channel_bitmap(ChannelKind::BaseColor, "projection checker", subcraft_image::Bitmap::new_u8(8, 8, 3, px)) { state.set_status(e, now); return; }
            s.brush.material.camera_projection = true;
            s.brush.material.texture_tiling = false;
            s.brush.material.projection.scale = [1.4, 1.4];
            s.brush.settings.size = 22.0;
            for y in [250.0, 300.0, 350.0, 400.0] { stroke(s, StrokeMode::Paint, &line(260.0, y, 640.0, y)); }
        }
        "emission" => {
            // A new project: bake curvature, add emission through the same command as the "+ emiss"
            // chip, keep only that channel on a white fill and mask it with the Edges generator.
            use subcraft_core::{ChannelKind, MeshMapKind};
            let mesh = s.scene().mesh.clone();
            let bvh = subcraft_mesh::Bvh::build(&mesh);
            let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot, t.resolution)).collect();
            for (ts, slot, size) in sets {
                let _ = s.execute(Command::SelectTextureSet(ts));
                let input = subcraft_bake::BakeInput { low: subcraft_bake::Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot };
                let settings = subcraft_bake::BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() };
                if let Ok(maps) = subcraft_bake::bake(&input, &settings, &subcraft_bake::Progress::new()) { s.set_mesh_maps(ts, maps); }
                let _ = s.execute(Command::AddLayer(NewLayer::Fill));
                let Some(layer) = s.selection().layer else { continue };
                let _ = s.execute(Command::AddFillChannel { layer, channel: ChannelKind::Emissive });
                for kind in s.channels() {
                    if kind != ChannelKind::Emissive {
                        let _ = s.execute(Command::ToggleFillChannel { layer, effect: None, channel: kind, solo: false });
                    }
                }
                let _ = s.execute(Command::AddMask { layer, white: false });
                let mut g = subcraft_doc::Generator::new(subcraft_doc::GeneratorKind::Edges);
                g.level = 0.55;
                let _ = s.execute(Command::AddMaskEffect { layer, kind: subcraft_doc::MaskEffectKind::Generator(g) });
                // Show the layer's own Properties (Channels), not the mask effect just added.
                let _ = s.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Content, effect: None });
            }
        }
        "radial" => {
            // Radial symmetry (Y x 8 unless --radial chose another): a few strokes, each repeated
            // around the axis by the same session calls a mouse stroke makes.
            if !s.brush.options.radial.enabled {
                let radial = subcraft_paint::RadialSymmetry { enabled: true, axis: 1, count: 8, ..s.brush.options.radial };
                let _ = s.execute(Command::SetStrokeOptions(subcraft_paint::StrokeOptions { radial, ..s.brush.options }));
            }
            s.brush.settings.size = 4.0;
            set(s, [0.9, 0.15, 0.05]);
            stroke(s, StrokeMode::Paint, &line(450.0, 300.0, 560.0, 250.0));
            set(s, [0.95, 0.8, 0.1]);
            stroke(s, StrokeMode::Paint, &line(380.0, 380.0, 520.0, 420.0));
        }
        "cards" => {
            // A fill layer with every channel on, Properties showing its channel blocks.
            use subcraft_core::ChannelKind;
            let _ = s.execute(Command::AddLayer(NewLayer::Fill));
            if let Some(layer) = s.selection().layer {
                let _ = s.execute(Command::AddFillChannel { layer, channel: ChannelKind::Emissive });
                let _ = s.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Content, effect: None });
            }
        }
        _ => state.set_status(format!("--capture-tool expects clone, smudge, projection, emission, radial or cards, not {which}"), now),
    }
}

fn build_demo(state: &mut UiState) {
    let s = &mut state.session;
    let run = |s: &mut Session, c: Command| {
        if let Err(e) = s.execute(c) {
            log::warn!("demo: {e}");
        }
    };
    let paint = s.selection().layer.expect("new sessions select a paint layer");
    // Base material (below the paint layer).
    run(s, Command::AddLayer(NewLayer::Fill));
    let base = s.selection().layer.unwrap();
    run(s, Command::MoveDown(base));
    let wood = FillSource::Procedural {
        params: ProceduralParams { scale: 3.0, ..ProceduralParams::new(ProceduralKind::WoodRings) },
        low: ChannelValue::rgb(0.18, 0.09, 0.04),
        high: ChannelValue::rgb(0.45, 0.25, 0.11),
    };
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: FillChannel::new(wood) });
    let rough = FillSource::Procedural {
        params: ProceduralParams { scale: 8.0, ..ProceduralParams::new(ProceduralKind::Grunge) },
        low: ChannelValue::scalar(0.45),
        high: ChannelValue::scalar(0.8),
    };
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel::new(rough) });
    run(s, Command::Rename { layer: base, name: "Wood".into() });
    // Moss on top-facing surfaces.
    run(s, Command::SelectLayer(Some(paint)));
    run(s, Command::AddLayer(NewLayer::Fill));
    let moss = s.selection().layer.unwrap();
    run(s, Command::Rename { layer: moss, name: "Moss".into() });
    let moss_color = FillSource::Procedural {
        params: ProceduralParams { scale: 6.0, ..ProceduralParams::new(ProceduralKind::Clouds) },
        low: ChannelValue::rgb(0.06, 0.12, 0.03),
        high: ChannelValue::rgb(0.2, 0.35, 0.08),
    };
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::BaseColor, fill: FillChannel::new(moss_color) });
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::Roughness, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.9))) });
    if let Some(LayerContent::Fill(f)) = s.selected_layer().map(|l| l.content.clone()) {
        for (k, ch) in f.channels {
            if !matches!(k, ChannelKind::BaseColor | ChannelKind::Roughness) && ch.enabled {
                run(s, Command::SetFillChannel { layer: moss, channel: k, fill: FillChannel { enabled: false, ..ch } });
            }
        }
    }
    run(s, Command::AddMask { layer: moss, white: false });
    let mut g = Generator::new(GeneratorKind::Directional);
    g.level = 0.85;
    g.softness = 0.2;
    g.noise_amount = 0.35;
    run(s, Command::AddMaskEffect { layer: moss, kind: MaskEffectKind::Generator(g) });
    // Painted strokes on the paint layer (now topmost).
    run(s, Command::SelectLayer(Some(paint)));
    run(s, Command::MoveUp(paint));
    s.brush.settings.size = 4.0;
    s.brush.material = subcraft_paint::BrushMaterial::with(&[
        (ChannelKind::BaseColor, ChannelValue::rgb(0.75, 0.12, 0.03)),
        (ChannelKind::Roughness, ChannelValue::scalar(0.35)),
        (ChannelKind::Metallic, ChannelValue::scalar(0.0)),
    ]);
    let cam = state.camera.paint_camera([1280, 800]);
    let center = state.session.scene().mesh.bounds.center();
    let anchor = cam.project(center).map(|p| glam::Vec2::new(p.x, p.y)).unwrap_or(glam::Vec2::new(640.0, 400.0));
    let s = &mut state.session;
    for row in 0..2 {
        if s.begin_stroke().is_err() {
            break;
        }
        for k in 0..60 {
            let t = k as f32 / 59.0;
            let p = anchor + glam::Vec2::new(-260.0 + 420.0 * t, 40.0 + row as f32 * 60.0 + (t * 12.0).sin() * 18.0);
            let _ = s.stroke_to(&cam, p, false);
        }
        s.end_stroke();
    }
}
    #[test]
    fn save_waits_for_pending_shelf_image_and_embeds_the_result() {
        let file=std::env::temp_dir().join(format!("subcraft-image-save-{}.subcraft",subcraft_core::ObjectId::new()));
        let mut session=Session::new(subcraft_mesh::primitives::cube(2.0),128);session.save(Some(&file)).unwrap();
        let mut app=SubcraftApp {state:UiState::new(session,None),dock:default_layout(),frames:0,screenshot:None,screenshot_requested:false,pending_now:None,active_tabs:HashSet::new(),inspector_selection:None,test_crash:false,popped_dock:None,profile:None,view_profile:None};
        app.state.asset_preview.queue(&app.state.session,0,None);app.perform(Action::Save,0.0);assert!(app.state.pen_after.is_some());
        let ctx=egui::Context::default();
        // The worker needs real scheduling time under parallel workspace tests; a fixed
        // number of fast headless frames can finish before image decoding gets a turn.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut out=ctx.run_ui(egui::RawInput::default(),|ui|crate::asset_preview::show(&mut app.state,ui.ctx()));out.textures_delta.clear();
            app.flush_pen_after(0.1);if !app.state.asset_preview.is_applying(){break;}
            assert!(std::time::Instant::now() < deadline, "pending shelf image did not finish within 10 seconds");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!app.state.asset_preview.is_applying());assert!(app.state.pen_after.is_none());assert!(!app.state.session.is_modified());
        let reopened=Session::open_project(&file).unwrap();assert_eq!(reopened.document(),app.state.session.document());assert!(reopened.resources().iter().any(|r|r.name.starts_with("Starter: ")));std::fs::remove_file(file).unwrap();
    }

#[cfg(test)]
mod export_path_tests {
    use super::*;

    #[test]
    fn export_path_sets_the_export_folder() {
        let default = "/home/me/exports";
        assert_eq!(LaunchOptions::default().export_folder(default), default);
        let options = LaunchOptions { export_path: Some(PathBuf::from("/work/hero/textures")), ..Default::default() };
        assert_eq!(options.export_folder(default), PathBuf::from("/work/hero/textures").display().to_string());
    }
}

#[cfg(test)]
mod seal_tests {
    use super::*;
    use subcraft_core::ChannelKind;

    /// QA 2026-11: a click that changed a value and the slider drag after it were one undo step.
    #[test]
    fn a_release_closes_the_undo_step_of_what_the_widgets_just_did() {
        let mut session = Session::sample();
        session.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
        let layer = session.selection().layer.unwrap();
        let ctx = egui::Context::default();
        let entries = session.history().entries().len();
        let frame = |session: &mut Session, events: Vec<egui::Event>, opacity: f32| {
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                // A widget dispatches its command during the frame...
                session.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity }).unwrap();
                // ...and the frame ends.
                seal_after_widgets(session, ui.ctx());
            });
            out.textures_delta.clear();
        };
        let at = egui::pos2(10.0, 10.0);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        // A drag: three values, released in the last frame.
        frame(&mut session, vec![egui::Event::PointerMoved(at), button(true)], 0.9);
        frame(&mut session, vec![], 0.8);
        frame(&mut session, vec![button(false)], 0.7);
        assert_eq!(session.history().entries().len(), entries + 1, "one drag is one undo step");
        // The next drag of the same control is its own step.
        frame(&mut session, vec![button(true)], 0.5);
        frame(&mut session, vec![button(false)], 0.4);
        assert_eq!(session.history().entries().len(), entries + 2);
        session.execute(Command::Undo).unwrap();
        assert_eq!(session.selected_layer().unwrap().blend[&ChannelKind::BaseColor].opacity, 0.7, "Undo returns to where the first drag ended");
    }
}
