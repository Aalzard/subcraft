//! Picking files on desktop and on the web.
//!
//! Callers say what the files are for ([`Purpose`]) instead of handling the answer inline: on
//! desktop the native dialog answers at once, in the browser the upload finishes a moment later.
//! Either way [`poll`] (once per frame) hands the chosen paths to [`handle`]. Browser uploads are
//! put into [`subcraft_core::vfs`] under `/upload/<n>/`, so the engine reads them like files
//! (a `.gltf` and its `.bin` picked together sit side by side).

use std::path::PathBuf;
use std::sync::Mutex;

use subcraft_core::{ChannelKind, EffectId, LayerId};

use crate::state::UiState;

/// What picked files are used for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    OpenMesh,
    /// The model chosen in the New Project window.
    NewProjectModel,
    /// The optional high-poly model of the New Project window.
    NewProjectHighPoly,
    OpenProject,
    BakeHighPoly,
    Stencil,
    TextureSet,
    BrushTextureSet,
    BrushChannel { channel: ChannelKind },
    ShelfTextures,
    Alpha,
    Hdri,
    /// Image for a fill layer channel.
    FillChannel { layer: LayerId, channel: ChannelKind },
    /// New image mask effect on the layer.
    AddMaskImage { layer: LayerId },
    /// Replace the image of an existing image mask effect.
    MaskImage { layer: LayerId, effect: EffectId },
}

pub const IMAGES: (&str, &[&str]) = ("Images", &subcraft_image::IMAGE_EXTENSIONS);
pub const MESHES: (&str, &[&str]) = ("Meshes", &subcraft_mesh::SUPPORTED_EXTENSIONS);
pub const PROJECTS: (&str, &[&str]) = ("SubCraft project", &[subcraft_app::PROJECT_EXTENSION]);
pub const ALPHAS: (&str, &[&str]) = ("Images", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"]);
pub const PANORAMAS: (&str, &[&str]) = ("Panoramas", &["hdr", "exr", "png", "jpg", "jpeg"]);

static PICKED: Mutex<Vec<(Purpose, Vec<PathBuf>)>> = Mutex::new(Vec::new());

fn deliver(purpose: Purpose, paths: Vec<PathBuf>) {
    if !paths.is_empty() {
        PICKED.lock().expect("picked files").push((purpose, paths));
    }
}

/// Ask for one file (or several with `multiple`) for `purpose`.
#[cfg(not(target_arch = "wasm32"))]
pub fn pick(_ctx: &egui::Context, purpose: Purpose, filter: (&str, &[&str]), multiple: bool, title: Option<&str>) {
    let mut d = rfd::FileDialog::new().add_filter(filter.0, filter.1);
    if let Some(t) = title {
        d = d.set_title(t);
    }
    let paths = if multiple { d.pick_files().unwrap_or_default() } else { d.pick_file().into_iter().collect() };
    deliver(purpose, paths);
}

/// Ask for one file (or several with `multiple`) for `purpose`: a browser upload, delivered once
/// read.
#[cfg(target_arch = "wasm32")]
pub fn pick(ctx: &egui::Context, purpose: Purpose, filter: (&str, &[&str]), multiple: bool, title: Option<&str>) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static UPLOADS: AtomicU32 = AtomicU32::new(0);
    let mut d = rfd::AsyncFileDialog::new().add_filter(filter.0, filter.1);
    if let Some(t) = title {
        d = d.set_title(t);
    }
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let handles = if multiple { d.pick_files().await.unwrap_or_default() } else { d.pick_file().await.into_iter().collect() };
        let dir = PathBuf::from(format!("/upload/{}", UPLOADS.fetch_add(1, Ordering::Relaxed)));
        let mut paths = Vec::new();
        for h in handles {
            let path = dir.join(h.file_name());
            subcraft_core::vfs::insert(&path, h.read().await);
            paths.push(path);
        }
        deliver(purpose, paths);
        ctx.request_repaint();
    });
}

/// Where projects and exports are written in the browser (each written file is downloaded).
#[cfg(target_arch = "wasm32")]
pub const DOWNLOADS: &str = "/download";

/// Hand picked files to their handlers (call once per frame).
pub fn poll(state: &mut UiState, now: f64) {
    let picked = std::mem::take(&mut *PICKED.lock().expect("picked files"));
    for (purpose, paths) in picked {
        handle(state, purpose, &paths, now);
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string()
}

/// Shelf → Import textures: keep the files in Shelf → Textures and, when their names say which
/// maps they are (basecolor, roughness, ao…), also make them one fill-layer material on the
/// selected texture set. Plain images (photos, decals) only go to the shelf.
pub fn import_shelf_textures(state: &mut UiState, paths: &[PathBuf], now: f64) {
    state.remember_textures(paths);
    match state.session.import_texture_set(paths) {
        Ok(r) => {
            for (file, why) in &r.skipped {
                log::info!("import textures: skipped {file}: {why}");
            }
            state.set_status(r.summary(), now);
        }
        Err(_) => state.set_status(format!("Added {} texture(s) to Shelf › Textures", paths.len()), now),
    }
}

fn handle(state: &mut UiState, purpose: Purpose, paths: &[PathBuf], now: f64) {
    let Some(first) = paths.first() else { return };
    match purpose {
        Purpose::BrushChannel { channel } => match state.session.set_brush_channel_image(channel, first) {
            Ok(()) => {
                state.textures.add_files(state.user_library.as_ref(), paths);
                state.set_status(format!("Brush {} image assigned", channel.display_name()), now);
            }
            Err(e) => { state.set_status(format!("Brush image: {e}"), now); }
        },
        Purpose::BrushTextureSet => match state.session.import_brush_texture_set(paths) {
            Ok(result) => {
                state.textures.add_files(state.user_library.as_ref(), paths);
                state.set_status(format!("Brush material: {}", result.summary()), now);
            }
            Err(e) => { state.set_status(format!("Brush material: {e}"), now); }
        },
        Purpose::NewProjectModel => state.new_project.load_model(first),
        Purpose::NewProjectHighPoly => state.new_project.load_high_poly(first),
        Purpose::OpenMesh => match subcraft_app::Session::open_mesh(first, subcraft_app::DEFAULT_RESOLUTION) {
            Ok((s, _)) => {
                state.set_session(s);
                state.welcome.remember(first);
                state.set_status(format!("Opened {}", file_name(first)), now);
            }
            Err(e) => state.set_status(format!("Could not open mesh: {e}"), now),
        },
        Purpose::OpenProject => crate::dialogs::open_project(state, first, now),
        Purpose::BakeHighPoly => state.bake.high_poly = first.display().to_string(),
        Purpose::Stencil => match subcraft_image::load_bitmap(first) {
            Ok(bmp) => state.session.brush.stencil = Some(std::sync::Arc::new(subcraft_paint::Stencil::new(std::sync::Arc::new(bmp)))),
            Err(e) => state.set_status(format!("Stencil: {e}"), now),
        },
        Purpose::TextureSet => match state.session.import_texture_set(paths) {
            Ok(r) => {
                state.remember_textures(paths);
                for (file, why) in &r.skipped {
                    log::info!("import texture set: skipped {file}: {why}");
                }
                state.set_status(r.summary(), now);
            }
            Err(e) => state.set_status(format!("Import texture set: {e}"), now),
        },
        Purpose::ShelfTextures => import_shelf_textures(state, paths, now),
        Purpose::Alpha => {
            // Copied into the library so it is still there next time.
            let imported = match &state.user_library {
                Some(lib) => lib
                    .add_file(subcraft_app::assets::UserAssetKind::Alpha, first)
                    .and_then(|stored| subcraft_app::assets::user_library::load_alpha(&stored)),
                None => subcraft_app::assets::alphas::import(first),
            };
            match imported {
                Ok(a) => {
                    state.session.set_brush_alpha(&a);
                    // Re-importing a file already in the library just selects it.
                    if !state.library.alphas.iter().any(|x| x.id == a.id) {
                        state.library.alphas.push(a);
                    }
                }
                Err(e) => state.set_status(format!("Import alpha: {e}"), now),
            }
        }
        Purpose::Hdri => {
            let imported = match &state.user_library {
                Some(lib) => lib
                    .add_file(subcraft_app::assets::UserAssetKind::Environment, first)
                    .and_then(|stored| subcraft_app::assets::user_library::load_environment(&stored)),
                None => subcraft_app::assets::environment::import_hdri(first),
            };
            match imported {
                Ok(e) => {
                    state.env.index = match state.library.environments.iter().position(|x| x.id == e.id) {
                        Some(i) => i,
                        None => {
                            state.library.environments.push(e);
                            state.library.environments.len() - 1
                        }
                    };
                }
                Err(e) => state.set_status(format!("Import HDRI: {e}"), now),
            }
        }
        Purpose::FillChannel { layer, channel } => match state.session.set_fill_channel_image(layer, channel, first) {
            Ok(()) => state.remember_textures(std::slice::from_ref(first)),
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
        Purpose::AddMaskImage { layer } => match state.session.import_image(first) {
            Ok(resource) => {
                state.remember_textures(std::slice::from_ref(first));
                let kind = subcraft_doc::MaskEffectKind::Image { resource, projection: subcraft_doc::Projection::default(), invert: false };
                state.run(vec![subcraft_app::Command::AddMaskEffect { layer, kind }], now);
            }
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
        Purpose::MaskImage { layer, effect } => match state.session.import_image(first) {
            Ok(r) => {
                state.remember_textures(std::slice::from_ref(first));
                let current = state.session.selected_texture_set().find(layer).and_then(|l| l.mask.as_ref()).and_then(|m| m.effects.iter().find(|e| e.id == effect)).map(|e| (**e).clone());
                if let Some(mut e) = current
                    && let subcraft_doc::MaskEffectKind::Image { resource, .. } = &mut e.kind
                {
                    *resource = r;
                    state.run(vec![subcraft_app::Command::SetMaskEffect { layer, effect: e }], now);
                }
            }
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
    }
}
