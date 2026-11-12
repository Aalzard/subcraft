//! File > Import resources (also the Assets panel's + button and files dropped on the panel):
//! one row per file with its usage and prefix (sub-folder), then Import into the current
//! session or a library.

use std::path::{Path, PathBuf};

use egui::RichText;
use subcraft_app::assets::UserAssetKind;

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme;

/// What an imported file is used as (Painter's usage).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usage {
    /// Not decided yet: the row is skipped until a usage is chosen.
    Undefined,
    Texture,
    Alpha,
    Environment,
    SmartMaterial,
    SmartMask,
    BrushPreset,
}

impl Usage {
    pub const ALL: [Usage; 7] = [Usage::Undefined, Usage::Texture, Usage::Alpha, Usage::Environment, Usage::SmartMaterial, Usage::SmartMask, Usage::BrushPreset];

    pub fn label(self) -> &'static str {
        match self {
            Usage::Undefined => tr("undefined"),
            Usage::Texture => tr("texture"),
            Usage::Alpha => tr("alpha"),
            Usage::Environment => tr("environment"),
            Usage::SmartMaterial => tr("smart material"),
            Usage::SmartMask => tr("smart mask"),
            Usage::BrushPreset => tr("brush preset"),
        }
    }

    pub fn kind(self) -> Option<UserAssetKind> {
        match self {
            Usage::Undefined => None,
            Usage::Texture => Some(UserAssetKind::Texture),
            Usage::Alpha => Some(UserAssetKind::Alpha),
            Usage::Environment => Some(UserAssetKind::Environment),
            Usage::SmartMaterial => Some(UserAssetKind::SmartMaterial),
            Usage::SmartMask => Some(UserAssetKind::SmartMask),
            Usage::BrushPreset => Some(UserAssetKind::BrushPreset),
        }
    }

    /// The Assets panel type that shows assets of this usage.
    pub fn shelf_tab(self) -> Option<crate::panels::shelf::ShelfTab> {
        use crate::panels::shelf::ShelfTab;
        Some(match self {
            Usage::Undefined => return None,
            Usage::Texture => ShelfTab::Textures,
            Usage::Alpha => ShelfTab::Alphas,
            Usage::Environment => ShelfTab::Environments,
            Usage::SmartMaterial => ShelfTab::SmartMaterials,
            Usage::SmartMask => ShelfTab::SmartMasks,
            Usage::BrushPreset => ShelfTab::Brushes,
        })
    }

    /// True when a file with extension `ext` can be used this way.
    pub fn accepts(self, ext: &str) -> bool {
        self.kind().is_some_and(|k| k.extensions().contains(&ext.to_ascii_lowercase().as_str()))
    }

    /// Painter's rule: a format with a single use gets it; an image can be several things
    /// (texture, alpha, environment) and starts undefined, except a `.hdr` panorama.
    pub fn guess(path: &Path) -> Usage {
        let ext = extension(path);
        match ext.as_str() {
            "scsmart" => Usage::SmartMaterial,
            "scmask" => Usage::SmartMask,
            "scbrush" => Usage::BrushPreset,
            "hdr" => Usage::Environment,
            _ => Usage::Undefined,
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// True when some usage can read the file.
pub fn supported(path: &Path) -> bool {
    let ext = extension(path);
    Usage::ALL.iter().any(|u| u.accepts(&ext))
}

/// One file of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub usage: Usage,
    /// Sub-folder inside the library ("grunge/old"); it becomes the asset's category.
    pub prefix: String,
    pub selected: bool,
}

impl Row {
    /// What is wrong with the row, if anything (shown as a warning; the row is not imported).
    pub fn warning(&self) -> Option<&'static str> {
        let ext = extension(&self.path);
        if !supported(&self.path) {
            Some("This file format cannot be imported")
        } else if self.usage != Usage::Undefined && !self.usage.accepts(&ext) {
            Some("This usage does not read this file format")
        } else if !self.path.is_file() {
            Some("The file is missing")
        } else {
            None
        }
    }

    pub fn importable(&self) -> bool {
        self.usage != Usage::Undefined && self.warning().is_none()
    }
}

/// Where Import puts the files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Read from where they are, for this run only.
    Session,
    /// Copied into the library with this id.
    Library(String),
}

/// The Import resources window.
#[derive(Clone, Debug, Default)]
pub struct ImportWindow {
    pub open: bool,
    pub rows: Vec<Row>,
    /// Show only the rows of one usage (`None`: all).
    pub filter: Option<Usage>,
    /// `None`: the default library.
    pub destination: Option<Destination>,
    /// Where the window was last drawn (files dropped there are added).
    rect: Option<egui::Rect>,
}

/// Deepest folder level searched for files when a folder is added.
const MAX_DEPTH: usize = 6;

fn files_in(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            if depth < MAX_DEPTH {
                files_in(&p, depth + 1, out);
            }
        } else if supported(&p) {
            out.push(p);
        }
    }
}

impl ImportWindow {
    /// Add files (a folder adds the files it holds); a file already listed is not added twice.
    pub fn add(&mut self, paths: &[PathBuf]) {
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                files_in(p, 0, &mut files);
            } else {
                files.push(p.clone());
            }
        }
        for path in files {
            if self.rows.iter().any(|r| r.path == path) {
                continue;
            }
            let usage = Usage::guess(&path);
            self.rows.push(Row { path, usage, prefix: String::new(), selected: false });
        }
    }

    /// Toolbar: Remove selected resources.
    pub fn remove_selected(&mut self) {
        self.rows.retain(|r| !r.selected);
    }

    /// A row's usage changed: with several rows selected, every selected row takes it.
    pub fn set_usage(&mut self, row: usize, usage: Usage) {
        let Some(r) = self.rows.get(row) else { return };
        if r.selected {
            for r in self.rows.iter_mut().filter(|r| r.selected) {
                r.usage = usage;
            }
        } else {
            self.rows[row].usage = usage;
        }
    }

    /// Rows the usage filter shows.
    pub fn visible(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.filter.is_none_or(|u| self.rows[i].usage == u)).collect()
    }

    pub fn importable(&self) -> usize {
        self.rows.iter().filter(|r| r.importable()).count()
    }
}

/// Open the window with `paths` added (files dropped on the Assets panel, File > Import).
pub fn open_with(state: &mut UiState, paths: &[PathBuf]) {
    state.import.open = true;
    state.import.add(paths);
}

/// What one Import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportResult {
    pub imported: Vec<String>,
    pub errors: Vec<String>,
}

/// Import every importable row to the chosen destination; imported rows leave the window.
pub fn import(state: &mut UiState) -> ImportResult {
    let mut result = ImportResult::default();
    let destination = state.import.destination.clone().unwrap_or_else(|| default_destination(state));
    let library = match &destination {
        Destination::Session => None,
        Destination::Library(id) => match state.library_by_id(id) {
            Some(lib) => Some(lib.clone()),
            None => {
                result.errors.push(format!("library \"{id}\" is not available"));
                return result;
            }
        },
    };
    let mut last_tab = None;
    let mut done = Vec::new();
    let rows = state.import.rows.clone();
    for (i, row) in rows.iter().enumerate() {
        let Some(kind) = row.usage.kind().filter(|_| row.importable()) else { continue };
        let file = match &library {
            Some(lib) => lib.add_file_to(kind, &row.prefix, &row.path),
            None => Ok(row.path.clone()),
        };
        match file.and_then(|f| {
            if library.is_none() {
                state.session_files.insert(f.clone());
            }
            state.add_library_file(kind, &f)
        }) {
            Ok(name) => {
                result.imported.push(name);
                last_tab = row.usage.shelf_tab();
                done.push(i);
            }
            Err(e) => result.errors.push(e),
        }
    }
    let mut k = 0;
    state.import.rows.retain(|_| {
        let keep = !done.contains(&k);
        k += 1;
        keep
    });
    if let Some(tab) = last_tab {
        state.shelf.open(tab, "");
    }
    result
}

/// The library imports go to when none is chosen: the default one, else the session.
fn default_destination(state: &UiState) -> Destination {
    match state.libraries.iter().find(|l| state.user_library.as_ref().is_some_and(|u| u.root() == l.lib.root())) {
        Some(l) => Destination::Library(l.id.clone()),
        None => Destination::Session,
    }
}

fn destination_label(state: &UiState, d: &Destination) -> String {
    match d {
        Destination::Session => tr("Current session").to_string(),
        Destination::Library(id) => format!("{} {}", tr("Library:"), crate::panels::shelf_filter::library_name(id)),
    }
}

/// Draw the window (call once per frame).
pub fn window(state: &mut UiState, ctx: &egui::Context) {
    if !state.import.open {
        return;
    }
    // Files dropped on the window join the list.
    if let Some(rect) = state.import.rect {
        let over = ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p));
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if over && !dropped.is_empty() {
            state.drops_taken = true;
            state.import.add(&dropped);
        }
    }
    let mut open = true;
    let mut do_import = false;
    let mut cancel = false;
    let now = ctx.input(|i| i.time);
    let response = crate::widgets::window(ctx, RichText::new(tr("Import resources")).font(theme::font_strong(14.0))).open(&mut open).collapsible(false).resizable(true).default_width(620.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button(format!("{}  {}", crate::icons::PLUS, tr("Add resources"))).clicked()
                && let Some(paths) = rfd::FileDialog::new().set_title(tr("Add resources")).pick_files()
            {
                state.import.add(&paths);
            }
            let any_selected = state.import.rows.iter().any(|r| r.selected);
            if ui.add_enabled(any_selected, egui::Button::new(format!("{}  {}", crate::icons::TRASH, tr("Remove selected resources")))).clicked() {
                state.import.remove_selected();
            }
            ui.label(RichText::new(tr("Show")).color(theme::TEXT_DIM));
            let shown = state.import.filter.map_or(tr("All usages"), |u| u.label());
            egui::ComboBox::from_id_salt("import-usage-filter").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut state.import.filter, None, tr("All usages"));
                for u in Usage::ALL {
                    ui.selectable_value(&mut state.import.filter, Some(u), u.label());
                }
            });
        });
        ui.add_space(6.0);
        if state.import.rows.is_empty() {
            ui.label(RichText::new(tr("Add resources, or drop files and folders here.")).color(theme::TEXT_FAINT));
        }
        let visible = state.import.visible();
        let mut usage_change = None;
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            egui::Grid::new("import-rows").num_columns(5).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new(tr("Usage")).color(theme::TEXT_DIM));
                ui.label(RichText::new(tr("Prefix")).color(theme::TEXT_DIM));
                ui.label(RichText::new(tr("Filename")).color(theme::TEXT_DIM));
                ui.label("");
                ui.end_row();
                for i in visible {
                    let row = &mut state.import.rows[i];
                    ui.checkbox(&mut row.selected, "");
                    let mut usage = row.usage;
                    egui::ComboBox::from_id_salt(("import-usage", i)).selected_text(usage.label()).width(110.0).show_ui(ui, |ui| {
                        for u in Usage::ALL {
                            ui.selectable_value(&mut usage, u, u.label());
                        }
                    });
                    if usage != row.usage {
                        usage_change = Some((i, usage));
                    }
                    ui.add(egui::TextEdit::singleline(&mut row.prefix).hint_text("folder/sub").desired_width(110.0));
                    let name = row.path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    ui.label(name).on_hover_text(row.path.display().to_string());
                    match row.warning() {
                        Some(w) => {
                            ui.label(RichText::new(crate::icons::WARNING).color(theme::WARNING)).on_hover_text(tr(w));
                        }
                        None if row.usage == Usage::Undefined => {
                            ui.label(RichText::new(crate::icons::INFO).color(theme::TEXT_FAINT)).on_hover_text(tr("Choose a usage to import this file"));
                        }
                        None => {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((i, u)) = usage_change {
            state.import.set_usage(i, u);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Import to")).color(theme::TEXT_DIM));
            let current = state.import.destination.clone().unwrap_or_else(|| default_destination(state));
            let mut chosen = current.clone();
            let label = destination_label(state, &current);
            let ids: Vec<String> = state.libraries.iter().map(|l| l.id.clone()).collect();
            egui::ComboBox::from_id_salt("import-destination").selected_text(label).width(200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, Destination::Session, tr("Current session"));
                ui.add_enabled(false, egui::Button::new(tr("Project")))
                    .on_disabled_hover_text(tr("Keeping resources inside the project file is not available yet"));
                for id in ids {
                    let text = format!("{} {}", tr("Library:"), crate::panels::shelf_filter::library_name(&id));
                    ui.selectable_value(&mut chosen, Destination::Library(id), text);
                }
            });
            if chosen != current {
                state.import.destination = Some(chosen);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::secondary_button(ui, tr("Cancel")).clicked() {
                    cancel = true;
                }
                let n = state.import.importable();
                if ui.add_enabled(n > 0, egui::Button::new(format!("{} ({n})", tr("Import")))).clicked() {
                    do_import = true;
                }
            });
        });
    });
    state.import.rect = response.map(|r| r.response.rect);
    if do_import {
        let r = import(state);
        for e in &r.errors {
            log::warn!("import resources: {e}");
        }
        let msg = match (r.imported.len(), r.errors.first()) {
            (n, None) => format!("Imported {n} resource(s)"),
            (n, Some(e)) => format!("Imported {n} resource(s); {} failed: {e}", r.errors.len()),
        };
        state.set_status(msg, now);
        if state.import.rows.is_empty() {
            open = false;
        }
    }
    if cancel || !open {
        state.import = ImportWindow::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{LibraryPath, SESSION};

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-import-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        subcraft_image::save_image(path, 8, 8, 1, &[0.6; 64], subcraft_image::FileFormat::Png8).unwrap();
    }

    fn state_with(yours: &Path) -> UiState {
        UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(1.0), 64), None).with_user_library(Some(subcraft_app::assets::UserLibrary::new(yours)))
    }

    #[test]
    fn usages_follow_the_format_and_images_start_undefined() {
        assert_eq!(Usage::guess(Path::new("a.scsmart")), Usage::SmartMaterial);
        assert_eq!(Usage::guess(Path::new("a.SCMASK")), Usage::SmartMask);
        assert_eq!(Usage::guess(Path::new("a.scbrush")), Usage::BrushPreset);
        assert_eq!(Usage::guess(Path::new("sky.hdr")), Usage::Environment);
        assert_eq!(Usage::guess(Path::new("dirt.png")), Usage::Undefined, "an image can be a texture, an alpha or an environment");
        assert!(Usage::Texture.accepts("PNG") && Usage::Environment.accepts("exr") && !Usage::Alpha.accepts("hdr"));
        assert!(!supported(Path::new("notes.txt")) && supported(Path::new("x.tga")));
    }

    #[test]
    fn rows_add_once_expand_folders_and_share_a_usage_when_selected() {
        let root = temp_root("rows");
        png(&root.join("a.png"));
        png(&root.join("sub").join("b.png"));
        std::fs::write(root.join("readme.txt"), "x").unwrap();
        let mut w = ImportWindow::default();
        w.add(std::slice::from_ref(&root));
        w.add(&[root.join("a.png")]);
        assert_eq!(w.rows.len(), 2, "folder files found once, unsupported files left out: {:?}", w.rows);
        assert_eq!(w.importable(), 0, "undefined rows wait for a usage");
        w.rows[0].selected = true;
        w.rows[1].selected = true;
        w.set_usage(0, Usage::Alpha);
        assert!(w.rows.iter().all(|r| r.usage == Usage::Alpha), "every selected row takes the usage");
        assert_eq!(w.importable(), 2);
        w.rows[1].selected = false;
        w.set_usage(1, Usage::Environment);
        assert_eq!((w.rows[0].usage, w.rows[1].usage), (Usage::Alpha, Usage::Environment));
        w.set_usage(1, Usage::SmartMask);
        assert_eq!(w.rows[1].warning(), Some("This usage does not read this file format"));
        w.filter = Some(Usage::Alpha);
        assert_eq!(w.visible(), [0]);
        w.remove_selected();
        assert_eq!(w.rows.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn import_to_a_library_copies_into_its_prefix_and_the_session_reads_in_place() {
        let root = temp_root("import");
        let yours = root.join("yours");
        let src = root.join("src");
        png(&src.join("splat.png"));
        png(&src.join("grain.png"));
        let mut s = state_with(&yours);
        // Into your_assets, under a prefix.
        open_with(&mut s, &[src.join("splat.png")]);
        s.import.set_usage(0, Usage::Alpha);
        s.import.rows[0].prefix = "Splats".into();
        let r = import(&mut s);
        assert_eq!((r.imported.clone(), r.errors.clone()), (vec!["splat".to_string()], vec![]));
        let stored = subcraft_app::assets::UserLibrary::new(&yours).folder(UserAssetKind::Alpha).join("Splats").join("splat.png");
        assert!(stored.is_file(), "copied into the prefix folder");
        assert!(s.import.rows.is_empty(), "imported rows leave the window");
        assert_eq!(s.shelf.tab, crate::panels::shelf::ShelfTab::Alphas, "the panel shows what was imported");
        let item = crate::panels::shelf::items(&s, crate::panels::shelf::ShelfTab::Alphas).into_iter().find(|i| i.name == "splat").unwrap();
        assert_eq!((item.library.as_str(), item.folder.as_deref()), (crate::panels::shelf_filter::YOURS, Some("Splats")));
        // Into the session: nothing is copied.
        open_with(&mut s, &[src.join("grain.png")]);
        s.import.set_usage(0, Usage::Texture);
        s.import.destination = Some(Destination::Session);
        assert_eq!(import(&mut s).imported, ["grain"]);
        assert!(!subcraft_app::assets::UserLibrary::new(&yours).folder(UserAssetKind::Texture).exists(), "the session copies nothing");
        let item = crate::panels::shelf::items(&s, crate::panels::shelf::ShelfTab::Textures).into_iter().find(|i| i.name == "grain").unwrap();
        assert_eq!(item.library, SESSION);
        // An extra library as the destination.
        let kit = root.join("kit");
        s.set_libraries(&[LibraryPath { name: "Kit".into(), path: kit.display().to_string() }], "your_assets");
        open_with(&mut s, &[src.join("grain.png")]);
        s.import.set_usage(0, Usage::Alpha);
        s.import.destination = Some(Destination::Library("Kit".into()));
        assert_eq!(import(&mut s).errors, Vec::<String>::new());
        assert!(subcraft_app::assets::UserLibrary::new(&kit).folder(UserAssetKind::Alpha).join("grain.png").is_file());
        // A library that went away is an error, not a panic.
        open_with(&mut s, &[src.join("grain.png")]);
        s.import.set_usage(0, Usage::Alpha);
        s.import.destination = Some(Destination::Library("Gone".into()));
        assert_eq!(import(&mut s).errors.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
