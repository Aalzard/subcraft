//! Plugins: folders that add scripts and assets to SubCraft.
//!
//! A plugin is a folder inside the plugins folder (`Documents/SubCraft/Plugins`) with a
//! `plugin.json` manifest:
//!
//! ```text
//! Rusty Props/
//!   plugin.json          name, version, author, ... (see `PluginManifest`)
//!   scripts/*.rhai       entries of the Plugins menu (Rhai scripts over the tool catalog)
//!   library/             assets, laid out like the user library: smart-materials/,
//!                        smart-masks/, brush-presets/, alphas/, textures/, environments/,
//!                        export-presets/, bake-presets/
//! ```
//!
//! Plugins hold no native code: scripts run in the Rhai engine and assets go through the same
//! loaders as the user library, so a plugin works the same on Windows and Linux and cannot crash
//! the app. Identity is the manifest's UUID, never the folder name: renaming or moving the folder
//! keeps the plugin's on/off state.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subcraft_assets::{UserAssetKind, UserLibrary};

/// The manifest's file name.
pub const MANIFEST: &str = "plugin.json";
/// Value of the manifest's `format` field.
pub const FORMAT: &str = "subcraft-plugin";
/// The newest manifest schema this build reads.
pub const SCHEMA: u32 = 1;
/// Sub-folder of a plugin with its Rhai scripts.
pub const SCRIPTS_DIR: &str = "scripts";
/// Sub-folder of a plugin with its assets (user library layout).
pub const LIBRARY_DIR: &str = "library";
/// The file extension of scripts.
pub const SCRIPT_EXTENSION: &str = "rhai";

/// Limits of a plugin `.zip`: more than this is refused (a zip bomb, or not a plugin).
const MAX_ZIP_ENTRIES: usize = 5000;
const MAX_ZIP_BYTES: u64 = 2 << 30;

/// Asset kinds a plugin's library can hold, with their names for people.
pub const ASSET_KINDS: [(UserAssetKind, &str, &str); 8] = [
    (UserAssetKind::SmartMaterial, "smart material", "smart materials"),
    (UserAssetKind::SmartMask, "smart mask", "smart masks"),
    (UserAssetKind::BrushPreset, "brush preset", "brush presets"),
    (UserAssetKind::Alpha, "brush alpha", "brush alphas"),
    (UserAssetKind::Texture, "texture", "textures"),
    (UserAssetKind::Environment, "environment", "environments"),
    (UserAssetKind::ExportPreset, "export preset", "export presets"),
    (UserAssetKind::BakePreset, "bake preset", "bake presets"),
];

/// `plugin.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// Manifest schema version ([`SCHEMA`]).
    pub schema: u32,
    /// The plugin's identity: a UUID that stays the same in every version of the plugin.
    pub id: String,
    pub name: String,
    /// The plugin's own version (any text; semantic versions read best).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Web page of the plugin (shown as a link).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub homepage: String,
    /// Oldest SubCraft version the plugin works with (`"0.7.0"`); empty for any.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub min_subcraft_version: String,
}

impl PluginManifest {
    /// Read and check a manifest. `app_version` is this build's version.
    pub fn parse(json: &str, app_version: &str) -> Result<Self, String> {
        let m: PluginManifest = serde_json::from_str(json).map_err(|e| format!("plugin.json is not valid: {e}"))?;
        if m.format != FORMAT {
            return Err(format!("plugin.json: \"format\" must be \"{FORMAT}\""));
        }
        if m.schema == 0 || m.schema > SCHEMA {
            return Err(format!("plugin.json schema {} needs a newer SubCraft (this one reads up to {SCHEMA})", m.schema));
        }
        if uuid::Uuid::parse_str(m.id.trim()).is_err() {
            return Err("plugin.json: \"id\" must be a UUID (for example 3f2c9a3e-8a51-4c0e-9d0a-2b7f4b9d6c11)".into());
        }
        if m.name.trim().is_empty() {
            return Err("plugin.json: \"name\" is empty".into());
        }
        if !m.min_subcraft_version.trim().is_empty() {
            let need = semver::Version::parse(m.min_subcraft_version.trim())
                .map_err(|_| format!("plugin.json: \"min_subcraft_version\" {:?} is not a version like 0.7.0", m.min_subcraft_version))?;
            if let Ok(have) = semver::Version::parse(app_version) {
                if have < need {
                    return Err(format!("needs SubCraft {need} or newer (this is {have})"));
                }
            }
        }
        Ok(m)
    }

    /// The id in one spelling (lower case, no spaces), for comparing and for the settings.
    pub fn key(&self) -> String {
        uuid::Uuid::parse_str(self.id.trim()).map(|u| u.to_string()).unwrap_or_else(|_| self.id.trim().to_lowercase())
    }
}

/// A plugin found on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// Its folder.
    pub dir: PathBuf,
    /// `None` when `plugin.json` is missing or broken (see [`Plugin::problem`]).
    pub manifest: Option<PluginManifest>,
    /// Why the plugin cannot be used; such a plugin is listed but nothing of it loads.
    pub problem: Option<String>,
    /// Its scripts, sorted by name: (display name, path).
    pub scripts: Vec<(String, PathBuf)>,
    /// Number of assets of each kind in its library (same order as [`ASSET_KINDS`]).
    pub asset_counts: [usize; ASSET_KINDS.len()],
}

impl Plugin {
    /// Read the plugin in `dir`.
    pub fn load(dir: &Path, app_version: &str) -> Plugin {
        let (manifest, problem) = match std::fs::read_to_string(dir.join(MANIFEST)) {
            Ok(json) => match PluginManifest::parse(&json, app_version) {
                Ok(m) => (Some(m), None),
                // Keep the name of a plugin that only needs a newer SubCraft.
                Err(e) => (serde_json::from_str::<PluginManifest>(&json).ok(), Some(e)),
            },
            Err(e) => (None, Some(format!("cannot read plugin.json: {e}"))),
        };
        let library = UserLibrary::new(dir.join(LIBRARY_DIR));
        let mut asset_counts = [0; ASSET_KINDS.len()];
        for (i, (kind, ..)) in ASSET_KINDS.iter().enumerate() {
            asset_counts[i] = library.list(*kind).len();
        }
        Plugin { dir: dir.to_path_buf(), manifest, problem, scripts: list_scripts(&dir.join(SCRIPTS_DIR)), asset_counts }
    }

    /// Name to show: the manifest's, else the folder's.
    pub fn name(&self) -> String {
        match &self.manifest {
            Some(m) => m.name.trim().to_string(),
            None => self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    /// The id key ([`PluginManifest::key`]), `None` without a readable manifest.
    pub fn key(&self) -> Option<String> {
        self.manifest.as_ref().map(PluginManifest::key)
    }

    /// True when the plugin can be used (its manifest is fine).
    pub fn usable(&self) -> bool {
        self.manifest.is_some() && self.problem.is_none()
    }

    /// The plugin's asset library, if it has any assets.
    pub fn library(&self) -> Option<UserLibrary> {
        (self.asset_counts.iter().sum::<usize>() > 0).then(|| UserLibrary::new(self.dir.join(LIBRARY_DIR)))
    }

    /// "2 scripts, 3 smart materials, 1 export preset" (or "empty").
    pub fn contents_summary(&self) -> String {
        let mut parts = Vec::new();
        let n = self.scripts.len();
        if n > 0 {
            parts.push(format!("{n} {}", if n == 1 { "script" } else { "scripts" }));
        }
        for (i, (_, one, many)) in ASSET_KINDS.iter().enumerate() {
            let n = self.asset_counts[i];
            if n > 0 {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        if parts.is_empty() { "empty".to_string() } else { parts.join(", ") }
    }
}

/// Asset libraries of the plugins that are on, for the lists that read presets on their own
/// (Export Textures and Bake presets). The UI sets them whenever plugins are found or switched.
static ACTIVE_LIBRARIES: std::sync::RwLock<Vec<UserLibrary>> = std::sync::RwLock::new(Vec::new());

/// Set the libraries of the plugins that are on.
pub fn set_active_libraries(libs: Vec<UserLibrary>) {
    *ACTIVE_LIBRARIES.write().unwrap_or_else(|e| e.into_inner()) = libs;
}

/// The libraries of the plugins that are on.
pub fn active_libraries() -> Vec<UserLibrary> {
    ACTIVE_LIBRARIES.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `.rhai` files in `dir` (not in sub-folders), sorted by name: (display name, path).
/// Underscores and dashes in file names read as spaces.
pub fn list_scripts(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(SCRIPT_EXTENSION)))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default(), p))
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.1.cmp(&b.1)));
    out
}

/// Every plugin folder in `dir` (sub-folders with a `plugin.json`), sorted by name. A second
/// plugin with an id already taken is listed with a problem, so it never shadows the first.
/// Folders whose name starts with `.` are skipped (half-finished installs).
pub fn discover(dir: &Path, app_version: &str) -> Vec<Plugin> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(MANIFEST).is_file())
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    dirs.sort();
    let mut plugins: Vec<Plugin> = dirs.iter().map(|d| Plugin::load(d, app_version)).collect();
    plugins.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()).then_with(|| a.dir.cmp(&b.dir)));
    let mut seen = std::collections::HashSet::new();
    for p in &mut plugins {
        if let Some(key) = p.key() {
            if !seen.insert(key) && p.problem.is_none() {
                p.problem = Some("another plugin has the same id; remove one of them".into());
            }
        }
    }
    plugins
}

/// True unless the plugin's id is in `disabled` (plugins are on when installed).
pub fn is_enabled(plugin: &Plugin, disabled: &[String]) -> bool {
    plugin.key().is_none_or(|k| !disabled.iter().any(|d| d.trim().eq_ignore_ascii_case(&k)))
}

/// Switch a plugin on or off in the `disabled` list of the settings.
pub fn set_enabled(plugin: &Plugin, on: bool, disabled: &mut Vec<String>) {
    let Some(key) = plugin.key() else { return };
    disabled.retain(|d| !d.trim().eq_ignore_ascii_case(&key));
    if !on {
        disabled.push(key);
    }
}

/// A folder name made from a plugin's name (characters not allowed in file names replaced).
fn folder_name(name: &str) -> String {
    let cleaned: String = name.trim().chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    if cleaned.is_empty() { "plugin".into() } else { cleaned }
}

/// Install the plugin in a `.zip` into `plugins_dir`. The zip holds `plugin.json` at its root or
/// inside one top folder. A plugin with the same id already installed is replaced (an update).
/// Returns the installed plugin.
pub fn install_zip(zip_path: &Path, plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    install_zip_reader(file, plugins_dir, app_version)
}

/// [`install_zip`] from any reader.
pub fn install_zip_reader<R: std::io::Read + std::io::Seek>(reader: R, plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("not a zip file: {e}"))?;
    if zip.len() > MAX_ZIP_ENTRIES {
        return Err(format!("the zip has {} files; a plugin may have at most {MAX_ZIP_ENTRIES}", zip.len()));
    }
    // Where plugin.json is: the root, or one top folder.
    let mut names = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("the zip has an unsafe path: {}", entry.name().unwrap_or_default()));
        };
        names.push((name, entry.is_dir()));
    }
    let manifests: Vec<&PathBuf> = names.iter().map(|(n, _)| n).filter(|n| n.file_name().is_some_and(|f| f == MANIFEST)).collect();
    let prefix = manifests
        .iter()
        .filter_map(|n| n.parent())
        .min_by_key(|p| p.components().count())
        .map(Path::to_path_buf)
        .ok_or("the zip has no plugin.json")?;
    if prefix.components().count() > 1 {
        return Err("plugin.json must be at the root of the zip or in one top folder".into());
    }
    let manifest_json = {
        let wanted = prefix.join(MANIFEST);
        let index = names.iter().position(|(n, _)| *n == wanted).ok_or("the zip has no plugin.json")?;
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let mut s = String::new();
        entry.by_ref().take(1 << 20).read_to_string(&mut s).map_err(|e| format!("plugin.json: {e}"))?;
        s
    };
    let manifest = PluginManifest::parse(&manifest_json, app_version)?;

    std::fs::create_dir_all(plugins_dir).map_err(|e| format!("{}: {e}", plugins_dir.display()))?;
    let staging = plugins_dir.join(format!(".installing-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.enclosed_name().ok_or("unsafe path")?;
            let Ok(rel) = name.strip_prefix(&prefix) else { continue };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let dest = staging.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            let left = MAX_ZIP_BYTES.saturating_sub(total);
            let n = std::io::copy(&mut entry.by_ref().take(left + 1), &mut out).map_err(|e| format!("{}: {e}", rel.display()))?;
            total += n;
            if total > MAX_ZIP_BYTES {
                return Err(format!("the plugin unpacks to more than {} MB", MAX_ZIP_BYTES >> 20));
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    finish_install(&staging, &manifest, plugins_dir, app_version)
}

/// Install the plugin folder `src` into `plugins_dir` by copying it (same rules as
/// [`install_zip`]).
pub fn install_folder(src: &Path, plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    let json = std::fs::read_to_string(src.join(MANIFEST)).map_err(|e| format!("{}: {e}", src.join(MANIFEST).display()))?;
    let manifest = PluginManifest::parse(&json, app_version)?;
    std::fs::create_dir_all(plugins_dir).map_err(|e| format!("{}: {e}", plugins_dir.display()))?;
    let staging = plugins_dir.join(format!(".installing-{}", uuid::Uuid::new_v4()));
    if let Err(e) = copy_dir(src, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    finish_install(&staging, &manifest, plugins_dir, app_version)
}

/// Install files given as (path inside the plugin, bytes), e.g. example plugins built into the
/// app. Same rules as [`install_zip`].
pub fn install_files(files: &[(&str, &[u8])], plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    let json = files.iter().find(|(p, _)| *p == MANIFEST).ok_or("no plugin.json")?.1;
    let manifest = PluginManifest::parse(&String::from_utf8_lossy(json), app_version)?;
    std::fs::create_dir_all(plugins_dir).map_err(|e| format!("{}: {e}", plugins_dir.display()))?;
    let staging = plugins_dir.join(format!(".installing-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        for (rel, bytes) in files {
            let rel = Path::new(rel);
            if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                return Err(format!("unsafe path {}", rel.display()));
            }
            let dest = staging.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    finish_install(&staging, &manifest, plugins_dir, app_version)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let to = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &to).map_err(|e| format!("{}: {e}", entry.path().display()))?;
        }
        // Symbolic links are skipped: a plugin is plain files.
    }
    Ok(())
}

/// Move a staged install into place: replace the installed plugin with the same id, or take a
/// free folder named after the plugin.
fn finish_install(staging: &Path, manifest: &PluginManifest, plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    let key = manifest.key();
    let old = discover(plugins_dir, app_version).into_iter().find(|p| p.key().as_deref() == Some(key.as_str()));
    let dest = match &old {
        Some(p) => p.dir.clone(),
        None => {
            let base = folder_name(&manifest.name);
            (1..).map(|n| if n == 1 { plugins_dir.join(&base) } else { plugins_dir.join(format!("{base} ({n})")) }).find(|d| !d.exists()).expect("a free name")
        }
    };
    if old.is_some() {
        // Keep the old version aside until the new one is in place.
        let backup = plugins_dir.join(format!(".replaced-{}", uuid::Uuid::new_v4()));
        std::fs::rename(&dest, &backup).map_err(|e| {
            let _ = std::fs::remove_dir_all(staging);
            format!("cannot replace {}: {e}", dest.display())
        })?;
        if let Err(e) = std::fs::rename(staging, &dest) {
            let _ = std::fs::rename(&backup, &dest);
            let _ = std::fs::remove_dir_all(staging);
            return Err(format!("{}: {e}", dest.display()));
        }
        let _ = std::fs::remove_dir_all(&backup);
    } else if let Err(e) = std::fs::rename(staging, &dest) {
        let _ = std::fs::remove_dir_all(staging);
        return Err(format!("{}: {e}", dest.display()));
    }
    log::info!("installed plugin {} {} into {}", manifest.name, manifest.version, dest.display());
    Ok(Plugin::load(&dest, app_version))
}

/// Delete an installed plugin's folder. Only a folder directly inside `plugins_dir` is removed.
pub fn uninstall(plugin: &Plugin, plugins_dir: &Path) -> Result<(), String> {
    let parent_ok = match (plugin.dir.parent().map(std::fs::canonicalize), std::fs::canonicalize(plugins_dir)) {
        (Some(Ok(a)), Ok(b)) => a == b,
        _ => false,
    };
    if !parent_ok || !plugin.dir.join(MANIFEST).is_file() {
        return Err(format!("{} is not a plugin in the plugins folder", plugin.dir.display()));
    }
    std::fs::remove_dir_all(&plugin.dir).map_err(|e| format!("{}: {e}", plugin.dir.display()))?;
    log::info!("removed plugin {} ({})", plugin.name(), plugin.dir.display());
    Ok(())
}

/// A new manifest for a plugin named `name`, with a fresh id.
pub fn new_manifest(name: &str, author: &str) -> PluginManifest {
    PluginManifest {
        format: FORMAT.into(),
        schema: SCHEMA,
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        version: "1.0.0".into(),
        author: author.trim().to_string(),
        description: String::new(),
        homepage: String::new(),
        min_subcraft_version: String::new(),
    }
}

/// Create an empty plugin (manifest, `scripts/` with a starter script, `library/` folders) in
/// `plugins_dir`, so people can start from something that already loads.
pub fn create_plugin(name: &str, author: &str, plugins_dir: &Path, app_version: &str) -> Result<Plugin, String> {
    if name.trim().is_empty() {
        return Err("give the plugin a name".into());
    }
    let mut manifest = new_manifest(name, author);
    manifest.description = "Describe what the plugin adds.".into();
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    let script = "// Runs on the open project from the Plugins menu.\n// The API: docs/scripting.md\nlet info = tool(\"project_info\");\nprint(`${info.texture_sets.len()} texture sets`);\n";
    let mut files: Vec<(String, Vec<u8>)> = vec![(MANIFEST.into(), json.into_bytes()), (format!("{SCRIPTS_DIR}/hello.rhai"), script.as_bytes().to_vec())];
    let lib = UserLibrary::new(LIBRARY_DIR);
    for (kind, ..) in ASSET_KINDS {
        let folder = lib.folder(kind).to_string_lossy().replace('\\', "/");
        files.push((format!("{folder}/.keep"), Vec::new()));
    }
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    install_files(&refs, plugins_dir, app_version)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod examples;
