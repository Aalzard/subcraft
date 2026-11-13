//! Send to Blender: find Blender, write the exchange folder ([`subcraft_export::blender`]),
//! launch Blender on it, and watch for the sculpt coming back as the bake high poly.
//!
//! The link is owned by the caller (the UI keeps it next to the session) and polled once a frame;
//! [`Link::poll`] only touches the disk every [`POLL_INTERVAL`] seconds. A sculpt is reported once
//! its file has stopped changing between two checks (Blender writes it under a temporary name and
//! renames it, so this is a second guard).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Session;

/// Seconds between file checks.
pub const POLL_INTERVAL: f64 = 1.0;
/// Environment variable naming the Blender executable (beats auto-detection, not the setting).
pub const EXECUTABLE_ENV: &str = "SUBCRAFT_BLENDER";

/// What the Send to Blender window asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct SendOptions {
    /// Texture export settings (template, size, padding...).
    pub settings: subcraft_export::ExportSettings,
    /// Off: mesh only, prepared for sculpting.
    pub include_textures: bool,
    /// Blender executable from Settings (empty = auto-detect).
    pub blender_path: String,
}

/// Exchange folder of a session: `<project folder>/<project>_blender` for a saved project,
/// otherwise a folder in the system temp directory named after the mesh.
pub fn exchange_dir(session: &Session) -> PathBuf {
    let name = subcraft_export::sanitize(&session.export_name());
    match session.project_path().and_then(Path::parent) {
        Some(dir) => dir.join(format!("{name}_blender")),
        None => std::env::temp_dir().join("subcraft-blender").join(name),
    }
}

/// Version numbers in a folder name ("Blender 4.2" → [4, 2]); used to prefer the newest install.
fn version_of(path: &Path) -> Vec<u32> {
    let text = path.iter().rev().filter_map(|c| c.to_str()).find(|c| c.chars().any(|ch| ch.is_ascii_digit())).unwrap_or("");
    let start = text.find(|c: char| c.is_ascii_digit()).unwrap_or(text.len());
    text[start..].split(|c: char| !c.is_ascii_digit()).take_while(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// `<root>/<entry matching prefix>/<exe>` for every matching entry of `root`.
fn versioned(root: &Path, prefix: &str, exe: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with(&prefix.to_lowercase()))
        .map(|e| e.path().join(exe))
        .collect()
}

/// The program a Windows shortcut (`.lnk`) points at when it is a Blender: the shortcut keeps
/// the target's full path as text (ANSI in its link info, UTF-16 in its strings), so the first
/// `X:\...\blender.exe` or `blender-launcher.exe` path found is it. The launcher is swapped for
/// the `blender.exe` beside it (the launcher returns at once and hides Blender's output).
pub fn shortcut_target(bytes: &[u8]) -> Option<PathBuf> {
    let ansi: String = bytes.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '\0' }).collect();
    let wide: String = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).map(|u| char::from_u32(u as u32).filter(|c| !c.is_control()).unwrap_or('\0')).collect();
    for text in [ansi, wide] {
        for run in text.split('\0') {
            // ASCII-only lowering keeps byte offsets valid for `run`.
            let lower = run.to_ascii_lowercase();
            let Some(end) = ["blender-launcher.exe", "blender.exe"].iter().find_map(|n| lower.find(n).map(|i| i + n.len())) else { continue };
            let Some(start) = lower[..end].char_indices().rev().find(|&(i, c)| c == ':' && i > 0 && lower[..i].chars().last().is_some_and(|d| d.is_ascii_alphabetic())).map(|(i, _)| i - 1) else { continue };
            let path = PathBuf::from(&run[start..end]);
            return Some(if lower[..end].ends_with("blender-launcher.exe") { path.with_file_name("blender.exe") } else { path });
        }
    }
    None
}

/// Blenders behind Start menu shortcuts named "Blender…" (installs in any folder, e.g. a zip
/// unpacked to another drive), newest version first by the shortcut's name.
fn start_menu_blenders(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<(Vec<u32>, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(Result::ok) {
            let path = e.path();
            if path.is_dir() && depth < 3 {
                walk(&path, depth + 1, out);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) && e.file_name().to_string_lossy().to_lowercase().starts_with("blender")
                && let Some(target) = std::fs::read(&path).ok().and_then(|b| shortcut_target(&b))
            {
                out.push((version_of(&path), target));
            }
        }
    }
    let mut found = Vec::new();
    for var in ["APPDATA", "ProgramData"] {
        if let Some(root) = env(var) {
            walk(&Path::new(&root).join("Microsoft").join("Windows").join("Start Menu").join("Programs"), 0, &mut found);
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, p)| p).collect()
}

/// Places Blender is usually installed, best first. Pure listing; existence is checked by
/// [`find_blender`]. `env` is passed in so tests can use a fake file system.
pub fn candidates(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let exe = format!("blender{}", std::env::consts::EXE_SUFFIX);
    if let Some(p) = env(EXECUTABLE_ENV) {
        out.push(PathBuf::from(p));
    }
    let mut installs: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(root) = env(var) {
                installs.extend(versioned(&Path::new(&root).join("Blender Foundation"), "blender", &exe));
                installs.push(Path::new(&root).join("Steam").join("steamapps").join("common").join("Blender").join(&exe));
            }
        }
        if let Some(local) = env("LOCALAPPDATA") {
            installs.extend(versioned(&Path::new(&local).join("Programs"), "blender", &exe));
            installs.extend(versioned(&Path::new(&local).join("Programs").join("Blender Foundation"), "blender", &exe));
        }
    } else if cfg!(target_os = "macos") {
        installs.push(PathBuf::from("/Applications/Blender.app/Contents/MacOS/Blender"));
    } else {
        installs.push(PathBuf::from("/snap/bin/blender"));
        installs.push(PathBuf::from("/var/lib/flatpak/exports/bin/org.blender.Blender"));
        if let Some(home) = env("HOME") {
            let home = PathBuf::from(home);
            installs.push(home.join(".local/share/flatpak/exports/bin/org.blender.Blender"));
            installs.push(home.join(".steam/steam/steamapps/common/Blender/blender"));
            installs.extend(versioned(&home, "blender-", "blender"));
            installs.extend(versioned(&home.join("Applications"), "blender", "blender"));
        }
        installs.extend(versioned(Path::new("/opt"), "blender", "blender"));
    }
    // Newest version first (folders without a number keep their order, after numbered ones).
    installs.sort_by(|a, b| version_of(b).cmp(&version_of(a)));
    if let Some(path) = env("PATH") {
        out.extend(std::env::split_paths(&path).map(|d| d.join(&exe)));
    }
    if cfg!(windows) {
        out.extend(start_menu_blenders(env));
    }
    out.extend(installs);
    out
}

/// The Blender to run: `setting` when it is set (an error if it does not exist), otherwise the
/// first existing [`candidates`] entry.
pub fn find_blender(setting: &str) -> Result<PathBuf, String> {
    let setting = setting.trim().trim_matches('"');
    if !setting.is_empty() {
        let p = PathBuf::from(setting);
        // A folder is accepted too (the Blender install folder).
        let p = if p.is_dir() { p.join(format!("blender{}", std::env::consts::EXE_SUFFIX)) } else { p };
        return if p.is_file() { Ok(p) } else { Err(format!("Blender was not found at {}. Fix the path in Settings or clear it to search automatically.", p.display())) };
    }
    candidates(&|k| std::env::var(k).ok()).into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "Blender was not found. Install Blender (blender.org) or set the path to blender.exe in the Send to Blender window.".to_string()
    })
}

/// Start Blender on the exchange folder; its console output goes to `blender_log.txt` there.
pub fn launch(blender: &Path, files: &subcraft_export::blender::BridgeFiles) -> Result<std::process::Child, String> {
    let dir = files.manifest.parent().unwrap_or(Path::new("."));
    let log = std::fs::File::create(dir.join("blender_log.txt")).map_err(|e| format!("cannot write the Blender log in {}: {e}", dir.display()))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(blender);
    cmd.arg("--python").arg(&files.script).arg("--").arg(&files.manifest).current_dir(dir).stdout(log).stderr(log2).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // blender.exe is a console program: without this an empty console window opens next to it.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| format!("cannot start {}: {e}", blender.display()))
}

/// Write the exchange folder for `session` into `dir` and open it in Blender.
pub fn send(session: &mut Session, dir: &Path, options: &SendOptions) -> Result<Link, String> {
    let blender = find_blender(&options.blender_path)?;
    let files = write(session, dir, options)?;
    launch(&blender, &files)?;
    log::info!("blender: sent {} to {} ({} texture(s))", files.mesh.display(), blender.display(), files.textures.len());
    Ok(Link::new(files, blender))
}

/// The exchange files alone (no Blender needed): for `send` and for tests.
pub fn write(session: &mut Session, dir: &Path, options: &SendOptions) -> Result<subcraft_export::blender::BridgeFiles, String> {
    if options.include_textures {
        session.update_composites();
    }
    let mut settings = session.effective_export_settings(&options.settings);
    // The export window's unchecked maps belong to its own template, not to this one.
    settings.selection = Default::default();
    let name = session.export_name();
    let sets = session.export_sets();
    subcraft_export::blender::write_bridge(&session.scene().mesh, &name, &sets, &settings, options.include_textures, dir).map_err(|e| e.to_string())
}

type Stamp = (SystemTime, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// A running Blender round trip: knows where the sculpt comes back.
pub struct Link {
    files: subcraft_export::blender::BridgeFiles,
    blender: PathBuf,
    applied: Option<Stamp>,
    pending: Option<Stamp>,
    last_poll: f64,
}

impl Link {
    /// A link that ignores a sculpt already in the folder (from an earlier round trip).
    pub fn new(files: subcraft_export::blender::BridgeFiles, blender: PathBuf) -> Self {
        let applied = stamp(&files.return_file);
        Self { files, blender, applied, pending: None, last_poll: f64::NEG_INFINITY }
    }

    pub fn files(&self) -> &subcraft_export::blender::BridgeFiles {
        &self.files
    }

    pub fn blender(&self) -> &Path {
        &self.blender
    }

    pub fn return_file(&self) -> &Path {
        &self.files.return_file
    }

    /// The sculpt file once a new one has settled (checks at most every [`POLL_INTERVAL`] s).
    pub fn poll(&mut self, now: f64) -> Option<PathBuf> {
        if now - self.last_poll < POLL_INTERVAL {
            return None;
        }
        self.last_poll = now;
        self.check()
    }

    /// One check, regardless of time.
    pub fn check(&mut self) -> Option<PathBuf> {
        let current = stamp(&self.files.return_file);
        if current.is_none() || current == self.applied {
            self.pending = None;
            return None;
        }
        if self.pending != current {
            self.pending = current;
            return None;
        }
        self.pending = None;
        self.applied = current;
        Some(self.files.return_file.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_blender_install_comes_first_and_the_env_override_before_it() {
        let root = std::env::temp_dir().join(format!("subcraft-blender-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let exe = format!("blender{}", std::env::consts::EXE_SUFFIX);
        let make = |rel: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"").unwrap();
            p
        };
        let (old, new) = if cfg!(windows) {
            (make(&format!("pf/Blender Foundation/Blender 3.6/{exe}")), make(&format!("pf/Blender Foundation/Blender 4.10/{exe}")))
        } else {
            (make("home/blender-3.6.0-linux-x64/blender"), make("home/blender-4.10.1-linux-x64/blender"))
        };
        let env = |k: &str| -> Option<String> {
            match k {
                "ProgramFiles" => Some(root.join("pf").display().to_string()),
                "HOME" => Some(root.join("home").display().to_string()),
                _ => None,
            }
        };
        let list: Vec<PathBuf> = candidates(&env).into_iter().filter(|p| p.is_file()).collect();
        assert_eq!(list, vec![new.clone(), old.clone()], "4.10 sorts above 3.6 (numerically, not as text)");
        let forced = make("custom/blender-dev");
        let env2 = |k: &str| if k == EXECUTABLE_ENV { Some(forced.display().to_string()) } else { env(k) };
        assert_eq!(candidates(&env2).into_iter().find(|p| p.is_file()), Some(forced.clone()));
        // The setting wins and accepts the install folder; a wrong setting is a clear error.
        assert_eq!(find_blender(&forced.display().to_string()), Ok(forced.clone()));
        assert_eq!(find_blender(&format!("\"{}\"", new.parent().unwrap().display())), Ok(new.clone()));
        let err = find_blender(&root.join("nope.exe").display().to_string()).unwrap_err();
        assert!(err.contains("not found") && err.contains("nope.exe"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_start_menu_shortcut_leads_to_blender_beside_its_launcher() {
        // The bytes a .lnk keeps: binary header, ANSI local path, UTF-16 strings.
        let mut lnk = vec![0x4c, 0, 0, 0, 1, 0x14, 2, 0];
        lnk.extend_from_slice(b"\x00\x10junk\x00E:\\Applications\\Blender\\blender-launcher.exe\x00\x00");
        assert_eq!(shortcut_target(&lnk), Some(PathBuf::from(r"E:\Applications\Blender\blender.exe")));
        let wide: Vec<u8> = "\0\0D:\\Tools\\Blender 4.2\\blender.exe\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(shortcut_target(&wide), Some(PathBuf::from(r"D:\Tools\Blender 4.2\blender.exe")));
        assert_eq!(shortcut_target(br"C:\Program Files\Krita\krita.exe"), None);
    }

    #[test]
    fn link_reports_a_new_sculpt_once_it_has_settled() {
        let dir = std::env::temp_dir().join(format!("subcraft-blender-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("from_blender")).unwrap();
        let ret = dir.join("from_blender").join("sculpt_high.obj");
        std::fs::write(&ret, b"old sculpt").unwrap();
        let files = subcraft_export::blender::BridgeFiles { mesh: dir.join("m.obj"), script: dir.join("s.py"), manifest: dir.join("bridge.json"), textures: Vec::new(), return_file: ret.clone() };
        let mut link = Link::new(files, PathBuf::from("blender"));
        assert_eq!(link.check(), None, "a sculpt from an earlier session is not picked up");
        std::fs::write(&ret, b"new sculpt, longer").unwrap();
        assert_eq!(link.check(), None, "first sight: wait for the file to settle");
        assert_eq!(link.check(), Some(ret.clone()));
        assert_eq!(link.check(), None, "reported once");
        assert_eq!(link.poll(10.0), None);
        std::fs::write(&ret, b"sculpt sent again with more detail").unwrap();
        assert_eq!(link.poll(10.5), None, "polls are throttled");
        assert_eq!(link.poll(11.0), None);
        assert_eq!(link.poll(12.0), Some(ret.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exchange_folder_sits_next_to_a_saved_project() {
        let session = Session::new(subcraft_mesh::primitives::cube(1.0), 64);
        let unsaved = exchange_dir(&session);
        assert!(unsaved.starts_with(std::env::temp_dir()), "{}", unsaved.display());
    }

    #[test]
    fn write_produces_the_exchange_folder_from_a_session() {
        let mut session = Session::new(subcraft_mesh::primitives::cube(1.0), 64);
        let dir = std::env::temp_dir().join(format!("subcraft-blender-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let options = SendOptions { settings: subcraft_export::ExportSettings { template: subcraft_export::blender::template(), size: Some(64), ..Default::default() }, include_textures: true, blender_path: String::new() };
        let files = write(&mut session, &dir, &options).unwrap();
        assert!(files.mesh.is_file() && files.manifest.is_file() && files.script.is_file());
        assert!(!files.textures.is_empty(), "the default document has base colour, roughness, metallic, normal");
        assert!(files.textures.iter().any(|t| t.to_string_lossy().contains("BaseColor")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
