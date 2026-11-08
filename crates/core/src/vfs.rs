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
        let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = path.with_file_name(tmp_name);
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
                Err(e) if cfg!(windows) && matches!(e.kind(), io::ErrorKind::PermissionDenied) && attempt < 5 => {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1) as u64));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
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
