//! Version history: each Save over an existing project file first keeps the file's previous
//! state in a folder next to it (`<name>.versions/<name>-<unix seconds>.subcraft`), up to a set
//! number of versions. File > Version history lists them; restoring one opens it in place of the
//! project as unsaved changes, so the next Save writes it back (and keeps what it replaced as a
//! version too).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::session::Session;

/// Versions kept per project unless the user picks another number.
pub const DEFAULT_KEEP: usize = 10;

/// One kept version of a project file.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub path: PathBuf,
    /// When this version was saved (the time is in the file name, so copying keeps it).
    pub saved: SystemTime,
    pub bytes: u64,
}

/// The folder versions of `project` go to.
pub fn folder(project: &Path) -> PathBuf {
    let stem = project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    project.with_file_name(format!("{stem}.versions"))
}

fn stem(project: &Path) -> String {
    project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into())
}

/// Seconds since 1970 and the same-second counter (1 when absent) in a version file name
/// (`<stem>-<secs>[-<n>].subcraft`), for files of `project`.
fn stamp_of(project: &Path, file: &Path) -> Option<(u64, u32)> {
    let name = file.file_stem()?.to_str()?;
    let rest = name.strip_prefix(&format!("{}-", stem(project)))?;
    let mut parts = rest.split('-');
    let secs = parts.next()?.parse().ok()?;
    let n = match parts.next() {
        Some(n) => n.parse().ok()?,
        None => 1,
    };
    parts.next().is_none().then_some((secs, n))
}

/// The versions of `project`, newest first. A missing folder has none.
pub fn list(project: &Path) -> Vec<Version> {
    let ext = project.extension().and_then(|e| e.to_str()).unwrap_or(crate::PROJECT_EXTENSION).to_string();
    let mut out: Vec<((u64, u32), Version)> = std::fs::read_dir(folder(project))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some(ext.as_str()) {
                return None;
            }
            let (secs, n) = stamp_of(project, &path)?;
            let bytes = e.metadata().ok()?.len();
            Some(((secs, n), Version { saved: UNIX_EPOCH + Duration::from_secs(secs), bytes, path }))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, v)| v).collect()
}

/// Keep the current `project` file as a version, stamped with the time it was saved (its
/// modified time, else `now`), then drop the oldest beyond `keep`. Nothing happens when the file
/// does not exist yet or `keep` is 0.
pub fn snapshot(project: &Path, keep: usize, now: SystemTime) -> Result<Option<PathBuf>, String> {
    if keep == 0 || !project.is_file() {
        return Ok(None);
    }
    let dir = folder(project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stamp = std::fs::metadata(project).and_then(|m| m.modified()).unwrap_or(now);
    let secs = stamp.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let ext = project.extension().and_then(|e| e.to_str()).unwrap_or(crate::PROJECT_EXTENSION);
    let mut target = dir.join(format!("{}-{secs}.{ext}", stem(project)));
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = dir.join(format!("{}-{secs}-{n}.{ext}", stem(project)));
    }
    std::fs::copy(project, &target).map_err(|e| format!("cannot keep a version in {}: {e}", dir.display()))?;
    prune(project, keep)?;
    Ok(Some(target))
}

/// Delete the oldest versions of `project` beyond `keep`.
pub fn prune(project: &Path, keep: usize) -> Result<(), String> {
    for v in list(project).into_iter().skip(keep) {
        std::fs::remove_file(&v.path).map_err(|e| format!("cannot delete {}: {e}", v.path.display()))?;
    }
    Ok(())
}

/// "3 min ago", "2 h ago", "5 days ago" (or a UTC date for older versions) for a version list.
pub fn describe_age(saved: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(saved).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..604_800 => format!("{} days ago", secs / 86_400),
        _ => utc_date(saved),
    }
}

/// `YYYY-MM-DD HH:MM UTC`.
pub fn utc_date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

impl Session {
    /// Open version `version` of the project at `project` in its place: the project path stays
    /// `project` and the session counts as unsaved, so Save writes the version back.
    pub fn open_version(project: &Path, version: &Path) -> Result<Session, String> {
        let mut s = Session::open_project_labeled(version, "Restore Version")?;
        s.mark_recovered(Some(project.to_path_buf()));
        Ok(s)
    }
}
