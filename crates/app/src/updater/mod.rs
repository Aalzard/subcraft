//! Desktop release checking, verified downloads and a separate update/restart helper.
//! No UI/GPU dependencies. Web builds have no native updater or HTTP dependencies.
mod apply;
#[cfg(test)]
mod tests;
pub use apply::{apply_job, resume_session, start_helper, UpdateJob};
use std::{path::{Path, PathBuf}, io::{Read, Write}, sync::mpsc::{self, Receiver}, time::Duration};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RELEASE_API: &str = "https://api.github.com/repos/Aalzard/subcraft/releases/latest";
pub const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageKind { WindowsInstaller { admin: bool }, WindowsPortable, LinuxPortable }
impl PackageKind {
    pub fn name(self, version: &str) -> String {
        match self { Self::WindowsInstaller { .. } => format!("SubCraft-{version}-setup.exe"), Self::WindowsPortable => format!("SubCraft-{version}-windows-x64.zip"), Self::LinuxPortable => format!("SubCraft-{version}-linux-x86_64.tar.gz") }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release { pub version: String, pub notes: String, pub kind: PackageKind, pub asset: Asset }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset { pub name: String, pub browser_download_url: String, pub size: u64, pub digest: String }
#[derive(Deserialize)]
struct ApiRelease { tag_name: String, #[serde(default)] draft: bool, #[serde(default)] prerelease: bool, body: Option<String>, assets: Vec<ApiAsset> }
#[derive(Deserialize)]
struct ApiAsset { name: String, browser_download_url: String, size: u64, digest: Option<String> }

pub fn parse_release(json: &str, current: &str, kind: PackageKind) -> Result<Option<Release>, String> {
    let api: ApiRelease = serde_json::from_str(json).map_err(|_| "GitHub returned an invalid release response")?;
    if api.draft || api.prerelease { return Ok(None); }
    let version = api.tag_name.strip_prefix('v').unwrap_or(&api.tag_name);
    let new = semver::Version::parse(version).map_err(|_| "Release version is invalid")?;
    let old = semver::Version::parse(current).map_err(|_| "Application version is invalid")?;
    if !new.pre.is_empty() || new.cmp_precedence(&old).is_le() { return Ok(None); }
    let name = kind.name(version);
    let asset = api.assets.into_iter().find(|a| a.name == name).ok_or("This release's packages are still being uploaded. Check again shortly.")?;
    let asset = Asset { name: asset.name, browser_download_url: asset.browser_download_url, size: asset.size, digest: asset.digest.ok_or("The release package has no SHA-256 digest yet. Check again shortly.")? };
    let release = Release { version: version.to_owned(), notes: api.body.unwrap_or_default(), kind, asset };
    release.validate()?; Ok(Some(release))
}
impl Release {
    pub fn validate(&self) -> Result<(), String> {
        let v = semver::Version::parse(&self.version).map_err(|_| "Invalid update version")?;
        if !v.pre.is_empty() || self.asset.name != self.kind.name(&self.version) || self.asset.size == 0 || self.asset.size > MAX_PACKAGE { return Err("Invalid update package".into()); }
        let expected = format!("https://github.com/Aalzard/subcraft/releases/download/v{}/{}", self.version, self.asset.name);
        if self.asset.browser_download_url != expected { return Err("Update package is outside the SubCraft release repository".into()); }
        digest_hex(&self.asset.digest)?; Ok(())
    }
}
pub fn digest_hex(digest: &str) -> Result<&str, String> {
    let value = digest.strip_prefix("sha256:").ok_or("Missing SHA-256 digest")?;
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Invalid SHA-256 digest".into()); } Ok(value)
}
pub fn verify_file(path: &Path, asset: &Asset) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() != asset.size { return Err("Update download size does not match the release".into()); }
    let mut hash = Sha256::new(); let mut buf = [0; 64 * 1024];
    loop { let n = file.read(&mut buf).map_err(|e| e.to_string())?; if n == 0 { break; } hash.update(&buf[..n]); }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(digest_hex(&asset.digest)?) { return Err("Update download failed SHA-256 verification".into()); } Ok(())
}
#[derive(Clone, Debug)]
pub enum Phase { Idle, Checking, Current, Available(Release), Downloading { bytes: u64, total: u64 }, Prepared { release: Release, package: PathBuf }, Preparing, Exiting, Error(String) }
pub struct Updater { pub phase: Phase, pub open: bool, pub target: PathBuf, pub kind: PackageKind, pub auto_install: bool, worker: Option<Receiver<Phase>> }
impl Updater {
    pub fn new() -> Result<Self, String> {
        let target = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())?;
        let kind = detect_kind(&target)?;
        if let Ok(root) = cache_dir() { compact_finished_cache(&root); }
        Ok(Self { phase: Phase::Idle, open: false, target, kind, auto_install: false, worker: None })
    }
    pub fn busy(&self) -> bool { self.worker.is_some() }
    pub fn poll(&mut self) { if let Some(rx) = &self.worker {
        loop { match rx.try_recv() { Ok(p) => self.phase = p, Err(mpsc::TryRecvError::Empty) => break, Err(mpsc::TryRecvError::Disconnected) => {
            if !matches!(self.phase, Phase::Current | Phase::Available(_) | Phase::Prepared { .. } | Phase::Exiting | Phase::Error(_)) { self.phase = Phase::Error("Update worker stopped unexpectedly; try again".into()); } break;
        } } }
        if matches!(self.phase, Phase::Current | Phase::Available(_) | Phase::Prepared { .. } | Phase::Exiting | Phase::Error(_)) { self.worker = None; }
    } }
    pub fn request_update(&mut self) { self.check(); self.auto_install = true; }
    pub fn check(&mut self) {
        if self.busy() { return; } self.auto_install = false; self.open = true; self.phase = Phase::Checking;
        let (tx, rx) = mpsc::channel(); self.worker = Some(rx); let kind = self.kind;
        std::thread::spawn(move || { let result = check_latest(env!("CARGO_PKG_VERSION"), kind).map(|r| r.map_or(Phase::Current, Phase::Available)).unwrap_or_else(Phase::Error); let _ = tx.send(result); });
    }
    pub fn download(&mut self, release: Release) {
        if self.busy() { return; }
        self.phase = Phase::Downloading { bytes: 0, total: release.asset.size };
        let (tx, rx) = mpsc::channel(); self.worker = Some(rx);
        std::thread::spawn(move || {
            let result = (|| { let dir = cache_dir()?.join(uuid::Uuid::new_v4().to_string()); std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?; }
                let path = dir.join(&release.asset.name); download_release(&release, &path, |bytes| { let _ = tx.send(Phase::Downloading { bytes, total: release.asset.size }); })?;
                Ok(Phase::Prepared { release, package: path }) })().unwrap_or_else(Phase::Error); let _ = tx.send(result);
        });
    }
    pub fn install(&mut self, session: &mut crate::Session) {
        let Phase::Prepared { release, package } = self.phase.clone() else { return };
        session.end_stroke();
        let snapshot = session.snapshot(); let original = session.project_path().map(Path::to_path_buf);
        let modified = session.is_modified(); let selection = session.selection(); let target = self.target.clone();
        self.phase = Phase::Preparing; let (tx, rx) = mpsc::channel(); self.worker = Some(rx);
        std::thread::spawn(move || { let result = start_helper(snapshot, original, modified, selection, target, release, package).map(|_| Phase::Exiting).unwrap_or_else(Phase::Error); let _ = tx.send(result); });
    }
}
fn agent(seconds: u64) -> ureq::Agent { ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(seconds))).https_only(fixture_base().is_none()).build().into() }
fn fixture_base() -> Option<String> {
    #[cfg(feature = "update-fixture")] {
        let value = std::env::var("SUBCRAFT_UPDATE_FIXTURE_URL").ok()?;
        let port = value.strip_prefix("http://127.0.0.1:")?;
        if !port.is_empty() && port.bytes().all(|c| c.is_ascii_digit()) && port.parse::<u16>().is_ok() { return Some(value); }
    } None
}
fn transport_url(url: &str) -> String {
    if let Some(base) = fixture_base() { return if url.starts_with("https://api.github.com/") { format!("{base}/latest.json") } else { format!("{base}/{}", url.rsplit('/').next().unwrap_or_default()) }; } url.to_owned()
}
pub fn check_latest(current: &str, kind: PackageKind) -> Result<Option<Release>, String> {
    check_release_api(RELEASE_API, current, kind)
}
fn check_release_api(url: &str, current: &str, kind: PackageKind) -> Result<Option<Release>, String> {
    let mut response = agent(20).get(&transport_url(url)).header("User-Agent", "SubCraft-updater").header("Accept", "application/vnd.github+json").call().map_err(|e| format!("Could not check GitHub releases: {e}"))?;
    let mut text = String::new(); response.body_mut().as_reader().take(1024 * 1024 + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() > 1024 * 1024 { return Err("Release response is too large".into()); } parse_release(&text, current, kind)
}
pub fn verify_release_authority(release: &Release) -> Result<(), String> {
    let url = format!("https://api.github.com/repos/Aalzard/subcraft/releases/tags/v{}", release.version);
    let official = check_release_api(&url, "0.0.0", release.kind)?.ok_or("Release is unavailable")?;
    if official.asset.digest != release.asset.digest || official.asset.size != release.asset.size || official.asset.name != release.asset.name { return Err("The release package changed; check and download it again".into()); } Ok(())
}
pub fn download_release(release: &Release, path: &Path, mut progress: impl FnMut(u64)) -> Result<(), String> {
    release.validate()?;
    let mut response = agent(300).get(&transport_url(&release.asset.browser_download_url)).header("User-Agent", "SubCraft-updater").header("Accept-Encoding", "identity").call().map_err(|e| format!("Could not download update: {e}"))?;
    copy_verified(response.body_mut().as_reader(), path, &release.asset, &mut progress)
}
pub fn copy_verified(mut reader: impl Read, path: &Path, asset: &Asset, mut progress: impl FnMut(u64)) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
    let result = (|| {
    let mut total = 0; let mut buf = [0; 64 * 1024];
    loop { let n = reader.read(&mut buf).map_err(|e| e.to_string())?; if n == 0 { break; } total += n as u64;
        if total > asset.size || total > MAX_PACKAGE { return Err("Update download exceeds its declared size".into()); } file.write_all(&buf[..n]).map_err(|e| e.to_string())?; progress(total);
    } file.sync_all().map_err(|e| e.to_string())?; drop(file); verify_file(path, asset)
    })(); if result.is_err() { let _ = std::fs::remove_file(path); } result
}
pub fn cache_dir() -> Result<PathBuf, String> { crate::recovery::data_dir().map(|p| p.join("updates")).ok_or("No writable update cache folder".into()) }
/// Keep project checkpoints, but discard the large payloads of completed jobs.
pub fn compact_finished_cache(root: &Path) {
    let Ok(root) = root.canonicalize() else { return };
    let Ok(entries) = std::fs::read_dir(&root) else { return };
    for entry in entries.flatten() {
        if entry.file_type().map_or(true, |t| !t.is_dir() || t.is_symlink()) { continue; }
        let Ok(dir) = entry.path().canonicalize() else { continue };
        if dir.parent() != Some(root.as_path()) { continue; }
        let path = dir.join("job.json"); let Ok(job) = UpdateJob::read(&path) else { continue };
        let Ok(result) = std::fs::read_to_string(dir.join("result.txt")) else { continue };
        if result != "Update installed" && !result.starts_with("Update failed:") { continue; }
        for name in [job.release.asset.name.as_str(), "new-app", "SubCraftUpdater.exe", "SubCraftUpdater"] {
            let file = dir.join(name);
            if file.canonicalize().ok().is_some_and(|p| p.parent() == Some(dir.as_path())) { let _ = std::fs::remove_file(file); }
        }
    }
}
pub fn detect_kind(target: &Path) -> Result<PackageKind, String> {
    if !cfg!(target_arch = "x86_64") { return Err("Automatic updates currently require an x64 desktop build".into()); }
    #[cfg(windows)] {
        use winreg::{RegKey, enums::*};
        let app_id = "2E7545F0-1CE8-4FCD-B26D-AC623D9B17A6".to_owned();
        #[cfg(feature = "update-fixture")]
        let app_id = std::env::var("SUBCRAFT_UPDATE_FIXTURE_APP_ID").ok().and_then(|value| uuid::Uuid::parse_str(&value).ok()).map(|id| id.to_string().to_uppercase()).unwrap_or(app_id);
        for (hive, admin) in [(HKEY_CURRENT_USER, false), (HKEY_LOCAL_MACHINE, true)] {
            if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{{{app_id}}}_is1"), KEY_READ | KEY_WOW64_64KEY)
                && let Ok(dir) = key.get_value::<String, _>("InstallLocation")
                && let Ok(dir) = Path::new(&dir).canonicalize() && target.parent() == Some(dir.as_path())
                && target.file_name().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("SubCraft.exe")) { return Ok(PackageKind::WindowsInstaller { admin }); }
        } Ok(PackageKind::WindowsPortable)
    }
    #[cfg(target_os = "linux")] { let _ = target; Ok(PackageKind::LinuxPortable) }
    #[cfg(not(any(windows, target_os = "linux")))] { let _ = target; Err("Automatic updates are available on Windows and Linux".into()) }
}
