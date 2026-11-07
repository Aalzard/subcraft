//! Shelf: materials, smart materials, smart masks, brush alphas and environments, with rendered
//! thumbnails. Clicking applies the asset through the session (undoable where it edits layers).

use crate::i18n::tr;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, RichText, Ui};
use glam::{Mat4, Vec3};
use subcraft_app::assets::{EnvironmentSource, Library};
use subcraft_app::thumbnails::PreviewScene;
use subcraft_core::{AssetId, ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Mask, MaskEffect};
use subcraft_render::{SHADED_CHANNELS, ViewParams, ViewportRenderer};

use crate::state::UiState;
use crate::theme;

/// A shelf item being dragged onto the viewport or the layer stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfDrag {
    Material(usize),
    SmartMaterial(usize),
    SmartMask(usize),
    /// An image from Shelf → Textures (drop on a channel row to use it there).
    Texture(usize),
    Starter(usize),
    /// A brush alpha (Shelf → Brushes): onto a mask it becomes the mask shape.
    Alpha(usize),
}

/// Apply a material / smart material (inserted above the selected layer of the selected
/// texture set) or a smart mask (on the selected layer). Shared by clicks and drops.
pub fn apply_shelf_item(s: &mut UiState, item: ShelfDrag, now: f64) {
    apply_shelf_item_with(s, item, false, now);
}

/// What dropping `item` will do, for the drop-target highlight. `target` names the layer under
/// the cursor; `on_mask` is true over a mask thumbnail.
pub fn drop_label(item: ShelfDrag, alt: bool, target: Option<&str>, on_mask: bool) -> String {
    let t = target.map_or(String::new(), |n| format!(" \"{n}\""));
    match item {
        ShelfDrag::Material(_) if alt => format!("Replace the material of{t}"),
        ShelfDrag::Material(_) => format!("Add material layer above{t}"),
        ShelfDrag::SmartMaterial(_) if alt => "Smart materials are added as a folder: release Alt".to_string(),
        ShelfDrag::SmartMaterial(_) => format!("Add smart material above{t}"),
        ShelfDrag::SmartMask(_) => format!("Apply mask to{t}"),
        ShelfDrag::Texture(_) | ShelfDrag::Starter(_) if alt => format!("Use this image in the active channel of{t}"),
        ShelfDrag::Texture(_) | ShelfDrag::Starter(_) => format!("Add texture layer above{t}"),
        ShelfDrag::Alpha(_) if on_mask || target.is_some() => format!("Use this alpha in the mask of{t}"),
        ShelfDrag::Alpha(_) => "Set as brush tip".to_string(),
    }
}

/// `apply_shelf_item` with the Alt reading (Substance-Painter style): Alt applies the item
/// *in place* to the selected layer instead of creating a layer.
/// * Material: replaces the selected fill layer's channels (name, blending and mask are kept).
/// * Texture: sets the active channel of the selected fill layer.
/// * Alpha: becomes an image effect of the selected layer's mask (adds the mask); without Alt it
///   is the brush tip.
/// * Smart material: no in-place form; says so instead of doing nothing.
/// * Smart mask: always applies to the selected layer.
pub fn apply_shelf_item_with(s: &mut UiState, item: ShelfDrag, alt: bool, now: f64) {
    let layer = s.session.selection().layer;
    let selected_fill = s.session.selected_layer().is_some_and(|l| matches!(l.content, LayerContent::Fill(_)));
    let result = match item {
        ShelfDrag::Starter(i) => {s.asset_preview.queue(&s.session,i,None);Ok(())},
        ShelfDrag::Material(i) if alt => match (s.library.materials.get(i).cloned(), layer) {
            (Some(m), Some(l)) if selected_fill => s.session.apply_material_to_layer(l, &m).map_err(|e| e.to_string()),
            (Some(_), _) => Err("Alt+click replaces a fill layer's material: select a fill layer first (a click without Alt adds a new layer)".to_string()),
            (None, _) => Ok(()),
        },
        ShelfDrag::Material(i) => match s.library.materials.get(i).cloned() {
            Some(m) => s.session.add_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMaterial(_) if alt => Err("A smart material is a folder of layers, so Alt cannot apply it in place: click without Alt to add it".to_string()),
        ShelfDrag::SmartMaterial(i) => match s.library.smart_materials.get(i).cloned() {
            Some(m) => s.session.add_smart_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMask(i) => match s.library.smart_masks.get(i).cloned() {
            Some(m) => s.session.apply_smart_mask(&m).map_err(|e| format!("{e} (select a layer first)")),
            None => Ok(()),
        },
        ShelfDrag::Texture(i) if alt => match (s.textures.items.get(i).map(|t| t.path.clone()), layer) {
            (Some(p), Some(l)) if selected_fill => {
                let channel = s.active_channel;
                s.session.set_fill_channel_image(l, channel, &p).map(|()| s.set_status(format!("Set {} from the image", channel.display_name()), now))
            }
            (Some(_), _) => Err("Alt+click puts the image in the active channel of the selected fill layer: select a fill layer first".to_string()),
            (None, _) => Ok(()),
        },
        ShelfDrag::Texture(i) => match s.textures.items.get(i).map(|t| t.path.clone()) {
            Some(p) => s.session.add_texture_layer(&p).map(|name| s.set_status(format!("Added \"{name}\" as a fill layer"), now)),
            None => Ok(()),
        },
        ShelfDrag::Alpha(i) => match s.library.alphas.get(i).cloned() {
            Some(a) if alt => match layer {
                Some(l) => s.session.apply_alpha_to_mask(l, &a).map(|()| s.set_status(format!("Using \"{}\" in the mask (Properties: projection)", a.name), now)).map_err(|e| e.to_string()),
                None => Err("Select a layer first: the alpha goes into its mask".to_string()),
            },
            Some(a) => { s.session.set_brush_alpha(&a); s.set_status(format!("Brush tip: {} (Alt+click or drag it onto a mask to use it there)", a.name), now); Ok(()) }
            None => Ok(()),
        },
    };
    match result {
        Ok(()) => s.recovery.autosaver.request_soon(),
        Err(e) => s.set_status(e, now),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfTab {
    Textures,
    /// Tangent-space normal maps (the starter pack's and the user's), apart from color textures.
    NormalMaps,
    Materials,
    SmartMaterials,
    SmartMasks,
    Brushes,
    Environments,
}

/// Size of the grayscale alpha thumbnails.
const THUMB: u32 = 96;
/// Size of the rendered material / environment thumbnails (the composite behind them is
/// `subcraft_app::thumbnails::THUMB_RESOLUTION` square; both are sized to stay sharp in the
/// largest shelf tile, 160 points, without costing a visible frame: see `generate_one_thumbnail`).
const RENDER_THUMB: u32 = 160;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SceneKind {
    Sphere,
    /// Sphere with baked mesh maps (smart materials and masks).
    BakedSphere,
}

pub struct ShelfUi {
    pub tab: ShelfTab,
    pub filter: crate::panels::shelf_filter::ShelfFilter,
    filter_tab: ShelfTab,
    thumbs: HashMap<AssetId, egui::TextureHandle>,
    sphere: Option<Arc<PreviewScene>>,
    baked_sphere: Option<Arc<PreviewScene>>,
    /// Thumbnail being composited on a worker thread (the composite is far longer than a frame).
    compositing: Option<Compositing>,
    renderer: Option<(ViewportRenderer, Option<SceneKind>)>,
    /// Thumbnail whose pixels are on their way back from the GPU.
    pending: Option<(AssetId, subcraft_render::PendingReadback)>,
    /// Thumbnails of the visible tab are still being rendered (scripted screenshots wait).
    pub busy: bool,
}

/// A thumbnail composite running off the UI thread. The worker also builds the preview scene on
/// first use (its mesh bakes take a good fraction of a second at thumbnail resolution).
struct Compositing {
    id: AssetId,
    kind: SceneKind,
    env_index: Option<usize>,
    rx: std::sync::mpsc::Receiver<(Arc<PreviewScene>, subcraft_compositor::Composite)>,
}

type LayerBuilder = Box<dyn Fn(&subcraft_doc::TextureSet) -> Layer + Send>;

impl Default for ShelfUi {
    fn default() -> Self {
        Self {
            tab: ShelfTab::SmartMaterials,
            filter: Default::default(),
            filter_tab: ShelfTab::SmartMaterials,
            thumbs: HashMap::new(),
            sphere: None,
            baked_sphere: None,
            compositing: None,
            renderer: None,
            pending: None,
            busy: false,
        }
    }
}

impl ShelfUi {
    /// Show `tab` with a search query (launch options; tab switches otherwise reset the search).
    pub fn open(&mut self, tab: ShelfTab, query: &str) {
        self.tab = tab;
        self.filter_tab = tab;
        self.filter = crate::panels::shelf_filter::ShelfFilter { query: query.into(), ..Default::default() };
    }
}

/// Viewport environment selection.
pub struct EnvUi {
    pub index: usize,
    pub rotation_deg: f32,
    pub intensity: f32,
    /// HDRI currently uploaded to the viewport renderer.
    pub uploaded: Option<AssetId>,
}

impl Default for EnvUi {
    fn default() -> Self {
        Self { index: 0, rotation_deg: 0.0, intensity: 1.0, uploaded: None }
    }
}

/// Configure `params` (and upload an HDRI when needed) for environment `index` of `library`.
pub fn apply_environment(
    library: &Library,
    index: usize,
    rotation_deg: f32,
    intensity: f32,
    params: &mut ViewParams,
    renderer: &mut ViewportRenderer,
    uploaded: &mut Option<AssetId>,
    device: &subcraft_render::wgpu_api::Device,
    queue: &subcraft_render::wgpu_api::Queue,
) {
    let Some(env) = library.environments.get(index) else { return };
    let rot = rotation_deg.to_radians();
    params.environment.rotation = rot;
    match &env.source {
        EnvironmentSource::Procedural(p) => {
            let e = &mut params.environment;
            e.use_hdri = false;
            e.ground = p.ground;
            e.horizon = p.horizon;
            e.zenith = p.zenith;
            e.irradiance_up = p.irradiance_up;
            e.irradiance_down = p.irradiance_down;
            e.panels = p.panels;
            e.top = p.top;
            e.key_softbox = p.key_softbox;
            params.light_intensity = p.light_intensity * intensity;
            params.light_color = p.light_color;
            params.environment_intensity = intensity;
        }
        EnvironmentSource::Hdri(_) | EnvironmentSource::Embedded(_) => {
            let Some(h) = env.source.hdri() else { return };
            if *uploaded != Some(env.id) {
                let levels: Vec<(u32, u32, &[f32])> = h.levels.iter().map(|l| (l.width, l.height, l.rgba.as_slice())).collect();
                renderer.set_hdri(device, queue, Some(subcraft_render::HdriUpload { levels: &levels, sh: h.sh }));
                *uploaded = Some(env.id);
            }
            params.environment.use_hdri = true;
            params.environment.hdri_intensity = intensity;
            params.environment_intensity = 1.0;
            // World direction of the sun: inverse of the shader's environment rotation.
            let s = Vec3::from_array(h.sun_direction);
            let (sn, c) = rot.sin_cos();
            params.light_dir = Vec3::new(c * s.x + sn * s.z, s.y, -sn * s.x + c * s.z);
            params.light_color = h.sun_color;
            params.light_intensity = 1.2 * intensity;
        }
    }
}

/// Thumbnail tile: square image with a hairline frame (accent when hovered / selected), name below.
pub(crate) fn tile(ui: &mut Ui, side: f32, thumb: Option<&egui::TextureHandle>, name: &str, selected: bool, tooltip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side + 22.0), egui::Sense::click_and_drag());
    if resp.dragged()
        && egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(ui.ctx())
        && let Some(p) = ui.ctx().pointer_latest_pos()
    {
        egui::Area::new(egui::Id::new("shelf-drag-ghost"))
            .order(egui::Order::Tooltip)
            .fixed_pos(p + egui::vec2(14.0, 10.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(format!("{}  {name}", crate::icons::PLUS)).color(theme::TEXT));
                    ui.label(RichText::new(tr("Drop on the model or a layer")).small().color(theme::TEXT_FAINT));
                });
            });
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
    let painter = ui.painter();
    painter.rect_filled(img, 0.0, theme::FIELD);
    match thumb {
        Some(t) => {
            painter.image(t.id(), img.shrink(1.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let icon = ui.data(|d| d.get_temp::<&'static str>(egui::Id::new("shelf_fallback_icon"))).unwrap_or(crate::icons::IMAGE);
            painter.text(img.center(), egui::Align2::CENTER_CENTER, icon, theme::font(24.0), theme::TEXT_FAINT);
        }
    }
    let stroke = if selected {
        egui::Stroke::new(2.0, theme::ACCENT)
    } else if resp.hovered() {
        egui::Stroke::new(1.0, theme::ACCENT_HOVER)
    } else {
        egui::Stroke::new(1.0, theme::BORDER)
    };
    painter.rect_stroke(img, 0.0, stroke, egui::StrokeKind::Inside);
    let color = if selected || resp.hovered() { theme::TEXT } else { theme::TEXT_DIM };
    let mut job = egui::text::LayoutJob::simple(name.to_string(), theme::font(11.0), color, side);
    job.wrap.max_rows = 1;
    let galley = painter.layout_job(job);
    let text_pos = egui::pos2(rect.center().x - galley.size().x / 2.0, img.bottom() + 5.0);
    painter.galley(text_pos, galley, color);
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Right-click menu of an asset stored in the user library. Returns true if it was removed.
fn library_menu(
    resp: &egui::Response,
    lib: Option<&subcraft_app::assets::UserLibrary>,
    file: &std::path::Path,
    status: &mut Option<String>,
) -> bool {
    let mut removed = false;
    resp.context_menu(|ui| {
        if ui.button(format!("{}  Show in folder", crate::icons::FOLDER_OPEN)).clicked() {
            show_in_folder(file);
            ui.close();
        }
        if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("scsmart"))
            && ui.button(format!("{}  Export smart material…", crate::icons::EXPORT)).clicked()
        {
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("smart material");
            if let Some(dest) = rfd::FileDialog::new().add_filter("SubCraft smart material", &["scsmart"]).set_file_name(format!("{stem}.scsmart")).save_file() {
                *status = Some(match subcraft_app::smart_import::export_smart_file(file, stem, &dest) {
                    Ok(()) => format!("Exported smart material to {}", dest.display()),
                    Err(e) => format!("Export smart material: {e}"),
                });
            }
            ui.close();
        }
        if ui.button(format!("{}  Remove from library", crate::icons::TRASH)).clicked() {
            match lib.map(|l| l.remove(file)) {
                Some(Ok(())) => removed = true,
                Some(Err(e)) => *status = Some(e),
                None => {}
            }
            ui.close();
        }
    });
    removed
}

/// File > Import smart material(s)…: `.scsmart` files and/or PBR texture images.
pub fn import_smart_dialog(ctx: &egui::Context) {
    crate::files::pick(ctx, crate::files::Purpose::SmartMaterials, crate::files::SMART_MATERIALS, true, Some("Import smart materials (.scsmart) or PBR texture maps"));
}

/// File > Import smart material folder…: a folder of `.scsmart` files or of PBR texture sets.
pub fn import_smart_folder_dialog(ctx: &egui::Context) {
    crate::files::pick_folder(ctx, crate::files::Purpose::SmartMaterialFolder, "Import smart materials from a folder (.scsmart files or PBR texture sets)");
}

/// Files dropped on the shelf: smart materials (`.scsmart`, folders) on any tab, textures too on
/// the Smart Materials tab. Draws a hint while files hover over it.
fn handle_drops(state: &mut UiState, ui: &Ui, now: f64) {
    let rect = ui.max_rect();
    let over = ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p));
    if over && ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
        ui.painter().rect_stroke(rect.shrink(2.0), 4.0, egui::Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
    if !over || dropped.is_empty() {
        return;
    }
    let smart_tab = state.shelf.tab == ShelfTab::SmartMaterials;
    let paths: Vec<std::path::PathBuf> = dropped
        .into_iter()
        .filter(|p| smart_tab || p.is_dir() || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("scsmart")))
        .collect();
    if !paths.is_empty() {
        crate::files::import_smart_materials(state, &paths, now);
    }
}

/// Open the system file browser at `path` (selecting it when it is a file).
pub fn show_in_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let r = if path.is_dir() {
        std::process::Command::new("explorer").arg(path).spawn()
    } else {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    };
    #[cfg(not(windows))]
    let r = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" })
        .arg(if path.is_dir() { path } else { path.parent().unwrap_or(path) })
        .spawn();
    if let Err(e) = r {
        log::warn!("could not open {}: {e}", path.display());
    }
}

type ShelfAction = Box<dyn FnOnce(&mut UiState)>;

/// Brush tiles: a click activates the brush; the second click of a double-click opens its
/// Brush settings instead (the first click already selected it, so no duplicate notice).
fn brush_click(resp: &egui::Response, ctx: &egui::Context, select: impl FnOnce(&mut UiState) + 'static) -> Option<ShelfAction> {
    if resp.double_clicked() {
        let ctx = ctx.clone();
        Some(Box::new(move |s: &mut UiState| { s.open_brush_editor = true; ctx.request_repaint(); }))
    } else if resp.clicked() {
        let ctx = ctx.clone();
        Some(Box::new(move |s: &mut UiState| { select(s); ctx.request_repaint(); }))
    } else { None }
}

/// Thumbnail of a brush tip by name, once the Shelf has rendered it.
pub(crate) fn tip_thumbnail(state: &UiState, tip: &str) -> Option<egui::TextureId> {
    state.library.alphas.iter().find(|a| a.name == tip).and_then(|a| state.shelf.thumbs.get(&a.id)).map(|t| t.id())
}

/// Underlined text tab.
/// Icon of the Normal Maps tab. The icon font has no bumpy-surface glyph, so it is drawn: a
/// rounded tile with a wavy relief line, in the weight of the font icons.
const BUMPS: &str = "bumps-icon";

fn draw_bumps(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    painter.rect_stroke(egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)), 3.0, stroke, egui::StrokeKind::Inside);
    let wave: Vec<egui::Pos2> = (0..=10).map(|i| {
        let t = i as f32 / 10.0;
        egui::pos2(center.x - 5.0 + 10.0 * t, center.y + 2.0 * (t * std::f32::consts::TAU * 1.5).sin())
    }).collect();
    painter.add(egui::Shape::line(wave, stroke));
}

fn shelf_tab(ui: &mut Ui, current: &mut ShelfTab, value: ShelfTab, icon: &str, label: &str) {
    let selected = *current == value;
    let drawn = icon == BUMPS;
    let text = if drawn { format!("      {label}") } else { format!("{icon}  {label}") };
    let color = if selected { theme::TEXT } else { theme::TEXT_FAINT };
    let galley = ui.painter().layout_no_wrap(text, theme::font_strong(12.0), color);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 16.0, 34.0), egui::Sense::click());
    let color = if resp.hovered() && !selected { theme::TEXT_DIM } else { color };
    let text_pos = rect.center() - galley.size() / 2.0;
    if drawn { draw_bumps(ui.painter(), egui::pos2(text_pos.x + 7.0, rect.center().y), color); }
    ui.painter().galley_with_override_text_color(text_pos, galley, color);
    if selected {
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom()), 0.0, theme::ACCENT);
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *current = value;
    }
}

fn shelf_hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(crate::icons::INFO).color(theme::TEXT_FAINT)).on_hover_text(text);
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    handle_drops(state, ui, now);
    header(state, ui);
    env_controls(state, ui);
    filter_row(state, ui);
    generate_one_thumbnail(state, ui.ctx());
    let fallback = category_icon(state.shelf.tab);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("shelf_fallback_icon"), fallback));
    let mut action: Option<Box<dyn FnOnce(&mut UiState)>> = None;
    let mut lib_status: Option<String> = None;
    let filter = state.shelf.filter.clone();
    let mut shown = 0usize;
    // Alt at the moment of the click: apply in place to the selected layer instead of adding one.
    let alt = ui.input(|i| i.modifiers.alt);
    let grid_width = ui.available_width();
    let tile_size = (state.recovery.prefs.shelf_preview_size.clamp(48, 160) as f32).min((grid_width - 2.0 * f32::from(theme::PAD)).max(1.0));
    let grid_height = (ui.available_height() - 44.0).max(0.0);
    ui.allocate_ui_with_layout(egui::vec2(grid_width, grid_height), egui::Layout::top_down(egui::Align::Min), |ui| {
    egui::ScrollArea::vertical().id_salt("shelf").auto_shrink([false, false]).min_scrolled_height(0.0).max_height(grid_height).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(theme::PAD)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.set_max_width((grid_width - 2.0 * f32::from(theme::PAD)).max(0.0));
            // Brushes: the built-in and user presets come first, the CC0 starter tips after them.
            if state.shelf.tab != ShelfTab::Brushes { shown += crate::asset_preview::shelf(state,ui,tile_size); }
            let thumbs = &state.shelf.thumbs;
            match state.shelf.tab {
                ShelfTab::Textures | ShelfTab::NormalMaps => {
                    let normals = state.shelf.tab == ShelfTab::NormalMaps;
                    for (i, t) in state.textures.items.iter().enumerate() {
                        if (t.category == "Normal") != normals || !filter.matches(&t.name, Some(t.category), true) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, t.thumb.as_ref(), &t.name, false, &format!("{} ({}): drag onto a channel, the model or a layer. Alt+click: use it in the active channel of the selected fill layer", t.name, t.category));
                        resp.dnd_set_drag_payload(ShelfDrag::Texture(i));
                        if library_menu(&resp, state.user_library.as_ref(), &t.path, &mut lib_status) {
                            let path = t.path.clone();
                            action = Some(Box::new(move |s: &mut UiState| s.textures.remove(&path)));
                        }
                        if resp.clicked() && alt {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::Texture(i), true, now)));
                        } else if resp.clicked() {
                            let name=t.name.clone();let path=t.path.clone();let ctx=ui.ctx().clone();
                            let kind=if t.category=="Normal" {subcraft_assets::starter::Kind::Normal} else {subcraft_assets::starter::Kind::Texture};
                            action = Some(Box::new(move |s: &mut UiState| s.asset_preview.start(&ctx,name,kind,crate::asset_preview::Source::File(path),true)));
                        }
                    }
                }
                ShelfTab::Materials => {
                    for (i, m) in state.library.materials.iter().enumerate() {
                        if !filter.matches(&m.name, Some(m.category), false) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{} ({}): click or drag onto the model / a layer (adds a layer). Alt+click: replace the selected fill layer's material", m.name, m.category));
                        resp.dnd_set_drag_payload(ShelfDrag::Material(i));
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::Material(i), alt, now)));
                        }
                    }
                }
                ShelfTab::SmartMaterials => {
                    for (i, m) in state.library.smart_materials.iter().enumerate() {
                        let info = subcraft_app::assets::smart_material_info(&m.name);
                        if !filter.matches(&m.name, info.map(|i| i.category), m.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let tip = match info {
                            Some(i) => format!("{} ({}): {}. Click or drag onto the model / a layer", m.name, i.category, i.description),
                            None => format!("{}: click or drag onto the model / a layer", m.name),
                        };
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
                        resp.dnd_set_drag_payload(ShelfDrag::SmartMaterial(i));
                        if let Some(file) = &m.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.smart_materials.remove(i);
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::SmartMaterial(i), alt, now)));
                        }
                    }
                }
                ShelfTab::SmartMasks => {
                    for (i, m) in state.library.smart_masks.iter().enumerate() {
                        let info = subcraft_app::assets::smart_mask_info(&m.name);
                        if !filter.matches(&m.name, info.map(|i| i.category), m.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let tip = match info {
                            Some(i) => format!("{} ({}): {}. Click to mask the selected layer, or drag onto a layer", m.name, i.category, i.description),
                            None => format!("{}: click to mask the selected layer, or drag onto a layer", m.name),
                        };
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
                        resp.dnd_set_drag_payload(ShelfDrag::SmartMask(i));
                        if let Some(file) = &m.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.smart_masks.remove(i);
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item(s, ShelfDrag::SmartMask(i), now)));
                        }
                    }
                }
                ShelfTab::Brushes => {
                    for (i, p) in state.brush_presets.iter().enumerate() {
                        if !filter.matches(&p.name, Some("preset"), true) {
                            continue;
                        }
                        shown += 1;
                        let tip_id = state.library.alphas.iter().find(|a| Some(&a.name) == p.tip.as_ref() || (p.tip.is_none() && a.bitmap.is_none())).map(|a| a.id);
                        let thumb = tip_id.and_then(|id| thumbs.get(&id));
                        let tip = match subcraft_app::brush_builtin::describe(&p.name).filter(|_| p.user_file.is_none()) {
                            Some(d) => format!("{}: {d}", p.name),
                            None => "Brush preset: tip, size, flow, jitter and material (right-click for options)".to_string(),
                        };
                        let resp = tile(ui, tile_size, thumb, &p.name, false, &tip);
                        if let Some(file) = &p.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.brush_presets.remove(i);
                            }));
                        }
                        if let Some(a) = brush_click(&resp, ui.ctx(), move |s: &mut UiState| {
                            let p = s.brush_presets[i].clone();
                            s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&p), now);
                        }) {
                            action = Some(a);
                        }
                    }
                    let current = state.session.brush.alpha_name.clone();
                    for (i, a) in state.library.alphas.iter().enumerate() {
                        if a.name.starts_with("Starter: ") {continue;}
                        if !filter.matches(&a.name, None, a.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let tip = if a.user_file.is_some() { "Brush tip (your library: right-click for options). Drag onto a layer's mask, or Alt+click, to use it as the mask shape" } else { "Brush tip. Drag onto a layer's mask, or Alt+click, to use it as the mask shape" };
                        let resp = tile(ui, tile_size, thumbs.get(&a.id), &a.name, a.name == current, tip);
                        if a.bitmap.is_some() { resp.dnd_set_drag_payload(ShelfDrag::Alpha(i)); }
                        if let Some(file) = &a.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.alphas.remove(i);
                            }));
                        }
                        if alt && resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::Alpha(i), true, now)));
                        } else if let Some(a) = brush_click(&resp, ui.ctx(), move |s: &mut UiState| {
                            let a = s.library.alphas[i].clone();
                            s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&a), now);
                        }) {
                            action = Some(a);
                        }
                    }
                }
                ShelfTab::Environments => {
                    for (i, e) in state.library.environments.iter().enumerate() {
                        if !filter.matches(&e.name, None, e.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let tip = if e.user_file.is_some() { "Viewport lighting (your library: right-click for options)" } else { "Viewport lighting" };
                        let resp = tile(ui, tile_size, thumbs.get(&e.id), &e.name, i == state.env.index, tip);
                        if let Some(file) = &e.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.environments.remove(i);
                                if s.env.index >= i {
                                    s.env.index = s.env.index.saturating_sub(1);
                                }
                                s.env.uploaded = None;
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| s.env.index = i));
                        }
                    }
                }
            }
            if state.shelf.tab == ShelfTab::Brushes { shown += crate::asset_preview::shelf(state,ui,tile_size); }
            if shown == 0 {
                let msg = if filter.is_active() { "No matches. Clear the search or pick another filter." } else { "Nothing here yet." };
                ui.label(RichText::new(msg).color(theme::TEXT_FAINT));
            }
        }));
    });
    });
    bottom_toolbar(state, ui);
    if let Some(a) = action {
        a(state);
    }
    if let Some(msg) = lib_status {
        state.set_status(msg, now);
    }
    if state.shelf.tab != ShelfTab::Brushes || state.shelf.thumbs.len() < state.library.alphas.len() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
}

/// Painter's category icon row, in its order. The shelf has no filter, generator or text assets
/// (those are made from the layer menus), so those categories are not listed.
const CATEGORIES: [(ShelfTab, &str, &str); 7] = [
    (ShelfTab::Materials, crate::icons::DROP, "Materials"),
    (ShelfTab::SmartMaterials, crate::icons::SPARKLE, "Smart Materials"),
    (ShelfTab::SmartMasks, crate::icons::MASK, "Smart Masks"),
    (ShelfTab::Brushes, crate::icons::PAINT_BRUSH, "Brushes"),
    (ShelfTab::Textures, crate::icons::IMAGE, "Textures"),
    (ShelfTab::NormalMaps, BUMPS, "Normal Maps"),
    (ShelfTab::Environments, crate::icons::SUN, "Environments"),
];

fn category_icon(tab: ShelfTab) -> &'static str {
    match tab {
        ShelfTab::NormalMaps => crate::icons::GRID,
        other => CATEGORIES.iter().find(|c| c.0 == other).map_or(crate::icons::IMAGE, |c| c.1),
    }
}

fn category_name(tab: ShelfTab) -> &'static str {
    match tab {
        ShelfTab::Materials => tr("Materials"),
        ShelfTab::SmartMaterials => tr("Smart Materials"),
        ShelfTab::SmartMasks => tr("Smart Masks"),
        ShelfTab::Brushes => tr("Brushes"),
        ShelfTab::Textures => tr("Textures"),
        ShelfTab::NormalMaps => tr("Normal Maps"),
        ShelfTab::Environments => tr("Environments"),
    }
}

/// Breadcrumb, search box and the row of category icons (always icons, never a dropdown).
fn header(state: &mut UiState, ui: &mut Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::icons::CARET_RIGHT).font(theme::font(11.0)).color(theme::TEXT_DIM));
        ui.label(RichText::new(tr("All libraries")).font(theme::font_strong(12.0)).color(theme::TEXT));
        ui.label(RichText::new("/").color(theme::TEXT_FAINT));
        ui.label(RichText::new(category_name(state.shelf.tab)).font(theme::font(12.0)).color(theme::TEXT_DIM));
    });
    ui.add_space(4.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            let stroke = egui::Stroke::new(1.6, theme::TEXT_DIM);
            ui.painter().circle_stroke(r.center() - egui::vec2(1.5, 1.5), 5.0, stroke);
            ui.painter().line_segment([r.center() + egui::vec2(2.0, 2.0), r.center() + egui::vec2(6.5, 6.5)], stroke);
            let filter = &mut state.shelf.filter;
            ui.add(egui::TextEdit::singleline(&mut filter.query).hint_text(tr("Search…")).desired_width(ui.available_width() - 28.0));
            if !filter.query.is_empty() && crate::widgets::icon_button(ui, crate::icons::X, "Clear search").clicked() { filter.query.clear(); }
        });
    });
    ui.add_space(4.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (tab, icon, _) in CATEGORIES {
                let selected = state.shelf.tab == tab;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 26.0), egui::Sense::click());
                let painter = ui.painter();
                if selected { painter.rect_filled(rect, 2.0, theme::BG_4); } else if resp.hovered() { painter.rect_filled(rect, 2.0, theme::BG_3); }
                let color = if selected { theme::TEXT } else { theme::TEXT_DIM };
                if icon == BUMPS { draw_bumps(painter, rect.center(), color); }
                else { painter.text(rect.center(), egui::Align2::CENTER_CENTER, icon, theme::font(16.0), color); }
                if resp.on_hover_text(category_name(tab)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { state.shelf.tab = tab; }
            }
        });
    });
    ui.add_space(4.0);
}

/// Rotation / intensity of the viewport environment (only on the Environments category).
fn env_controls(state: &mut UiState, ui: &mut Ui) {
    if state.shelf.tab != ShelfTab::Environments { return; }
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        let width = ui.available_width();
        for (label, value, range) in [("Rotation", &mut state.env.rotation_deg, -180.0..=180.0), ("Intensity", &mut state.env.intensity, 0.1..=4.0)] {
            ui.allocate_ui_with_layout(egui::vec2(width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_sized([60.0, 18.0], egui::Label::new(RichText::new(label).color(theme::TEXT_DIM)));
                ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(20.0);
                let slider = egui::Slider::new(value, range);
                ui.add(if label == "Rotation" { slider.suffix("°") } else { slider.logarithmic(true) });
            });
        }
    });
}

/// Bottom icon toolbar: the category's import / library-folder actions on the left, the
/// preview-size slider on the right.
fn bottom_toolbar(state: &mut UiState, ui: &mut Ui) {
    use crate::widgets::icon_button;
    crate::widgets::divider(ui);
    type Pick = (crate::files::Purpose, (&'static str, &'static [&'static str]), bool);
    let mut pick: Option<Pick> = None;
    let mut folder: Option<subcraft_app::assets::UserAssetKind> = None;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            match state.shelf.tab {
                ShelfTab::Textures | ShelfTab::NormalMaps => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import textures…").clicked() { pick = Some((crate::files::Purpose::ShelfTextures, crate::files::IMAGES, true)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Texture);
                }
                ShelfTab::Materials => {
                    if icon_button(ui, crate::icons::DOWNLOAD, tr("Select a texture's maps (color, roughness, normal…) to make one fill layer.")).clicked() { crate::panels::layers::import_texture_set_dialog(ui.ctx()); }
                }
                ShelfTab::Brushes => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import alpha…").clicked() { pick = Some((crate::files::Purpose::Alpha, crate::files::ALPHAS, false)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Alpha);
                }
                ShelfTab::Environments => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import HDRI…").clicked() { pick = Some((crate::files::Purpose::Hdri, crate::files::PANORAMAS, false)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Environment);
                }
                ShelfTab::SmartMaterials => {
                    if icon_button(ui, crate::icons::DOWNLOAD, tr("Import smart materials (.scsmart) or PBR texture sets (Color, NormalGL, Roughness…)")).clicked() { import_smart_dialog(ui.ctx()); }
                    if icon_button(ui, crate::icons::FOLDER_OPEN, tr("Import a folder of .scsmart files or PBR texture sets")).clicked() { import_smart_folder_dialog(ui.ctx()); }
                }
                ShelfTab::SmartMasks => {}
            }
            if let Some(kind) = folder && let Some(lib) = &state.user_library {
                let dir = lib.folder(kind);
                if icon_button(ui, crate::icons::FOLDER, &format!("Library folder: {}\nFiles here appear in the shelf at startup.", dir.display())).clicked() {
                    if let Err(e) = std::fs::create_dir_all(&dir) { log::warn!("could not create {}: {e}", dir.display()); }
                    show_in_folder(&dir);
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Preview size").font(theme::font(10.5)).color(theme::TEXT_FAINT));
            let v = &mut state.recovery.prefs.shelf_preview_size;
            *v = (*v).clamp(48, 160);
            ui.spacing_mut().slider_width = (ui.available_width() - 12.0).clamp(24.0, 90.0);
            ui.add(egui::Slider::new(v, 48..=160).show_value(false)).on_hover_text("Preview size in UI points; texture resolution stays unchanged");
        });
    });
    if let Some((purpose, filter, multi)) = pick { crate::files::pick(ui.ctx(), purpose, filter, multi, None); }
}

/// Grayscale bitmap → egui image.
fn alpha_image(b: &subcraft_image::Bitmap) -> egui::ColorImage {
    let n = THUMB as usize;
    let mut px = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let t = b.sample((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32, subcraft_image::Wrap::Clamp, false);
            let v = (t[0] * t[3]).clamp(0.0, 1.0);
            let g = (40.0 + v * 215.0) as u8;
            px.push(Color32::from_gray(g));
        }
    }
    egui::ColorImage { size: [n, n], pixels: px, source_size: egui::vec2(n as f32, n as f32) }
}

/// Render one missing thumbnail of the visible tab per frame (keeps the UI responsive).
fn generate_one_thumbnail(state: &mut UiState, ctx: &egui::Context) {
    // Finish the thumbnail in flight first (the GPU copy is never waited on).
    state.shelf.busy = state.shelf.pending.is_some() || state.shelf.compositing.is_some();
    if let Some((id, pending)) = &state.shelf.pending {
        let Some(rs) = state.render_state.clone() else { return };
        match pending.try_take(&rs.device) {
            None => {
                ctx.request_repaint();
                return;
            }
            Some(bytes) => {
                if let Some(bytes) = bytes {
                    let img = egui::ColorImage::from_rgba_unmultiplied([RENDER_THUMB as usize, RENDER_THUMB as usize], &bytes);
                    let tex = ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR);
                    state.shelf.thumbs.insert(*id, tex);
                }
                state.shelf.pending = None;
            }
        }
    }
    // A composite finished on the worker: draw it. At most one step of work per frame.
    if state.shelf.compositing.is_some() {
        let Some(rs) = state.render_state.clone() else { return };
        let message = state.shelf.compositing.as_ref().map(|c| c.rx.try_recv());
        match message {
            Some(Ok((scene, composite))) => {
                if let Some(done) = state.shelf.compositing.take() {
                    render_composited(state, ctx, &rs, done, scene, composite);
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(8)),
            _ => {
                // The worker died without a result: show an empty tile instead of retrying forever.
                if let Some(done) = state.shelf.compositing.take() {
                    log::warn!("shelf thumbnail worker failed for {}", done.id);
                    let blank = egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]);
                    state.shelf.thumbs.insert(done.id, ctx.load_texture(format!("thumb-{}", done.id), blank, egui::TextureOptions::LINEAR));
                }
            }
        }
        return;
    }
    let lib = &state.library;
    let missing = |id: &AssetId| !state.shelf.thumbs.contains_key(id);
    enum Job {
        Alpha(usize),
        /// The layer builder and the images the layer uses.
        Layer(AssetId, SceneKind, LayerBuilder, Vec<std::sync::Arc<subcraft_doc::Resource>>),
        Environment(usize),
    }
    let job = match state.shelf.tab {
        // Texture thumbnails come from their own background loader.
        ShelfTab::Textures | ShelfTab::NormalMaps => None,
        ShelfTab::Brushes => lib.alphas.iter().position(|a| missing(&a.id)).map(Job::Alpha),
        ShelfTab::Materials => lib.materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            let (ids, images) = subcraft_assets::presets::decode_images(&m.images);
            Job::Layer(m.id, SceneKind::Sphere, Box::new(move |ts| preview_layer(m.build_with(ts, &ids))), images)
        }),
        ShelfTab::SmartMaterials => lib.smart_materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            let (ids, decoded) = subcraft_assets::presets::decode_images(m.images());
            let images = m.resources().iter().cloned().chain(decoded).collect();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| preview_layer(m.build_with(ts, &ids))), images)
        }),
        ShelfTab::SmartMasks => lib.smart_masks.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| mask_preview_layer(ts, &m.effects)), Vec::new())
        }),
        ShelfTab::Environments => lib.environments.iter().position(|e| missing(&e.id)).map(Job::Environment),
    };
    state.shelf.busy |= job.is_some();
    let Some(job) = job else { return };
    if let Job::Alpha(i) = job {
        let a = &state.library.alphas[i];
        let img = match &a.bitmap {
            Some(b) => alpha_image(b),
            None => alpha_image(&subcraft_image::Bitmap::from_fn_gray(64, 64, |u, v| {
                let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
                subcraft_paint::falloff(d, 0.5)
            })),
        };
        let tex = ctx.load_texture(format!("thumb-{}", a.id), img, egui::TextureOptions::LINEAR);
        state.shelf.thumbs.insert(a.id, tex);
        return;
    }
    if state.render_state.is_none() { return; }
    let (id, kind, layer_fn, images, env_index) = match job {
        Job::Layer(id, k, f, images) => (id, k, Some(f), images, None),
        Job::Environment(i) => (state.library.environments[i].id, SceneKind::Sphere, None, Vec::new(), Some(i)),
        Job::Alpha(_) => unreachable!(),
    };
    // Composite on a worker thread: at thumbnail resolution it takes 10-90 ms, several frames.
    let existing = match kind {
        SceneKind::Sphere => state.shelf.sphere.clone(),
        SceneKind::BakedSphere => state.shelf.baked_sphere.clone(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let scene = existing.unwrap_or_else(|| Arc::new(match kind {
            SceneKind::Sphere => PreviewScene::sphere(),
            SceneKind::BakedSphere => PreviewScene::baked_sphere(),
        }));
        let composite = match &layer_fn {
            Some(f) => scene.composite_with(f(scene.texture_set()), &images),
            None => {
                // Environment preview: a glossy metal sphere.
                let ts = scene.texture_set();
                let mut l = Layer::new_fill("Chrome", ts);
                if let LayerContent::Fill(f) = &mut l.content {
                    let set = |k| FillChannel::new(FillSource::uniform(k));
                    f.channels.insert(ChannelKind::BaseColor, set(ChannelValue::rgb(0.9, 0.9, 0.9)));
                    f.channels.insert(ChannelKind::Metallic, set(ChannelValue::scalar(1.0)));
                    f.channels.insert(ChannelKind::Roughness, set(ChannelValue::scalar(0.12)));
                }
                scene.composite(l)
            }
        };
        let _ = tx.send((scene, composite));
    });
    state.shelf.compositing = Some(Compositing { id, kind, env_index, rx });
    state.shelf.busy = true;
    ctx.request_repaint_after(std::time::Duration::from_millis(8));
}

/// Second half of a thumbnail: the composite is ready, draw it (the GPU copy is never waited on).
fn render_composited(state: &mut UiState, ctx: &egui::Context, rs: &subcraft_render::egui_wgpu::RenderState, done: Compositing, scene: Arc<PreviewScene>, composite: subcraft_compositor::Composite) {
    let Compositing { id, kind, env_index, .. } = done;
    match kind {
        SceneKind::Sphere => state.shelf.sphere = Some(scene.clone()),
        SceneKind::BakedSphere => state.shelf.baked_sphere = Some(scene.clone()),
    }
    let mesh = scene.mesh.clone();
    let (renderer, current) = state.shelf.renderer.get_or_insert_with(|| (ViewportRenderer::new(&rs.device, &rs.queue), None));
    if *current != Some(kind) {
        renderer.set_mesh(&rs.device, &mesh);
        *current = Some(kind);
    }
    for k in SHADED_CHANNELS {
        if let Some(img) = composite.channel(k) {
            renderer.upload_channel(&rs.device, &rs.queue, 0, k, img, None);
        }
    }
    let eye = Vec3::new(0.0, 0.3, 3.4);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(38f32.to_radians(), 1.0, 0.1, 50.0));
    params.background = [0.035, 0.036, 0.04];
    if let Some(i) = env_index {
        let mut uploaded = None;
        apply_environment(&state.library, i, 0.0, 1.0, &mut params, renderer, &mut uploaded, &rs.device, &rs.queue);
    }
    let pending = renderer.render(&rs.device, &rs.queue, [RENDER_THUMB, RENDER_THUMB], &params).read_rgba8_async(&rs.device, &rs.queue);
    state.shelf.pending = Some((id, pending));
    ctx.request_repaint();
}

/// A material as the shelf thumbnail shows it: same layer, tiling boosted for the small sphere
/// (the saved asset is not changed).
fn preview_layer(mut layer: Layer) -> Layer {
    subcraft_app::thumbnails::zoom_out_tiling(&mut layer);
    layer
}

/// White paint shown through a smart mask (for mask thumbnails).
fn mask_preview_layer(ts: &subcraft_doc::TextureSet, effects: &[subcraft_doc::MaskEffectKind]) -> Layer {
    let mut l = Layer::new_fill("Mask preview", ts);
    if let LayerContent::Fill(f) = &mut l.content {
        for c in f.channels.values_mut() {
            c.enabled = false;
        }
        f.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.95, 0.55, 0.2))));
    }
    let mut mask = Mask::new(ts.resolution, false);
    mask.effects = effects.iter().cloned().map(|k| Arc::new(MaskEffect::new(k))).collect();
    l.mask = Some(mask);
    l
}

/// "Open library folder" (creates it on first use so the user can drop files in).
fn library_folder_button(ui: &mut Ui, state: &UiState, kind: subcraft_app::assets::UserAssetKind) {
    let Some(lib) = &state.user_library else { return };
    let dir = lib.folder(kind);
    if ui
        .button(format!("{}  Library folder", crate::icons::FOLDER_OPEN))
        .on_hover_text(format!("{}\nFiles here appear in the shelf at startup.", dir.display()))
        .clicked()
    {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("could not create {}: {e}", dir.display());
        }
        show_in_folder(&dir);
    }
}

/// Search box + category chips for the visible tab (chips reset when the tab changes).
fn filter_row(state: &mut UiState, ui: &mut Ui) {
    use crate::panels::shelf_filter::{Chip, categories};
    if state.shelf.filter_tab != state.shelf.tab {
        state.shelf.filter_tab = state.shelf.tab;
        state.shelf.filter.chip = Chip::All;
    }
    let mut chips = vec![(Chip::All, "All".to_string()), (Chip::Mine, "Mine".to_string())];
    if state.shelf.tab == ShelfTab::Materials {
        for c in categories(state.library.materials.iter().map(|m| m.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if state.shelf.tab == ShelfTab::SmartMaterials {
        for c in categories(state.library.smart_materials.iter().filter_map(|m| subcraft_app::assets::smart_material_info(&m.name)).map(|i| i.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if state.shelf.tab == ShelfTab::SmartMasks {
        for c in categories(state.library.smart_masks.iter().filter_map(|m| subcraft_app::assets::smart_mask_info(&m.name)).map(|i| i.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if matches!(state.shelf.tab, ShelfTab::Textures | ShelfTab::NormalMaps) {
        chips.push((Chip::Tileable,"Tileable".into()));
        let normals = state.shelf.tab == ShelfTab::NormalMaps;
        for c in categories(state.textures.items.iter().filter(|t| (t.category == "Normal") == normals).map(|t| t.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if matches!(state.shelf.tab,ShelfTab::NormalMaps|ShelfTab::Brushes) {chips.push((Chip::Category("Starter pack".into()),"Starter pack".into()));}
    egui::Frame::new().inner_margin(egui::Margin { left: theme::PAD, right: theme::PAD, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let filter = &mut state.shelf.filter;
            ui.spacing_mut().item_spacing.x = 4.0;
            for (chip, label) in chips {
                let selected = filter.chip == chip;
                let text = RichText::new(label).font(theme::font(11.5)).color(if selected { theme::ACCENT } else { theme::TEXT_DIM });
                let b = egui::Button::new(text).fill(if selected { theme::ACCENT_SOFT } else { theme::BG_3 }).stroke(egui::Stroke::NONE);
                if ui.add(b).clicked() {
                    filter.chip = if selected { Chip::All } else { chip };
                }
            }
        });
    });
}
