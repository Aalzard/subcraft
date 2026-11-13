//! Whole-file reads and writes (thin wrappers over `std::fs`).

use std::io;
use std::path::Path;

mod imp {
    use super::*;

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    /// Atomic replace: write a temporary file next to `path`, sync it, then rename over the
    /// target. A full disk, a killed process or an antivirus lock leaves the old file intact and
    /// no partial file behind.
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        // One temporary name per call: two threads writing the same file used to share one
        // temporary file (one truncated the other's bytes before the rename). Short, so a name
        // close to the 255-character limit still gets a temporary file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let tmp = path.with_file_name(format!(".sc{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let result = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            drop(f);
            rename_over(&tmp, path)
        })();
        if result.is_err() { let _ = std::fs::remove_file(&tmp); }
        result
    }

    /// `rename` that retries briefly: on Windows a virus scanner or indexer may hold the file for
    /// a moment right after it was written or while the old one is open for reading.
    fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
        let mut last = None;
        for attempt in 0..6 {
            match std::fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) if cfg!(windows) && is_transient_lock(&e) && attempt < 5 => {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1) as u64));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
    }

    /// A file briefly held by another program: access denied, or a sharing / lock violation
    /// (Windows errors 32 and 33, which std does not report as `PermissionDenied`).
    fn is_transient_lock(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33))
    }

    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }
}

pub use imp::*;

/// Read a file into memory.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    /// QA 2026-11: the temporary file was named after the target and the process, so threads
    /// writing the same file at once shared it: one truncated it while another renamed it
    /// (a corrupt file, then "file not found" for the other writer).
    #[test]
    fn concurrent_writes_of_one_file_each_land_whole() {
        let dir = std::env::temp_dir().join(format!("subcraft-vfs-race-{}", std::process::id()));
        super::create_dir_all(&dir).unwrap();
        let p = dir.join("same.bin");
        let contents: Vec<Vec<u8>> = (0..8u8).map(|i| vec![i; 256 * 1024]).collect();
        for _ in 0..4 {
            std::thread::scope(|s| {
                for c in &contents {
                    let p = &p;
                    s.spawn(move || super::write(p, c).expect("every writer succeeds"));
                }
            });
            let got = super::read(&p).unwrap();
            assert!(contents.contains(&got), "the file is one writer's bytes, whole");
        }
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).filter(|n| n != "same.bin").collect();
        assert!(left.is_empty(), "no temporary files left behind: {left:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file name near the 255-character limit is valid; its temporary name must be too.
    #[test]
    fn long_file_names_still_write() {
        let dir = std::env::temp_dir().join(format!("subcraft-vfs-long-{}", std::process::id()));
        super::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{}.png", "n".repeat(240)));
        super::write(&p, b"x").unwrap();
        assert_eq!(super::read(&p).unwrap(), b"x");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("subcraft-vfs-{}", std::process::id()));
        super::create_dir_all(&dir).unwrap();
        let p = dir.join("a.bin");
        super::write(&p, b"abc").unwrap();
        assert!(super::exists(&p));
        assert_eq!(super::read(&p).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_replaces_atomically_and_failure_keeps_the_old_file() {
        let dir = std::env::temp_dir().join(format!("subcraft-vfs-atomic-{}", std::process::id()));
        super::create_dir_all(&dir).unwrap();
        let p = dir.join("texture space.bin");
        super::write(&p, b"old").unwrap();
        super::write(&p, b"new contents").unwrap();
        assert_eq!(super::read(&p).unwrap(), b"new contents");
        // The target is a directory: the write fails, nothing is left behind, the directory survives.
        let d = dir.join("target");
        std::fs::create_dir_all(&d).unwrap();
        assert!(super::write(&d, b"x").is_err());
        assert!(d.is_dir());
        // Missing parent folder: error, no file.
        assert!(super::write(&dir.join("missing").join("f.bin"), b"x").is_err());
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
