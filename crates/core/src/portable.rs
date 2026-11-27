//! Portable mode (issue #759): a folder named `portable` next to the SubCraft executable holds
//! everything SubCraft would otherwise keep in per-user system folders: preferences, window
//! layout, logs, crash reports, recovery copies, caches and the user library. The whole install
//! can then be copied to another machine, and several versions can sit side by side without
//! sharing settings. Same idea as Blender's `portable` folder.

use std::path::{Path, PathBuf};

/// Name of the folder that switches portable mode on.
pub const FOLDER: &str = "portable";

/// The portable folder in force, if any: `<folder of the executable>/portable` when that folder
/// exists. For an AppImage the executable runs from a read-only mount, so the folder next to the
/// `.AppImage` file (`$APPIMAGE`) is used instead.
pub fn dir() -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::current_exe().ok());
        exe.and_then(|exe| find(&exe))
    })
    .clone()
}

/// [`dir`] for an explicit executable path (testable without touching the real one).
pub fn find(exe: &Path) -> Option<PathBuf> {
    let candidate = exe.parent().filter(|p| !p.as_os_str().is_empty())?.join(FOLDER);
    candidate.is_dir().then_some(candidate)
}

/// True when SubCraft runs in portable mode.
pub fn is_active() -> bool {
    dir().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_folder_next_to_the_executable_switches_it_on() {
        let root = std::env::temp_dir().join(format!("subcraft-portable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("subcraft.exe");
        assert_eq!(find(&exe), None, "no folder: normal install");
        std::fs::write(root.join(FOLDER), b"").unwrap();
        assert_eq!(find(&exe), None, "a file named portable is not the folder");
        std::fs::remove_file(root.join(FOLDER)).unwrap();
        std::fs::create_dir(root.join(FOLDER)).unwrap();
        assert_eq!(find(&exe), Some(root.join(FOLDER)));
        assert_eq!(find(Path::new("subcraft")), None, "a bare name has no folder to look in");
        let _ = std::fs::remove_dir_all(&root);
    }
}
