use super::*;
use super::apply::*;
use std::io::Cursor;
fn dir() -> PathBuf { let p = std::env::temp_dir().join(format!("subcraft-update-test-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(&p).unwrap(); p }
fn asset(bytes: &[u8]) -> Asset { Asset { name: "SubCraft-0.1.4-windows-x64.zip".into(), browser_download_url: "https://github.com/Aalzard/subcraft/releases/download/v0.1.4/SubCraft-0.1.4-windows-x64.zip".into(), size: bytes.len() as u64, digest: format!("sha256:{:x}", Sha256::digest(bytes)) } }
fn json(version: &str, kind: PackageKind) -> String {
    let name = kind.name(version); serde_json::json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,"body":"notes", "assets":[{"name":name,"browser_download_url":format!("https://github.com/Aalzard/subcraft/releases/download/v{version}/{name}"),"size":4,"digest":format!("sha256:{:x}",Sha256::digest(b"test"))}]}).to_string()
}
#[test]
fn release_selection_uses_semver_and_the_install_type() {
    for kind in [PackageKind::WindowsPortable, PackageKind::WindowsInstaller { admin: false }, PackageKind::WindowsInstaller { admin: true }, PackageKind::LinuxPortable] {
        let data = json("0.1.10", kind); let release = parse_release(&data,"0.1.9",kind).unwrap().unwrap(); assert_eq!(release.kind,kind);
        assert!(parse_release(&data,"0.1.10",kind).unwrap().is_none()); assert!(parse_release(&data,"0.2.0",kind).unwrap().is_none());
        assert!(parse_release(&data,"0.1.10-beta.1",kind).unwrap().is_some()); assert!(parse_release(&data,"0.1.10+build.2",kind).unwrap().is_none());
    }
}
#[test]
fn release_rejects_other_repositories_bad_digests_and_unready_assets() {
    let mut value: serde_json::Value = serde_json::from_str(&json("0.1.4",PackageKind::WindowsPortable)).unwrap();
    value["assets"][0]["browser_download_url"] = "https://evil.example/update.exe".into(); assert!(parse_release(&value.to_string(),"0.1.3",PackageKind::WindowsPortable).is_err());
    value = serde_json::from_str(&json("0.1.4",PackageKind::WindowsPortable)).unwrap(); value["assets"][0]["digest"]="sha256:bad".into(); assert!(parse_release(&value.to_string(),"0.1.3",PackageKind::WindowsPortable).is_err());
    value["assets"] = serde_json::json!([]); assert!(parse_release(&value.to_string(),"0.1.3",PackageKind::WindowsPortable).unwrap_err().contains("still being uploaded"));
    value["draft"] = true.into(); assert!(parse_release(&value.to_string(),"0.1.3",PackageKind::WindowsPortable).unwrap().is_none());
    value["draft"] = false.into(); value["prerelease"] = true.into(); assert!(parse_release(&value.to_string(),"0.1.3",PackageKind::WindowsPortable).unwrap().is_none());
}
#[test]
fn verified_download_is_bounded_reports_progress_and_cleans_failures() {
    let root=dir(); let good=asset(b"payload"); let path=root.join("package"); let mut progress=0;
    copy_verified(Cursor::new(b"payload"),&path,&good,|n|progress=n).unwrap(); assert_eq!(progress,7); verify_file(&path,&good).unwrap();
    assert!(copy_verified(Cursor::new(b"wrong"),&path,&good, |_|{}).is_err()); assert_eq!(std::fs::read(&path).unwrap(),b"payload");
    for bytes in [b"payloaX".as_slice(),b"short".as_slice(),b"oversized payload".as_slice()] { let path=root.join(uuid::Uuid::new_v4().to_string()); assert!(copy_verified(Cursor::new(bytes),&path,&good, |_|{}).is_err()); assert!(!path.exists()); }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn portable_replacement_keeps_neighbors_and_missing_staging_keeps_old_binary() {
    let root=dir(); let target=root.join("SubCraft.exe"); let source=root.join("staged"); let neighbor=root.join("work.subcraft");
    std::fs::write(&target,b"old").unwrap(); std::fs::write(&source,b"new").unwrap(); std::fs::write(&neighbor,b"user work").unwrap();
    replace_portable(&target,&root.join("missing"),uuid::Uuid::new_v4()).unwrap_err(); assert_eq!(std::fs::read(&target).unwrap(),b"old");
    replace_portable(&target,&source,uuid::Uuid::new_v4()).unwrap(); assert_eq!(std::fs::read(&target).unwrap(),b"new"); assert_eq!(std::fs::read(&neighbor).unwrap(),b"user work");
    assert!(!std::fs::read_dir(&root).unwrap().any(|f|f.unwrap().file_name().to_string_lossy().starts_with(".subcraft-"))); std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn zip_extracts_only_the_expected_executable() {
    let root=dir(); let path=root.join("package.zip"); let mut zip=zip::ZipWriter::new(std::fs::File::create(&path).unwrap()); let options=zip::write::SimpleFileOptions::default();
    zip.start_file("../escape.exe",options).unwrap(); zip.write_all(b"malicious neighbor").unwrap();
    zip.start_file("SubCraft-0.1.4-windows-x64/SubCraft.exe",options).unwrap(); let mut binary=vec![0;128]; binary[..2].copy_from_slice(b"MZ"); zip.write_all(&binary).unwrap(); zip.finish().unwrap();
    let bytes=std::fs::read(&path).unwrap(); let release=Release {version:"0.1.4".into(),notes:String::new(),kind:PackageKind::WindowsPortable,asset:asset(&bytes)};
    let out=root.join("new-app"); extract_binary(&release,&path,&out).unwrap(); assert_eq!(std::fs::read(out).unwrap(),binary); assert!(!root.parent().unwrap().join("escape.exe").exists()); std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn restart_snapshot_restores_unsaved_document_and_original_path() {
    let root=dir(); let id=uuid::Uuid::new_v4(); let folder=root.join(id.to_string()); std::fs::create_dir(&folder).unwrap();
    let mut s=crate::Session::new(subcraft_mesh::primitives::cube(2.0),128); let original=root.join("original.subcraft"); s.save(Some(&original)).unwrap();
    s.execute(crate::Command::AddLayer(crate::NewLayer::Fill)).unwrap(); let original_bytes=std::fs::read(&original).unwrap(); s.snapshot().save(&folder.join("workspace.subcraft")).unwrap();
    let job=UpdateJob {schema:1,id,parent_pid:0,target:root.join("SubCraft.exe"),old_digest:String::new(),release:parse_release(&json("0.1.4",PackageKind::WindowsPortable),"0.1.3",PackageKind::WindowsPortable).unwrap().unwrap(),package:folder.join(PackageKind::WindowsPortable.name("0.1.4")),original_project:Some(original.clone()),modified:true,texture_set:s.selection().texture_set,layer:s.selection().layer};
    let path=folder.join("job.json"); std::fs::write(&path,serde_json::to_vec(&job).unwrap()).unwrap(); let back=resume_session(&path).unwrap();
    assert_eq!(back.document(),s.document()); assert_eq!(back.selection(),s.selection()); assert!(back.is_modified()); assert_eq!(back.project_path(),Some(original.as_path())); assert_eq!(std::fs::read(original).unwrap(),original_bytes);
    assert!(UpdateJob::read(&path.with_file_name("other.json")).is_err());
    let mut invalid=job.clone(); invalid.package=root.join(&job.release.asset.name); std::fs::write(&path,serde_json::to_vec(&invalid).unwrap()).unwrap(); assert!(UpdateJob::read(&path).is_err());
    invalid.package=folder.join("wrong.zip"); std::fs::write(&path,serde_json::to_vec(&invalid).unwrap()).unwrap(); assert!(UpdateJob::read(&path).is_err());
    std::fs::write(&path,serde_json::to_vec(&job).unwrap()).unwrap();
    std::fs::write(&job.package,b"cached package").unwrap(); std::fs::write(folder.join("new-app"),b"staged executable").unwrap(); std::fs::write(folder.join("keep.txt"),b"neighbor").unwrap();
    compact_finished_cache(&root); assert!(job.package.exists());
    std::fs::write(folder.join("result.txt"),"Update installed").unwrap(); compact_finished_cache(&root);
    assert!(!job.package.exists()); assert!(!folder.join("new-app").exists()); assert!(folder.join("workspace.subcraft").exists()); assert!(folder.join("keep.txt").exists()); assert!(resume_session(&path).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn disconnected_worker_reports_error_and_success_is_not_overwritten() {
    let (tx,rx)=mpsc::channel(); drop(tx);
    let mut updater=Updater {phase:Phase::Checking,open:true,target:PathBuf::new(),kind:PackageKind::WindowsPortable,auto_install:false,worker:Some(rx)};
    updater.poll(); assert!(matches!(updater.phase,Phase::Error(_))); assert!(!updater.busy());
    let (tx,rx)=mpsc::channel(); tx.send(Phase::Current).unwrap(); drop(tx); updater.worker=Some(rx); updater.phase=Phase::Checking;
    updater.poll(); assert!(matches!(updater.phase,Phase::Current)); assert!(!updater.busy());
}
#[test]
fn replacement_refuses_to_overwrite_an_existing_backup() {
    let root=dir(); let id=uuid::Uuid::new_v4(); let target=root.join("app"); let staged=root.join("staged"); let backup=root.join(format!(".subcraft-previous-{id}"));
    std::fs::write(&target,b"old").unwrap(); std::fs::write(&staged,b"new").unwrap(); std::fs::write(&backup,b"earlier backup").unwrap();
    assert!(replace_portable(&target,&staged,id).is_err()); assert_eq!(std::fs::read(&target).unwrap(),b"old"); assert_eq!(std::fs::read(&backup).unwrap(),b"earlier backup"); std::fs::remove_dir_all(root).unwrap();
}
#[cfg(windows)]
#[test]
fn installer_receives_ordinary_paths_instead_of_windows_device_prefixes() {
    assert_eq!(installer_path(Path::new(r"\\?\E:\apps with spaces\SubCraft")),r"E:\apps with spaces\SubCraft");
    assert_eq!(installer_path(Path::new(r"\\?\UNC\server\share\SubCraft")),r"\\server\share\SubCraft");
    assert_eq!(installer_path(Path::new(r"E:\SubCraft")),r"E:\SubCraft");
}
#[cfg(target_os="linux")]
#[test]
fn linux_tar_extracts_only_the_expected_regular_executable() {
    let root=dir(); let path=root.join("package.tar.gz"); let gzip=flate2::write::GzEncoder::new(std::fs::File::create(&path).unwrap(),flate2::Compression::default()); let mut tar=tar::Builder::new(gzip);
    let mut binary=vec![0;128]; binary[..4].copy_from_slice(b"\x7fELF"); let mut header=tar::Header::new_gnu(); header.set_size(binary.len() as u64); header.set_mode(0o755); header.set_cksum();
    tar.append_data(&mut header,"SubCraft-0.1.4-linux-x86_64/subcraft",binary.as_slice()).unwrap(); tar.into_inner().unwrap().finish().unwrap();
    let bytes=std::fs::read(&path).unwrap(); let mut release=parse_release(&json("0.1.4",PackageKind::LinuxPortable),"0.1.3",PackageKind::LinuxPortable).unwrap().unwrap(); release.asset.size=bytes.len() as u64; release.asset.digest=format!("sha256:{:x}",Sha256::digest(&bytes));
    let out=root.join("new-app"); extract_binary(&release,&path,&out).unwrap(); assert_eq!(std::fs::read(out).unwrap(),binary); std::fs::remove_dir_all(root).unwrap();
}
