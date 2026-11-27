//! Substance archives (`.sbsar`) in the Assets panel: everything in the libraries' `substances/`
//! folders. Each archive is read once on a background thread to tell a material (channel
//! outputs: Shelf → Materials, a fill layer) from a generator (a mask output only: Shelf →
//! Generators, a mask effect).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use subcraft_core::AssetId;

pub struct SubstanceItem {
    pub id: AssetId,
    pub name: String,
    pub path: PathBuf,
    /// `Some(true)` for a generator; `None` until the archive has been read (shown as a material).
    pub generator: Option<bool>,
}

#[derive(Default)]
pub struct SubstanceLibrary {
    pub items: Vec<SubstanceItem>,
    loaders: Vec<Receiver<(PathBuf, bool)>>,
}

impl SubstanceLibrary {
    /// List files that are already where they stay (a library folder or a session import).
    pub fn add_listed(&mut self, paths: &[PathBuf]) {
        let mut new = Vec::new();
        for p in paths {
            if self.items.iter().any(|i| &i.path == p) {
                continue;
            }
            let file = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            self.items.push(SubstanceItem {
                id: AssetId::from_name(&format!("substance/user/{}", p.display())),
                name: p.file_stem().and_then(|s| s.to_str()).unwrap_or(&file).to_string(),
                path: p.clone(),
                generator: None,
            });
            new.push(p.clone());
        }
        if new.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        subcraft_core::task::spawn(move || {
            for p in new {
                let generator = subcraft_app::graph_layer::is_generator_file(&p);
                if tx.send((p, generator)).is_err() {
                    break;
                }
            }
        });
        self.loaders.push(rx);
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|i| i.path != path);
    }

    /// Indices of the archives shown in Shelf → Materials (`false`) or → Generators (`true`).
    pub fn indices(&self, generators: bool) -> impl Iterator<Item = usize> + '_ {
        self.items.iter().enumerate().filter(move |(_, i)| i.generator.unwrap_or(false) == generators).map(|(k, _)| k)
    }

    /// Pick up finished classifications (call once per frame).
    pub fn poll(&mut self, ctx: Option<&egui::Context>) {
        let mut done = Vec::new();
        for (k, rx) in self.loaders.iter().enumerate() {
            loop {
                match rx.try_recv() {
                    Ok((path, generator)) => {
                        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
                            item.generator = Some(generator);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        done.push(k);
                        break;
                    }
                }
            }
        }
        for k in done.into_iter().rev() {
            self.loaders.remove(k);
        }
        if let Some(ctx) = ctx
            && !self.loaders.is_empty()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub fn is_loading(&self) -> bool {
        !self.loaders.is_empty()
    }

    /// Wait for every archive to be read (tests).
    #[cfg(test)]
    pub fn wait(&mut self) {
        let start = std::time::Instant::now();
        while self.is_loading() && start.elapsed().as_secs() < 30 {
            self.poll(None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../sbsar/tests/data").join(name)
    }

    #[test]
    fn archives_are_sorted_into_materials_and_generators() {
        let mut lib = SubstanceLibrary::default();
        let material = data("CardBoard.sbsar");
        let generator = data("EdgeGenerator.sbsar");
        lib.add_listed(&[material.clone(), generator.clone(), material.clone()]);
        assert_eq!(lib.items.len(), 2, "a file is listed once");
        assert_eq!(lib.indices(false).count(), 2, "unread archives show as materials");
        lib.wait();
        assert_eq!(lib.indices(false).map(|i| lib.items[i].path.clone()).collect::<Vec<_>>(), vec![material.clone()]);
        assert_eq!(lib.indices(true).map(|i| lib.items[i].name.clone()).collect::<Vec<_>>(), vec!["EdgeGenerator".to_string()]);
        lib.remove(&material);
        assert_eq!(lib.items.len(), 1);
    }
}
