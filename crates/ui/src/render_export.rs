//! "Render quality" window (tone mapping, bloom, ambient occlusion) and high-quality still /
//! turntable export. Rendering uses the viewport's own renderer, so what you export is what you
//! see, supersampled and without the grid or guides.

use egui::{Context, RichText};
use subcraft_render::{ToneMap, offline};

use crate::state::UiState;
use crate::theme::TEXT_DIM;
use crate::viewport_bar::ViewportPrefs;

/// Output sizes offered for stills and turntables (square-ish; the view's aspect is kept).
pub const SIZES: [u32; 4] = [1024, 1536, 2048, 4096];
pub const FRAME_COUNTS: [u32; 4] = [24, 36, 72, 120];

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    /// Long side of the picture, in pixels.
    pub size: u32,
    /// Render at twice the size and filter down (smoother edges).
    pub supersample: bool,
    pub frames: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { size: 2048, supersample: true, frames: 36 }
    }
}

impl ExportSettings {
    /// Pixel size of a picture of the view's aspect ratio with this long side.
    pub fn picture(&self, view: [u32; 2]) -> [u32; 2] {
        let (w, h) = (view[0].max(1) as f32, view[1].max(1) as f32);
        let k = self.size as f32 / w.max(h);
        [((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1)]
    }
}

/// What a scripted run (`--render-image`, `--turntable`) should produce.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Image(std::path::PathBuf),
    Turntable(std::path::PathBuf),
}

pub fn window(state: &mut UiState, ctx: &Context) {
    if !state.render_window {
        return;
    }
    let mut open = true;
    egui::Window::new("Render quality").open(&mut open).resizable(false).default_width(330.0).show(ctx, |ui| {
        let prefs = &mut state.recovery.prefs.viewport;
        ui.label(RichText::new("Look").color(TEXT_DIM));
        egui::Grid::new("render_look").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Tone mapping");
            let mut tone = prefs.tone_map();
            egui::ComboBox::from_id_salt("tone_map").selected_text(tone.label()).show_ui(ui, |ui| {
                for t in ToneMap::ALL {
                    ui.selectable_value(&mut tone, t, t.label());
                }
            }).response.on_hover_text("How bright light is squeezed into the display range. ACES is the default look; Linear clips highlights.");
            prefs.tone_map = ToneMap::ALL.iter().position(|t| *t == tone).unwrap_or(0);
            ui.end_row();
            ui.label("Exposure");
            ui.add(egui::Slider::new(&mut prefs.exposure, 0.25..=4.0).logarithmic(true)).on_hover_text("Overall brightness before tone mapping");
            ui.end_row();
            ui.label("Bloom");
            ui.add(egui::Slider::new(&mut prefs.bloom, 0.0..=2.0)).on_hover_text("Glow around bright areas. 0 is off.");
            ui.end_row();
            ui.label("Bloom threshold");
            ui.add_enabled(prefs.bloom > 0.0, egui::Slider::new(&mut prefs.bloom_threshold, 0.0..=1.0)).on_hover_text("Brightness above which an area glows");
            ui.end_row();
            ui.label("Ambient occlusion");
            ui.add(egui::Slider::new(&mut prefs.ssao, 0.0..=1.0)).on_hover_text("Soft shadow in creases, from the view's depth. 0 is off.");
            ui.end_row();
            ui.label("AO radius");
            ui.add_enabled(prefs.ssao > 0.0, egui::Slider::new(&mut prefs.ssao_radius, 0.5..=10.0).suffix("%")).on_hover_text("How far occlusion reaches, in percent of the model size");
            ui.end_row();
        });
        ui.label(RichText::new("Emission").color(TEXT_DIM));
        egui::Grid::new("render_emission").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Glow");
            ui.add(egui::Slider::new(&mut prefs.emission_glow, 0.0..=3.0)).on_hover_text("Soft glow around emissive areas only. 0 is off; the default is subtle.");
            ui.end_row();
            ui.label("Glow radius");
            ui.add_enabled(prefs.emission_glow > 0.0, egui::Slider::new(&mut prefs.glow_radius, 0.25..=8.0).logarithmic(true)).on_hover_text("How far the glow spreads");
            ui.end_row();
            ui.label("Glare");
            ui.add(egui::Slider::new(&mut prefs.glare, 0.0..=3.0)).on_hover_text("Streaks of light from bright emission. 0 is off.");
            ui.end_row();
            ui.label("Glare shape");
            let mut mode = subcraft_render::GlareMode::ALL.get(prefs.glare_mode).copied().unwrap_or_default();
            ui.add_enabled_ui(prefs.glare > 0.0, |ui| {
                egui::ComboBox::from_id_salt("glare_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                    for m in subcraft_render::GlareMode::ALL { ui.selectable_value(&mut mode, m, m.label()); }
                });
            });
            prefs.glare_mode = subcraft_render::GlareMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
            ui.end_row();
            ui.label("Streak length");
            ui.add_enabled(prefs.glare > 0.0, egui::Slider::new(&mut prefs.glare_length, 0.0..=1.0)).on_hover_text("How long the streaks reach");
            ui.end_row();
        });
        ui.label(RichText::new("Glow and glare are viewport effects: exported textures never contain them.").small().color(TEXT_DIM));
        if ui.button("Reset look").clicked() {
            let keep = ViewportPrefs {
                tone_map: 0, exposure: 1.0, bloom: 0.0, bloom_threshold: 0.8, ssao: 0.0, ssao_radius: 3.0,
                emission_glow: 0.35, glow_radius: 1.0, glare: 0.0, glare_mode: 0, glare_length: 0.5,
                ..prefs.clone()
            };
            *prefs = keep;
        }
        {
            ui.separator();
            ui.label(RichText::new("Export").color(TEXT_DIM));
            let settings = &mut state.render_export;
            egui::Grid::new("render_export").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Size");
                egui::ComboBox::from_id_salt("render_size").selected_text(format!("{} px", settings.size)).show_ui(ui, |ui| {
                    for s in SIZES { ui.selectable_value(&mut settings.size, s, format!("{s} px")); }
                });
                ui.end_row();
                ui.label("Smoothing");
                ui.checkbox(&mut settings.supersample, "2x supersampling").on_hover_text("Render at double size and filter down: cleaner edges, slower");
                ui.end_row();
                ui.label("Turntable frames");
                egui::ComboBox::from_id_salt("render_frames").selected_text(settings.frames.to_string()).show_ui(ui, |ui| {
                    for f in FRAME_COUNTS { ui.selectable_value(&mut settings.frames, f, f.to_string()); }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Save image…").on_hover_text("Render the current view as a PNG").clicked() {
                    state.render_request = Some(RenderRequest::Image);
                }
                if ui.button("Render turntable…").on_hover_text("A full turn around the model as numbered PNG frames in a folder").clicked() {
                    state.render_request = Some(RenderRequest::Turntable);
                }
            });
            ui.label(RichText::new("The grid and guides are left out. 4096 px with smoothing needs about 1 GB of video memory.").small().color(TEXT_DIM));
        }
    });
    state.render_window = open;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderRequest {
    Image,
    Turntable,
}

/// Carry out a request made in the window (file pickers are blocking, desktop only).
pub fn handle_request(state: &mut UiState, now: f64) {
    let Some(request) = state.render_request.take() else { return };
    match request {
        RenderRequest::Image => {
            let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("render.png").save_file() else { return };
            let result = run(state, &Job::Image(path));
            report(state, now, result);
        }
        RenderRequest::Turntable => {
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let result = run(state, &Job::Turntable(dir));
            report(state, now, result);
        }
    }
}


fn report(state: &mut UiState, now: f64, result: Result<String, String>) {
    match result {
        Ok(message) => state.set_status(message, now),
        Err(e) => state.set_status(format!("Render failed: {e}"), now),
    }
}

/// Render one frame of `camera` at `picture` pixels (times 2 when supersampling), as sRGB RGBA8.
pub fn render_frame(state: &mut UiState, camera: crate::orbit::OrbitCamera, picture: [u32; 2], supersample: bool) -> Result<(Vec<u8>, [u32; 2]), String> {
    let rs = state.render_state.clone().ok_or("no GPU renderer is available")?;
    state.sync_gpu();
    let mut renderer = state.renderer.take().ok_or("no GPU renderer is available")?;
    let factor = if supersample { 2 } else { 1 };
    let big = [picture[0] * factor, picture[1] * factor];
    let prefs = &state.recovery.prefs.viewport;
    let mut params = camera.view_params(big);
    params.height_scale = state.height_scale;
    params.antialias = true;
    params.exposure = prefs.exposure;
    params.post = prefs.post_params(state.session.scene().mesh.bounds.diagonal());
    crate::panels::shelf::apply_environment(&state.library, state.env.index, state.env.rotation_deg, state.env.intensity,
        &mut params, &mut renderer, &mut state.env.uploaded, &rs.device, &rs.queue);
    let pixels = renderer.render(&rs.device, &rs.queue, big, &params).read_rgba8(&rs.device, &rs.queue);
    state.renderer = Some(renderer);
    Ok(offline::downsample_rgba8(&pixels, big, factor))
}

fn save_png(path: &std::path::Path, pixels: &[u8], size: [u32; 2]) -> Result<(), String> {
    let data: Vec<f32> = pixels.iter().map(|&b| b as f32 / 255.0).collect();
    subcraft_image::save_image(path, size[0], size[1], 4, &data, subcraft_image::FileFormat::Png8).map_err(|e| format!("{}: {e}", path.display()))
}

/// Run an export job with the current view and settings. Returns what to tell the user.
pub fn run(state: &mut UiState, job: &Job) -> Result<String, String> {
    let view = state.last_view_size;
    let picture = state.render_export.picture(view);
    let supersample = state.render_export.supersample;
    let camera = state.camera;
    match job {
        Job::Image(path) => {
            let (pixels, size) = render_frame(state, camera, picture, supersample)?;
            save_png(path, &pixels, size)?;
            Ok(format!("Saved {} ({} x {})", path.display(), size[0], size[1]))
        }
        Job::Turntable(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let frames = state.render_export.frames;
            for (i, yaw) in offline::turntable_yaws(camera.yaw, frames).into_iter().enumerate() {
                let (pixels, size) = render_frame(state, crate::orbit::OrbitCamera { yaw, ..camera }, picture, supersample)?;
                save_png(&dir.join(offline::turntable_file_name(i as u32)), &pixels, size)?;
            }
            Ok(format!("Saved {frames} turntable frames to {}", dir.display()))
        }
    }
}
