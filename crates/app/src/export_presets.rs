//! Export presets the Export Textures window offers: the read-only built-in templates followed
//! by the user's presets from `<library>/export-presets`. Every edit is written to disk at once;
//! without a library folder the presets live for the session only.

use std::path::Path;

use subcraft_assets::{UserAssetKind, UserLibrary};
use subcraft_core::ChannelKind;
use subcraft_export::preset::{self, PresetLibrary};
use subcraft_export::{ExportTemplate, OutputMap, Source};
use subcraft_image::FileFormat;

#[derive(Clone, Debug, Default)]
pub struct ExportPresets {
    library: Option<PresetLibrary>,
    builtin: Vec<ExportTemplate>,
    user: Vec<ExportTemplate>,
    /// Problems reading the library (unreadable files), for the status line.
    pub load_errors: Vec<String>,
}

impl ExportPresets {
    /// Built-ins plus the presets stored in `lib` (none when `lib` is `None`).
    pub fn load(lib: Option<&UserLibrary>) -> Self {
        let library = lib.map(|l| PresetLibrary::new(l.folder(UserAssetKind::ExportPreset)));
        let (user, load_errors) = library.as_ref().map(PresetLibrary::load).unwrap_or_default();
        Self { library, builtin: ExportTemplate::builtin(), user, load_errors }
    }

    /// Built-ins first, then user presets by name.
    pub fn all(&self) -> impl Iterator<Item = &ExportTemplate> {
        self.builtin.iter().chain(&self.user)
    }

    pub fn builtin(&self) -> &[ExportTemplate] {
        &self.builtin
    }

    pub fn user(&self) -> &[ExportTemplate] {
        &self.user
    }

    /// Preset by [`ExportTemplate::key`] (user UUID or built-in name).
    pub fn find(&self, key: &str) -> Option<&ExportTemplate> {
        self.user.iter().find(|p| p.id.as_deref() == Some(key)).or_else(|| self.builtin.iter().find(|p| p.name == key))
    }

    fn names(&self) -> Vec<&str> {
        self.all().map(|p| p.name.as_str()).collect()
    }

    fn store(&mut self, p: ExportTemplate) -> Result<ExportTemplate, String> {
        if let Some(lib) = &self.library {
            lib.save(&p).map_err(|e| e.to_string())?;
        }
        match self.user.iter_mut().find(|u| u.id == p.id) {
            Some(slot) => *slot = p.clone(),
            None => self.user.push(p.clone()),
        }
        self.user.sort_by_key(|u| u.name.to_lowercase());
        Ok(p)
    }

    /// A new preset with one sRGB base color map, named `New preset` (numbered if taken).
    pub fn create(&mut self) -> Result<ExportTemplate, String> {
        let name = preset::unique_name("New preset", &self.names());
        let base = OutputMap {
            pattern: Some("$mesh_$textureSet_$suffix".into()),
            ..OutputMap::new("BaseColor", FileFormat::Png8, (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect(), true)
        };
        self.store(ExportTemplate { name, maps: vec![base], id: Some(preset::new_id()), ..Default::default() })
    }

    /// Copy any preset (built-in or user) as a new user preset (`<name> copy`).
    pub fn duplicate(&mut self, key: &str) -> Result<ExportTemplate, String> {
        let src = self.find(key).cloned().ok_or("no such preset")?;
        let name = preset::unique_name(&format!("{} copy", src.name), &self.names());
        self.store(preset::duplicate(&src, &name))
    }

    /// Save an edited user preset (refuses built-ins and presets that would not export).
    pub fn update(&mut self, p: ExportTemplate) -> Result<ExportTemplate, String> {
        if p.is_builtin() || !self.user.iter().any(|u| u.id == p.id) {
            return Err("built-in templates are read-only: duplicate one to edit it".into());
        }
        if let Some(problem) = p.problems().into_iter().next() {
            return Err(problem);
        }
        self.store(p)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<ExportTemplate, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("the preset needs a name".into());
        }
        let mut p = self.find(id).filter(|p| !p.is_builtin()).cloned().ok_or("built-in templates can't be renamed")?;
        let others: Vec<&str> = self.all().filter(|o| o.id.as_deref() != Some(id)).map(|o| o.name.as_str()).collect();
        p.name = preset::unique_name(name, &others);
        self.store(p)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if !self.user.iter().any(|u| u.id.as_deref() == Some(id)) {
            return Err("built-in templates can't be deleted".into());
        }
        if let Some(lib) = &self.library {
            lib.delete(id).map_err(|e| e.to_string())?;
        }
        self.user.retain(|u| u.id.as_deref() != Some(id));
        Ok(())
    }

    /// Import a SubCraft preset file or a Painter export preset. Imported presets get fresh ids
    /// and unique names. Returns them with notes about Painter features that were dropped.
    pub fn import(&mut self, path: &Path) -> Result<(Vec<ExportTemplate>, Vec<String>), String> {
        let imported = preset::read_file(path).map_err(|e| e.to_string())?;
        let mut added = Vec::new();
        for p in imported.presets {
            let name = preset::unique_name(&p.name, &self.names());
            added.push(self.store(preset::duplicate(&p, &name))?);
        }
        Ok((added, imported.warnings))
    }

    /// Write a preset (built-in or user) as a SubCraft preset JSON file.
    pub fn export_to(&self, key: &str, path: &Path) -> Result<(), String> {
        let p = self.find(key).ok_or("no such preset")?;
        subcraft_core::vfs::write(path, preset::to_json(p).as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }
}
