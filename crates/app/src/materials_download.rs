//! One-click download of the "SubCraft Materials" pack: the newest `materials-vN` release of the
//! SubCraft repository (separate from the app releases), as a 1K or 2K zip with a `.sha256` file
//! next to it. The zip's top level is the pack itself (one sub-folder per material, `INDEX.md`).
//! It is downloaded into SubCraft's data folder (never next to a project), verified, unpacked
//! and then used as the Settings > Libraries pack folder. No UI dependencies.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use serde::Deserialize;

use crate::updater::{verify_file, Asset};

/// Every release of the repository (the pack has its own `materials-vN` tags).
pub const RELEASES_API: &str = "https://api.github.com/repos/Aalzard/subcraft/releases?per_page=100";
/// Where release assets are downloaded from.
const DOWNLOAD_BASE: &str = "https://github.com/Aalzard/subcraft/releases/download/";
/// Largest pack zip accepted (a 2K pack of ~100 materials is well under this).
pub const MAX_PACK: u64 = 8 * 1024 * 1024 * 1024;

/// One downloadable size of a pack release.
#[derive(Clone, Debug)]
pub struct PackOption {
    /// "1K" or "2K".
    pub resolution: &'static str,
    pub zip: Asset,
    /// URL of the `.sha256` file next to the zip (its digest is read from there when the
    /// release API gives none).
    pub sha256_url: Option<String>,
}

/// The newest pack release.
#[derive(Clone, Debug)]
pub struct PackRelease {
    pub version: u32,
    pub tag: String,
    /// Smallest first.
    pub options: Vec<PackOption>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

/// Version of a pack tag (`materials-v3` → 3).
pub fn pack_version(tag: &str) -> Option<u32> {
    tag.strip_prefix("materials-v")?.parse().ok().filter(|v| *v > 0)
}

/// Asset file name of a pack zip.
pub fn zip_name(version: u32, resolution: &str) -> String {
    format!("subcraft-materials-v{version}-{}.zip", resolution.to_ascii_lowercase())
}

/// The newest published `materials-vN` release in a GitHub releases listing, with the sizes it
/// offers. App releases, drafts, pre-releases and assets outside the repository are ignored.
pub fn parse_releases(json: &str) -> Result<Option<PackRelease>, String> {
    let releases: Vec<ApiRelease> = serde_json::from_str(json).map_err(|_| "GitHub returned an invalid releases response".to_string())?;
    let Some((version, release)) = releases.into_iter().filter(|r| !r.draft && !r.prerelease).filter_map(|r| pack_version(&r.tag_name).map(|v| (v, r))).max_by_key(|(v, _)| *v) else {
        return Ok(None);
    };
    let base = format!("{DOWNLOAD_BASE}{}/", release.tag_name);
    let mut options = Vec::new();
    for resolution in ["1K", "2K"] {
        let name = zip_name(version, resolution);
        let Some(zip) = release.assets.iter().find(|a| a.name == name) else { continue };
        if zip.browser_download_url != format!("{base}{name}") || zip.size == 0 || zip.size > MAX_PACK {
            continue;
        }
        let sha_name = format!("{name}.sha256");
        let sha256_url = release.assets.iter().find(|a| a.name == sha_name).map(|a| a.browser_download_url.clone()).filter(|u| *u == format!("{base}{sha_name}"));
        // A digest from the API wins; otherwise the `.sha256` file is read before downloading.
        let digest = zip.digest.clone().filter(|d| crate::updater::digest_hex(d).is_ok()).unwrap_or_default();
        if digest.is_empty() && sha256_url.is_none() {
            continue;
        }
        options.push(PackOption { resolution, zip: Asset { name: zip.name.clone(), browser_download_url: zip.browser_download_url.clone(), size: zip.size, digest }, sha256_url });
    }
    if options.is_empty() {
        return Err(format!("The {} pack is still being uploaded. Check again shortly.", release.tag_name));
    }
    Ok(Some(PackRelease { version, tag: release.tag_name, options }))
}

/// The digest written in a `.sha256` file (`sha256sum` style: the hex first, a file name may
/// follow), as `sha256:<hex>`.
pub fn parse_sha256_file(text: &str) -> Result<String, String> {
    let hex = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let digest = format!("sha256:{hex}");
    crate::updater::digest_hex(&digest).map_err(|_| "The pack's .sha256 file is not a SHA-256 digest".to_string())?;
    Ok(digest)
}

/// Unpack a pack zip into `dest` (created). Entries that would land outside it are refused.
/// Returns the number of files written.
pub fn extract_pack(zip_path: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("The pack is not a valid zip: {e}"))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut written = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!("The pack has an unsafe path: {}", entry.name().map(|n| n.into_owned()).unwrap_or_default()));
        };
        let out = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        written += 1;
    }
    Ok(written)
}

/// Where a pack version and size is unpacked: `<data folder>/materials/materials-vN-<res>`.
pub fn pack_folder(data_dir: &Path, version: u32, resolution: &str) -> PathBuf {
    data_dir.join("materials").join(format!("materials-v{version}-{}", resolution.to_ascii_lowercase()))
}

/// Progress of a check or download, polled by the UI.
#[derive(Clone, Debug)]
pub enum Phase {
    Idle,
    Checking,
    /// No pack release published yet.
    NoRelease,
    Available(PackRelease),
    Downloading { bytes: u64, total: u64 },
    Extracting,
    /// Unpacked here: use it as the pack folder.
    Done(PathBuf),
    Error(String),
}

/// Background checks and downloads of the pack.
#[derive(Default)]
pub struct MaterialsDownloader {
    pub phase: Phase,
    worker: Option<Receiver<Phase>>,
}

impl Default for Phase {
    fn default() -> Self {
        Phase::Idle
    }
}

impl MaterialsDownloader {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    /// Take the worker's latest news.
    pub fn poll(&mut self) {
        let Some(rx) = &self.worker else { return };
        loop {
            match rx.try_recv() {
                Ok(p) => self.phase = p,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if matches!(self.phase, Phase::Checking | Phase::Downloading { .. } | Phase::Extracting) {
                        self.phase = Phase::Error("The download stopped unexpectedly; try again".into());
                    }
                    self.worker = None;
                    break;
                }
            }
        }
    }

    /// Look for the newest pack release.
    pub fn check(&mut self) {
        if self.busy() {
            return;
        }
        self.phase = Phase::Checking;
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        std::thread::spawn(move || {
            let phase = fetch_text(RELEASES_API, 4 * 1024 * 1024).and_then(|t| parse_releases(&t)).map(|r| r.map_or(Phase::NoRelease, Phase::Available)).unwrap_or_else(Phase::Error);
            let _ = tx.send(phase);
        });
    }

    /// Download, verify and unpack one size of `release` into the data folder.
    pub fn download(&mut self, release: &PackRelease, option: &PackOption) {
        if self.busy() {
            return;
        }
        let Some(data) = crate::recovery::data_dir() else {
            self.phase = Phase::Error("No writable SubCraft data folder".into());
            return;
        };
        self.phase = Phase::Downloading { bytes: 0, total: option.zip.size };
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        let (version, option) = (release.version, option.clone());
        std::thread::spawn(move || {
            let phase = (|| {
                let mut asset = option.zip.clone();
                if asset.digest.is_empty() {
                    let url = option.sha256_url.as_deref().ok_or("The pack has no SHA-256 digest")?;
                    asset.digest = parse_sha256_file(&fetch_text(url, 4096)?)?;
                }
                let staging = data.join("materials").join(format!(".download-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
                let zip_path = staging.join(&asset.name);
                let result = (|| {
                    let mut response = agent(3600).get(&asset.browser_download_url).header("User-Agent", "SubCraft-materials").header("Accept-Encoding", "identity").call().map_err(|e| format!("Could not download the pack: {e}"))?;
                    copy_pack(response.body_mut().as_reader(), &zip_path, &asset, |bytes| {
                        let _ = tx.send(Phase::Downloading { bytes, total: asset.size });
                    })?;
                    let _ = tx.send(Phase::Extracting);
                    let dest = pack_folder(&data, version, option.resolution);
                    let _ = std::fs::remove_dir_all(&dest);
                    extract_pack(&zip_path, &dest)?;
                    Ok(dest)
                })();
                let _ = std::fs::remove_dir_all(&staging);
                result
            })()
            .map(Phase::Done)
            .unwrap_or_else(Phase::Error);
            let _ = tx.send(phase);
        });
    }
}

/// Write a downloading pack to `path`, refusing more bytes than declared, then check its size
/// and SHA-256 (a failed download leaves no file).
pub fn copy_pack(mut reader: impl Read, path: &Path, asset: &Asset, mut progress: impl FnMut(u64)) -> Result<(), String> {
    use std::io::Write;
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
        let (mut total, mut buf) = (0u64, vec![0u8; 256 * 1024]);
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > asset.size || total > MAX_PACK {
                return Err("The pack download is larger than the release says".to_string());
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            progress(total);
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        verify_file(path, asset).map_err(|_| "The pack download failed its size or SHA-256 check".to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn agent(seconds: u64) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(seconds))).https_only(true).build().into()
}

fn fetch_text(url: &str, limit: u64) -> Result<String, String> {
    let mut response = agent(30).get(url).header("User-Agent", "SubCraft-materials").header("Accept", "application/vnd.github+json").call().map_err(|e| format!("Could not reach GitHub: {e}"))?;
    let mut text = String::new();
    response.body_mut().as_reader().take(limit + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() as u64 > limit {
        return Err("GitHub's response is too large".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn release(tag: &str, draft: bool, assets: &[(&str, u64, Option<&str>)]) -> String {
        let assets: Vec<String> = assets.iter().map(|(name, size, digest)| {
            let digest = digest.map_or("null".to_string(), |d| format!("\"{d}\""));
            format!(r#"{{"name":"{name}","browser_download_url":"https://github.com/Aalzard/subcraft/releases/download/{tag}/{name}","size":{size},"digest":{digest}}}"#)
        }).collect();
        format!(r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":false,"assets":[{}]}}"#, assets.join(","))
    }

    #[test]
    fn the_newest_published_pack_release_and_its_sizes_are_found() {
        let digest = format!("sha256:{HEX}");
        let json = format!(
            "[{},{},{},{}]",
            release("v0.4.0", false, &[("SubCraft-0.4.0-setup.exe", 10, Some(&digest))]),
            release("materials-v3", true, &[("subcraft-materials-v3-2k.zip", 10, Some(&digest))]),
            release("materials-v2", false, &[("subcraft-materials-v2-2k.zip", 900, None), ("subcraft-materials-v2-2k.zip.sha256", 80, None), ("subcraft-materials-v2-1k.zip", 300, Some(&digest))]),
            release("materials-v1", false, &[("subcraft-materials-v1-2k.zip", 800, Some(&digest))]),
        );
        let r = parse_releases(&json).unwrap().expect("a pack release");
        assert_eq!((r.version, r.tag.as_str()), (2, "materials-v2"), "the newest published one (v3 is a draft, v0.4.0 is the app)");
        let res: Vec<&str> = r.options.iter().map(|o| o.resolution).collect();
        assert_eq!(res, ["1K", "2K"]);
        assert_eq!((r.options[0].zip.size, r.options[0].zip.digest.as_str()), (300, digest.as_str()));
        assert!(r.options[1].zip.digest.is_empty() && r.options[1].sha256_url.as_deref().is_some_and(|u| u.ends_with("subcraft-materials-v2-2k.zip.sha256")), "2K is verified through its .sha256 file");
        // Nothing published yet, an invalid response, and a release without usable assets.
        assert!(parse_releases(&format!("[{}]", release("v0.4.0", false, &[]))).unwrap().is_none());
        assert!(parse_releases("{").is_err());
        assert!(parse_releases(&format!("[{}]", release("materials-v1", false, &[("subcraft-materials-v1-2k.zip", 5, None)]))).is_err(), "no digest and no .sha256 file: still uploading");
        assert_eq!((pack_version("materials-v12"), pack_version("materials-v0"), pack_version("v1.0")), (Some(12), None, None));
    }

    #[test]
    fn download_links_outside_the_repository_are_ignored() {
        let digest = format!("sha256:{HEX}");
        let json = r#"[{"tag_name":"materials-v1","assets":[{"name":"subcraft-materials-v1-1k.zip","browser_download_url":"https://example.com/subcraft-materials-v1-1k.zip","size":5,"digest":"DIGEST"}]}]"#.replace("DIGEST", &digest);
        assert!(parse_releases(&json).is_err(), "the only asset points elsewhere");
    }

    #[test]
    fn a_download_is_kept_only_when_its_size_and_digest_match() {
        use sha2::Digest;
        let root = std::env::temp_dir().join(format!("subcraft-pack-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bytes = b"pack bytes".to_vec();
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
        let asset = |size: u64, digest: &str| Asset { name: "p.zip".into(), browser_download_url: String::new(), size, digest: digest.into() };
        let mut seen = 0;
        copy_pack(&bytes[..], &root.join("ok.zip"), &asset(bytes.len() as u64, &digest), |b| seen = b).unwrap();
        assert_eq!(seen, bytes.len() as u64);
        assert!(root.join("ok.zip").is_file());
        assert!(copy_pack(&bytes[..], &root.join("big.zip"), &asset(4, &digest), |_| {}).is_err(), "more bytes than declared");
        assert!(!root.join("big.zip").exists());
        assert!(copy_pack(&bytes[..], &root.join("bad.zip"), &asset(bytes.len() as u64, &format!("sha256:{HEX}")), |_| {}).is_err(), "wrong digest");
        assert!(!root.join("bad.zip").exists(), "a failed download leaves no file");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sha256_files_give_the_digest() {
        assert_eq!(parse_sha256_file(&format!("{}  subcraft-materials-v1-2k.zip\n", HEX.to_uppercase())).unwrap(), format!("sha256:{HEX}"));
        assert!(parse_sha256_file("not a digest").is_err());
        assert!(parse_sha256_file("").is_err());
    }

    #[test]
    fn a_pack_zip_is_unpacked_and_unsafe_paths_are_refused() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("subcraft-pack-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let make = |name: &str, entries: &[(&str, &[u8])]| {
            let path = root.join(name);
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for (entry, bytes) in entries {
                w.start_file(*entry, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(bytes).unwrap();
            }
            w.finish().unwrap();
            path
        };
        let good = make("good.zip", &[("INDEX.md", b"# pack"), ("wood_test/material.json", b"{}"), ("wood_test/wood_test_basecolor.png", b"png")]);
        let dest = pack_folder(&root, 1, "2K");
        assert!(dest.ends_with("materials/materials-v1-2k"));
        assert_eq!(extract_pack(&good, &dest).unwrap(), 3);
        assert!(dest.join("wood_test").join("material.json").is_file() && dest.join("INDEX.md").is_file(), "the zip's top level is the pack");
        let evil = make("evil.zip", &[("../escaped.txt", b"x")]);
        assert!(extract_pack(&evil, &root.join("other")).is_err());
        assert!(!root.join("escaped.txt").exists(), "nothing lands outside the folder");
        assert!(extract_pack(&root.join("missing.zip"), &root.join("x")).is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
