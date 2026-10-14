//! Launcher data and quick-start text, independent of UI and rendering.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::{Session, DEFAULT_RESOLUTION};

pub const MAX_RECENT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind { Project, Model }

pub fn file_kind(path: &Path) -> Option<FileKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == crate::PROJECT_EXTENSION { Some(FileKind::Project) }
    else if subcraft_mesh::SUPPORTED_EXTENSIONS.contains(&ext.as_str()) { Some(FileKind::Model) }
    else { None }
}

/// Recent locations are navigation history, never identity for project objects or resources.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentFiles { files: Vec<PathBuf> }

#[derive(Clone, Debug)]
pub struct RecentItem { pub path: PathBuf, pub name: String, pub kind: FileKind, pub available: bool }

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() } else { a == b }
}

impl RecentFiles {
    pub fn remember(&mut self, path: &Path) {
        if file_kind(path).is_none() { return; }
        let path = path.canonicalize().unwrap_or_else(|_| {
            if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) }
        });
        self.files.retain(|p| !same_path(p, &path));
        self.files.insert(0, path); self.files.truncate(MAX_RECENT);
    }
    pub fn remove(&mut self, path: &Path) { self.files.retain(|p| !same_path(p, path)); }
    pub fn clear(&mut self) { self.files.clear(); }
    pub fn paths(&self) -> &[PathBuf] { &self.files }

    /// Refresh on startup / an open / a save, not during every UI frame.
    pub fn items(&self) -> Vec<RecentItem> {
        self.files.iter().filter_map(|path| {
            Some(RecentItem { path: path.clone(), kind: file_kind(path)?,
                name: path.file_name()?.to_string_lossy().into_owned(),
                available: std::fs::metadata(path).is_ok_and(|m| m.is_file()) })
        }).collect()
    }
}

impl Session {
    pub fn open_recent(path: &Path) -> Result<(Self, Vec<String>), String> {
        match file_kind(path) {
            Some(FileKind::Project) => Self::open_project(path).map(|s| (s, Vec::new())),
            Some(FileKind::Model) => Self::open_mesh(path, DEFAULT_RESOLUTION),
            None => Err("Choose a SubCraft project or an OBJ, FBX, glTF or GLB model".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAction { OpenModel, Bake, SmartMaterial, PaintParts, Save, Export }
pub struct GuideStep { pub title: &'static str, pub text: &'static str, pub button: &'static str, pub action: GuideAction }
pub const QUICK_START: [GuideStep; 6] = [
    GuideStep { title: "Open a model", text: "Start with an OBJ, GLB/glTF or FBX model with UVs. The sample crate is ready to use if you want to learn first. FBX is desktop only.", button: "Open model…", action: GuideAction::OpenModel },
    GuideStep { title: "Bake mesh maps", text: "Bake curvature and ambient occlusion before using wear and dirt generators. Start at 512 pixels, then increase quality for your final textures.", button: "Bake mesh maps…", action: GuideAction::Bake },
    GuideStep { title: "Drag a smart material", text: "In Shelf → Smart Materials, drag Worn Painted Metal onto the model or a layer. Adjust the fills and generator masks in Properties.", button: "Show smart materials", action: GuideAction::SmartMaterial },
    GuideStep { title: "Paint and use Parts", text: "Use a paint layer for brush strokes. Right-click a piece to fill it or paint only there; Parts also lists UV islands, objects and material IDs. I isolates, H hides and Esc restores.", button: "Show Parts", action: GuideAction::PaintParts },
    GuideStep { title: "Save your project", text: "Save a .subcraft project to keep layers, imported images, baked maps and the model together. Autosave recovery copies are a backup; keep your own project file too.", button: "Save project…", action: GuideAction::Save },
    GuideStep { title: "Export textures", text: "Choose the PBR, Unreal, Unity or glTF template and an output folder. The browser downloads the files. Check the maps in the app or engine that will use them.", button: "Export textures…", action: GuideAction::Export },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recents_deduplicate_bound_and_keep_missing_files_removable() {
        let root = std::env::temp_dir().join(format!("subcraft-recent-{}", subcraft_core::ObjectId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("crate.subcraft"); std::fs::write(&path, "placeholder").unwrap();
        let mut list = RecentFiles::default(); list.remember(&path); list.remember(&path);
        assert_eq!(list.paths().len(), 1); assert!(list.items()[0].available);
        std::fs::remove_file(&path).unwrap(); assert!(!list.items()[0].available);
        for i in 0..MAX_RECENT + 5 { list.remember(&root.join(format!("{i}.glb"))); }
        assert_eq!(list.paths().len(), MAX_RECENT);
        assert!(list.paths()[0].ends_with("16.glb"));
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(serde_json::from_str::<RecentFiles>(&json).unwrap(), list);
        let first = list.paths()[0].clone(); list.remove(&first); assert_eq!(list.paths().len(), MAX_RECENT - 1);
        list.remember(&root.join("ignored.png")); assert_eq!(list.paths().len(), MAX_RECENT - 1);
        list.clear(); assert!(list.items().is_empty()); std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn supported_files_and_all_six_guide_actions_are_explicit() {
        assert_eq!(file_kind(Path::new("A.SUBCRAFT")), Some(FileKind::Project));
        for ext in subcraft_mesh::SUPPORTED_EXTENSIONS { assert_eq!(file_kind(Path::new(&format!("A.{ext}"))), Some(FileKind::Model)); }
        assert_eq!(file_kind(Path::new("A.png")), None);
        assert_eq!(QUICK_START.map(|s| s.action), [GuideAction::OpenModel, GuideAction::Bake, GuideAction::SmartMaterial, GuideAction::PaintParts, GuideAction::Save, GuideAction::Export]);
    }
}
