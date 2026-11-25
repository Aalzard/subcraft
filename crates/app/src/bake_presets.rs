//! Bake presets: named Bake window settings to reuse in other projects (maps, ray settings,
//! output size, anti-aliasing, dilation...). A preset never holds files or texture-set choices:
//! applying one keeps the project's high poly, cage and ticked sets.
//!
//! Built-in presets come first; the user's live in `<library>/bake-presets`, one versioned JSON
//! file per preset:
//! ```json
//! { "format": "subcraft.bake-preset", "version": 1, "id": "<uuid>", "name": "...", "config": { ... } }
//! ```
//! `config` is the project file's [`BakeConfig`] (fields it lacks read as defaults).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subcraft_assets::{UserAssetKind, UserLibrary};
use subcraft_core::MeshMapKind;
use subcraft_doc::BakeConfig;

pub const FORMAT: &str = "subcraft.bake-preset";
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BakePreset {
    /// UUID of a user preset; `None` for built-ins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub config: BakeConfig,
}

impl BakePreset {
    /// A preset of `config` without its files and texture-set choices.
    pub fn new(name: &str, config: &BakeConfig) -> Self {
        Self { id: Some(uuid::Uuid::new_v4().to_string()), name: name.trim().to_string(), config: settings_only(config) }
    }

    /// The user preset's id, or the built-in's name.
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }

    /// `current` with this preset's settings, keeping `current`'s files and texture-set choices.
    pub fn apply_to(&self, current: &BakeConfig) -> BakeConfig {
        BakeConfig {
            high_poly: current.high_poly.clone(),
            extra_high: current.extra_high.clone(),
            cage_file: current.cage_file.clone(),
            skip_sets: current.skip_sets.clone(),
            vertex_color_set: current.vertex_color_set,
            extra_vertex_color_sets: current.extra_vertex_color_sets.clone(),
            ..self.config.clone()
        }
    }

    /// Whether `config` already has this preset's settings.
    pub fn matches(&self, config: &BakeConfig) -> bool {
        settings_only(config) == self.config
    }
}

/// `config` without the parts that belong to one project (files, texture sets, vertex colour
/// sets of those files).
pub fn settings_only(config: &BakeConfig) -> BakeConfig {
    BakeConfig {
        high_poly: String::new(),
        extra_high: Vec::new(),
        cage_file: String::new(),
        skip_sets: Vec::new(),
        vertex_color_set: 0,
        extra_vertex_color_sets: Vec::new(),
        ..config.clone()
    }
}

/// The presets that ship with SubCraft.
pub fn builtin() -> Vec<BakePreset> {
    let d = BakeConfig { average_normals: true, ignore_backface: true, ..BakeConfig::default() };
    let preset = |name: &str, config: BakeConfig| BakePreset { id: None, name: name.into(), config };
    vec![
        preset(
            "Fast preview",
            BakeConfig {
                size: Some(512),
                maps: vec![MeshMapKind::Normal, MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature, MeshMapKind::WorldSpaceNormal, MeshMapKind::Position, MeshMapKind::Id],
                ao_samples: 16,
                thickness_samples: 8,
                antialiasing: 1,
                ..d.clone()
            },
        ),
        preset("Standard", BakeConfig { ao_samples: 64, thickness_samples: 32, antialiasing: 2, ..d.clone() }),
        preset("High quality", BakeConfig { ao_samples: 256, thickness_samples: 128, antialiasing: 4, dilation: 32, ..d }),
    ]
}

#[derive(Serialize, Deserialize)]
struct PresetFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    preset: BakePreset,
}

pub fn to_json(p: &BakePreset) -> String {
    serde_json::to_string_pretty(&PresetFile { format: FORMAT.into(), version: VERSION, preset: p.clone() }).expect("presets serialize")
}

pub fn from_json(text: &str) -> Result<BakePreset, String> {
    let file: PresetFile = serde_json::from_str(text).map_err(|e| format!("not a bake preset: {e}"))?;
    if file.format != FORMAT {
        return Err("not a bake preset".into());
    }
    if file.version > VERSION {
        return Err(format!("this preset was saved by a newer SubCraft (version {})", file.version));
    }
    Ok(file.preset)
}

/// Built-in and user bake presets. Saving and deleting write the library folder at once; without a
/// library folder user presets live for the session only.
#[derive(Clone, Debug, Default)]
pub struct BakePresets {
    dir: Option<PathBuf>,
    builtin: Vec<BakePreset>,
    user: Vec<BakePreset>,
    /// Files that could not be read, for the status line.
    pub load_errors: Vec<String>,
}

impl BakePresets {
    pub fn load(lib: Option<&UserLibrary>) -> Self {
        Self::load_dir(lib.map(|l| l.folder(UserAssetKind::BakePreset)))
    }

    pub fn load_dir(dir: Option<PathBuf>) -> Self {
        let mut user = Vec::new();
        let mut load_errors = Vec::new();
        if let Some(dir) = &dir {
            let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
            files.sort();
            for path in files {
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| from_json(&t)) {
                    Ok(mut p) => {
                        // The file name is the id: a copied file must not overwrite its original.
                        p.id = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                        user.push(p);
                    }
                    Err(e) => load_errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        user.sort_by_key(|p| p.name.to_lowercase());
        Self { dir, builtin: builtin(), user, load_errors }.with_plugin_presets(&crate::plugins::active_libraries())
    }

    /// Add the bake presets of plugin libraries after the built-ins, read-only like them. Their
    /// ids are `plugin:<file name>`, so they never collide with the user's own files.
    pub fn with_plugin_presets(mut self, libs: &[UserLibrary]) -> Self {
        for lib in libs {
            let mut files: Vec<PathBuf> = lib.list(UserAssetKind::BakePreset);
            files.sort();
            for path in files {
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| from_json(&t)) {
                    Ok(mut p) => {
                        p.id = path.file_stem().map(|s| format!("plugin:{}", s.to_string_lossy()));
                        if !self.builtin.iter().any(|b| b.key() == p.key()) {
                            self.builtin.push(p);
                        }
                    }
                    Err(e) => self.load_errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        self
    }

    /// Built-ins first, then user presets by name.
    pub fn all(&self) -> impl Iterator<Item = &BakePreset> {
        self.builtin.iter().chain(&self.user)
    }

    pub fn user(&self) -> &[BakePreset] {
        &self.user
    }

    pub fn find(&self, key: &str) -> Option<&BakePreset> {
        self.all().find(|p| p.key() == key)
    }

    /// A preset whose settings `config` has: the user's own first (a preset saved from a
    /// built-in's settings shows under its own name).
    pub fn matching(&self, config: &BakeConfig) -> Option<&BakePreset> {
        self.user.iter().chain(&self.builtin).find(|p| p.matches(config))
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{id}.json")))
    }

    /// Save `config`'s settings as a user preset named `name`. A user preset of the same name is
    /// replaced; a built-in's name gets a number.
    pub fn save(&mut self, name: &str, config: &BakeConfig) -> Result<BakePreset, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a preset needs a name".into());
        }
        let name = if self.builtin.iter().any(|p| p.name.eq_ignore_ascii_case(name)) { unique(name, &self.all().map(|p| p.name.clone()).collect::<Vec<_>>()) } else { name.to_string() };
        let mut preset = BakePreset::new(&name, config);
        if let Some(old) = self.user.iter().find(|p| p.name.eq_ignore_ascii_case(&name)) {
            preset.id = old.id.clone();
        }
        let id = preset.id.clone().expect("user presets have ids");
        if let Some(path) = self.path(&id) {
            write(&path, &to_json(&preset))?;
        }
        self.user.retain(|p| p.id != preset.id);
        self.user.push(preset.clone());
        self.user.sort_by_key(|p| p.name.to_lowercase());
        Ok(preset)
    }

    /// Remove a user preset (built-ins cannot be deleted).
    pub fn delete(&mut self, key: &str) -> Result<(), String> {
        let Some(i) = self.user.iter().position(|p| p.key() == key) else { return Err("only your own presets can be deleted".into()) };
        if let Some(path) = self.path(key)
            && path.exists()
        {
            std::fs::remove_file(&path).map_err(|e| format!("cannot delete {}: {e}", path.display()))?;
        }
        self.user.remove(i);
        Ok(())
    }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn unique(base: &str, taken: &[String]) -> String {
    (2..).map(|n| format!("{base} {n}")).find(|c| !taken.iter().any(|t| t.eq_ignore_ascii_case(c))).expect("some number is free")
}
