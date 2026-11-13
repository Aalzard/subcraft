//! Asset libraries, as Painter's Settings > Libraries lists them: the built-in starter assets,
//! the user's library (your_assets), extra library folders the user adds, and the current
//! session (resources imported without copying them anywhere, gone on quit).

use std::path::{Path, PathBuf};

use egui::RichText;
use subcraft_app::assets::{UserAssetKind, UserLibrary};

use crate::i18n::tr;
use crate::panels::shelf_filter::{STARTER, YOURS};
use crate::state::UiState;
use crate::theme;

/// Library of the resources imported for the current session only.
pub const SESSION: &str = "session";

/// Ids the user cannot give an extra library.
const RESERVED: [&str; 4] = [STARTER, YOURS, SESSION, "project"];

/// An extra library folder added in Settings > Libraries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryPath {
    /// Its name: the breadcrumbs, Filter by path and `s:` use it.
    pub name: String,
    pub path: String,
}

/// A library on disk with the id the Assets panel knows it by.
#[derive(Clone, Debug)]
pub struct NamedLibrary {
    pub id: String,
    pub lib: UserLibrary,
}

/// Why a new library cannot be added, or `None` when it can.
pub fn check_new_library(name: &str, path: &str, existing: &[LibraryPath]) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Give the library a name");
    }
    if path.trim().is_empty() {
        return Some("Choose the library folder");
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) || existing.iter().any(|l| l.name.trim().eq_ignore_ascii_case(name)) {
        return Some("A library already has this name");
    }
    if name.contains(['/', '"', ',']) {
        return Some("A library name cannot hold / , or quotes");
    }
    None
}

impl UiState {
    /// The library a file belongs to: built-in content has no file; a file inside a library
    /// folder belongs to that library; any other file was imported for this session only.
    pub fn library_of(&self, file: Option<&Path>) -> String {
        match file {
            None => STARTER.to_string(),
            Some(f) => self.libraries.iter().find(|l| l.lib.contains(f)).map_or_else(|| SESSION.to_string(), |l| l.id.clone()),
        }
    }

    /// Sub-folder of a library file inside its kind's folder (its category in the Assets panel).
    pub fn library_subfolder(&self, kind: UserAssetKind, file: Option<&Path>) -> Option<String> {
        let f = file?;
        self.libraries.iter().find(|l| l.lib.contains(f)).and_then(|l| l.lib.subfolder(kind, f))
    }

    pub fn library_by_id(&self, id: &str) -> Option<&UserLibrary> {
        self.libraries.iter().find(|l| l.id == id).map(|l| &l.lib)
    }

    /// Use the extra libraries of the preferences and `default` as the library new presets and
    /// imports go to, then read every library again.
    pub fn set_libraries(&mut self, extra: &[LibraryPath], default: &str) {
        let yours = self.libraries.iter().find(|l| l.id == YOURS).cloned();
        self.libraries = yours.into_iter().collect();
        for l in extra {
            let id = l.name.trim().to_string();
            if id.is_empty() || self.libraries.iter().any(|x| x.id.eq_ignore_ascii_case(&id)) {
                continue;
            }
            self.libraries.push(NamedLibrary { id, lib: UserLibrary::new(PathBuf::from(l.path.trim())) });
        }
        if let Some(d) = self.libraries.iter().find(|l| l.id == default).or_else(|| self.libraries.first()) {
            self.user_library = Some(d.lib.clone());
        }
        self.refresh_libraries();
    }

    /// Read the libraries again (files added or removed on disk, a library added or removed in
    /// the settings). Built-in assets and the current session's imports stay.
    pub fn refresh_libraries(&mut self) {
        let keep = |file: Option<&Path>, session: &std::collections::HashSet<PathBuf>| file.is_none_or(|f| session.contains(f));
        let session = self.session_files.clone();
        self.library.alphas.retain(|a| keep(a.user_file.as_deref(), &session));
        self.library.smart_materials.retain(|m| keep(m.user_file.as_deref(), &session));
        self.library.smart_masks.retain(|m| keep(m.user_file.as_deref(), &session));
        let env_before = self.library.environments.get(self.env.index).map(|e| e.id);
        self.library.environments.retain(|e| keep(e.user_file.as_deref(), &session));
        self.env.index = env_before.and_then(|id| self.library.environments.iter().position(|e| e.id == id)).unwrap_or(0);
        self.env.uploaded = None;
        self.brush_presets.retain(|p| keep(p.user_file.as_deref(), &session));
        self.textures.items.retain(|t| session.contains(&t.path));
        let libraries = self.libraries.clone();
        let mut environments = Vec::new();
        for l in &libraries {
            self.load_library_content(&l.lib);
            environments.extend(l.lib.list(UserAssetKind::Environment));
        }
        if let Some(lib) = &self.user_library {
            self.favorites = subcraft_app::brush_preset::Favorites::load(lib);
        }
        self.load_environments(environments);
    }

    /// Add the alphas, smart materials, smart masks, brush presets and textures of `lib` to the
    /// shelf (environments load in the background: [`Self::load_environments`]).
    pub fn load_library_content(&mut self, lib: &UserLibrary) {
        let (alphas, errors) = lib.load_alphas();
        for e in errors {
            log::warn!("library alpha skipped: {e}");
        }
        for a in alphas {
            if !self.library.alphas.iter().any(|x| x.id == a.id) {
                self.library.alphas.push(a);
            }
        }
        let (materials, errors) = subcraft_app::library::load_smart_materials(lib);
        for e in errors {
            log::warn!("library smart material skipped: {e}");
        }
        self.library.smart_materials.extend(materials);
        let (masks, errors) = subcraft_app::library::load_smart_masks(lib);
        for e in errors {
            log::warn!("library smart mask skipped: {e}");
        }
        self.library.smart_masks.extend(masks);
        let (presets, errors) = subcraft_app::brush_preset::load_brush_presets(lib);
        for e in errors {
            log::warn!("library brush preset skipped: {e}");
        }
        self.brush_presets.extend(presets);
        self.textures.add_listed(&lib.list(UserAssetKind::Texture));
    }

    /// Prefilter `files` (HDRIs) on a background thread; [`Self::poll_library`] picks them up.
    pub fn load_environments(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        subcraft_core::task::spawn(move || {
            for f in files {
                if tx.send(subcraft_app::assets::user_library::load_environment(&f)).is_err() {
                    break;
                }
            }
        });
        // A loader still running is replaced: its files are part of this list again.
        self.env_loader = Some(rx);
    }

    /// Put one library (or session) file of `kind` into the shelf. Returns its asset name.
    pub fn add_library_file(&mut self, kind: UserAssetKind, file: &Path) -> Result<String, String> {
        use subcraft_app::assets::user_library::{load_alpha, load_environment};
        let same = |f: Option<&Path>| f == Some(file);
        match kind {
            UserAssetKind::Texture => {
                self.textures.add_listed(&[file.to_path_buf()]);
                Ok(file.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string())
            }
            UserAssetKind::Alpha => {
                let a = load_alpha(file)?;
                let name = a.name.clone();
                if !self.library.alphas.iter().any(|x| x.id == a.id) {
                    self.library.alphas.push(a);
                }
                Ok(name)
            }
            UserAssetKind::Environment => {
                let e = load_environment(file)?;
                let name = e.name.clone();
                if !self.library.environments.iter().any(|x| x.id == e.id) {
                    self.library.environments.push(e);
                }
                Ok(name)
            }
            UserAssetKind::SmartMaterial => {
                let m = subcraft_app::library::load_smart_material_file(file)?;
                let name = m.name.clone();
                if !self.library.smart_materials.iter().any(|x| same(x.user_file.as_deref())) {
                    self.library.smart_materials.push(m);
                }
                Ok(name)
            }
            UserAssetKind::SmartMask => {
                let m = subcraft_app::library::load_smart_mask_file(file)?;
                let name = m.name.clone();
                if !self.library.smart_masks.iter().any(|x| same(x.user_file.as_deref())) {
                    self.library.smart_masks.push(m);
                }
                Ok(name)
            }
            UserAssetKind::BrushPreset => {
                let p = subcraft_app::brush_preset::load_brush_preset(file)?;
                let name = p.name.clone();
                if !self.brush_presets.iter().any(|x| same(x.user_file.as_deref())) {
                    self.brush_presets.push(p);
                }
                Ok(name)
            }
            UserAssetKind::ExportPreset => Err("export presets are imported from the Export window".into()),
        }
    }
}

/// What the Libraries settings asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibrariesChange {
    /// A library was added or removed, or the default changed: apply and read them again.
    Changed,
    /// Read the libraries again.
    Refresh,
}

/// Name and folder typed for the next library.
#[derive(Clone, Debug, Default)]
pub struct LibrariesUi {
    pub name: String,
    pub path: String,
}

/// Settings > Libraries: the libraries with their folders, the default one (where new presets
/// and imports go), removing an added library, adding one (name, folder, +) and Refresh.
pub fn settings(ui: &mut egui::Ui, prefs: &mut crate::recovery::Preferences, yours: Option<&Path>, form: &mut LibrariesUi) -> Option<LibrariesChange> {
    let mut change = None;
    ui.label(RichText::new(tr("Libraries")).font(theme::font_strong(12.5)));
    ui.add_space(4.0);
    ui.label(RichText::new(tr("Folders the Assets panel reads. The default library receives new presets and imports.")).color(theme::TEXT_DIM));
    ui.add_space(6.0);
    let mut remove = None;
    egui::Grid::new("libraries_grid").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(RichText::new(tr("Default")).color(theme::TEXT_DIM));
        ui.label(RichText::new(tr("Name")).color(theme::TEXT_DIM));
        ui.label(RichText::new(tr("Path")).color(theme::TEXT_DIM));
        ui.label("");
        ui.end_row();
        ui.label("");
        ui.label(STARTER);
        ui.label(RichText::new(tr("built in (read only)")).color(theme::TEXT_FAINT));
        ui.label("");
        ui.end_row();
        if let Some(path) = yours {
            if ui.radio(prefs.default_library == YOURS, "").clicked() && prefs.default_library != YOURS {
                prefs.default_library = YOURS.to_string();
                change = Some(LibrariesChange::Changed);
            }
            ui.label(YOURS);
            ui.label(path.display().to_string());
            ui.label("");
            ui.end_row();
        }
        for (i, l) in prefs.extra_libraries.iter().enumerate() {
            let is_default = prefs.default_library == l.name;
            if ui.radio(is_default, "").clicked() && !is_default {
                prefs.default_library = l.name.clone();
                change = Some(LibrariesChange::Changed);
            }
            ui.label(&l.name);
            ui.label(&l.path);
            if crate::widgets::icon_button(ui, crate::icons::X, tr("Remove this library (its files stay on disk)")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        let removed = prefs.extra_libraries.remove(i);
        if prefs.default_library == removed.name {
            prefs.default_library = YOURS.to_string();
        }
        change = Some(LibrariesChange::Changed);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut form.name).hint_text(tr("Name")).desired_width(90.0));
        ui.add(egui::TextEdit::singleline(&mut form.path).hint_text(tr("Path")).desired_width(160.0));
        if ui.button(crate::icons::FOLDER_OPEN).on_hover_text(tr("Choose the library folder")).clicked()
            && let Some(dir) = rfd::FileDialog::new().set_title(tr("Choose the library folder")).pick_folder()
        {
            form.path = dir.display().to_string();
        }
        let problem = check_new_library(&form.name, &form.path, &prefs.extra_libraries);
        let add = ui.add_enabled(problem.is_none(), egui::Button::new(crate::icons::PLUS));
        let add = match problem {
            Some(why) => add.on_disabled_hover_text(tr(why)),
            None => add.on_hover_text(tr("Add the library (the folder is created when missing)")),
        };
        if add.clicked() {
            let path = form.path.trim().to_string();
            if let Err(e) = std::fs::create_dir_all(&path) {
                log::warn!("could not create library folder {path}: {e}");
            }
            prefs.extra_libraries.push(LibraryPath { name: form.name.trim().to_string(), path });
            *form = LibrariesUi::default();
            change = Some(LibrariesChange::Changed);
        }
    });
    ui.add_space(4.0);
    if crate::widgets::secondary_button(ui, tr("Refresh libraries")).on_hover_text(tr("Read every library folder again (files added or removed outside SubCraft)")).clicked() && change.is_none() {
        change = Some(LibrariesChange::Refresh);
    }
    change
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::shelf::ShelfTab;

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-libraries-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        subcraft_image::save_image(path, 8, 8, 1, &[0.6; 64], subcraft_image::FileFormat::Png8).unwrap();
    }

    fn state_with(yours: &Path) -> UiState {
        UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(1.0), 64), None).with_user_library(Some(UserLibrary::new(yours)))
    }

    #[test]
    fn new_library_names_must_be_free_and_plain() {
        let existing = vec![LibraryPath { name: "Studio".into(), path: "D:/lib".into() }];
        assert_eq!(check_new_library("", "x", &existing), Some("Give the library a name"));
        assert_eq!(check_new_library("Kit", " ", &existing), Some("Choose the library folder"));
        assert_eq!(check_new_library("studio", "x", &existing), Some("A library already has this name"));
        assert_eq!(check_new_library("your_assets", "x", &existing), Some("A library already has this name"));
        assert_eq!(check_new_library("a/b", "x", &existing), Some("A library name cannot hold / , or quotes"));
        assert_eq!(check_new_library("Kit", "x", &existing), None);
    }

    #[test]
    fn an_extra_library_is_read_listed_and_can_be_the_default() {
        let root = temp_root("extra");
        let yours = root.join("yours");
        let kit = root.join("kit");
        png(&UserLibrary::new(&kit).folder(UserAssetKind::Alpha).join("Splats").join("splat.png"));
        let mut s = state_with(&yours);
        let before = s.library.alphas.len();
        s.set_libraries(&[LibraryPath { name: "Kit".into(), path: kit.display().to_string() }], "Kit");
        assert_eq!(s.library.alphas.len(), before + 1, "the kit's alpha is in the shelf");
        let splat = s.library.alphas.iter().find(|a| a.name == "splat").unwrap();
        assert_eq!(s.library_of(splat.user_file.as_deref()), "Kit");
        assert_eq!(s.library_subfolder(UserAssetKind::Alpha, splat.user_file.as_deref()).as_deref(), Some("Splats"), "a sub-folder is its category");
        assert_eq!(s.user_library.as_ref().map(|l| l.root().to_path_buf()), Some(kit.clone()), "new presets go to the default library");
        let alphas = crate::panels::shelf::items(&s, ShelfTab::Alphas);
        assert!(alphas.iter().any(|a| a.library == "Kit" && a.folder.as_deref() == Some("Splats")), "the Assets panel places it");
        // Removing the library takes its assets out of the shelf again.
        s.set_libraries(&[], "Kit");
        assert_eq!(s.library.alphas.len(), before);
        assert_eq!(s.user_library.as_ref().map(|l| l.root().to_path_buf()), Some(yours.clone()), "back to your_assets");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn refresh_finds_new_files_and_keeps_session_imports() {
        let root = temp_root("refresh");
        let yours = root.join("yours");
        let mut s = state_with(&yours);
        let before = s.library.alphas.len();
        let outside = root.join("elsewhere").join("dots.png");
        png(&outside);
        s.session_files.insert(outside.clone());
        s.add_library_file(UserAssetKind::Alpha, &outside).unwrap();
        assert_eq!(s.library_of(Some(&outside)), SESSION);
        png(&UserLibrary::new(&yours).folder(UserAssetKind::Alpha).join("new.png"));
        s.refresh_libraries();
        assert_eq!(s.library.alphas.len(), before + 2, "the new library file and the session import");
        assert!(s.library.alphas.iter().any(|a| a.name == "dots") && s.library.alphas.iter().any(|a| a.name == "new"));
        s.refresh_libraries();
        assert_eq!(s.library.alphas.len(), before + 2, "refreshing twice adds nothing");
        std::fs::remove_dir_all(&root).ok();
    }
}
