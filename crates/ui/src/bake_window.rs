//! Baking window, laid out like Painter's: bakers on the left (common parameters first, then one
//! row per mesh map with its checkbox), the selected page's parameters in the middle, texture sets
//! and the baked maps on the right, progress and "Bake selected textures" at the bottom.
//!
//! Maps are installed in the session one by one while the bake runs (see
//! [`subcraft_app::BakeJob`]), so the thumbnails, the 2D view and the 3D view fill in as maps finish.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use egui::{Align, Color32, Layout, RichText, Sense, Ui};
use subcraft_app::{BakeJob, BakeRequest, BakeSettings, IdSource, MapStatus, MatchMode, map_preview_rgba};
use subcraft_bake::matching::{MatchReport, base_name};
use subcraft_core::{MeshMapKind, TextureSetId};
use subcraft_image::TiledImage;
use subcraft_render::PreviewSource;

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, BG_3, BORDER, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{badge, caption, divider, hint, list_row, primary_button, prop_row, secondary_button, segmented, slider_row};

/// Edge of a map thumbnail in the window (texels of the preview image).
const THUMB: u32 = 128;
const THUMB_SIZE: f32 = 84.0;

/// Left-column page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BakePage {
    Common,
    Map(MeshMapKind),
}

pub struct BakeUi {
    pub open: bool,
    pub settings: BakeSettings,
    /// High-definition meshes (merged); empty = bake the low poly onto itself.
    pub high_polys: Vec<PathBuf>,
    /// Texture sets left out of "Bake selected textures" (new sets are included by default).
    pub excluded: BTreeSet<TextureSetId>,
    pub page: BakePage,
    /// Texture set whose maps the right column shows (follows the bake unless clicked).
    pub focus: Option<TextureSetId>,
    pub job: Option<BakeJob>,
    /// Name-matching result of the last bake.
    pub last_report: Option<MatchReport>,
    thumbs: HashMap<(TextureSetId, MeshMapKind), (TiledImage, egui::TextureHandle)>,
}

impl Default for BakeUi {
    fn default() -> Self {
        Self {
            open: false,
            settings: BakeSettings::default(),
            high_polys: Vec::new(),
            excluded: BTreeSet::new(),
            page: BakePage::Common,
            focus: None,
            job: None,
            last_report: None,
            thumbs: HashMap::new(),
        }
    }
}

impl BakeUi {
    /// Add high-definition mesh files (duplicates ignored).
    pub fn add_high_polys(&mut self, paths: &[PathBuf]) {
        for p in paths {
            if !self.high_polys.contains(p) {
                self.high_polys.push(p.clone());
            }
        }
    }

    /// The request "Bake selected textures" sends for these texture sets (id, material slot).
    pub fn request(&self, texture_sets: &[(TextureSetId, u32)]) -> BakeRequest {
        let mut settings = self.settings.clone();
        // Bake in Painter's list order regardless of the order the boxes were ticked.
        settings.maps = MeshMapKind::ALL.into_iter().filter(|k| self.settings.maps.contains(k)).collect();
        BakeRequest {
            settings,
            high_poly: self.high_polys.clone(),
            texture_sets: texture_sets.iter().filter(|(id, _)| !self.excluded.contains(id)).copied().collect(),
        }
    }
}

/// Apply results of a running bake (call every frame).
pub fn poll_bake(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(job) = state.bake.job.as_mut() else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let result = job.poll(&mut state.session);
    if let Some(report) = job.matching() {
        state.bake.last_report = Some(report.clone());
    }
    if let Some(result) = result {
        state.bake.job = None;
        match result {
            Ok(n) => {
                state.set_status(format!("Baked mesh maps for {n} texture set(s). See them in the 2D view."), now);
                state.recovery.autosaver.request_soon();
            }
            Err(e) => state.set_status(format!("Bake: {e}"), now),
        }
    }
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
}

fn map_description(kind: MeshMapKind) -> &'static str {
    match kind {
        MeshMapKind::Normal => {
            "Tangent-space normal map (OpenGL +Y, MikkTSpace frame of the low poly). Detail comes from the high-definition meshes; without them the low poly bakes onto itself and the map stays flat."
        }
        MeshMapKind::WorldSpaceNormal => "Normals of the detail surface in world space. Used by Directional / Position-style masks and as the base for other generators.",
        MeshMapKind::Id => "Flat colors that separate parts, for quick color-selection masks.",
        MeshMapKind::AmbientOcclusion => "How much of the hemisphere is open above each texel. Feeds the Cavity generator and shades the viewport.",
        MeshMapKind::Curvature => "Convex edges bright, concave creases dark, flat 50% gray. Computed in 3D, so it is continuous across UV seams. Feeds the Edges generator.",
        MeshMapKind::Position => "Low-poly position normalized to the mesh bounds (R = X, G = Y, B = Z). Drives gradients.",
        MeshMapKind::Thickness => "Rays cast into the surface: bright where the object is thin. Good for subsurface and edge wear on thin parts.",
        MeshMapKind::Height => "Distance of the high poly above (bright) or below (dark) the low poly along its normal; 50% gray is on the surface. The max frontal / rear distance is the range.",
        MeshMapKind::BentNormal => "Average unoccluded direction (world space) from the occlusion rays. Uses the Ambient Occlusion samples and distance.",
        MeshMapKind::Opacity => "White where the projection found the high poly, black where it missed (holes, unmatched parts, cage too small).",
    }
}

pub fn bake_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.bake.open {
        return;
    }
    let mut open = true;
    let sets: Vec<(TextureSetId, u32, String)> =
        state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot, t.name.clone())).collect();
    let selected_set = state.session.selected_texture_set().id;
    egui::Window::new(RichText::new("Baking").font(theme::font_strong(14.0)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([1060.0, 640.0])
        .min_size([820.0, 480.0])
        .show(ctx, |ui| {
            let busy = state.bake.job.is_some();
            let footer = 58.0;
            let body_h = (ui.available_height() - footer).max(320.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                column(ui, 220.0, body_h, |ui| baker_list(ui, state, busy));
                separator(ui, body_h);
                let right = 320.0;
                let center = (ui.available_width() - right - 1.0).max(260.0);
                column(ui, center, body_h, |ui| {
                    egui::ScrollArea::vertical().id_salt("bake_params").auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(8.0);
                        ui.add_enabled_ui(!busy, |ui| match state.bake.page {
                            BakePage::Common => common_page(ui, state),
                            BakePage::Map(kind) => map_page(ui, state, kind),
                        });
                    });
                });
                separator(ui, body_h);
                column(ui, right, body_h, |ui| texture_column(ui, state, &sets, selected_set, busy));
            });
            divider(ui);
            ui.add_space(6.0);
            footer_bar(ui, state, &sets, now);
        });
    state.bake.open = open;
}

fn column(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.set_height(height);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
            ui.set_width(width - 16.0);
            add(ui);
        });
    });
}

fn separator(ui: &mut Ui, height: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, height), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, BORDER));
}

// ----- left column -----------------------------------------------------------------------------

fn baker_list(ui: &mut Ui, state: &mut UiState, busy: bool) {
    let b = &mut state.bake;
    ui.label(caption("Parameters"));
    ui.add_space(2.0);
    if list_row(ui, b.page == BakePage::Common, |ui| {
        ui.label(RichText::new(icons::GEAR).font(theme::font(14.0)).color(TEXT_DIM));
        ui.label(RichText::new("Common parameters").color(TEXT));
    })
    .clicked()
    {
        b.page = BakePage::Common;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(caption("Mesh maps"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if ui.small_button("None").clicked() {
                    b.settings.maps.clear();
                }
                if ui.small_button("All").clicked() {
                    b.settings.maps = MeshMapKind::ALL.to_vec();
                }
            });
        });
    });
    ui.add_space(2.0);
    egui::ScrollArea::vertical().id_salt("bake_bakers").auto_shrink([false, false]).show(ui, |ui| {
        for kind in MeshMapKind::ALL {
            let mut on = b.settings.maps.contains(&kind);
            let status = b.job.as_ref().filter(|j| j.maps().contains(&kind)).and_then(|j| {
                let ts = j.current().map(|c| c.0).or_else(|| j.texture_sets().last().copied())?;
                Some(j.status(ts, kind))
            });
            let row = list_row(ui, b.page == BakePage::Map(kind), |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            b.settings.maps.push(kind);
                        } else {
                            b.settings.maps.retain(|m| *m != kind);
                        }
                    }
                });
                ui.label(RichText::new(kind.display_name()).color(if on { TEXT } else { TEXT_FAINT }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match status {
                    Some(MapStatus::Baking(f)) => {
                        ui.label(RichText::new(format!("{:.0}%", f * 100.0)).small().color(ACCENT));
                    }
                    Some(MapStatus::Done) => {
                        ui.label(RichText::new(icons::CHECK).color(ACCENT));
                    }
                    _ => {}
                });
            });
            if row.clicked() {
                b.page = BakePage::Map(kind);
            }
        }
    });
}

// ----- center column: parameters ---------------------------------------------------------------

fn page_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::font_strong(15.0)).color(TEXT));
    ui.add_space(4.0);
}

fn common_page(ui: &mut Ui, state: &mut UiState) {
    page_title(ui, "Common parameters");
    let low_names = low_object_names(state);
    let b = &mut state.bake;
    prop_row(ui, "Output size", |ui| {
        egui::ComboBox::from_id_salt("bake_size").selected_text(format!("{0} × {0}", b.settings.size)).show_ui(ui, |ui| {
            for r in [256u32, 512, 1024, 2048, 4096] {
                ui.selectable_value(&mut b.settings.size, r, format!("{r} × {r}"));
            }
        });
    });
    prop_row(ui, "Dilation", |ui| ui.label(dim("Infinite (gutters padded from the nearest island texel)")));
    ui.add_space(10.0);

    ui.label(caption("High definition meshes"));
    ui.add_space(2.0);
    if b.high_polys.is_empty() {
        hint(ui, "None: the low poly is used as its own high poly (low on low). Add one or more files with the detailed meshes.");
    }
    let mut remove = None;
    for (i, p) in b.high_polys.iter().enumerate() {
        list_row(ui, false, |ui| {
            ui.label(RichText::new(icons::CUBE).color(TEXT_DIM));
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            ui.label(RichText::new(name).color(TEXT)).on_hover_text(p.display().to_string());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if crate::widgets::icon_button(ui, icons::X, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        b.high_polys.remove(i);
    }
    ui.horizontal(|ui| {
        if secondary_button(ui, format!("{}  Add high poly…", icons::PLUS)).clicked() {
            crate::files::pick(ui.ctx(), crate::files::Purpose::BakeHighPoly, crate::files::MESHES, true, Some("High definition meshes"));
        }
        if !b.high_polys.is_empty() && secondary_button(ui, "Clear").clicked() {
            b.high_polys.clear();
        }
    });
    ui.add_space(10.0);

    ui.label(caption("Projection"));
    ui.add_space(2.0);
    let has_high = !b.high_polys.is_empty();
    ui.add_enabled_ui(has_high, |ui| {
        slider_row(ui, "Max frontal", egui::Slider::new(&mut b.settings.cage_front, 0.0..=0.5).logarithmic(true).smallest_positive(0.0005))
            .on_hover_text("How far above the low poly the high poly is searched, as a fraction of the bounding-box diagonal.");
        slider_row(ui, "Max rear", egui::Slider::new(&mut b.settings.cage_rear, 0.0..=0.5).logarithmic(true).smallest_positive(0.0005))
            .on_hover_text("How far below the low poly the high poly is searched, as a fraction of the bounding-box diagonal.");
        prop_row(ui, "Match", |ui| {
            segmented(ui, &mut b.settings.match_mode, &[(MatchMode::Always, "Always"), (MatchMode::ByMeshName, "By Mesh Name")]);
        });
    });
    if !has_high {
        hint(ui, "Projection settings apply once a high definition mesh is added.");
    }
    let by_name = b.settings.match_mode == MatchMode::ByMeshName;
    if by_name || b.settings.ao_self_only {
        prop_row(ui, "Low suffix", |ui| ui.add(egui::TextEdit::singleline(&mut b.settings.low_suffix).desired_width(120.0)));
        prop_row(ui, "High suffix", |ui| ui.add(egui::TextEdit::singleline(&mut b.settings.high_suffix).desired_width(120.0)));
        hint(
            ui,
            "Mesh names are compared without these suffixes and without case: \"Bolt_low\" only receives detail from \"Bolt_high\". Low meshes without a partner bake without high-poly detail (opacity 0).",
        );
        ui.add_space(6.0);
        egui::CollapsingHeader::new(RichText::new(format!("Low poly meshes ({})", low_names.len())).color(TEXT_DIM)).id_salt("bake_low_names").show(
            ui,
            |ui| {
                egui::ScrollArea::vertical().id_salt("bake_low_names_list").max_height(140.0).show(ui, |ui| {
                    for n in &low_names {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(n).color(TEXT));
                            ui.label(RichText::new(format!("→ {}", base_name(n, &b.settings.low_suffix))).color(TEXT_FAINT));
                        });
                    }
                });
            },
        );
    }
    if let Some(report) = &b.last_report {
        ui.add_space(8.0);
        match_report(ui, report);
    }
}

fn low_object_names(state: &UiState) -> Vec<String> {
    let mesh = &state.session.scene().mesh;
    let mut names: Vec<String> = mesh.objects.iter().map(|o| o.name.clone()).collect();
    if names.is_empty() {
        names.push(mesh.name.clone());
    }
    names.sort();
    names.dedup();
    names
}

fn match_report(ui: &mut Ui, report: &MatchReport) {
    ui.label(caption("Last name match"));
    ui.add_space(2.0);
    let group = |ui: &mut Ui, title: &str, color: Color32, names: &[String]| {
        if names.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            badge(ui, &format!("{title} {}", names.len()), color);
            ui.label(RichText::new(names.join(", ")).small().color(TEXT_DIM));
        });
    };
    group(ui, "Matched", ACCENT, &report.matched);
    group(ui, "Low without high", theme::WARNING, &report.unmatched_low);
    group(ui, "High without low", TEXT_FAINT, &report.unmatched_high);
    if report.matched.is_empty() {
        hint(ui, "No names matched: check the suffixes or use Match: Always.");
    }
}

fn map_page(ui: &mut Ui, state: &mut UiState, kind: MeshMapKind) {
    let b = &mut state.bake;
    ui.horizontal(|ui| {
        let mut on = b.settings.maps.contains(&kind);
        if ui.checkbox(&mut on, "").changed() {
            if on {
                b.settings.maps.push(kind);
            } else {
                b.settings.maps.retain(|m| *m != kind);
            }
        }
        page_title(ui, kind.display_name());
    });
    hint(ui, map_description(kind));
    ui.add_space(8.0);
    let s = &mut b.settings;
    match kind {
        MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal => {
            slider_row(ui, "Secondary rays", egui::Slider::new(&mut s.ao_samples, 4..=256).logarithmic(true));
            slider_row(ui, "Max distance", egui::Slider::new(&mut s.ao_distance, 0.01..=1.0).logarithmic(true))
                .on_hover_text("Occlusion ray length as a fraction of the bounding-box diagonal.");
            prop_row(ui, "Self occlusion", |ui| {
                segmented(ui, &mut s.ao_self_only, &[(false, "Always"), (true, "Only same mesh name")]);
            });
            if kind == MeshMapKind::BentNormal {
                hint(ui, "Shared with Ambient Occlusion.");
            }
        }
        MeshMapKind::Curvature => {
            slider_row(ui, "Radius", egui::Slider::new(&mut s.curvature_radius, 0.002..=0.1).logarithmic(true))
                .on_hover_text("Neighbourhood radius as a fraction of the bounding-box diagonal.");
        }
        MeshMapKind::Thickness => {
            slider_row(ui, "Secondary rays", egui::Slider::new(&mut s.thickness_samples, 4..=128).logarithmic(true));
            slider_row(ui, "Max distance", egui::Slider::new(&mut s.thickness_distance, 0.01..=1.0).logarithmic(true));
        }
        MeshMapKind::Id => {
            prop_row(ui, "Color source", |ui| {
                segmented(ui, &mut s.id_source, &[(IdSource::MeshElement, "Mesh element"), (IdSource::MaterialSlot, "Material"), (IdSource::MeshObject, "Mesh name")]);
            });
        }
        MeshMapKind::Height => {
            slider_row(ui, "Max frontal", egui::Slider::new(&mut s.cage_front, 0.0..=0.5).logarithmic(true).smallest_positive(0.0005));
            slider_row(ui, "Max rear", egui::Slider::new(&mut s.cage_rear, 0.0..=0.5).logarithmic(true).smallest_positive(0.0005));
            hint(ui, "Shared with the projection in Common parameters. Without a high poly the map is 50% gray.");
        }
        MeshMapKind::Normal | MeshMapKind::WorldSpaceNormal | MeshMapKind::Position | MeshMapKind::Opacity => {
            hint(ui, "Uses the common parameters (output size, high definition meshes, projection).");
        }
    }
}

// ----- right column: texture sets and baked maps ----------------------------------------------

fn texture_column(ui: &mut Ui, state: &mut UiState, sets: &[(TextureSetId, u32, String)], selected_set: TextureSetId, busy: bool) {
    ui.horizontal(|ui| {
        ui.label(caption("Texture sets"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{} / {}", sets.iter().filter(|s| !state.bake.excluded.contains(&s.0)).count(), sets.len())).small().color(TEXT_FAINT));
        });
    });
    ui.add_space(2.0);
    let current = state.bake.job.as_ref().and_then(|j| j.current()).map(|c| c.0);
    if busy && let Some(c) = current {
        state.bake.focus = Some(c);
    }
    let focus = state.bake.focus.filter(|f| sets.iter().any(|s| s.0 == *f)).unwrap_or(selected_set);
    egui::ScrollArea::vertical().id_salt("bake_sets").max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
        for (id, _, name) in sets {
            let mut on = !state.bake.excluded.contains(id);
            let row = list_row(ui, *id == focus, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            state.bake.excluded.remove(id);
                        } else {
                            state.bake.excluded.insert(*id);
                        }
                    }
                });
                ui.label(RichText::new(name).color(if on { TEXT } else { TEXT_FAINT }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if current == Some(*id) {
                        badge(ui, "baking", ACCENT);
                    } else if let Some(n) = state.session.mesh_maps(*id).map(|m| m.len()) {
                        ui.label(RichText::new(format!("{n} maps")).small().color(TEXT_FAINT));
                    }
                });
            });
            if row.clicked() {
                state.bake.focus = Some(*id);
            }
        }
    });
    ui.add_space(8.0);
    let focus_name = sets.iter().find(|s| s.0 == focus).map_or("", |s| s.2.as_str());
    ui.label(caption(&format!("Baked maps — {focus_name}")));
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("bake_maps").auto_shrink([false, false]).show(ui, |ui| {
        let kinds: Vec<MeshMapKind> = match &state.bake.job {
            Some(j) => j.maps().to_vec(),
            None => MeshMapKind::ALL.into_iter().filter(|k| state.bake.settings.maps.contains(k) || state.session.mesh_maps(focus).and_then(|m| m.get(*k)).is_some()).collect(),
        };
        if kinds.is_empty() {
            hint(ui, "Tick mesh maps on the left to bake them.");
        }
        let per_row = ((ui.available_width() + 6.0) / (THUMB_SIZE + 6.0)).floor().max(1.0) as usize;
        for chunk in kinds.chunks(per_row) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for &kind in chunk {
                    map_tile(ui, state, focus, kind);
                }
            });
            ui.add_space(6.0);
        }
    });
}

fn map_tile(ui: &mut Ui, state: &mut UiState, ts: TextureSetId, kind: MeshMapKind) {
    let status = state.bake.job.as_ref().filter(|j| j.texture_sets().contains(&ts)).map(|j| j.status(ts, kind));
    let image = state.session.mesh_maps(ts).and_then(|m| m.get(kind)).cloned();
    let texture = image.as_ref().map(|img| thumbnail(ui.ctx(), &mut state.bake.thumbs, ts, kind, img));
    ui.allocate_ui_with_layout(egui::vec2(THUMB_SIZE, THUMB_SIZE + 30.0), Layout::top_down(Align::Center), |ui| {
        ui.set_width(THUMB_SIZE);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(THUMB_SIZE, THUMB_SIZE), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BG_3);
        // A map still waiting for this bake shows dimmed (the old result, if any, stays visible).
        let fresh = !matches!(status, Some(MapStatus::Queued | MapStatus::Baking(_)));
        if let Some(tex) = &texture {
            let tint = if fresh { Color32::WHITE } else { Color32::from_gray(80) };
            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        match status {
            Some(MapStatus::Baking(f)) => {
                painter.rect_filled(egui::Rect::from_min_size(rect.left_bottom() - egui::vec2(0.0, 4.0), egui::vec2(rect.width() * f, 4.0)), 0.0, ACCENT);
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{:.0}%", f * 100.0), theme::font_strong(13.0), ACCENT);
            }
            Some(MapStatus::Queued) => {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "queued", theme::font(11.0), TEXT_FAINT);
            }
            _ if texture.is_none() => {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "not baked", theme::font(11.0), TEXT_FAINT);
            }
            _ => {}
        }
        let border = if matches!(status, Some(MapStatus::Baking(_))) { ACCENT } else { BORDER };
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
        ui.add(egui::Label::new(RichText::new(kind.display_name()).small().color(if texture.is_some() { TEXT } else { TEXT_FAINT })).truncate());
        if let Some(tex) = &texture {
            let resp = resp.on_hover_ui(|ui| {
                ui.label(RichText::new(kind.display_name()).strong());
                ui.image((tex.id(), egui::vec2(256.0, 256.0)));
                ui.label(RichText::new("Click to show it in the 2D view").small().color(TEXT_FAINT));
            });
            if resp.clicked() {
                if state.session.selection().texture_set != ts {
                    let _ = state.session.execute(subcraft_app::Command::SelectTextureSet(ts));
                }
                state.uv.source = PreviewSource::MeshMap(kind);
            }
        }
    });
}

fn thumbnail(
    ctx: &egui::Context,
    cache: &mut HashMap<(TextureSetId, MeshMapKind), (TiledImage, egui::TextureHandle)>,
    ts: TextureSetId,
    kind: MeshMapKind,
    img: &TiledImage,
) -> egui::TextureHandle {
    if let Some((cached, tex)) = cache.get(&(ts, kind))
        && cached.same_storage(img)
    {
        return tex.clone();
    }
    let rgba = map_preview_rgba(img, THUMB);
    let color = egui::ColorImage::from_rgba_unmultiplied([THUMB as usize; 2], &rgba);
    let tex = ctx.load_texture(format!("bake-{}-{}", ts.0, kind.key()), color, egui::TextureOptions::LINEAR);
    cache.insert((ts, kind), (img.clone(), tex.clone()));
    tex
}

// ----- footer ---------------------------------------------------------------------------------

fn footer_bar(ui: &mut Ui, state: &mut UiState, sets: &[(TextureSetId, u32, String)], now: f64) {
    let all: Vec<(TextureSetId, u32)> = sets.iter().map(|s| (s.0, s.1)).collect();
    match &state.bake.job {
        Some(job) => {
            let current = job.current().map(|(ts, kind)| {
                let name = sets.iter().find(|s| s.0 == ts).map_or("", |s| s.2.as_str());
                format!("{name} · {}", kind.display_name())
            });
            ui.horizontal(|ui| {
                ui.label(dim(&current.unwrap_or_else(|| job.stage())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        job.cancel();
                    }
                    ui.label(RichText::new(format!("{:.0}%  ·  {} maps done", job.fraction() * 100.0, job.finished().len())).small().color(TEXT_FAINT));
                });
            });
            ui.add(egui::ProgressBar::new(job.fraction()).fill(ACCENT).desired_height(6.0));
        }
        None => {
            let request = state.bake.request(&all);
            let maps = request.settings.maps.len();
            let count = request.texture_sets.len();
            let can = maps > 0 && count > 0;
            ui.horizontal(|ui| {
                let summary = format!(
                    "{maps} map(s) × {count} texture set(s) at {0}×{0}{1}",
                    request.settings.size,
                    if request.high_poly.is_empty() { " · low on low".to_string() } else { format!(" · {} high poly file(s)", request.high_poly.len()) }
                );
                ui.label(dim(&summary));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let go = ui.add_enabled_ui(can, |ui| primary_button(ui, format!("{}  Bake selected textures", icons::MAGIC_WAND))).inner;
                    if go.clicked() {
                        state.bake.last_report = None;
                        state.bake.job = Some(state.session.start_bake(request));
                        state.set_status("Baking…", now);
                    }
                    if secondary_button(ui, "Close").clicked() {
                        state.bake.open = false;
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_keeps_painter_order_and_skips_excluded_sets() {
        let mut b = BakeUi::default();
        b.settings.maps = vec![MeshMapKind::Opacity, MeshMapKind::Normal, MeshMapKind::Height];
        let (a, c) = (TextureSetId::new(), TextureSetId::new());
        b.excluded.insert(c);
        b.add_high_polys(&[PathBuf::from("a.fbx"), PathBuf::from("b.obj"), PathBuf::from("a.fbx")]);
        let r = b.request(&[(a, 0), (c, 1)]);
        assert_eq!(r.settings.maps, vec![MeshMapKind::Normal, MeshMapKind::Height, MeshMapKind::Opacity]);
        assert_eq!(r.texture_sets, vec![(a, 0)]);
        assert_eq!(r.high_poly, vec![PathBuf::from("a.fbx"), PathBuf::from("b.obj")]);
    }
}
