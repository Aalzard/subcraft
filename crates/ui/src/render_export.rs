//! "Render quality" window (tone mapping, bloom, ambient occlusion) and high-quality still /
//! turntable export. Rendering uses the viewport's own renderer, so what you export is what you
//! see, supersampled and without the grid or guides.

use crate::i18n::tr;
use egui::{Context, RichText};
use subcraft_render::offline;

use crate::state::UiState;
use crate::theme::TEXT_DIM;


/// Output sizes offered for stills and turntables (square-ish; the view's aspect is kept).
pub const SIZES: [u32; 4] = [1024, 1536, 2048, 4096];
/// Resolution multipliers applied to the chosen size.
pub const SCALES: [u32; 4] = [1, 2, 3, 4];
pub const FRAME_COUNTS: [u32; 4] = [24, 36, 72, 120];

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    /// Long side of the picture, in pixels.
    pub size: u32,
    /// Render at twice the size and filter down (smoother edges).
    pub supersample: bool,
    pub frames: u32,
    /// Multiplies `size` (1x..4x).
    pub scale: u32,
    /// Alpha channel instead of the background color (two renders over black and white).
    pub transparent: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { size: 2048, supersample: true, frames: 36, scale: 1, transparent: false }
    }
}

impl ExportSettings {
    /// Pixel size of a picture of the view's aspect ratio with this long side.
    pub fn picture(&self, view: [u32; 2]) -> [u32; 2] {
        let (w, h) = (view[0].max(1) as f32, view[1].max(1) as f32);
        let k = (self.size * self.scale.max(1)) as f32 / w.max(h);
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
    crate::widgets::window(ctx, "Render quality").open(&mut open).resizable(false).default_width(330.0).show(ctx, |ui| window_body(state, ui));
    state.render_window = open;
}

fn window_body(state: &mut UiState, ui: &mut egui::Ui) {
    {
        // The look is the Display Settings panel's Camera and Post effects groups.
        crate::panels::display_settings::camera(state, ui);
        crate::panels::display_settings::post_effects(state, ui);
        {
            ui.separator();
            ui.label(RichText::new(tr("Export")).color(TEXT_DIM));
            let settings = &mut state.render_export;
            egui::Grid::new("render_export").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Size"));
                egui::ComboBox::from_id_salt("render_size").selected_text(format!("{} px", settings.size)).show_ui(ui, |ui| {
                    for s in SIZES { ui.selectable_value(&mut settings.size, s, format!("{s} px")); }
                });
                ui.end_row();
                ui.label(tr("Multiplier"));
                egui::ComboBox::from_id_salt("render_scale").selected_text(format!("{}x", settings.scale)).show_ui(ui, |ui| {
                    for s in SCALES { ui.selectable_value(&mut settings.scale, s, format!("{s}x")); }
                });
                ui.end_row();
                ui.label(tr("Background"));
                ui.checkbox(&mut settings.transparent, tr("Transparent")).on_hover_text(tr("Save with an alpha channel (the model alone). Needs two renders, so it takes twice as long."));
                ui.end_row();
                ui.label(tr("Smoothing"));
                ui.checkbox(&mut settings.supersample, tr("2x supersampling")).on_hover_text(tr("Render at double size and filter down: cleaner edges, slower"));
                ui.end_row();
                ui.label(tr("Turntable frames"));
                egui::ComboBox::from_id_salt("render_frames").selected_text(settings.frames.to_string()).show_ui(ui, |ui| {
                    for f in FRAME_COUNTS { ui.selectable_value(&mut settings.frames, f, f.to_string()); }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button(tr("Save image…")).on_hover_text(tr("Render the current view as a PNG")).clicked() {
                    state.render_request = Some(RenderRequest::Image);
                }
                if ui.button(tr("Render turntable…")).on_hover_text(tr("A full turn around the model as numbered PNG frames in a folder")).clicked() {
                    state.render_request = Some(RenderRequest::Turntable);
                }
            });
            ui.label(RichText::new(tr("The grid and guides are left out. 4096 px with smoothing needs about 1 GB of video memory.")).small().color(TEXT_DIM));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderRequest {
    Image,
    Turntable,
    /// The Rendering mode's path-traced image as it is shown now (Painter's "Save Render").
    PathTraced,
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
        RenderRequest::PathTraced => {
            let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("render.png").save_file() else { return };
            let result = save_path_traced(state, &path);
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
pub fn render_frame(state: &mut UiState, camera: crate::orbit::OrbitCamera, picture: [u32; 2], supersample: bool, transparent: bool) -> Result<(Vec<u8>, [u32; 2]), String> {
    let rs = state.render_state.clone().ok_or("no GPU renderer is available")?;
    state.sync_gpu();
    let mut renderer = state.renderer.take().ok_or("no GPU renderer is available")?;
    let limit = rs.device.limits().max_texture_dimension_2d;
    if picture.iter().any(|&p| p > limit) {
        return Err(format!("{}x{} is larger than this graphics card can render ({limit} px per side)", picture[0], picture[1]));
    }
    // Supersampling needs twice the size; drop it rather than fail when that does not fit.
    let factor = if supersample && picture.iter().all(|&p| p * 2 <= limit) { 2 } else { 1 };
    let big = [picture[0] * factor, picture[1] * factor];
    let prefs = &state.recovery.prefs.viewport;
    let mut params = camera.view_params(big);
    params.height_scale = state.height_scale;
    params.antialias = true;
    params.exposure = prefs.exposure_value();
    params.post = prefs.post_params_at(state.session.scene().mesh.bounds.diagonal(), camera.eye_distance());
    let (env_rotation, env_intensity) = crate::panels::shelf::environment_view(&prefs.display, state.env.rotation_deg, camera.yaw);
    params.environment.background_opacity = prefs.env_opacity;
    params.environment.background_blur = prefs.env_blur;
    crate::panels::shelf::apply_environment(&state.library, state.env.index, env_rotation, env_intensity,
        &mut params, &mut renderer, &mut state.env.uploaded, &rs.device, &rs.queue);
    // The renderer goes back into the state whether or not the GPU managed the readback, so a
    // failed render (device lost, out of video memory) reports an error and the viewport keeps working.
    let mut read = |params: &subcraft_render::ViewParams| renderer.render(&rs.device, &rs.queue, big, params).try_read_rgba8(&rs.device, &rs.queue);
    let pixels = if transparent {
        params.background = [0.0; 3];
        read(&params).and_then(|black| {
            params.background = [1.0; 3];
            read(&params).map(|white| offline::matte_from_black_white(&black, &white))
        })
    } else {
        read(&params)
    };
    state.renderer = Some(renderer);
    let mut pixels = pixels?;
    if !transparent {
        for a in pixels.iter_mut().skip(3).step_by(4) { *a = 255; }
    }
    Ok(offline::downsample_rgba8(&pixels, big, factor))
}

fn save_png(path: &std::path::Path, pixels: &[u8], size: [u32; 2]) -> Result<(), String> {
    let data: Vec<f32> = pixels.iter().map(|&b| b as f32 / 255.0).collect();
    subcraft_image::save_image(path, size[0], size[1], 4, &data, subcraft_image::FileFormat::Png8).map_err(|e| format!("{}: {e}", path.display()))
}

/// The path-traced image as shown (tone mapped, sRGB, opaque) and its size, or why there is none.
pub fn path_traced_pixels(state: &UiState) -> Result<(Vec<u8>, [u32; 2]), String> {
    let rs = state.render_state.as_ref().ok_or("no GPU renderer is available")?;
    let target = state.renderer.as_ref().and_then(|r| r.path_target()).filter(|_| state.pt_status.is_some())
        .ok_or("Turn on the Rendering mode (F10) first: there is no path-traced image yet")?;
    let mut pixels = target.try_read_rgba8(&rs.device, &rs.queue)?;
    for a in pixels.iter_mut().skip(3).step_by(4) { *a = 255; }
    Ok((pixels, target.size))
}

/// Save the Rendering mode's current image to `path` as a PNG.
pub fn save_path_traced(state: &UiState, path: &std::path::Path) -> Result<String, String> {
    let (pixels, size) = path_traced_pixels(state)?;
    save_png(path, &pixels, size)?;
    let samples = state.pt_status.map_or(0, |s| s.samples);
    Ok(format!("Saved {} ({} x {}, {samples} samples)", path.display(), size[0], size[1]))
}

/// Run an export job with the current view and settings. Returns what to tell the user.
pub fn run(state: &mut UiState, job: &Job) -> Result<String, String> {
    let view = state.last_view_size;
    let picture = state.render_export.picture(view);
    let supersample = state.render_export.supersample;
    let transparent = state.render_export.transparent;
    let camera = state.camera;
    match job {
        Job::Image(path) => {
            let (pixels, size) = render_frame(state, camera, picture, supersample, transparent)?;
            save_png(path, &pixels, size)?;
            Ok(format!("Saved {} ({} x {})", path.display(), size[0], size[1]))
        }
        Job::Turntable(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let frames = state.render_export.frames;
            for (i, yaw) in offline::turntable_yaws(camera.yaw, frames).into_iter().enumerate() {
                let (pixels, size) = render_frame(state, crate::orbit::OrbitCamera { yaw, ..camera }, picture, supersample, transparent)?;
                save_png(&dir.join(offline::turntable_file_name(i as u32)), &pixels, size)?;
            }
            Ok(format!("Saved {frames} turntable frames to {}", dir.display()))
        }
    }
}
