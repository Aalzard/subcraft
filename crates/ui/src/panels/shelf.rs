//! Shelf: materials, smart materials, smart masks, brush alphas and environments, with rendered
//! thumbnails. Clicking applies the asset through the session (undoable where it edits layers).

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
    /// User library `.sbsar` (Materials tab).
    Sbsar(usize),
    SmartMaterial(usize),
    SmartMask(usize),
    /// An image from Shelf → Textures (drop on a channel row to use it there).
    Texture(usize),
    Starter(usize),
}

pub(crate) fn shelf_thumb_ready(state: &UiState, id: &AssetId) -> bool {
    state.shelf.thumbs.contains_key(id)
}

/// Apply a material / smart material (inserted above the selected layer of the selected
/// texture set) or a smart mask (on the selected layer). Shared by clicks and drops.
pub fn apply_shelf_item(s: &mut UiState, item: ShelfDrag, now: f64) {
    let result = match item {
        ShelfDrag::Starter(i) => {s.asset_preview.queue(&s.session,i,None);Ok(())},
        ShelfDrag::Material(i) => match s.library.materials.get(i).cloned() {
            Some(m) => s.session.add_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::Sbsar(i) => crate::panels::sbsar_materials::apply_shelf(s, i, now),
        ShelfDrag::SmartMaterial(i) => match s.library.smart_materials.get(i).cloned() {
            Some(m) => s.session.add_smart_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMask(i) => match s.library.smart_masks.get(i).cloned() {
            Some(m) => s.session.apply_smart_mask(&m).map_err(|e| format!("{e} (select a layer first)")),
            None => Ok(()),
        },
        ShelfDrag::Texture(i) => match s.textures.items.get(i).map(|t| t.path.clone()) {
            Some(p) => s.session.add_texture_layer(&p).map(|name| s.set_status(format!("Added \"{name}\" as a fill layer"), now)),
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
    Materials,
    SmartMaterials,
    SmartMasks,
    Brushes,
    Environments,
}

const THUMB: u32 = 96;

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
    sphere: Option<PreviewScene>,
    baked_sphere: Option<PreviewScene>,
    renderer: Option<(ViewportRenderer, Option<SceneKind>)>,
    /// Thumbnail whose pixels are on their way back from the GPU.
    pending: Option<(AssetId, subcraft_render::PendingReadback)>,
}

impl Default for ShelfUi {
    fn default() -> Self {
        Self {
            tab: ShelfTab::SmartMaterials,
            filter: Default::default(),
            filter_tab: ShelfTab::SmartMaterials,
            thumbs: HashMap::new(),
            sphere: None,
            baked_sphere: None,
            renderer: None,
            pending: None,
        }
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
                    ui.label(RichText::new("Drop on the model or a layer").small().color(theme::TEXT_FAINT));
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
            painter.text(img.center(), egui::Align2::CENTER_CENTER, crate::icons::IMAGE, theme::font(20.0), theme::TEXT_FAINT);
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
pub(crate) fn library_menu(
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
fn shelf_tab(ui: &mut Ui, current: &mut ShelfTab, value: ShelfTab, icon: &str, label: &str) {
    let selected = *current == value;
    let text = format!("{icon}  {label}");
    let color = if selected { theme::TEXT } else { theme::TEXT_FAINT };
    let galley = ui.painter().layout_no_wrap(text, theme::font_strong(12.0), color);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 16.0, 34.0), egui::Sense::click());
    let color = if resp.hovered() && !selected { theme::TEXT_DIM } else { color };
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, color);
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
    egui::Frame::new().inner_margin(egui::Margin { left: 6, right: theme::PAD, top: 0, bottom: 0 }).show(ui, |ui| {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let narrow = ui.available_width() < 720.0;
        let tab = &mut state.shelf.tab;
        if narrow {
            let label = match tab { ShelfTab::Textures => "Textures", ShelfTab::Materials => "Materials", ShelfTab::SmartMaterials => "Smart Materials", ShelfTab::SmartMasks => "Smart Masks", ShelfTab::Brushes => "Brushes", ShelfTab::Environments => "Environments" };
            egui::ComboBox::from_id_salt("shelf_category").selected_text(label).width(144.0).show_ui(ui, |ui| {
                for (kind, name) in [(ShelfTab::Textures,"Textures"),(ShelfTab::Materials,"Materials"),(ShelfTab::SmartMaterials,"Smart Materials"),(ShelfTab::SmartMasks,"Smart Masks"),(ShelfTab::Brushes,"Brushes"),(ShelfTab::Environments,"Environments")] {
                    ui.selectable_value(tab, kind, name);
                }
            });
        } else {
        shelf_tab(ui, tab, ShelfTab::Textures, crate::icons::IMAGE, "Textures");
        shelf_tab(ui, tab, ShelfTab::Materials, crate::icons::DROP, "Materials");
        shelf_tab(ui, tab, ShelfTab::SmartMaterials, crate::icons::SPARKLE, "Smart Materials");
        shelf_tab(ui, tab, ShelfTab::SmartMasks, crate::icons::MASK, "Smart Masks");
        shelf_tab(ui, tab, ShelfTab::Brushes, crate::icons::PAINT_BRUSH, "Brushes");
        shelf_tab(ui, tab, ShelfTab::Environments, crate::icons::SUN, "Environments");
        }
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(16.0);
        match state.shelf.tab {
            ShelfTab::Textures => {
                if ui.button(format!("{}  Import textures…", crate::icons::DOWNLOAD)).clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::ShelfTextures, crate::files::IMAGES, true, None);
                }
                library_folder_button(ui, state, subcraft_app::assets::UserAssetKind::Texture);
                shelf_hint(ui, "Imported maps become one material. Drag a texture onto a channel in Properties (Base Color, Normal…), or onto the model / a layer for a new fill layer.");
            }
            ShelfTab::Materials => {
                if ui.button(format!("{}  Import texture set…", crate::icons::DOWNLOAD)).clicked() {
                    crate::panels::layers::import_texture_set_dialog(ui.ctx());
                }
                shelf_hint(ui, "Select a texture's maps (color, roughness, normal…) to make one fill layer.");
            }
            ShelfTab::Brushes => {
                if ui.button(format!("{}  Import alpha…", crate::icons::DOWNLOAD)).clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::Alpha, crate::files::ALPHAS, false, None);
                }
                library_folder_button(ui, state, subcraft_app::assets::UserAssetKind::Alpha);
            }
            ShelfTab::Environments => {
                if ui.button(format!("{}  Import HDRI…", crate::icons::DOWNLOAD)).clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::Hdri, crate::files::PANORAMAS, false, None);
                }
                library_folder_button(ui, state, subcraft_app::assets::UserAssetKind::Environment);
                let width = ui.max_rect().width().min(300.0);
                for (label, value, range) in [("Rotation", &mut state.env.rotation_deg, -180.0..=180.0), ("Intensity", &mut state.env.intensity, 0.1..=4.0)] {
                    // Give compound sliders a known width before wrapping their parent row.
                    ui.allocate_ui_with_layout(egui::vec2(width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_sized([60.0, 18.0], egui::Label::new(RichText::new(label).color(theme::TEXT_DIM)));
                        ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(20.0);
                        let slider = egui::Slider::new(value, range);
                        ui.add(if label == "Rotation" { slider.suffix("°") } else { slider.logarithmic(true) });
                    });
                }
            }
            ShelfTab::SmartMasks => {
                shelf_hint(ui, "Applies to the selected layer. Edges / Cavity need baked maps (Ctrl+B).");
            }
            ShelfTab::SmartMaterials => {
                shelf_hint(ui, "Wear and dirt follow baked curvature / AO (Ctrl+B).");
            }
        }
    });
    });
    if state.shelf.tab == ShelfTab::Materials {
        crate::panels::sbsar_materials::materials_toolbar(ui, state, now);
    }
    filter_row(state, ui);
    crate::widgets::divider(ui);
    generate_one_thumbnail(state, ui.ctx());
    let mut action: Option<Box<dyn FnOnce(&mut UiState)>> = None;
    let mut lib_status: Option<String> = None;
    let filter = state.shelf.filter.clone();
    let mut shown = 0usize;
    let grid_width = ui.available_width();
    let tile_size = (state.recovery.prefs.shelf_preview_size.clamp(48, 160) as f32).min((grid_width - 2.0 * f32::from(theme::PAD)).max(1.0));
    let grid_height = (ui.available_height() - 44.0).max(0.0);
    ui.allocate_ui_with_layout(egui::vec2(grid_width, grid_height), egui::Layout::top_down(egui::Align::Min), |ui| {
    egui::ScrollArea::vertical().id_salt("shelf").auto_shrink([false, false]).min_scrolled_height(0.0).max_height(grid_height).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(theme::PAD)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.set_max_width((grid_width - 2.0 * f32::from(theme::PAD)).max(0.0));
            shown += crate::asset_preview::shelf(state,ui,tile_size);
            let thumbs = &state.shelf.thumbs;
            match state.shelf.tab {
                ShelfTab::Textures => {
                    for (i, t) in state.textures.items.iter().enumerate() {
                        if !filter.matches(&t.name, Some(t.category), true) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, t.thumb.as_ref(), &t.name, false, &format!("{} ({}): drag onto a channel, the model or a layer", t.name, t.category));
                        resp.dnd_set_drag_payload(ShelfDrag::Texture(i));
                        if library_menu(&resp, state.user_library.as_ref(), &t.path, &mut lib_status) {
                            let path = t.path.clone();
                            action = Some(Box::new(move |s: &mut UiState| s.textures.remove(&path)));
                        }
                        if resp.clicked() {
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
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{} ({}): click or drag onto the model / a layer", m.name, m.category));
                        resp.dnd_set_drag_payload(ShelfDrag::Material(i));
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item(s, ShelfDrag::Material(i), now)));
                        }
                    }
                    shown += crate::panels::sbsar_materials::paint_sbsar_grid(
                        ui,
                        thumbs,
                        state,
                        &filter,
                        now,
                        tile_size,
                        &mut action,
                        &mut lib_status,
                    );
                }
                ShelfTab::SmartMaterials => {
                    for (i, m) in state.library.smart_materials.iter().enumerate() {
                        if !filter.matches(&m.name, None, m.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{}: click or drag onto the model / a layer", m.name));
                        resp.dnd_set_drag_payload(ShelfDrag::SmartMaterial(i));
                        if let Some(file) = &m.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.smart_materials.remove(i);
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item(s, ShelfDrag::SmartMaterial(i), now)));
                        }
                    }
                }
                ShelfTab::SmartMasks => {
                    for (i, m) in state.library.smart_masks.iter().enumerate() {
                        if !filter.matches(&m.name, None, m.user_file.is_some()) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{}: click to mask the selected layer, or drag onto a layer", m.name));
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
                        let resp = tile(ui, tile_size, thumb, &p.name, false, "Brush preset: tip, size, flow, jitter and material (right-click for options)");
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
                        let tip = if a.user_file.is_some() { "Brush tip (your library: right-click for options)" } else { "Brush tip" };
                        let resp = tile(ui, tile_size, thumbs.get(&a.id), &a.name, a.name == current, tip);
                        if let Some(file) = &a.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.alphas.remove(i);
                            }));
                        }
                        if let Some(a) = brush_click(&resp, ui.ctx(), move |s: &mut UiState| {
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
            if shown == 0 {
                let msg = if filter.is_active() { "No matches. Clear the search or pick another filter." } else { "Nothing here yet." };
                ui.label(RichText::new(msg).color(theme::TEXT_FAINT));
            }
        }));
    });
    });
    crate::widgets::preview_size_footer(ui, &mut state.recovery.prefs.shelf_preview_size, 48..=160);
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
    if let Some((id, pending)) = &state.shelf.pending {
        let Some(rs) = state.render_state.clone() else { return };
        match pending.try_take(&rs.device) {
            None => {
                ctx.request_repaint();
                return;
            }
            Some(bytes) => {
                if let Some(bytes) = bytes {
                    let img = egui::ColorImage::from_rgba_unmultiplied([THUMB as usize, THUMB as usize], &bytes);
                    let tex = ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR);
                    state.shelf.thumbs.insert(*id, tex);
                }
                state.shelf.pending = None;
            }
        }
    }
    let lib = &state.library;
    let missing = |id: &AssetId| !state.shelf.thumbs.contains_key(id);
    enum Job {
        Alpha(usize),
        /// The layer builder and the images the layer uses.
        Layer(AssetId, SceneKind, Box<dyn Fn(&subcraft_doc::TextureSet) -> Layer>, Vec<std::sync::Arc<subcraft_doc::Resource>>),
        Environment(usize),
    }
    let job = match state.shelf.tab {
        // Texture thumbnails come from their own background loader.
        ShelfTab::Textures => None,
        ShelfTab::Brushes => lib.alphas.iter().position(|a| missing(&a.id)).map(Job::Alpha),
        ShelfTab::Materials => lib.materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::Sphere, Box::new(move |ts| m.build(ts)), Vec::new())
        }),
        ShelfTab::SmartMaterials => lib.smart_materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            let images = m.resources().to_vec();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| m.build(ts)), images)
        }),
        ShelfTab::SmartMasks => lib.smart_masks.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| mask_preview_layer(ts, &m.effects)), Vec::new())
        }),
        ShelfTab::Environments => lib.environments.iter().position(|e| missing(&e.id)).map(Job::Environment),
    };
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
    let Some(rs) = state.render_state.clone() else { return };
    let (id, kind, layer_fn, images, env_index) = match job {
        Job::Layer(id, k, f, images) => (id, k, Some(f), images, None),
        Job::Environment(i) => (state.library.environments[i].id, SceneKind::Sphere, None, Vec::new(), Some(i)),
        Job::Alpha(_) => unreachable!(),
    };
    let scene = match kind {
        SceneKind::Sphere => state.shelf.sphere.get_or_insert_with(PreviewScene::sphere),
        SceneKind::BakedSphere => state.shelf.baked_sphere.get_or_insert_with(PreviewScene::baked_sphere),
    };
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
    let pending = renderer.render(&rs.device, &rs.queue, [THUMB, THUMB], &params).read_rgba8_async(&rs.device, &rs.queue);
    state.shelf.pending = Some((id, pending));
    ctx.request_repaint();
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
pub(crate) fn library_folder_button(ui: &mut Ui, state: &UiState, kind: subcraft_app::assets::UserAssetKind) {
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
    if state.shelf.tab == ShelfTab::Textures {
        chips.push((Chip::Tileable,"Tileable".into()));
        for c in categories(state.textures.items.iter().map(|t| t.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if matches!(state.shelf.tab,ShelfTab::Textures|ShelfTab::Brushes) {chips.push((Chip::Category("Starter pack".into()),"Starter pack".into()));}
    egui::Frame::new().inner_margin(egui::Margin { left: theme::PAD, right: theme::PAD, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let filter = &mut state.shelf.filter;
            ui.add(egui::TextEdit::singleline(&mut filter.query).hint_text("Search…").desired_width(ui.available_width().min(180.0)));
            if !filter.query.is_empty() && crate::widgets::icon_button(ui, crate::icons::X, "Clear search").clicked() {
                filter.query.clear();
            }
            ui.add_space(8.0);
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
