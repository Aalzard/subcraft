//! Autosave and crash recovery.
//!
//! Every running SubCraft owns a session folder under `<data dir>/recovery/<id>/` holding
//! `session.json` (what is open), `running.lock` (locked for as long as the process lives) and
//! the last few `autosave-NNNN.subcraft` copies. A clean exit deletes the folder. A folder whose
//! lock nobody holds belongs to a SubCraft that crashed or was killed: [`RecoveryStore::crashed`]
//! finds those so the user can recover the newest copy.
//!
//! Autosaves never touch the user's own project file: they only go into the session folder.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use subcraft_doc::Document;
use web_time::Instant;

use crate::session::Session;

/// Overrides the data folder (logs, crash reports, recovery copies); handy for tests.
pub const DATA_DIR_ENV: &str = "SUBCRAFT_DATA";

/// Default time between autosaves.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Recovery copies kept per session.
pub const DEFAULT_KEEP: usize = 3;
/// After a big operation an autosave runs at once, unless the last one is more recent than this.
pub const MIN_GAP_AFTER_OPERATION: Duration = Duration::from_secs(30);

const INFO_FILE: &str = "session.json";
const LOCK_FILE: &str = "running.lock";
const PREFIX: &str = "autosave-";

/// SubCraft's per-user data folder: `$SUBCRAFT_DATA`, else the platform's local data folder
/// (`%LOCALAPPDATA%\SubCraft` on Windows). `None` in the browser.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    #[cfg(target_arch = "wasm32")]
    return None;
    #[cfg(not(target_arch = "wasm32"))]
    dirs::data_local_dir().map(|d| d.join("SubCraft"))
}

/// What a session folder belongs to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// SubCraft version that wrote it.
    pub version: String,
    /// Process start, seconds since the Unix epoch.
    pub started: u64,
    /// Project shown in the title bar ("New Project", the file name, the mesh name).
    pub name: String,
    /// The user's own project file, if it was saved / opened from one.
    pub project_path: Option<PathBuf>,
}

impl SessionInfo {
    pub fn for_session(session: &Session) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            started: unix_now(),
            name: session.export_name(),
            project_path: session.project_path().map(Path::to_path_buf),
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The `recovery/` folder with one subfolder per running (or crashed) SubCraft.
#[derive(Clone, Debug)]
pub struct RecoveryStore {
    root: PathBuf,
}

impl RecoveryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `<data dir>/recovery`, or `None` when there is no data folder (web).
    pub fn default_store() -> Option<Self> {
        data_dir().map(|d| Self::new(d.join("recovery")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Claim a new session folder for this process.
    pub fn begin_session(&self, info: &SessionInfo, keep: usize) -> io::Result<LiveSession> {
        let dir = self.root.join(uuid::Uuid::new_v4().simple().to_string());
        std::fs::create_dir_all(&dir)?;
        let lock = File::options().create(true).truncate(false).write(true).open(dir.join(LOCK_FILE))?;
        lock.try_lock().map_err(|e| io::Error::other(format!("could not lock the session folder: {e}")))?;
        // The info is written after the lock is held, so a folder with info but no lock holder
        // really is abandoned.
        let live = LiveSession { dir, lock: Some(lock), keep: keep.max(1), info: info.clone() };
        live.write_info()?;
        Ok(live)
    }

    /// Sessions that ended without a clean exit and left recovery copies, newest first.
    /// Abandoned folders without any copy are deleted (nothing to recover).
    pub fn crashed(&self) -> Vec<CrashedSession> {
        let Ok(entries) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut out = Vec::new();
        for dir in entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
            if is_locked(&dir) {
                continue; // another SubCraft is running with it
            }
            let Some(info) = read_info(&dir) else {
                // Being created right now, or broken. Only clean up old ones.
                if age(&dir).is_some_and(|a| a > Duration::from_secs(600)) {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                continue;
            };
            remove_partial_saves(&dir);
            let autosaves = list_autosaves(&dir);
            if autosaves.is_empty() {
                let _ = std::fs::remove_dir_all(&dir);
                continue;
            }
            let saved_at = autosaves.last().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
            out.push(CrashedSession { dir, info, autosaves, saved_at });
        }
        out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
        out
    }
}

/// True if another process holds the folder's lock.
fn is_locked(dir: &Path) -> bool {
    let Ok(f) = File::options().write(true).open(dir.join(LOCK_FILE)) else { return false };
    match f.try_lock() {
        Ok(()) => {
            let _ = f.unlock();
            false
        }
        Err(_) => true,
    }
}

fn read_info(dir: &Path) -> Option<SessionInfo> {
    serde_json::from_slice(&std::fs::read(dir.join(INFO_FILE)).ok()?).ok()
}

fn age(path: &Path) -> Option<Duration> {
    std::fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()
}

/// Sequence number of an autosave file name (`autosave-0007.subcraft` → 7).
fn autosave_seq(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix(PREFIX)?.strip_suffix(&format!(".{}", subcraft_project::EXTENSION))?;
    rest.parse().ok()
}

/// Complete autosave copies in `dir`, oldest first.
pub fn list_autosaves(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(u32, PathBuf)> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter_map(|p| autosave_seq(&p).map(|s| (s, p))).collect();
    files.sort();
    files.into_iter().map(|(_, p)| p).collect()
}

/// Delete all but the newest `keep` autosaves in `dir`. Returns the deleted files.
pub fn rotate(dir: &Path, keep: usize) -> Vec<PathBuf> {
    let files = list_autosaves(dir);
    let excess = files.len().saturating_sub(keep);
    let mut deleted = Vec::new();
    for f in files.into_iter().take(excess) {
        if std::fs::remove_file(&f).is_ok() {
            deleted.push(f);
        }
    }
    deleted
}

/// Leftovers of saves interrupted by the crash (`*.saving`).
fn remove_partial_saves(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for p in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            if p.extension().is_some_and(|e| e == "saving") {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// This process's session folder. Dropping it without [`LiveSession::finish_clean`] leaves the
/// folder behind, which is exactly what a crash looks like.
#[derive(Debug)]
pub struct LiveSession {
    dir: PathBuf,
    lock: Option<File>,
    keep: usize,
    info: SessionInfo,
}

impl LiveSession {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn set_keep(&mut self, keep: usize) {
        self.keep = keep.max(1);
    }

    fn write_info(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.info).map_err(io::Error::other)?;
        std::fs::write(self.dir.join(INFO_FILE), json)
    }

    /// Record what is open now (after New / Open / Save As).
    pub fn set_info(&mut self, info: SessionInfo) -> io::Result<()> {
        if info.name == self.info.name && info.project_path == self.info.project_path {
            return Ok(());
        }
        self.info = SessionInfo { started: self.info.started, ..info };
        self.write_info()
    }

    /// Path for the next autosave copy (one past the newest existing one).
    pub fn next_autosave_path(&self) -> PathBuf {
        let next = list_autosaves(&self.dir).last().and_then(|p| autosave_seq(p)).map_or(1, |s| s + 1);
        self.dir.join(format!("{PREFIX}{next:04}.{}", subcraft_project::EXTENSION))
    }

    /// Newest complete autosave.
    pub fn latest(&self) -> Option<PathBuf> {
        list_autosaves(&self.dir).pop()
    }

    /// Delete copies beyond the configured count.
    pub fn rotate(&self) -> Vec<PathBuf> {
        rotate(&self.dir, self.keep)
    }

    /// Move a crashed session's copies into this folder (after recovering from it), so they
    /// stay protected until this session exits cleanly, then delete the old folder.
    pub fn adopt(&self, crashed: CrashedSession) -> io::Result<()> {
        for src in &crashed.autosaves {
            let dst = self.next_autosave_path();
            if std::fs::rename(src, &dst).is_err() {
                std::fs::copy(src, &dst)?;
            }
        }
        self.rotate();
        crashed.discard()
    }

    /// Clean exit: release the lock and delete the folder with its copies.
    pub fn finish_clean(mut self) -> io::Result<()> {
        drop(self.lock.take());
        std::fs::remove_dir_all(&self.dir)
    }
}

/// A session that did not exit cleanly.
#[derive(Clone, Debug)]
pub struct CrashedSession {
    pub dir: PathBuf,
    pub info: SessionInfo,
    /// Recovery copies, oldest first.
    pub autosaves: Vec<PathBuf>,
    /// When the newest copy was written.
    pub saved_at: SystemTime,
}

impl CrashedSession {
    pub fn latest(&self) -> &Path {
        self.autosaves.last().expect("crashed sessions have at least one copy")
    }

    /// Delete the folder and its copies.
    pub fn discard(self) -> io::Result<()> {
        std::fs::remove_dir_all(&self.dir)
    }
}

/// Finished autosave reported by [`Autosaver::poll`].
#[derive(Debug)]
pub enum AutosaveEvent {
    Saved(PathBuf),
    Failed(String),
}

/// Decides when to autosave and writes copies on a background thread.
pub struct Autosaver {
    live: Option<LiveSession>,
    /// Time between autosaves; zero turns the timer off (operations still autosave).
    pub interval: Duration,
    last_save: Instant,
    /// Document and mesh-map generation of the last copy (only changed work is saved again).
    last_saved: Option<(Document, u64)>,
    soon: bool,
    job: Option<Receiver<Result<PathBuf, String>>>,
}

impl Autosaver {
    /// `live = None` disables autosaving (no data folder, or it could not be created).
    pub fn new(live: Option<LiveSession>, interval: Duration) -> Self {
        Self { live, interval, last_save: Instant::now(), last_saved: None, soon: false, job: None }
    }

    pub fn disabled() -> Self {
        Self::new(None, DEFAULT_INTERVAL)
    }

    pub fn live(&self) -> Option<&LiveSession> {
        self.live.as_ref()
    }

    pub fn is_enabled(&self) -> bool {
        self.live.is_some()
    }

    pub fn is_saving(&self) -> bool {
        self.job.is_some()
    }

    pub fn set_keep(&mut self, keep: usize) {
        if let Some(l) = &mut self.live {
            l.set_keep(keep);
        }
    }

    /// Ask for an autosave as soon as possible (after a bake, a smart material, an import…).
    pub fn request_soon(&mut self) {
        self.soon = true;
    }

    /// A new session replaced the old one: its copies stay until the next autosave rotates
    /// them out, but the change tracking starts over.
    pub fn session_replaced(&mut self) {
        self.last_saved = None;
    }

    /// Whether a copy is due at `now` (pure scheduling, see [`Autosaver::tick`]).
    pub fn is_due(&self, session: &Session, now: Instant) -> bool {
        if self.live.is_none() || self.job.is_some() || !session.is_modified() {
            return false;
        }
        let changed = match &self.last_saved {
            Some((doc, maps)) => doc != session.history().current() || *maps != session.maps_generation(),
            None => true,
        };
        if !changed {
            return false;
        }
        let since = now.saturating_duration_since(self.last_save);
        let timer = !self.interval.is_zero() && since >= self.interval;
        let soon = self.soon && since >= MIN_GAP_AFTER_OPERATION;
        timer || soon
    }

    /// Call once per frame: keeps the session info current, starts a background copy when one
    /// is due and reports a finished one.
    pub fn tick(&mut self, session: &Session, now: Instant) -> Option<AutosaveEvent> {
        if let Some(live) = &mut self.live {
            let info = SessionInfo::for_session(session);
            if let Err(e) = live.set_info(info) {
                log::warn!("recovery info not updated: {e}");
            }
        }
        let event = self.poll();
        if self.is_due(session, now) {
            self.start(session, now);
        }
        event
    }

    /// Write a copy now on a background thread (ignores the timer).
    pub fn start(&mut self, session: &Session, now: Instant) {
        let Some(live) = &self.live else { return };
        if self.job.is_some() {
            return;
        }
        let path = live.next_autosave_path();
        // Belt and braces: copies only ever go into the session folder.
        if session.project_path().is_some_and(|p| same_file(p, &path)) || !path.starts_with(live.dir()) {
            log::error!("autosave refused to write {}", path.display());
            return;
        }
        let snapshot = session.snapshot();
        let (tx, rx) = std::sync::mpsc::channel();
        subcraft_core::task::spawn(move || {
            let _ = tx.send(snapshot.save(&path).map(|()| path));
        });
        self.job = Some(rx);
        self.last_save = now;
        self.last_saved = Some((session.history().current().clone(), session.maps_generation()));
        self.soon = false;
    }

    /// Finished background copy, if any (old copies are rotated out after a success).
    pub fn poll(&mut self) -> Option<AutosaveEvent> {
        let rx = self.job.as_ref()?;
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("autosave thread stopped".into()),
        };
        self.job = None;
        Some(match result {
            Ok(path) => {
                if let Some(l) = &self.live {
                    l.rotate();
                }
                AutosaveEvent::Saved(path)
            }
            Err(e) => {
                self.last_saved = None; // try again next time
                AutosaveEvent::Failed(e)
            }
        })
    }

    /// Block until a running copy finished (tests, shutdown).
    pub fn wait(&mut self) -> Option<AutosaveEvent> {
        let rx = self.job.take()?;
        let result = rx.recv().unwrap_or_else(|_| Err("autosave thread stopped".into()));
        Some(match result {
            Ok(path) => {
                if let Some(l) = &self.live {
                    l.rotate();
                }
                AutosaveEvent::Saved(path)
            }
            Err(e) => AutosaveEvent::Failed(e),
        })
    }

    /// Clean exit: wait for a running copy, then delete the session folder.
    pub fn finish_clean(&mut self) {
        let _ = self.wait();
        if let Some(live) = self.live.take()
            && let Err(e) = live.finish_clean()
        {
            log::warn!("recovery folder not removed: {e}");
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl Session {
    /// Open a recovery copy. The session points at the user's original project file again (if
    /// it had one) and counts as modified until it is saved.
    pub fn recover(crashed: &CrashedSession) -> Result<Self, String> {
        let mut s = Self::open_project_labeled(crashed.latest(), "Recovered Session")?;
        s.mark_recovered(crashed.info.project_path.clone());
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-recovery-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn rotation_keeps_the_newest_copies_in_number_order() {
        let d = temp_dir("rotate");
        // Number order, not name order: 0010 is newer than 0009.
        for n in [1, 2, 9, 10] {
            touch(&d, &format!("autosave-{n:04}.subcraft"));
        }
        touch(&d, "notes.txt");
        touch(&d, "autosave-x.subcraft");
        let deleted = rotate(&d, 2);
        assert_eq!(deleted.len(), 2);
        let left: Vec<String> = list_autosaves(&d).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["autosave-0009.subcraft", "autosave-0010.subcraft"]);
        assert!(d.join("notes.txt").exists(), "other files are left alone");
        assert!(rotate(&d, 2).is_empty(), "nothing more to delete");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn next_path_follows_the_newest_copy() {
        let root = temp_dir("next");
        let store = RecoveryStore::new(&root);
        let live = store.begin_session(&SessionInfo::default(), 3).unwrap();
        assert!(live.next_autosave_path().ends_with("autosave-0001.subcraft"));
        touch(live.dir(), "autosave-0041.subcraft");
        assert!(live.next_autosave_path().ends_with("autosave-0042.subcraft"));
        live.finish_clean().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_exit_leaves_nothing_and_a_crash_leaves_a_recoverable_session() {
        let root = temp_dir("marker");
        let store = RecoveryStore::new(&root);
        let info = SessionInfo { name: "Axe".into(), ..Default::default() };

        // Clean exit: the folder is gone, nothing to recover.
        let live = store.begin_session(&info, 3).unwrap();
        touch(live.dir(), "autosave-0001.subcraft");
        let dir = live.dir().to_path_buf();
        live.finish_clean().unwrap();
        assert!(!dir.exists());
        assert!(store.crashed().is_empty());

        // A running session is never reported (its lock is held).
        let live = store.begin_session(&info, 3).unwrap();
        touch(live.dir(), "autosave-0001.subcraft");
        touch(live.dir(), "autosave-0002.subcraft.saving");
        assert!(store.crashed().is_empty(), "a live session is not a crash");

        // "Crash": the process goes away without a clean exit (the lock is released).
        drop(live);
        let crashed = store.crashed();
        assert_eq!(crashed.len(), 1);
        assert_eq!(crashed[0].info.name, "Axe");
        assert!(crashed[0].latest().ends_with("autosave-0001.subcraft"));
        assert!(!crashed[0].dir.join("autosave-0002.subcraft.saving").exists(), "partial saves are removed");

        // Discard deletes it.
        crashed.into_iter().next().unwrap().discard().unwrap();
        assert!(store.crashed().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abandoned_sessions_without_copies_are_cleaned_up() {
        let root = temp_dir("empty");
        let store = RecoveryStore::new(&root);
        let live = store.begin_session(&SessionInfo::default(), 3).unwrap();
        let dir = live.dir().to_path_buf();
        drop(live);
        assert!(store.crashed().is_empty());
        assert!(!dir.exists(), "nothing to recover, folder removed");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adopting_moves_copies_into_the_new_session() {
        let root = temp_dir("adopt");
        let store = RecoveryStore::new(&root);
        let old = store.begin_session(&SessionInfo::default(), 3).unwrap();
        touch(old.dir(), "autosave-0005.subcraft");
        drop(old);
        let crashed = store.crashed().pop().unwrap();
        let old_dir = crashed.dir.clone();
        let live = store.begin_session(&SessionInfo::default(), 3).unwrap();
        live.adopt(crashed).unwrap();
        assert!(!old_dir.exists());
        assert_eq!(list_autosaves(live.dir()).len(), 1);
        assert!(store.crashed().is_empty());
        live.finish_clean().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn autosave_names_are_parsed_strictly() {
        assert_eq!(autosave_seq(Path::new("autosave-0003.subcraft")), Some(3));
        assert_eq!(autosave_seq(Path::new("autosave-0003.subcraft.saving")), None);
        assert_eq!(autosave_seq(Path::new("my-project.subcraft")), None);
    }
}
