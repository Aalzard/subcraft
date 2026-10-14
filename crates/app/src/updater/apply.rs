use super::*;
use crate::{ProjectSnapshot, Session, Command};
use subcraft_core::{TextureSetId, LayerId};
use std::{fs, process::Command as Process, time::Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateJob {
    pub schema: u32, pub id: uuid::Uuid, pub parent_pid: u32, pub target: PathBuf,
    pub old_digest: String, pub release: Release, pub package: PathBuf,
    pub original_project: Option<PathBuf>, pub modified: bool,
    pub texture_set: TextureSetId, pub layer: Option<LayerId>,
}
impl UpdateJob {
    pub fn read(path: &Path) -> Result<Self, String> {
        let mut data = Vec::new(); fs::File::open(path).map_err(|e| e.to_string())?.take(1024 * 1024 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > 1024 * 1024 { return Err("Update job is too large".into()); }
        let job: Self = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        if job.schema != 1 || path.file_name().and_then(|s| s.to_str()) != Some("job.json") { return Err("Invalid update job".into()); }
        let dir = path.parent().ok_or("Missing update folder")?.canonicalize().map_err(|e| e.to_string())?;
        if dir.file_name().and_then(|s| s.to_str()) != Some(job.id.to_string().as_str()) || job.package.parent().and_then(|p| p.canonicalize().ok()) != Some(dir) { return Err("Update paths do not match their job folder".into()); }
        job.release.validate()?;
        if job.package.file_name().and_then(|s| s.to_str()) != Some(job.release.asset.name.as_str()) { return Err("Update package name does not match the release".into()); }
        Ok(job)
    }
}
fn hash(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?; let mut hash = Sha256::new(); let mut buf = [0; 65536];
    loop { let n = f.read(&mut buf).map_err(|e| e.to_string())?; if n == 0 { break; } hash.update(&buf[..n]); } Ok(format!("{:x}", hash.finalize()))
}
/// Extract only the application executable. Never write archive paths into the user's folder.
pub fn extract_binary(release: &Release, package: &Path, output: &Path) -> Result<(), String> {
    verify_file(package, &release.asset)?;
    match release.kind {
        PackageKind::WindowsInstaller { .. } => { return Ok(()); }
        PackageKind::WindowsPortable => {
            let mut zip = zip::ZipArchive::new(fs::File::open(package).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let expected = format!("SubCraft-{}-windows-x64/SubCraft.exe", release.version);
            let entry = zip.by_name(&expected).map_err(|_| "Update archive is missing SubCraft.exe")?;
            if entry.is_symlink() || entry.size() > MAX_PACKAGE || entry.size() < 64 { return Err("Invalid executable in update archive".into()); }
            let mut dest = fs::File::create(output).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry.take(MAX_PACKAGE + 1), &mut dest).map_err(|e| e.to_string())?; dest.sync_all().map_err(|e| e.to_string())?;
        }
        PackageKind::LinuxPortable => {
            #[cfg(target_os = "linux")] {
                let decoder = flate2::read::GzDecoder::new(fs::File::open(package).map_err(|e| e.to_string())?);
                let mut tar = tar::Archive::new(decoder); let expected = PathBuf::from(format!("SubCraft-{}-linux-x86_64/subcraft", release.version)); let mut found = false;
                for entry in tar.entries().map_err(|e| e.to_string())? { let entry = entry.map_err(|e| e.to_string())?;
                    if entry.path().map_err(|e| e.to_string())?.as_ref() != expected { continue; }
                    if !entry.header().entry_type().is_file() || entry.size() > MAX_PACKAGE || entry.size() < 64 { return Err("Invalid Linux executable".into()); }
                    let mut dest = fs::File::create(output).map_err(|e| e.to_string())?;
                    std::io::copy(&mut entry.take(MAX_PACKAGE + 1), &mut dest).map_err(|e| e.to_string())?; dest.sync_all().map_err(|e| e.to_string())?; found = true; break;
                } if !found { return Err("Update archive is missing subcraft".into()); }
            }
            #[cfg(not(target_os = "linux"))] { return Err("Linux updates require Linux".into()); }
        }
    }
    let mut magic = [0; 4]; fs::File::open(output).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    let valid = match release.kind { PackageKind::WindowsPortable => magic[..2] == *b"MZ", _ => magic == *b"\x7fELF" };
    if !valid { return Err("Update executable has an invalid header".into()); } Ok(())
}
pub fn start_helper(snapshot: ProjectSnapshot, original_project: Option<PathBuf>, modified: bool, selection: crate::Selection,
    target: PathBuf, release: Release, package: PathBuf) -> Result<PathBuf, String> {
    let dir = package.parent().ok_or("Missing update directory")?.canonicalize().map_err(|e| e.to_string())?;
    let id = dir.file_name().and_then(|s| s.to_str()).and_then(|s| uuid::Uuid::parse_str(s).ok()).ok_or("Invalid update directory")?;
    let package = package.canonicalize().map_err(|e| e.to_string())?;
    extract_binary(&release, &package, &dir.join("new-app"))?;
    snapshot.save(&dir.join("workspace.subcraft"))?;
    let helper = dir.join(if cfg!(windows) { "SubCraftUpdater.exe" } else { "SubCraftUpdater" });
    fs::copy(&target, &helper).map_err(|e| format!("Could not prepare update helper: {e}"))?;
    let job = UpdateJob { schema: 1, id, parent_pid: std::process::id(), old_digest: hash(&target)?, target, release, package, original_project, modified, texture_set: selection.texture_set, layer: selection.layer };
    let job_path = dir.join("job.json"); fs::write(&job_path, serde_json::to_vec(&job).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut process = Process::new(helper); process.arg("--apply-update").arg(&job_path);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; process.creation_flags(0x08000000); }
    let mut child = process.spawn().map_err(|e| format!("Could not start update helper: {e}"))?;
    let start = Instant::now();
    loop {
        if dir.join("ready.txt").exists() { return Ok(job_path); }
        if let Ok(error) = fs::read_to_string(dir.join("helper-error.txt")) { return Err(error); }
        if let Ok(Some(status)) = child.try_wait() { return Err(format!("Update helper exited before preparation: {status}")); }
        if start.elapsed() > Duration::from_secs(35) { return Err("Update helper did not become ready; the app will stay open".into()); }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub fn resume_session(path: &Path) -> Result<Session, String> {
    let job = UpdateJob::read(path)?;
    let mut session = Session::open_project(&path.parent().ok_or("Missing update folder")?.join("workspace.subcraft"))?;
    session.restore_after_update(job.original_project, job.modified);
    let _ = session.execute(Command::SelectTextureSet(job.texture_set));
    let _ = session.execute(Command::SelectLayer(job.layer)); Ok(session)
}
fn wait_parent(pid: u32) -> Result<(), String> {
    let mut system = sysinfo::System::new(); let pid = sysinfo::Pid::from_u32(pid); let start = Instant::now();
    loop { system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[pid]), true, sysinfo::ProcessRefreshKind::nothing());
        if system.process(pid).is_none() { return Ok(()); }
        if start.elapsed() > Duration::from_secs(90) { return Err("The app did not close; update was not installed".into()); } std::thread::sleep(Duration::from_millis(250));
    }
}
/// Atomic executable replacement with rollback; neighboring user files are never removed.
pub fn replace_portable(target: &Path, staged: &Path, id: uuid::Uuid) -> Result<(), String> {
    let parent = target.parent().ok_or("Missing application folder")?.canonicalize().map_err(|e| e.to_string())?;
    if target.canonicalize().map_err(|e| e.to_string())?.parent() != Some(parent.as_path()) { return Err("Target resolves outside the application folder".into()); }
    let backup = parent.join(format!(".subcraft-previous-{id}")); let replacement = parent.join(format!(".subcraft-new-{id}"));
    if backup.exists() { return Err("A previous update backup already exists".into()); }
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&replacement).map_err(|e| format!("Application folder is not writable: {e}"))?;
    let result = (|| { std::io::copy(&mut fs::File::open(staged).map_err(|e| e.to_string())?, &mut file).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?; drop(file);
        fs::set_permissions(&replacement, fs::metadata(target).map_err(|e| e.to_string())?.permissions()).map_err(|e| e.to_string())?;
        fs::rename(target, &backup).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&replacement, target) { let _ = fs::rename(&backup, target); return Err(e.to_string()); }
        let _ = fs::remove_file(&backup); Ok(()) })();
    if result.is_err() { let _ = fs::remove_file(&replacement); } result
}
#[cfg(windows)]
pub(super) fn installer_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") { format!("\\\\{unc}") }
    else { value.strip_prefix("\\\\?\\").unwrap_or(&value).to_owned() }
}
#[cfg(windows)]
fn install_windows(job: &UpdateJob, log: &Path) -> Result<(), String> {
    let PackageKind::WindowsInstaller { admin } = job.release.kind else { return Err("Invalid installer mode".into()); };
    let dir = job.target.parent().ok_or("Missing install folder")?;
    let args = vec!["/SP-".to_owned(), "/VERYSILENT".into(), "/SUPPRESSMSGBOXES".into(), "/NORESTART".into(), "/NOCLOSEAPPLICATIONS".into(), "/NORESTARTAPPLICATIONS".into(),
        if admin { "/ALLUSERS".into() } else { "/CURRENTUSER".into() }, format!("/DIR={}", installer_path(dir)), format!("/LOG={}", installer_path(log))];
    use std::os::windows::process::CommandExt;
    let code = if admin {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        // Array items retain double quotes around paths when Start-Process joins ArgumentList.
        let args = args.iter().map(|a| quote(&format!("\"{a}\""))).collect::<Vec<_>>().join(",");
        let script = format!("$ErrorActionPreference='Stop'; try {{ $p=Start-Process -FilePath {} -ArgumentList @({args}) -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1 }}", quote(&installer_path(&job.package)));
        Process::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]).creation_flags(0x08000000).status().map_err(|e| e.to_string())?
    } else { Process::new(&job.package).args(args).creation_flags(0x08000000).status().map_err(|e| e.to_string())? };
    if !code.success() { return Err("Windows update installation failed or permission was cancelled; your project snapshot is retained".into()); } Ok(())
}
pub fn apply_job(path: &Path) -> Result<(), String> {
    let job = UpdateJob::read(path)?;
    let preflight: Result<(), String> = (|| {
    if hash(&std::env::current_exe().map_err(|e| e.to_string())?)? != job.old_digest || hash(&job.target)? != job.old_digest { return Err("Update helper does not match the application it is replacing".into()); }
    if detect_kind(&job.target)? != job.release.kind { return Err("Installation type changed; update was cancelled".into()); }
    verify_file(&job.package, &job.release.asset)?;
    verify_release_authority(&job.release)?; Ok(())
    })();
    if let Err(ref error) = preflight { let _ = fs::write(path.with_file_name("helper-error.txt"), error); }
    preflight?;
    fs::write(path.with_file_name("ready.txt"), "ready").map_err(|e| e.to_string())?;
    wait_parent(job.parent_pid)?;
    let result = verify_file(&job.package, &job.release.asset).and_then(|()| match job.release.kind {
        PackageKind::WindowsInstaller { .. } => {
            #[cfg(windows)] { install_windows(&job, &path.with_file_name("install.log")) }
            #[cfg(not(windows))] { Err("Windows installer requires Windows".into()) }
        }
        _ => extract_binary(&job.release, &job.package, &path.with_file_name("new-app")).and_then(|()| replace_portable(&job.target, &path.with_file_name("new-app"), job.id)),
    });
    fs::write(path.with_file_name("result.txt"), result.as_ref().map(|_| "Update installed".to_owned()).unwrap_or_else(|e| format!("Update failed: {e}"))).map_err(|e| e.to_string())?;
    let mut process = Process::new(&job.target); process.arg("--resume-update").arg(path);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; process.creation_flags(0x08000000); }
    process.spawn().map_err(|e| format!("Could not restart app: {e}. Reopen {} to restore your project.", path.with_file_name("workspace.subcraft").display()))?; result
}
