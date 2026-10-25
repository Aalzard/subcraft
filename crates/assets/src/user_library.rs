//! The user's asset library on disk: imported brush alphas and HDRI environments survive restarts.
//!
//! Layout: `<root>/alphas/*` and `<root>/environments/*`, plain image files the user can also drop
//! in by hand. The root is `Documents/SubCraft/Library`, or `$SUBCRAFT_LIBRARY` when set.

use std::path::{Path, PathBuf};

use subcraft_core::AssetId;

use crate::alphas::BrushAlpha;
use crate::environment::Environment;

/// Environment variable overriding the library location (portable installs, tests).
pub const ENV_VAR: &str = "SUBCRAFT_LIBRARY";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAssetKind {
    Alpha,
    Environment,
    SmartMaterial,
    SmartMask,
    BrushPreset,
    /// Image textures (base color, normal, roughness maps, ...).
    Texture,
    /// Substance `.sbsar` archives (baked when applied from the shelf). Append-only for schema stability.
    Sbsar,
}

impl UserAssetKind {
    fn folder(self) -> &'static str {
        match self {
            UserAssetKind::Alpha => "alphas",
            UserAssetKind::Environment => "environments",
            UserAssetKind::SmartMaterial => "smart-materials",
            UserAssetKind::SmartMask => "smart-masks",
            UserAssetKind::BrushPreset => "brush-presets",
            UserAssetKind::Texture => "textures",
            UserAssetKind::Sbsar => "sbsar",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            UserAssetKind::Alpha => &subcraft_image::IMAGE_EXTENSIONS,
            UserAssetKind::Environment => &["hdr", "exr", "png", "jpg", "jpeg"],
            UserAssetKind::SmartMaterial => &["scsmart"],
            UserAssetKind::SmartMask => &["scmask"],
            UserAssetKind::BrushPreset => &["scbrush"],
            UserAssetKind::Texture => &subcraft_image::IMAGE_EXTENSIONS,
            UserAssetKind::Sbsar => &["sbsar"],
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserLibrary {
    root: PathBuf,
}

impl UserLibrary {
    /// `$SUBCRAFT_LIBRARY`, else `Documents/SubCraft/Library`.
    pub fn default_root() -> Option<PathBuf> {
        resolve_root(std::env::var_os(ENV_VAR), dirs::document_dir())
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn folder(&self, kind: UserAssetKind) -> PathBuf {
        self.root.join(kind.folder())
    }

    /// Supported files of `kind`, sorted by name. A missing folder is simply empty.
    pub fn list(&self, kind: UserAssetKind) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.folder(kind)) else { return Vec::new() };
        let mut out: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| kind.extensions().contains(&e.to_ascii_lowercase().as_str()))
            })
            .collect();
        out.sort();
        out
    }

    /// Copy `src` into the library. An identical file already there is reused; a different file
    /// with the same name gets a numbered name (`wood (2).png`). Returns the library path.
    pub fn add_file(&self, kind: UserAssetKind, src: &Path) -> Result<PathBuf, String> {
        let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
        let dir = self.folder(kind);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
        for n in 1.. {
            let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem} ({n}).{ext}") };
            let dest = dir.join(name);
            match std::fs::read(&dest) {
                Ok(existing) if existing == bytes => return Ok(dest),
                Ok(_) => continue,
                Err(_) => {
                    std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
                    return Ok(dest);
                }
            }
        }
        unreachable!("the loop returns")
    }

    /// A new file path `<folder>/<name>.<ext>` that doesn't exist yet (`name (2).ext`, ...).
    /// Characters not allowed in file names are replaced.
    pub fn new_file_path(&self, kind: UserAssetKind, name: &str) -> PathBuf {
        let clean: String = name
            .chars()
            .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
            .collect();
        let clean = clean.trim().trim_end_matches('.');
        let stem = if clean.is_empty() { "Untitled" } else { clean };
        let ext = kind.extensions()[0];
        let dir = self.folder(kind);
        (1..)
            .map(|n| if n == 1 { dir.join(format!("{stem}.{ext}")) } else { dir.join(format!("{stem} ({n}).{ext}")) })
            .find(|p| !p.exists())
            .expect("an unused name exists")
    }

    /// Delete a library file. Refuses paths outside the library.
    pub fn remove(&self, path: &Path) -> Result<(), String> {
        let inside = path
            .parent()
            .zip(self.root.canonicalize().ok())
            .and_then(|(parent, root)| parent.canonicalize().ok().map(|p| p.starts_with(root)))
            .unwrap_or(false);
        if !inside {
            return Err(format!("{} is not in the library", path.display()));
        }
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Load every alpha in the library; unreadable files are reported, not fatal.
    pub fn load_alphas(&self) -> (Vec<BrushAlpha>, Vec<String>) {
        let mut ok = Vec::new();
        let mut errors = Vec::new();
        for p in self.list(UserAssetKind::Alpha) {
            match load_alpha(&p) {
                Ok(a) => ok.push(a),
                Err(e) => errors.push(e),
            }
        }
        (ok, errors)
    }
}

/// Library root from the override variable's value and the Documents folder.
fn resolve_root(override_var: Option<std::ffi::OsString>, documents: Option<PathBuf>) -> Option<PathBuf> {
    match override_var.filter(|p| !p.is_empty()) {
        Some(p) => Some(PathBuf::from(p)),
        None => documents.map(|d| d.join("SubCraft").join("Library")),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string()
}

/// A brush alpha from a library file (stable id from the file name).
pub fn load_alpha(path: &Path) -> Result<BrushAlpha, String> {
    let mut a = crate::alphas::import(path).map_err(|e| format!("{}: {e}", file_name(path)))?;
    a.id = AssetId::from_name(&format!("alpha/user/{}", file_name(path)));
    a.user_file = Some(path.to_path_buf());
    Ok(a)
}

/// An HDRI environment from a library file (prefiltering is slow: call off the UI thread).
pub fn load_environment(path: &Path) -> Result<Environment, String> {
    let mut e = crate::environment::import_hdri(path).map_err(|e| format!("{}: {e}", file_name(path)))?;
    e.id = AssetId::from_name(&format!("environment/user/{}", file_name(path)));
    e.user_file = Some(path.to_path_buf());
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_image::{FileFormat, save_image};

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-userlib-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn png(path: &Path, v: f32) {
        save_image(path, 8, 8, 1, &[v; 64], FileFormat::Png8).unwrap();
    }

    #[test]
    fn imported_alphas_come_back_after_restart() {
        let root = temp_root("alphas");
        let src_dir = root.with_extension("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        png(&src_dir.join("dots.png"), 0.7);
        std::fs::write(src_dir.join("notes.txt"), "x").unwrap();

        let lib = UserLibrary::new(&root);
        assert!(lib.list(UserAssetKind::Alpha).is_empty(), "missing folder = empty");
        let stored = lib.add_file(UserAssetKind::Alpha, &src_dir.join("dots.png")).unwrap();
        assert!(stored.starts_with(lib.folder(UserAssetKind::Alpha)));
        std::fs::remove_dir_all(&src_dir).unwrap(); // the original may disappear

        // "Restart": a fresh library object reads the folder.
        let (alphas, errors) = UserLibrary::new(&root).load_alphas();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(alphas.len(), 1);
        assert_eq!(alphas[0].name, "dots");
        assert_eq!(alphas[0].user_file.as_deref(), Some(stored.as_path()));
        assert_eq!(alphas[0].id, load_alpha(&stored).unwrap().id, "stable id across loads");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn same_file_is_reused_and_name_clashes_get_numbers() {
        let root = temp_root("dedup");
        let src = root.with_extension("src");
        std::fs::create_dir_all(src.join("a")).unwrap();
        std::fs::create_dir_all(src.join("b")).unwrap();
        png(&src.join("a/tip.png"), 0.2);
        png(&src.join("b/tip.png"), 0.9);
        let lib = UserLibrary::new(&root);
        let first = lib.add_file(UserAssetKind::Alpha, &src.join("a/tip.png")).unwrap();
        assert_eq!(lib.add_file(UserAssetKind::Alpha, &src.join("a/tip.png")).unwrap(), first, "identical file reused");
        let second = lib.add_file(UserAssetKind::Alpha, &src.join("b/tip.png")).unwrap();
        assert_eq!(second.file_name().unwrap(), "tip (2).png");
        assert_eq!(lib.list(UserAssetKind::Alpha).len(), 2);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&src).ok();
    }

    #[test]
    fn broken_files_are_reported_and_remove_stays_inside_the_library() {
        let root = temp_root("broken");
        let lib = UserLibrary::new(&root);
        std::fs::create_dir_all(lib.folder(UserAssetKind::Alpha)).unwrap();
        std::fs::write(lib.folder(UserAssetKind::Alpha).join("bad.png"), b"nope").unwrap();
        png(&lib.folder(UserAssetKind::Alpha).join("good.png"), 0.5);
        let (alphas, errors) = lib.load_alphas();
        assert_eq!(alphas.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("bad.png"));

        let outside = root.with_extension("outside.png");
        png(&outside, 0.5);
        assert!(lib.remove(&outside).is_err(), "never deletes outside the library");
        assert!(outside.exists());
        lib.remove(&lib.folder(UserAssetKind::Alpha).join("good.png")).unwrap();
        assert_eq!(lib.list(UserAssetKind::Alpha).len(), 1);
        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hdri_environment_loads_from_the_library() {
        let root = temp_root("env");
        let lib = UserLibrary::new(&root);
        let dir = lib.folder(UserAssetKind::Environment);
        std::fs::create_dir_all(&dir).unwrap();
        // Tiny 2:1 float panorama: bright top half.
        let data: Vec<f32> = (0..16 * 8).flat_map(|i| if i < 64 { [4.0, 4.0, 3.5] } else { [0.1, 0.1, 0.1] }).collect();
        save_image(&dir.join("sky.exr"), 16, 8, 3, &data, FileFormat::Exr32).unwrap();
        let files = lib.list(UserAssetKind::Environment);
        assert_eq!(files.len(), 1);
        let env = load_environment(&files[0]).unwrap();
        assert_eq!(env.name, "sky");
        assert!(env.user_file.is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn new_file_paths_are_unique_and_safe() {
        let root = temp_root("names");
        let lib = UserLibrary::new(&root);
        let p = lib.new_file_path(UserAssetKind::SmartMaterial, "Rusty: Iron/Old?");
        assert_eq!(p.file_name().unwrap(), "Rusty_ Iron_Old_.scsmart");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        let q = lib.new_file_path(UserAssetKind::SmartMaterial, "Rusty: Iron/Old?");
        assert_eq!(q.file_name().unwrap(), "Rusty_ Iron_Old_ (2).scsmart");
        assert_eq!(lib.new_file_path(UserAssetKind::SmartMask, "  ").file_name().unwrap(), "Untitled.scmask");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn root_resolution() {
        let docs = Some(PathBuf::from("D:/Docs"));
        assert_eq!(resolve_root(None, docs.clone()), Some(PathBuf::from("D:/Docs").join("SubCraft").join("Library")));
        assert_eq!(resolve_root(Some("E:/Portable/Lib".into()), docs.clone()), Some(PathBuf::from("E:/Portable/Lib")));
        assert_eq!(resolve_root(Some("".into()), docs), Some(PathBuf::from("D:/Docs").join("SubCraft").join("Library")), "empty = unset");
        assert_eq!(resolve_root(None, None), None);
    }
}
