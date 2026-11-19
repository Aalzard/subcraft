//! Project templates saved by the user (Painter's File > Save as template): the open project's
//! New Project settings (channels, resolution, normal format, export preset, UDIM tiles) under a
//! name, offered next to the built-in templates when a new project is made.
//!
//! One JSON file per template in `<data folder>/templates`. Layers are not part of a template,
//! as in Painter; a smart material carries those.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;

use crate::Session;
use crate::new_project::NewProjectSettings;

/// A named set of New Project settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserTemplate {
    pub name: String,
    pub settings: NewProjectSettings,
}

impl UserTemplate {
    /// `settings` without what belongs to one model (its high poly, per-material resolutions).
    pub fn new(name: impl Into<String>, mut settings: NewProjectSettings) -> Self {
        settings.high_poly = None;
        settings.resolution_overrides.clear();
        Self { name: name.into().trim().to_string(), settings }
    }

    /// Put this template's settings into `target`, keeping what the user chose for this model
    /// (its high poly and per-material resolutions, and whether to read its cameras).
    pub fn apply_to(&self, target: &mut NewProjectSettings) {
        let keep = (target.high_poly.take(), std::mem::take(&mut target.resolution_overrides), target.import_cameras);
        *target = self.settings.clone();
        (target.high_poly, target.resolution_overrides, target.import_cameras) = keep;
    }
}

/// The folder user templates live in.
pub fn templates_dir() -> Option<PathBuf> {
    crate::recovery::data_dir().map(|d| d.join("templates"))
}

/// A file name for a template name: letters, digits, spaces, `-` and `_` kept, the rest `_`.
pub fn file_name(name: &str) -> String {
    let clean: String = name.trim().chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
    format!("{}.json", if clean.trim().is_empty() { "template" } else { clean.trim() })
}

/// Save `template` in `dir` (replacing a template of the same name). Returns the file.
pub fn save(dir: &Path, template: &UserTemplate) -> Result<PathBuf, String> {
    if template.name.is_empty() {
        return Err("give the template a name".into());
    }
    if template.settings.channels.is_empty() {
        return Err("a template needs at least one channel".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(file_name(&template.name));
    let json = serde_json::to_string_pretty(template).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Every readable template in `dir`, sorted by name (unreadable files are skipped).
pub fn list(dir: &Path) -> Vec<UserTemplate> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<UserTemplate> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .filter_map(|p| serde_json::from_str::<UserTemplate>(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|t| !t.name.is_empty() && !t.settings.channels.is_empty())
        .collect();
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

/// Remove the template called `name` from `dir`.
pub fn delete(dir: &Path, name: &str) -> Result<(), String> {
    let path = dir.join(file_name(name));
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Session {
    /// New Project settings that would recreate this project's setup: the channels of every
    /// texture set (in the order the first set lists them, then any others), the most common
    /// resolution, the normal format, the export preset and whether it uses UDIM tiles.
    pub fn template_settings(&self) -> NewProjectSettings {
        let doc = self.document();
        let mut channels: Vec<ChannelKind> = Vec::new();
        for ts in &doc.texture_sets {
            for k in ts.channel_kinds() {
                if !channels.contains(&k) {
                    channels.push(k);
                }
            }
        }
        let mut counts: Vec<(u32, usize)> = Vec::new();
        for ts in &doc.texture_sets {
            match counts.iter_mut().find(|(r, _)| *r == ts.resolution) {
                Some((_, n)) => *n += 1,
                None => counts.push((ts.resolution, 1)),
            }
        }
        let defaults = NewProjectSettings::default();
        let resolution = counts.iter().max_by_key(|(r, n)| (*n, *r)).map_or(defaults.resolution, |(r, _)| *r);
        let settings = self.settings();
        let export = settings.export_template.trim();
        NewProjectSettings {
            channels,
            resolution: NewProjectSettings::valid_resolution(resolution),
            normal_format: settings.normal_format.unwrap_or(defaults.normal_format),
            export_template: (!export.is_empty()).then(|| export.to_string()),
            udim: doc.texture_sets.iter().any(|ts| ts.udim_tile.is_some()),
            ..defaults
        }
    }
}
