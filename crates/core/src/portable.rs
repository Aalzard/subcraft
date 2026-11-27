//! Portable mode (issue #759): a folder named `portable` next to the SubCraft executable holds
//! everything SubCraft would otherwise keep in per-user system folders (settings and window
//! layout, logs, crash reports, recovery copies, caches, the user library and plugins). The app
//! can then be moved to another machine with its settings, and several versions can sit side by
//! side without sharing settings. Without that folder nothing changes.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Name of the folder next to the executable that switches portable mode on.
pub const FOLDER: &str = "portable";

/// The `portable` folder next to the running executable, when it exists. Looked up once per
/// process.
pub fn dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        // Resolve symlinks (a `subcraft` link in ~/bin should still find the real install).
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        dir_next_to(exe.parent()?)
    })
    .clone()
}

/// The `portable` folder inside `install_dir`, when it is a folder.
pub fn dir_next_to(install_dir: &Path) -> Option<PathBuf> {
    let p = install_dir.join(FOLDER);
    p.is_dir().then_some(p)
}

/// Whether SubCraft runs in portable mode.
pub fn is_active() -> bool {
    dir().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_switches_portable_mode_on_and_a_file_does_not() {
        let root = std::env::temp_dir().join(format!("subcraft-portable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(dir_next_to(&root), None, "no folder: normal mode");
        std::fs::write(root.join(FOLDER), b"").unwrap();
        assert_eq!(dir_next_to(&root), None, "a file named portable is not the folder");
        std::fs::remove_file(root.join(FOLDER)).unwrap();
        std::fs::create_dir(root.join(FOLDER)).unwrap();
        assert_eq!(dir_next_to(&root), Some(root.join(FOLDER)));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
