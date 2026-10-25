//! Substance `.sbsar` files in the shelf Materials tab (`Library/sbsar/`).
//!
//! Shelf UI for SBSAR lives here so upstream merges touch `shelf.rs` minimally.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use egui::{RichText, Ui};
use subcraft_app::assets::{UserAssetKind, UserLibrary};
use subcraft_core::AssetId;

use crate::panels::shelf::{library_folder_button, library_menu, tile, ShelfDrag};
use crate::panels::shelf_filter::{categories, ShelfFilter};
use crate::state::UiState;
use crate::theme;

pub const CATEGORY: &str = "Substance Material";
pub const SMART_MATERIAL_CATEGORY: &str = "Substance Smart Material";

#[derive(Clone)]
pub struct SbsarMaterialItem {
    pub id: AssetId,
    pub name: String,
    pub path: PathBuf,
    /// Baked preview maps (background); sphere thumb is rendered in `shelf::generate_one_thumbnail`.
    pub preview_map_paths: Option<Vec<PathBuf>>,
}

type PreviewResult = (PathBuf, Result<Vec<PathBuf>, String>);

#[derive(Default)]
pub struct SbsarMaterialLibrary {
    pub items: Vec<SbsarMaterialItem>,
    loaders: Vec<Receiver<PreviewResult>>,
}

fn preview_cache_dir(lib_sbsar_folder: Option<PathBuf>, sbsar: &Path) -> PathBuf {
    let stem = sbsar
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("material");
    match lib_sbsar_folder {
        Some(root) => root.join(".preview").join(stem),
        None => std::env::temp_dir().join("subcraft-sbsar-preview").join(stem),
    }
}

impl SbsarMaterialLibrary {
    pub fn load(lib: &UserLibrary) -> Self {
        let mut s = Self::default();
        let files = lib.list(UserAssetKind::Sbsar);
        s.push_items(Some(lib), &files);
        s
    }

    fn push_items(&mut self, lib: Option<&UserLibrary>, paths: &[PathBuf]) {
        let mut new = Vec::new();
        for p in paths {
            if let Some(existing) = self.items.iter().find(|i| i.path == *p) {
                if existing.preview_map_paths.is_none() {
                    new.push(p.clone());
                }
                continue;
            }
            let file = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            self.items.push(SbsarMaterialItem {
                id: AssetId::from_name(&format!("sbsar/user/{file}")),
                name: p.file_stem().and_then(|s| s.to_str()).unwrap_or(file).to_string(),
                path: p.clone(),
                preview_map_paths: None,
            });
            new.push(p.clone());
        }
        if new.is_empty() {
            return;
        }
        let lib_folder = lib.map(|l| l.folder(UserAssetKind::Sbsar));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for p in new {
                let cache = preview_cache_dir(lib_folder.clone(), &p);
                let result = subcraft_app::ensure_preview_bake(&p, &cache);
                if tx.send((p, result)).is_err() {
                    break;
                }
            }
        });
        self.loaders.push(rx);
    }

    pub fn add_files(&mut self, lib: Option<&UserLibrary>, paths: &[PathBuf]) -> Vec<String> {
        let mut errors = Vec::new();
        let mut stored = Vec::new();
        for p in paths {
            match lib {
                Some(lib) => match lib.add_file(UserAssetKind::Sbsar, p) {
                    Ok(dest) => stored.push(dest),
                    Err(e) => errors.push(e),
                },
                None => stored.push(p.clone()),
            }
        }
        self.push_items(lib, &stored);
        errors
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|i| i.path != path);
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut done = Vec::new();
        for (k, rx) in self.loaders.iter().enumerate() {
            loop {
                match rx.try_recv() {
                    Ok((path, Ok(paths))) => {
                        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
                            item.preview_map_paths = Some(paths);
                        }
                    }
                    Ok((path, Err(e))) => log::warn!("SBSAR preview bake {}: {e}", path.display()),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        done.push(k);
                        break;
                    }
                }
            }
        }
        for k in done.into_iter().rev() {
            self.loaders.remove(k);
        }
        if !self.loaders.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// Next SBSAR item that needs a GPU sphere thumbnail (for `shelf::generate_one_thumbnail`).
pub fn next_sphere_thumb_job(state: &UiState) -> Option<(AssetId, Vec<PathBuf>)> {
    state
        .sbsar_materials
        .items
        .iter()
        .find(|m| m.preview_map_paths.is_some() && !crate::panels::shelf::shelf_thumb_ready(state, &m.id))
        .map(|m| (m.id, m.preview_map_paths.clone().unwrap()))
}

/// Apply a library `.sbsar` from the shelf (bake + fill layer).
pub fn apply_shelf(state: &mut UiState, index: usize, now: f64) -> Result<(), String> {
    let path = state
        .sbsar_materials
        .items
        .get(index)
        .map(|m| m.path.clone())
        .ok_or_else(|| "SBSAR material not found".to_string())?;
    let size = state.sbsar_bake_size;
    match state.session.import_sbsar_at(&path, size) {
        Ok(r) => {
            state.set_status(r.summary(), now);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Toolbar row under Materials tabs (import texture set + SBSAR + library folder).
pub fn materials_toolbar(ui: &mut Ui, state: &mut UiState, now: f64) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: theme::PAD,
            right: theme::PAD,
            top: 2,
            bottom: 4,
        })
        .show(ui, |ui| {
            let narrow = ui.available_width() < 420.0;
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(ui.available_width());
                ui.spacing_mut().item_spacing.x = 6.0;
                if narrow {
                    ui.menu_button(format!("{} Import", crate::icons::DOWNLOAD), |ui| {
                        if ui.button("Texture set…").clicked() {
                            crate::panels::layers::import_texture_set_dialog(ui.ctx());
                            ui.close();
                        }
                        if ui.button("SBSAR…").clicked() {
                            crate::panels::layers::import_sbsar_to_library_dialog(state, now);
                            ui.close();
                        }
                    });
                } else {
                    if ui.button(format!("{}  Import texture set…", crate::icons::DOWNLOAD)).clicked() {
                        crate::panels::layers::import_texture_set_dialog(ui.ctx());
                    }
                    if ui.button(format!("{}  Import SBSAR…", crate::icons::SPARKLE)).clicked() {
                        crate::panels::layers::import_sbsar_to_library_dialog(state, now);
                    }
                }
                library_folder_button(ui, state, UserAssetKind::Sbsar);
                ui.label(RichText::new("Apply").small().color(theme::TEXT_DIM));
                let size = state.sbsar_bake_size;
                egui::ComboBox::from_id_salt("sbsar_apply_size")
                    .selected_text(format!("{size}²"))
                    .width(72.0)
                    .show_ui(ui, |ui| {
                        for s in [512_u32, 1024, 2048, 4096, 8192] {
                            if ui.selectable_label(size == s, format!("{s}²")).clicked() {
                                state.sbsar_bake_size = s;
                            }
                        }
                    })
                    .response
                    .on_hover_text("Bake resolution when applying an SBSAR (preview stays 512²)");
            });
        });
}

/// Extra filter chip labels for Shelf → Materials (includes built-in preset categories).
pub fn materials_filter_categories<'a>(preset_categories: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    categories(
        preset_categories
            .into_iter()
            .chain([CATEGORY, SMART_MATERIAL_CATEGORY]),
    )
}

/// Paint SBSAR and Substance Smart Material tiles in the Materials grid. Returns how many items were shown.
pub fn paint_sbsar_grid(
    ui: &mut Ui,
    thumbs: &HashMap<AssetId, egui::TextureHandle>,
    state: &UiState,
    filter: &ShelfFilter,
    now: f64,
    tile_size: f32,
    action: &mut Option<Box<dyn FnOnce(&mut UiState)>>,
    lib_status: &mut Option<String>,
) -> usize {
    let mut shown = 0usize;
    for (i, m) in state.sbsar_materials.items.iter().enumerate() {
        if !filter.matches(&m.name, Some(CATEGORY), true) {
            continue;
        }
        shown += 1;
        let tip = format!(
            "{} ({}): click or drag onto the model / a layer (bakes at {}² on apply)",
            m.name,
            CATEGORY,
            state.sbsar_bake_size
        );
        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
        resp.dnd_set_drag_payload(ShelfDrag::Sbsar(i));
        if library_menu(&resp, state.user_library.as_ref(), &m.path, lib_status) {
            let path = m.path.clone();
            *action = Some(Box::new(move |s: &mut UiState| s.sbsar_materials.remove(&path)));
        }
        if resp.clicked() {
            *action = Some(Box::new(move |s: &mut UiState| {
                if let Err(e) = apply_shelf(s, i, now) {
                    s.set_status(e, now);
                }
            }));
        }
    }
    for (i, m) in state.library.smart_materials.iter().enumerate() {
        // User-library smart materials only (built-ins stay on the Smart Materials tab).
        if m.user_file.is_none() {
            continue;
        }
        if !filter.matches(&m.name, Some(SMART_MATERIAL_CATEGORY), true) {
            continue;
        }
        shown += 1;
        let tip = format!(
            "{} ({}): click or drag onto the model / a layer",
            m.name, SMART_MATERIAL_CATEGORY
        );
        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
        resp.dnd_set_drag_payload(ShelfDrag::SmartMaterial(i));
        if let Some(file) = &m.user_file
            && library_menu(&resp, state.user_library.as_ref(), file, lib_status)
        {
            *action = Some(Box::new(move |s: &mut UiState| {
                s.library.smart_materials.remove(i);
            }));
        }
        if resp.clicked() {
            *action = Some(Box::new(move |s: &mut UiState| {
                if let Some(sm) = s.library.smart_materials.get(i).cloned() {
                    if let Err(e) = s.session.add_smart_material(&sm) {
                        s.set_status(e.to_string(), now);
                    }
                }
            }));
        }
    }
    shown
}

fn parse_override_floats(override_str: Option<&String>, default_val_str: &str) -> Vec<f32> {
    let src = override_str.map(|s| s.as_str()).unwrap_or(default_val_str);
    let parsed: Vec<f32> = src
        .split(',')
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .collect();
    if parsed.is_empty() {
        vec![0.0]
    } else {
        parsed
    }
}

fn render_sbsar_input_row(
    ui: &mut Ui,
    inp: &subcraft_app::SbsarInput,
    overrides: &mut std::collections::BTreeMap<String, String>,
) -> bool {
    let mut changed = false;
    let vals = parse_override_floats(overrides.get(&inp.identifier), &inp.value);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&inp.label)
                .font(theme::font(11.5))
                .color(theme::TEXT_DIM),
        )
        .on_hover_text(&inp.identifier);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let is_overridden = overrides.contains_key(&inp.identifier);
            if is_overridden
                && ui
                    .small_button("↺")
                    .on_hover_text("Reset parameter to default")
                    .clicked()
            {
                overrides.remove(&inp.identifier);
                changed = true;
                return;
            }

            if (inp.kind == "float3" || inp.kind == "float4")
                && (inp.widget == "color" || inp.identifier.contains("color"))
            {
                let r = vals.first().copied().unwrap_or(0.5).clamp(0.0, 1.0);
                let g = vals.get(1).copied().unwrap_or(0.5).clamp(0.0, 1.0);
                let b = vals.get(2).copied().unwrap_or(0.5).clamp(0.0, 1.0);
                let mut srgb = [
                    (r * 255.0).round() as u8,
                    (g * 255.0).round() as u8,
                    (b * 255.0).round() as u8,
                ];
                if ui.color_edit_button_srgb(&mut srgb).changed() {
                    let nr = srgb[0] as f32 / 255.0;
                    let ng = srgb[1] as f32 / 255.0;
                    let nb = srgb[2] as f32 / 255.0;
                    if inp.kind == "float4" {
                        let a = vals.get(3).copied().unwrap_or(1.0);
                        overrides.insert(
                            inp.identifier.clone(),
                            format!("{nr:.4},{ng:.4},{nb:.4},{a:.4}"),
                        );
                    } else {
                        overrides.insert(
                            inp.identifier.clone(),
                            format!("{nr:.4},{ng:.4},{nb:.4}"),
                        );
                    }
                    changed = true;
                }
            } else if inp.kind == "float" {
                let mut v = vals.first().copied().unwrap_or(0.0);
                let (min, max) = match (inp.min, inp.max) {
                    (Some(a), Some(b)) if b > a => (a, b),
                    _ => (0.0, 1.0),
                };
                let resp = ui.add(
                    egui::Slider::new(&mut v, min..=max)
                        .max_decimals(3)
                        .clamping(egui::SliderClamping::Always),
                );
                if resp.changed() {
                    overrides.insert(inp.identifier.clone(), format!("{v:.4}"));
                    changed = true;
                }
            } else if inp.kind.starts_with("int") {
                let mut v = vals.first().copied().unwrap_or(0.0).round() as i32;
                if inp.widget == "toggle" {
                    let mut on = v != 0;
                    if ui.checkbox(&mut on, "").changed() {
                        overrides.insert(
                            inp.identifier.clone(),
                            if on { "1".into() } else { "0".into() },
                        );
                        changed = true;
                    }
                } else if !inp.enum_values.is_empty() {
                    let current_label = inp
                        .enum_values
                        .iter()
                        .find(|ev| ev.value == v)
                        .map(|ev| ev.label.as_str())
                        .unwrap_or("Select");
                    egui::ComboBox::from_id_salt(("sbsar_enum", &inp.identifier))
                        .selected_text(current_label)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for ev in &inp.enum_values {
                                if ui.selectable_label(v == ev.value, &ev.label).clicked() {
                                    overrides
                                        .insert(inp.identifier.clone(), ev.value.to_string());
                                    changed = true;
                                }
                            }
                        });
                } else {
                    let (min, max) = match (inp.min, inp.max) {
                        (Some(a), Some(b)) if b > a => (a as i32, b as i32),
                        _ => (0, 16),
                    };
                    if ui.add(egui::Slider::new(&mut v, min..=max)).changed() {
                        overrides.insert(inp.identifier.clone(), v.to_string());
                        changed = true;
                    }
                }
            } else if inp.kind == "float2" {
                let mut x = vals.first().copied().unwrap_or(0.0);
                let mut y = vals.get(1).copied().unwrap_or(0.0);
                let r1 = ui.add(egui::DragValue::new(&mut y).speed(0.01).prefix("Y: "));
                let r0 = ui.add(egui::DragValue::new(&mut x).speed(0.01).prefix("X: "));
                if r0.changed() || r1.changed() {
                    overrides.insert(inp.identifier.clone(), format!("{x:.4},{y:.4}"));
                    changed = true;
                }
            }
        });
    });

    changed
}

/// Interactive Substance `.sbsar` parameter editor shown in the Properties panel for imported
/// SBSAR fill layers.
pub fn sbsar_layer_properties_ui(
    ui: &mut Ui,
    state: &mut UiState,
    layer_id: subcraft_core::LayerId,
    now: f64,
) {
    let Some(mut params) = state.session.sbsar_layers.get(&layer_id).cloned() else {
        return;
    };
    if params.inputs.is_empty() {
        return;
    }

    let mut any_changed = false;
    let mut reset_all = false;
    let mut next_size = params.size_px;
    let has_overrides = !params.overrides.is_empty();

    crate::widgets::section_with(
        ui,
        "sbsar_params",
        "Substance Parameters (SBSAR)",
        true,
        |ui| {
            egui::ComboBox::from_id_salt(("sbsar_layer_size", layer_id))
                .selected_text(format!("{next_size}²"))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for s in [512_u32, 1024, 2048, 4096] {
                        if ui.selectable_label(next_size == s, format!("{s}²")).clicked() {
                            next_size = s;
                        }
                    }
                });
            if has_overrides
                && ui
                    .small_button("Reset")
                    .on_hover_text("Reset all Substance parameters to graph defaults")
                    .clicked()
            {
                reset_all = true;
            }
        },
        |ui| {
            let is_technical = |inp: &subcraft_app::SbsarInput| {
                inp.group.eq_ignore_ascii_case("Channels")
                    || inp.group.eq_ignore_ascii_case("Technical parameters")
                    || inp.identifier.starts_with("output_")
                    || inp.identifier == "normal_format"
            };

            let primary: Vec<subcraft_app::SbsarInput> = params
                .inputs
                .iter()
                .filter(|i| !is_technical(i))
                .cloned()
                .collect();
            let technical: Vec<subcraft_app::SbsarInput> = params
                .inputs
                .iter()
                .filter(|i| {
                    inp_is_adjustable_technical(i)
                })
                .cloned()
                .collect();

            if primary.is_empty() && technical.is_empty() {
                crate::widgets::hint(ui, "This Substance archive exposes no adjustable parameters.");
                return;
            }

            let mut current_group = String::new();
            for inp in &primary {
                if !inp.group.is_empty() && inp.group != current_group {
                    current_group = inp.group.clone();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&current_group)
                            .font(theme::font_strong(11.0))
                            .color(theme::ACCENT),
                    );
                }
                if render_sbsar_input_row(ui, inp, &mut params.overrides) {
                    any_changed = true;
                }
            }

            if !technical.is_empty() {
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    RichText::new("Technical Adjustments")
                        .font(theme::font(11.5))
                        .color(theme::TEXT_DIM),
                )
                .id_salt(("sbsar_tech", layer_id))
                .default_open(primary.is_empty())
                .show(ui, |ui| {
                    for inp in &technical {
                        if render_sbsar_input_row(ui, inp, &mut params.overrides) {
                            any_changed = true;
                        }
                    }
                });
            }
        },
    );

    if next_size != params.size_px {
        params.size_px = next_size;
        any_changed = true;
    }

    if reset_all {
        params.overrides.clear();
        params.pending_rebake = true;
    } else if any_changed {
        params.pending_rebake = true;
    }

    let pointer_down = ui.input(|i| i.pointer.any_down());
    let should_rebake = params.pending_rebake && !pointer_down;
    if should_rebake {
        params.pending_rebake = false;
    }

    state.session.sbsar_layers.insert(layer_id, params);

    if should_rebake {
        if let Err(e) = state.session.rebake_sbsar_layer(layer_id) {
            state.set_status(format!("SBSAR rebake failed: {e}"), now);
        }
    }
}

fn inp_is_adjustable_technical(inp: &subcraft_app::SbsarInput) -> bool {
    if inp.group.eq_ignore_ascii_case("Channels") || inp.identifier.starts_with("output_") || inp.identifier == "normal_format" {
        return false;
    }
    inp.group.eq_ignore_ascii_case("Technical parameters")
}

/// Interactive Substance `.sbsar` parameter editor for imported SBSAR content effects (`MatFinish`, filters).
pub fn sbsar_effect_properties_ui(
    ui: &mut Ui,
    state: &mut UiState,
    layer_id: subcraft_core::LayerId,
    effect_id: subcraft_core::EffectId,
    now: f64,
) {
    let Some(mut params) = state
        .session
        .sbsar_effects
        .get(&(layer_id, effect_id))
        .cloned()
    else {
        return;
    };
    if params.inputs.is_empty() {
        return;
    }

    let mut any_changed = false;
    let mut reset_all = false;
    let mut next_size = params.size_px;
    let has_overrides = !params.overrides.is_empty();

    crate::widgets::section_with(
        ui,
        "sbsar_effect_params",
        "Substance Parameters (SBSAR)",
        true,
        |ui| {
            egui::ComboBox::from_id_salt(("sbsar_eff_size", effect_id))
                .selected_text(format!("{next_size}²"))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for s in [512_u32, 1024, 2048, 4096] {
                        if ui.selectable_label(next_size == s, format!("{s}²")).clicked() {
                            next_size = s;
                        }
                    }
                });
            if has_overrides
                && ui
                    .small_button("Reset")
                    .on_hover_text("Reset all Substance parameters to graph defaults")
                    .clicked()
            {
                reset_all = true;
            }
        },
        |ui| {
            let is_technical = |inp: &subcraft_app::SbsarInput| {
                inp.group.eq_ignore_ascii_case("Channels")
                    || inp.group.eq_ignore_ascii_case("Technical parameters")
                    || inp.identifier.starts_with("output_")
                    || inp.identifier == "normal_format"
            };

            let primary: Vec<subcraft_app::SbsarInput> = params
                .inputs
                .iter()
                .filter(|i| !is_technical(i))
                .cloned()
                .collect();
            let technical: Vec<subcraft_app::SbsarInput> = params
                .inputs
                .iter()
                .filter(|i| inp_is_adjustable_technical(i))
                .cloned()
                .collect();

            let mut current_group = String::new();
            for inp in &primary {
                if !inp.group.is_empty() && inp.group != current_group {
                    current_group = inp.group.clone();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&current_group)
                            .font(theme::font_strong(11.0))
                            .color(theme::ACCENT),
                    );
                }
                if render_sbsar_input_row(ui, inp, &mut params.overrides) {
                    any_changed = true;
                }
            }

            if !technical.is_empty() {
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    RichText::new("Technical Adjustments")
                        .font(theme::font(11.5))
                        .color(theme::TEXT_DIM),
                )
                .id_salt(("sbsar_eff_tech", effect_id))
                .default_open(primary.is_empty())
                .show(ui, |ui| {
                    for inp in &technical {
                        if render_sbsar_input_row(ui, inp, &mut params.overrides) {
                            any_changed = true;
                        }
                    }
                });
            }
        },
    );

    if next_size != params.size_px {
        params.size_px = next_size;
        any_changed = true;
    }

    if reset_all {
        params.overrides.clear();
        params.pending_rebake = true;
    } else if any_changed {
        params.pending_rebake = true;
    }

    let pointer_down = ui.input(|i| i.pointer.any_down());
    let should_rebake = params.pending_rebake && !pointer_down;
    if should_rebake {
        params.pending_rebake = false;
    }

    state
        .session
        .sbsar_effects
        .insert((layer_id, effect_id), params);

    if should_rebake {
        if let Err(e) = state.session.rebake_sbsar_content_effect(layer_id, effect_id) {
            state.set_status(format!("SBSAR rebake failed: {e}"), now);
        }
    }
}
