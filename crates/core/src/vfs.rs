//! Whole-file reads and writes that also work in the browser.
//!
//! On desktop these are `std::fs`. The web build has no file system: files the user uploads are
//! placed in an in-memory store under virtual paths (e.g. `/upload/axe.glb`) and read back from
//! there, and every write is handed to a sink (the web front end turns it into a download) as
//! well as kept in the store, so the rest of the engine keeps working with paths.

use std::io;
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    type Sink = Box<dyn Fn(&Path, &[u8])>;

    thread_local! {
        static FILES: RefCell<HashMap<PathBuf, std::rc::Rc<[u8]>>> = RefCell::new(HashMap::new());
        static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        FILES.with(|f| f.borrow().get(path).map(|b| b.to_vec())).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} was not uploaded", path.display())))
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        insert(path, bytes.to_vec());
        SINK.with(|s| {
            if let Some(sink) = s.borrow().as_ref() {
                sink(path, bytes);
            }
        });
        Ok(())
    }

    pub fn create_dir_all(_path: &Path) -> io::Result<()> {
        Ok(())
    }

    pub fn exists(path: &Path) -> bool {
        FILES.with(|f| f.borrow().contains_key(path))
    }

    /// Put a file into the store (an upload) without passing it to the sink.
    pub fn insert(path: &Path, bytes: Vec<u8>) {
        FILES.with(|f| f.borrow_mut().insert(path.to_path_buf(), bytes.into()));
    }

    /// Where written files go (the web front end offers them as downloads).
    pub fn set_sink(sink: impl Fn(&Path, &[u8]) + 'static) {
        SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
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
