//! Image textures in the shelf: everything in the library's `textures/` folder, with thumbnails
//! decoded on a background thread (large textures never stall the UI).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use subcraft_app::assets::texture_maps::{Classified, classify};
use subcraft_app::assets::{UserAssetKind, UserLibrary};
use subcraft_core::AssetId;

const THUMB: u32 = 96;

pub struct TextureItem {
    pub id: AssetId,
    pub name: String,
    pub path: PathBuf,
    /// Channel the file name suggests ("Normal", "Roughness"…), used as its shelf category.
    pub category: &'static str,
    pub thumb: Option<egui::TextureHandle>,
}

type ThumbResult = (PathBuf, Result<(u32, u32, Vec<u8>), String>);

#[derive(Default)]
pub struct TextureLibrary {
    pub items: Vec<TextureItem>,
    loaders: Vec<Receiver<ThumbResult>>,
}

/// Shelf category of a texture file name.
pub fn category_of(name: &str) -> &'static str {
    match classify(name) {
        Classified::Map(m) => m.channel.display_name(),
        Classified::Packed => "Packed",
        Classified::Unknown => "Other",
    }
}

impl TextureLibrary {
    /// Every texture in the library folder (thumbnails follow in the background).
    pub fn load(lib: &UserLibrary) -> Self {
        let mut t = Self::default();
        let files = lib.list(UserAssetKind::Texture);
        t.push_items(&files);
        t
    }

    fn push_items(&mut self, paths: &[PathBuf]) {
        let mut new = Vec::new();
        for p in paths {
            if self.items.iter().any(|i| &i.path == p) {
                continue;
            }
            let file = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            self.items.push(TextureItem {
                id: AssetId::from_name(&format!("texture/user/{file}")),
                name: p.file_stem().and_then(|s| s.to_str()).unwrap_or(&file).to_string(),
                path: p.clone(),
                category: category_of(&file),
                thumb: None,
            });
            new.push(p.clone());
        }
        if new.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        subcraft_core::task::spawn(move || {
            for p in new {
                let thumb = subcraft_image::load_bitmap(&p).map(|b| b.preview_rgba8(THUMB)).map_err(|e| e.to_string());
                if tx.send((p, thumb)).is_err() {
                    break;
                }
            }
        });
        self.loaders.push(rx);
    }

    /// Copy image files into the library (when there is one) and list them. Returns errors.
    pub fn add_files(&mut self, lib: Option<&UserLibrary>, paths: &[PathBuf]) -> Vec<String> {
        let mut errors = Vec::new();
        let mut stored = Vec::new();
        for p in paths {
            match lib {
                Some(lib) => match lib.add_file(UserAssetKind::Texture, p) {
                    Ok(dest) => stored.push(dest),
                    Err(e) => errors.push(e),
                },
                None => stored.push(p.clone()),
            }
        }
        self.push_items(&stored);
        errors
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|i| i.path != path);
    }

    /// Turn finished thumbnails into textures (call once per frame).
    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut done = Vec::new();
        for (k, rx) in self.loaders.iter().enumerate() {
            loop {
                match rx.try_recv() {
                    Ok((path, Ok((w, h, rgba)))) => {
                        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
                            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                            item.thumb = Some(ctx.load_texture(format!("tex-{}", item.id), img, egui::TextureOptions::LINEAR));
                        }
                    }
                    Ok((path, Err(e))) => log::warn!("texture {}: {e}", path.display()),
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
        if !self.loaders.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub fn is_loading(&self) -> bool {
        !self.loaders.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_file_names() {
        assert_eq!(category_of("Bricks_2K_Color.jpg"), "Base Color");
        assert_eq!(category_of("bricks_nor_gl.png"), "Normal");
        assert_eq!(category_of("rock_arm.png"), "Packed");
        assert_eq!(category_of("holiday.png"), "Other");
    }

    #[test]
    fn adding_copies_into_the_library_once() {
        let root = std::env::temp_dir().join(format!("subcraft-texlib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("wall_normal.png");
        subcraft_image::save_image(&file, 4, 4, 3, &[0.5; 48], subcraft_image::FileFormat::Png8).unwrap();
        let lib = UserLibrary::new(root.join("Library"));
        let mut t = TextureLibrary::default();
        assert!(t.add_files(Some(&lib), &[file.clone()]).is_empty());
        assert!(t.add_files(Some(&lib), &[file.clone()]).is_empty());
        assert_eq!(t.items.len(), 1, "same file listed once");
        assert_eq!(t.items[0].category, "Normal");
        assert!(t.items[0].path.starts_with(lib.folder(UserAssetKind::Texture)));
        // A fresh load (restart) finds it in the library.
        assert_eq!(TextureLibrary::load(&lib).items.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
