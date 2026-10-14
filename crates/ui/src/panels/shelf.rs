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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfTab {
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
    Cube,
}

pub struct ShelfUi {
    pub tab: ShelfTab,
    thumbs: HashMap<AssetId, egui::TextureHandle>,
    sphere: Option<PreviewScene>,
    cube: Option<PreviewScene>,
    renderer: Option<(ViewportRenderer, Option<SceneKind>)>,
}

impl Default for ShelfUi {
    fn default() -> Self {
        Self { tab: ShelfTab::SmartMaterials, thumbs: HashMap::new(), sphere: None, cube: None, renderer: None }
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
        EnvironmentSource::Hdri(h) => {
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

const TILE: f32 = 88.0;

/// Thumbnail tile: square image with a hairline frame (accent when hovered / selected), name below.
fn tile(ui: &mut Ui, thumb: Option<&egui::TextureHandle>, name: &str, selected: bool, tooltip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE, TILE + 22.0), egui::Sense::click());
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(TILE, TILE));
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
    let galley = painter.layout(name.to_string(), theme::font(11.0), color, TILE);
    let text_pos = egui::pos2(rect.center().x - galley.size().x / 2.0, img.bottom() + 5.0);
    painter.galley(text_pos, galley, color);
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
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

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    egui::Frame::new().inner_margin(egui::Margin { left: 6, right: theme::PAD, top: 0, bottom: 0 }).show(ui, |ui| {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let tab = &mut state.shelf.tab;
        shelf_tab(ui, tab, ShelfTab::Materials, crate::icons::DROP, "Materials");
        shelf_tab(ui, tab, ShelfTab::SmartMaterials, crate::icons::SPARKLE, "Smart Materials");
        shelf_tab(ui, tab, ShelfTab::SmartMasks, crate::icons::MASK, "Smart Masks");
        shelf_tab(ui, tab, ShelfTab::Brushes, crate::icons::PAINT_BRUSH, "Brushes");
        shelf_tab(ui, tab, ShelfTab::Environments, crate::icons::SUN, "Environments");
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(16.0);
        match state.shelf.tab {
            ShelfTab::Materials => {
                if ui.button(format!("{}  Import texture set…", crate::icons::DOWNLOAD)).clicked() {
                    crate::panels::layers::import_texture_set_dialog(state, now);
                }
                ui.label(RichText::new("Select a texture's maps (color, roughness, normal…) to make one fill layer.").small().color(theme::TEXT_FAINT));
            }
            ShelfTab::Brushes => {
                if ui.button(format!("{}  Import alpha…", crate::icons::DOWNLOAD)).clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"]).pick_file()
                {
                    match subcraft_app::assets::alphas::import(&p) {
                        Ok(a) => {
                            state.session.set_brush_alpha(&a);
                            state.library.alphas.push(a);
                        }
                        Err(e) => state.set_status(format!("Import alpha: {e}"), now),
                    }
                }
            }
            ShelfTab::Environments => {
                if ui.button(format!("{}  Import HDRI…", crate::icons::DOWNLOAD)).clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("Panoramas", &["hdr", "exr", "png", "jpg", "jpeg"]).pick_file()
                {
                    match subcraft_app::assets::environment::import_hdri(&p) {
                        Ok(e) => {
                            state.library.environments.push(e);
                            state.env.index = state.library.environments.len() - 1;
                        }
                        Err(e) => state.set_status(format!("Import HDRI: {e}"), now),
                    }
                }
                ui.spacing_mut().slider_width = 110.0;
                ui.label(RichText::new("Rotation").color(theme::TEXT_DIM));
                ui.add(egui::Slider::new(&mut state.env.rotation_deg, -180.0..=180.0).suffix("°"));
                ui.label(RichText::new("Intensity").color(theme::TEXT_DIM));
                ui.add(egui::Slider::new(&mut state.env.intensity, 0.1..=4.0).logarithmic(true));
            }
            ShelfTab::SmartMasks => {
                ui.label(RichText::new("Applies to the selected layer. Edges / Cavity need baked maps (Ctrl+B).").small().color(theme::TEXT_FAINT));
            }
            ShelfTab::SmartMaterials => {
                ui.label(RichText::new("Wear and dirt follow baked curvature / AO (Ctrl+B).").small().color(theme::TEXT_FAINT));
            }
        }
    });
    });
    crate::widgets::divider(ui);
    generate_one_thumbnail(state, ui.ctx());
    let mut action: Option<Box<dyn FnOnce(&mut UiState)>> = None;
    egui::ScrollArea::vertical().id_salt("shelf").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(theme::PAD)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            let thumbs = &state.shelf.thumbs;
            match state.shelf.tab {
                ShelfTab::Materials => {
                    for (i, m) in state.library.materials.iter().enumerate() {
                        if tile(ui, thumbs.get(&m.id), &m.name, false, &format!("{} ({}): add as a fill layer", m.name, m.category)) {
                            action = Some(Box::new(move |s: &mut UiState| {
                                let m = s.library.materials[i].clone();
                                if let Err(e) = s.session.add_material(&m) {
                                    s.set_status(e.to_string(), now);
                                }
                            }));
                        }
                    }
                }
                ShelfTab::SmartMaterials => {
                    for (i, m) in state.library.smart_materials.iter().enumerate() {
                        if tile(ui, thumbs.get(&m.id), &m.name, false, &format!("{}: add as a folder of masked layers", m.name)) {
                            action = Some(Box::new(move |s: &mut UiState| {
                                let m = s.library.smart_materials[i].clone();
                                if let Err(e) = s.session.add_smart_material(&m) {
                                    s.set_status(e.to_string(), now);
                                }
                            }));
                        }
                    }
                }
                ShelfTab::SmartMasks => {
                    for (i, m) in state.library.smart_masks.iter().enumerate() {
                        if tile(ui, thumbs.get(&m.id), &m.name, false, &format!("{}: mask the selected layer", m.name)) {
                            action = Some(Box::new(move |s: &mut UiState| {
                                let m = s.library.smart_masks[i].clone();
                                if let Err(e) = s.session.apply_smart_mask(&m) {
                                    s.set_status(format!("{e} (select a layer first)"), now);
                                }
                            }));
                        }
                    }
                }
                ShelfTab::Brushes => {
                    let current = state.session.brush.alpha_name.clone();
                    for (i, a) in state.library.alphas.iter().enumerate() {
                        if tile(ui, thumbs.get(&a.id), &a.name, a.name == current, "Brush tip") {
                            action = Some(Box::new(move |s: &mut UiState| {
                                let a = s.library.alphas[i].clone();
                                s.session.set_brush_alpha(&a);
                                if s.tool == crate::state::Tool::PolygonFill {
                                    s.tool = crate::state::Tool::Brush;
                                }
                            }));
                        }
                    }
                }
                ShelfTab::Environments => {
                    for (i, e) in state.library.environments.iter().enumerate() {
                        if tile(ui, thumbs.get(&e.id), &e.name, i == state.env.index, "Viewport lighting") {
                            action = Some(Box::new(move |s: &mut UiState| s.env.index = i));
                        }
                    }
                }
            }
        }));
    });
    if let Some(a) = action {
        a(state);
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
    let lib = &state.library;
    let missing = |id: &AssetId| !state.shelf.thumbs.contains_key(id);
    enum Job {
        Alpha(usize),
        Layer(AssetId, SceneKind, Box<dyn Fn(&subcraft_doc::TextureSet) -> Layer>),
        Environment(usize),
    }
    let job = match state.shelf.tab {
        ShelfTab::Brushes => lib.alphas.iter().position(|a| missing(&a.id)).map(Job::Alpha),
        ShelfTab::Materials => lib.materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::Sphere, Box::new(move |ts| m.build(ts)))
        }),
        ShelfTab::SmartMaterials => lib.smart_materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::Cube, Box::new(move |ts| m.build(ts)))
        }),
        ShelfTab::SmartMasks => lib.smart_masks.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::Cube, Box::new(move |ts| mask_preview_layer(ts, &m.effects)))
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
    let (id, kind, layer_fn, env_index) = match job {
        Job::Layer(id, k, f) => (id, k, Some(f), None),
        Job::Environment(i) => (state.library.environments[i].id, SceneKind::Sphere, None, Some(i)),
        Job::Alpha(_) => unreachable!(),
    };
    let scene = match kind {
        SceneKind::Sphere => state.shelf.sphere.get_or_insert_with(PreviewScene::sphere),
        SceneKind::Cube => state.shelf.cube.get_or_insert_with(PreviewScene::cube),
    };
    let composite = match &layer_fn {
        Some(f) => scene.composite(f(scene.texture_set())),
        None => {
            // Environment preview: a glossy metal sphere.
            let ts = scene.texture_set();
            let mut l = Layer::new_fill("Chrome", ts);
            if let LayerContent::Fill(f) = &mut l.content {
                let set = |k| FillChannel { enabled: true, source: FillSource::uniform(k) };
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
    let eye = match kind {
        SceneKind::Sphere => Vec3::new(0.0, 0.3, 3.4),
        SceneKind::Cube => Vec3::new(2.2, 1.9, 2.6),
    };
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(38f32.to_radians(), 1.0, 0.1, 50.0));
    params.background = [0.035, 0.036, 0.04];
    if let Some(i) = env_index {
        let mut uploaded = None;
        apply_environment(&state.library, i, 0.0, 1.0, &mut params, renderer, &mut uploaded, &rs.device, &rs.queue);
    }
    let bytes = renderer.render(&rs.device, &rs.queue, [THUMB, THUMB], &params).read_rgba8(&rs.device, &rs.queue);
    let img = egui::ColorImage::from_rgba_unmultiplied([THUMB as usize, THUMB as usize], &bytes);
    let tex = ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR);
    state.shelf.thumbs.insert(id, tex);
}

/// White paint shown through a smart mask (for mask thumbnails).
fn mask_preview_layer(ts: &subcraft_doc::TextureSet, effects: &[subcraft_doc::MaskEffectKind]) -> Layer {
    let mut l = Layer::new_fill("Mask preview", ts);
    if let LayerContent::Fill(f) = &mut l.content {
        for c in f.channels.values_mut() {
            c.enabled = false;
        }
        f.channels.insert(ChannelKind::BaseColor, FillChannel { enabled: true, source: FillSource::uniform(ChannelValue::rgb(0.95, 0.55, 0.2)) });
    }
    let mut mask = Mask::new(ts.resolution, false);
    mask.effects = effects.iter().cloned().map(|k| Arc::new(MaskEffect::new(k))).collect();
    l.mask = Some(mask);
    l
}
