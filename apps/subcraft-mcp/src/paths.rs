//! File paths as the agent sees them. Claude Code on Windows passes Windows paths
//! (`D:\models\crate.fbx`) while the server may run inside WSL, where that file is
//! `/mnt/d/models/crate.fbx`. Inputs are mapped to the local form, outputs back to the form the
//! agent can open.

use std::path::{Path, PathBuf};

/// True when running inside WSL (Linux with access to the Windows drives under `/mnt`).
fn in_wsl() -> bool {
    static WSL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WSL.get_or_init(|| {
        cfg!(target_os = "linux")
            && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists())
    })
}

/// A path from the agent, in the form this process can open.
pub fn input(raw: &str) -> PathBuf {
    let raw = raw.trim().trim_matches('"');
    if in_wsl() {
        let b = raw.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            let rest = raw[2..].replace('\\', "/");
            let rest = rest.trim_start_matches('/');
            return PathBuf::from(format!("/mnt/{}/{}", (b[0] as char).to_ascii_lowercase(), rest));
        }
        if let Some(rest) = raw.strip_prefix(r"\\wsl$\").or_else(|| raw.strip_prefix(r"\\wsl.localhost\")) {
            // \\wsl$\Ubuntu\home\me\x -> /home/me/x
            let rest = rest.replace('\\', "/");
            if let Some((_distro, path)) = rest.split_once('/') {
                return PathBuf::from(format!("/{path}"));
            }
        }
    }
    PathBuf::from(raw)
}

/// A path to report back, in the form the agent (and the user's Windows apps) can open.
pub fn output(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    if in_wsl() {
        if let Some(rest) = s.strip_prefix("/mnt/") {
            let mut parts = rest.splitn(2, '/');
            if let (Some(drive), tail) = (parts.next(), parts.next()) {
                if drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
                    return format!("{}:\\{}", drive.to_ascii_uppercase(), tail.unwrap_or("").replace('/', "\\"));
                }
            }
        }
        if s.starts_with('/') {
            let distro = std::env::var("WSL_DISTRO_NAME").unwrap_or_else(|_| "Ubuntu".into());
            return format!(r"\\wsl.localhost\{distro}{}", s.replace('/', "\\"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip_when_in_wsl() {
        if !super::in_wsl() {
            return;
        }
        let p = super::input(r"D:\models\crate one.fbx");
        assert_eq!(p, std::path::PathBuf::from("/mnt/d/models/crate one.fbx"));
        assert_eq!(super::output(&p), r"D:\models\crate one.fbx");
        assert_eq!(super::input("C:/x/y.png"), std::path::PathBuf::from("/mnt/c/x/y.png"));
    }
}
