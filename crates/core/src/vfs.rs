//! Whole-file reads and writes (thin wrappers over `std::fs`).

use std::io;
use std::path::Path;

mod imp {
    use super::*;

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
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
}
