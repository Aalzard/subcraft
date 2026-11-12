//! Picking files with the native dialog.
//!
//! Callers say what the files are for ([`Purpose`]) instead of handling the answer inline.
//! [`poll`] (once per frame) hands the chosen paths to [`handle`].

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
    /// Already-baked mesh maps for the New Project window.
    NewProjectMeshMaps,
    /// Already-baked mesh maps for the project's texture sets (Texture Set Settings).
    ImportMeshMaps,
    OpenProject,
    /// The updated model for File > Reload mesh.
    ReloadMesh,
    BakeHighPoly,
    Stencil,
    /// A `.ttf` / `.otf` font for the text tool.
    Font,
    TextureSet,
    /// `.scsmart` files and texture images to import as smart materials.
    SmartMaterials,
    /// A folder to search for smart materials and PBR texture sets.
    SmartMaterialFolder,
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
    /// Image a filter reads (Gradient Dynamic's source), in a mask (`mask`) or content stack.
    FilterImage { layer: LayerId, effect: EffectId, mask: bool },
    /// Custom grunge image of a generator mask effect.
    GeneratorGrunge { layer: LayerId, effect: EffectId },
    /// Image of a Mask Editor generator's texture group (`slot` 0 = Texture, 1 = Texture 2).
    GeneratorTexture { layer: LayerId, effect: EffectId, slot: u8 },
}

pub const SMART_MATERIALS: (&str, &[&str]) = ("Smart materials and textures", &["scsmart", "png", "jpg", "jpeg", "tga", "tif", "tiff", "exr", "bmp"]);
pub const IMAGES: (&str, &[&str]) = ("Images", &subcraft_image::IMAGE_EXTENSIONS);
pub const MESHES: (&str, &[&str]) = ("Meshes", &subcraft_mesh::SUPPORTED_EXTENSIONS);
pub const PROJECTS: (&str, &[&str]) = ("SubCraft project", &[subcraft_app::PROJECT_EXTENSION]);
pub const PAINTER_PROJECTS: (&str, &[&str]) = ("Substance Painter project \u{2014} reconstructed", &["spp"]);
/// File > Open: both project kinds, first entry selected. Only `.subcraft` used to be offered, so
/// a Painter `.spp` could not be picked there at all (it was only reachable from New).
pub const OPEN_FILTERS: [(&str, &[&str]); 3] =
    [("All supported projects", &[subcraft_app::PROJECT_EXTENSION, "spp"]), PROJECTS, PAINTER_PROJECTS];
pub const FONTS: (&str, &[&str]) = ("Fonts", &["ttf", "otf", "ttc"]);
pub const ALPHAS: (&str, &[&str]) = ("Images", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"]);
pub const PANORAMAS: (&str, &[&str]) = ("Panoramas", &["hdr", "exr", "png", "jpg", "jpeg"]);

static PICKED: Mutex<Vec<(Purpose, Vec<PathBuf>)>> = Mutex::new(Vec::new());

fn deliver(purpose: Purpose, paths: Vec<PathBuf>) {
    if !paths.is_empty() {
        PICKED.lock().expect("picked files").push((purpose, paths));
    }
}

/// Ask for one file (or several with `multiple`) for `purpose`.
pub fn pick(_ctx: &egui::Context, purpose: Purpose, filter: (&str, &[&str]), multiple: bool, title: Option<&str>) {
    let mut d = rfd::FileDialog::new().add_filter(filter.0, filter.1);
    if let Some(t) = title {
        d = d.set_title(t);
    }
    let paths = if multiple { d.pick_files().unwrap_or_default() } else { d.pick_file().into_iter().collect() };
    deliver(purpose, paths);
}

/// [`pick`] with several filters, the first one selected.
pub fn pick_with_filters(purpose: Purpose, filters: &[(&str, &[&str])], title: Option<&str>) {
    let mut d = rfd::FileDialog::new();
    for (name, extensions) in filters {
        d = d.add_filter(*name, extensions);
    }
    if let Some(t) = title {
        d = d.set_title(t);
    }
    deliver(purpose, d.pick_file().into_iter().collect());
}

/// Ask for a folder for `purpose`.
pub fn pick_folder(_ctx: &egui::Context, purpose: Purpose, title: &str) {
    let paths: Vec<PathBuf> = rfd::FileDialog::new().set_title(title).pick_folder().into_iter().collect();
    deliver(purpose, paths);
}

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

/// Import smart materials (`.scsmart` files, folders of them, folders / images of PBR texture
/// sets) into the user library and the shelf. Problems are listed in the log; the status line
/// gives the outcome.
pub fn import_smart_materials(state: &mut UiState, paths: &[PathBuf], now: f64) {
    let Some(lib) = state.user_library.clone() else {
        state.set_status("No library folder (Documents not found; set SUBCRAFT_LIBRARY)", now);
        return;
    };
    let report = subcraft_app::smart_import::import_smart_materials(paths, &lib, &state.library.smart_materials);
    for line in report.lines() {
        log::info!("import smart materials: {line}");
    }
    if !report.imported.is_empty() || !report.already_present.is_empty() {
        state.shelf.tab = crate::panels::shelf::ShelfTab::SmartMaterials;
    }
    let msg = report.summary();
    let lines = report.lines();
    state.library.smart_materials.extend(report.imported);
    // Only problems open the list; a clean import just says so in the status line.
    state.import_report = if report.errors.is_empty() { Vec::new() } else { lines };
    state.set_status(msg, now);
}

/// The list of problems of the last smart-material import.
pub fn import_report_window(state: &mut UiState, ctx: &egui::Context) {
    if state.import_report.is_empty() {
        return;
    }
    let mut open = true;
    egui::Window::new("Smart material import").open(&mut open).default_width(520.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for line in &state.import_report {
                ui.add(egui::Label::new(line).wrap());
            }
        });
    });
    if !open {
        state.import_report.clear();
    }
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
        Err(_) => state.set_status(format!("Added {} texture(s) to Shelf › Textures (normal maps appear under Normal Maps)", paths.len()), now),
    }
}

/// Import a Painter smart material (`.spsm`) onto the selected texture set (one undo step).
pub fn import_painter_smart_material(state: &mut UiState, path: &std::path::Path, now: f64) {
    match state.session.import_painter_smart_material(path) {
        Ok((name, count)) => state.set_status(format!("Smart material \"{name}\" added ({count} layers)"), now),
        Err(e) => state.set_status(format!("Smart material: {e}"), now),
    }
}

/// What a file dropped anywhere on the window (outside the Shelf) does: a model starts a New
/// Project with it (after the unsaved-changes question), a `.subcraft` project opens, a Painter
/// smart material (`.spsm`) is added to the selected texture set.
pub enum WindowDrop {
    Model(PathBuf),
    Project(PathBuf),
    SmartMaterial(PathBuf),
    /// A Painter project: imported straight away (after the unsaved-changes question).
    PainterProject(PathBuf),
}

/// Classify dropped files (unknown types are ignored).
pub fn window_drops(paths: &[PathBuf]) -> Vec<WindowDrop> {
    let ext = |p: &PathBuf| p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    paths.iter().filter_map(|p| {
        let e = ext(p);
        if e == "spsm" { Some(WindowDrop::SmartMaterial(p.clone())) }
        else if e == "spp" { Some(WindowDrop::PainterProject(p.clone())) }
        else if e == subcraft_app::PROJECT_EXTENSION { Some(WindowDrop::Project(p.clone())) }
        else if subcraft_mesh::SUPPORTED_EXTENSIONS.contains(&e.as_str()) { Some(WindowDrop::Model(p.clone())) }
        else { None }
    }).collect()
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
        Purpose::NewProjectMeshMaps => state.new_project.add_mesh_maps(paths),
        Purpose::ImportMeshMaps => {
            let sets = state.session.mesh_map_set_keys();
            if state.mesh_map_import.open {
                crate::mesh_map_import::add_files(&mut state.mesh_map_import.files, paths, &sets);
            } else {
                state.mesh_map_import.start(paths, &sets);
            }
            let unknown = state.mesh_map_import.files.iter().filter(|f| f.kind.is_none()).count();
            if unknown > 0 {
                state.set_status(format!("{unknown} file(s) not recognised as a mesh map by name: choose the map in the list or leave them out"), now);
            }
        }
        Purpose::OpenMesh if crate::spp_import::is_spp(first) => crate::spp_import::start(state, first),
        Purpose::OpenMesh => match subcraft_app::Session::open_mesh(first, subcraft_app::DEFAULT_RESOLUTION) {
            Ok((s, warnings)) => {
                state.set_session(s);
                state.welcome.remember(first);
                let status = match warnings.first() {
                    Some(w) => format!("Opened {} with {} warning(s): {w}", file_name(first), warnings.len()),
                    None => format!("Opened {}", file_name(first)),
                };
                state.set_status(status, now);
            }
            Err(e) => state.set_status(format!("Could not open mesh: {e}"), now),
        },
        // File > Open offers Painter projects too: those are imported.
        Purpose::OpenProject if crate::spp_import::is_spp(first) => crate::spp_import::start(state, first),
        Purpose::OpenProject => crate::dialogs::open_project(state, first, now),
        Purpose::ReloadMesh => {
            state.mesh_reload.path = first.display().to_string();
            crate::mesh_reload::start_check(state);
        }
        Purpose::BakeHighPoly => state.bake.high_poly = first.display().to_string(),
        Purpose::Stencil => match subcraft_image::load_bitmap(first) {
            Ok(bmp) => state.session.brush.stencil = Some(std::sync::Arc::new(subcraft_paint::Stencil::new(std::sync::Arc::new(bmp)))),
            Err(e) => state.set_status(format!("Stencil: {e}"), now),
        },
        Purpose::Font => match state.session.import_font(first) {
            Ok(font) => {
                if let Some(d) = state.text.edit.as_mut() { d.font = font.clone(); }
                if let Some(d) = state.text.draft.as_mut() { d.font = font.clone(); }
                // Editing a text layer: apply the new font straight away.
                if let Some(d) = state.text.edit.clone() && state.session.selected_text().is_some() {
                    if let Err(e) = state.session.update_selected_text(d) { state.set_status(format!("Text: {e}"), now); }
                } else {
                    state.set_status("Font added to the project", now);
                }
            }
            Err(e) => state.set_status(format!("Font: {e}"), now),
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
        Purpose::SmartMaterials | Purpose::SmartMaterialFolder => import_smart_materials(state, paths, now),
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
                // "Add bitmap mask" on a layer without a mask: a black mask first, then the image.
                let masked = state.session.selected_texture_set().find(layer).is_some_and(|l| l.mask.is_some());
                let mut cmds = Vec::new();
                if !masked { cmds.push(subcraft_app::Command::AddMask { layer, white: false }); }
                cmds.push(subcraft_app::Command::AddMaskEffect { layer, kind });
                state.run(cmds, now);
            }
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
        Purpose::GeneratorTexture { layer, effect, slot } => match state.session.import_image(first) {
            Ok(r) => {
                state.remember_textures(std::slice::from_ref(first));
                let current = state.session.selected_texture_set().find(layer).and_then(|l| l.mask.as_ref()).and_then(|m| m.effects.iter().find(|e| e.id == effect)).map(|e| (**e).clone());
                if let Some(mut e) = current
                    && let subcraft_doc::MaskEffectKind::Generator(g) = &mut e.kind
                {
                    if slot == 2 {
                        // Grayscale Conversion's source image.
                        g.shape = Some(subcraft_doc::ShapeSettings { texture: Some(r), ..g.shape_settings() });
                    } else {
                        let mut editor = g.editor_settings();
                        if slot == 0 { editor.texture.resource = Some(r) } else { editor.texture2.resource = Some(r) }
                        g.editor = Some(editor);
                    }
                    state.run(vec![subcraft_app::Command::SetMaskEffect { layer, effect: e }], now);
                }
            }
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
        Purpose::FilterImage { layer, effect, mask } => match state.session.import_image(first) {
            Ok(r) => {
                state.remember_textures(std::slice::from_ref(first));
                let set_source = |f: &mut subcraft_doc::Filter| {
                    if let subcraft_doc::Filter::GradientDynamic { source, .. } = f {
                        *source = r;
                        true
                    } else {
                        false
                    }
                };
                let l = state.session.selected_texture_set().find(layer).cloned();
                let cmd = l.and_then(|l| {
                    if mask {
                        let mut e = (**l.mask.as_ref()?.effects.iter().find(|e| e.id == effect)?).clone();
                        let subcraft_doc::MaskEffectKind::Filter(f) = &mut e.kind else { return None };
                        set_source(f).then(|| subcraft_app::Command::SetMaskEffect { layer, effect: e })
                    } else {
                        let mut e = (**l.content_effects.iter().find(|e| e.id == effect)?).clone();
                        let subcraft_doc::ContentEffectKind::Filter { filter, .. } = &mut e.kind else { return None };
                        set_source(filter).then(|| subcraft_app::Command::SetContentEffect { layer, effect: e })
                    }
                });
                if let Some(cmd) = cmd {
                    state.run(vec![cmd], now);
                }
            }
            Err(e) => state.set_status(format!("Import image: {e}"), now),
        },
        Purpose::GeneratorGrunge { layer, effect } => match state.session.import_image(first) {
            Ok(r) => {
                state.remember_textures(std::slice::from_ref(first));
                let current = state.session.selected_texture_set().find(layer).and_then(|l| l.mask.as_ref()).and_then(|m| m.effects.iter().find(|e| e.id == effect)).map(|e| (**e).clone());
                if let Some(mut e) = current
                    && let subcraft_doc::MaskEffectKind::Generator(g) = &mut e.kind
                {
                    g.painter.get_or_insert_with(subcraft_doc::PainterControls::default).custom_grunge = Some(r);
                    state.run(vec![subcraft_app::Command::SetMaskEffect { layer, effect: e }], now);
                }
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

#[cfg(test)]
mod window_drop_tests {
    use super::{WindowDrop, window_drops};
    use std::path::PathBuf;

    #[test]
    fn models_projects_and_smart_materials_are_recognised_by_extension() {
        let paths: Vec<PathBuf> = ["tank.FBX", "crate.obj", "scene.glb", "old.subcraft", "Leather.spsm", "notes.txt", "photo.png", "Robot.SPP"].iter().map(PathBuf::from).collect();
        let kinds: Vec<&str> = window_drops(&paths).iter().map(|d| match d {
            WindowDrop::Model(_) => "model",
            WindowDrop::Project(_) => "project",
            WindowDrop::SmartMaterial(_) => "smart",
            WindowDrop::PainterProject(_) => "painter",
        }).collect();
        assert_eq!(kinds, ["model", "model", "model", "project", "smart", "painter"], "unknown types are ignored");
    }

    /// Discord report (Windows): File > Open showed no file type for `.spp`, so a Painter
    /// project could not be opened there. The dialog lists both kinds, "all supported" first.
    #[test]
    fn file_open_offers_subcraft_and_painter_projects() {
        let filters = super::OPEN_FILTERS;
        assert_eq!(filters[0].1, [subcraft_app::PROJECT_EXTENSION, "spp"], "the default filter shows both");
        assert!(filters.iter().any(|f| f.1 == [subcraft_app::PROJECT_EXTENSION]));
        assert!(filters.iter().any(|f| f.0.contains("Painter") && f.0.contains("reconstructed") && f.1 == ["spp"]), "the name says it is a reconstruction");
    }

    /// A `.spp` picked in File > Open is imported (it is not a SubCraft project file).
    #[test]
    fn a_painter_project_picked_in_file_open_starts_the_import() {
        let dir = std::env::temp_dir().join(format!("subcraft-open-spp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Missing.spp"); // the import fails on the missing file; it must still be the import that runs
        let mut state = crate::state::UiState::new(subcraft_app::Session::sample(), None);
        super::handle(&mut state, super::Purpose::OpenProject, &[path], 0.0);
        assert!(state.spp_import.is_some(), "the Painter import started");
        let _ = std::fs::remove_dir_all(dir);
    }
}
