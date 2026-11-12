//! Edit in PhotoCraft (ADR-0040): write the selected texture set's layer stack as one PSD per
//! channel, open them in PhotoCraft, and bring every save back as one undo step.
//!
//! The exchange is owned by the caller (the UI keeps it next to the session) and polled once a
//! frame; [`Exchange::poll`] only touches the disk every [`POLL_INTERVAL`] seconds.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use subcraft_core::{ChannelKind, TextureSetId};
use subcraft_export::psd::{self, ChannelBaseline};

use crate::{Command, Session};

/// Seconds between file checks.
pub const POLL_INTERVAL: f64 = 0.5;

/// Environment variable naming the PhotoCraft executable.
pub const EXECUTABLE_ENV: &str = "SUBCRAFT_PHOTOCRAFT";

/// A file's modification time and size: a save is applied once two checks agree.
type Stamp = (SystemTime, u64);

struct ChannelFile {
    channel: ChannelKind,
    path: PathBuf,
    baseline: ChannelBaseline,
    applied: Option<Stamp>,
    pending: Option<Stamp>,
}

/// A running exchange for one texture set.
pub struct Exchange {
    texture_set: TextureSetId,
    set_name: String,
    dir: PathBuf,
    files: Vec<ChannelFile>,
    last_poll: f64,
}

/// What a poll brought back.
#[derive(Debug, Clone, PartialEq)]
pub enum PollEvent {
    /// A save was applied (status text for the user).
    Applied(String),
    /// A save could not be applied; the exchange keeps running.
    Failed(String),
    /// The texture set is gone; the exchange has to stop.
    Ended(String),
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl Exchange {
    /// Writes the PSDs of the selected texture set into `dir` and hands their paths to `open`.
    pub fn start(session: &Session, dir: &Path, open: impl FnOnce(&[PathBuf]) -> Result<(), String>) -> Result<Self, String> {
        let set = session.selected_texture_set();
        if !has_paint_layer(&set.layers) {
            return Err("this texture set has no paint layers to edit (fill layers stay in SubCraft)".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let mut files = Vec::new();
        for channel in set.channel_kinds() {
            let out = psd::write_channel(set, channel).map_err(|e| format!("{}: {e}", channel.display_name()))?;
            let name = format!("{}_{}.psd", subcraft_export::sanitize(&set.name), subcraft_export::sanitize(channel.display_name()));
            let path = dir.join(name);
            std::fs::write(&path, &out.bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            files.push(ChannelFile { channel, applied: stamp(&path), pending: None, path, baseline: out.baseline });
        }
        let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        open(&paths)?;
        log::info!("photocraft: editing {} ({} files in {})", set.name, files.len(), dir.display());
        Ok(Self { texture_set: set.id, set_name: set.name.clone(), dir: dir.to_path_buf(), files, last_poll: f64::NEG_INFINITY })
    }

    pub fn texture_set(&self) -> TextureSetId {
        self.texture_set
    }

    pub fn set_name(&self) -> &str {
        &self.set_name
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(|f| f.path.as_path())
    }

    /// Checks the files (at most every [`POLL_INTERVAL`] seconds of `now`) and applies each save
    /// that has settled.
    pub fn poll(&mut self, session: &mut Session, now: f64) -> Vec<PollEvent> {
        if now - self.last_poll < POLL_INTERVAL {
            return Vec::new();
        }
        self.last_poll = now;
        self.check(session)
    }

    /// One check, regardless of time.
    pub fn check(&mut self, session: &mut Session) -> Vec<PollEvent> {
        let mut events = Vec::new();
        // Replacing the layers would commit a partially drawn stroke, and the save would then be
        // marked applied although it was refused; it is picked up once the stroke is over.
        if session.is_stroking() {
            return events;
        }
        for i in 0..self.files.len() {
            let current = stamp(&self.files[i].path);
            let file = &mut self.files[i];
            if current.is_none() || current == file.applied {
                file.pending = None;
                continue;
            }
            // Wait until the editor has finished writing (two checks see the same file).
            if file.pending != current {
                file.pending = current;
                continue;
            }
            file.pending = None;
            file.applied = current;
            match self.apply(i, session) {
                Ok(Some(text)) => events.push(PollEvent::Applied(text)),
                Ok(None) => {}
                Err(PollEvent::Ended(text)) => return vec![PollEvent::Ended(text)],
                Err(e) => events.push(e),
            }
        }
        events
    }

    fn apply(&mut self, index: usize, session: &mut Session) -> Result<Option<String>, PollEvent> {
        let Some(set) = session.document().texture_set(self.texture_set) else {
            return Err(PollEvent::Ended(format!("{} was removed; stopped editing in PhotoCraft", self.set_name)));
        };
        let file = &mut self.files[index];
        let label = file.channel.display_name();
        let bytes = std::fs::read(&file.path).map_err(|e| PollEvent::Failed(format!("{label}: cannot read {}: {e}", file.path.display())))?;
        let edit = psd::apply_channel(set, file.channel, &bytes, &file.baseline).map_err(|e| PollEvent::Failed(format!("{label}: {e}")))?;
        file.baseline = edit.baseline;
        let r = edit.report;
        if r.is_empty() {
            return Ok((r.missing > 0).then(|| missing_text(label, r.missing)));
        }
        session
            .execute(Command::ReplaceTextureSetLayers { texture_set: self.texture_set, layers: edit.layers, label: format!("Edit in PhotoCraft ({label})") })
            .map_err(|e| PollEvent::Failed(format!("{label}: {e}")))?;
        let mut parts = Vec::new();
        if r.changed_tiles > 0 { parts.push(format!("{} tile(s) repainted", r.changed_tiles)); }
        if !r.added.is_empty() { parts.push(format!("{} layer(s) added", r.added.len())); }
        if r.renamed > 0 { parts.push(format!("{} renamed", r.renamed)); }
        if r.visibility > 0 { parts.push(format!("{} visibility change(s)", r.visibility)); }
        let mut text = format!("PhotoCraft → {label}: {}", parts.join(", "));
        if r.missing > 0 { text = format!("{text}. {}", missing_text(label, r.missing)); }
        log::info!("photocraft: {text}");
        Ok(Some(text))
    }
}

fn missing_text(label: &str, n: usize) -> String {
    format!("{n} layer(s) deleted in the {label} file were kept; delete them in SubCraft")
}

fn has_paint_layer(layers: &[std::sync::Arc<subcraft_doc::Layer>]) -> bool {
    layers.iter().any(|l| matches!(l.content, subcraft_doc::LayerContent::Paint(_)) || has_paint_layer(l.children()))
}

/// Session folder for a texture set's exchange files.
pub fn exchange_dir(texture_set: TextureSetId) -> PathBuf {
    std::env::temp_dir().join("subcraft-photocraft").join(texture_set.0.simple().to_string())
}

/// The PhotoCraft executable: [`EXECUTABLE_ENV`], then `photocraft` on `PATH`, then a sibling
/// development checkout next to this program's folder (`…/photocraft/target/{release,debug}`).
pub fn find_executable() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(EXECUTABLE_ENV).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = format!("photocraft{}", std::env::consts::EXE_SUFFIX);
    if let Some(p) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&exe)).find(|p| p.is_file())) {
        return Some(p);
    }
    let here = std::env::current_exe().ok()?;
    here.ancestors().skip(1).find_map(|dir| {
        ["release", "debug"].iter().map(|profile| dir.join("photocraft").join("target").join(profile).join(&exe)).find(|p| p.is_file())
    })
}

/// Opens `paths` in PhotoCraft (one process; it shows each file as a document).
pub fn launch(paths: &[PathBuf]) -> Result<(), String> {
    let exe = find_executable().ok_or_else(|| format!("PhotoCraft was not found: install it on PATH or set {EXECUTABLE_ENV} to its executable"))?;
    std::process::Command::new(&exe).args(paths).spawn().map(|_| ()).map_err(|e| format!("cannot start {}: {e}", exe.display()))
}
