use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

const V: &str = "0.7.0";
const ID: &str = "3f2c9a3e-8a51-4c0e-9d0a-2b7f4b9d6c11";

fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-plugin-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn manifest(id: &str, name: &str, extra: &str) -> String {
    format!(r#"{{"format":"subcraft-plugin","schema":1,"id":"{id}","name":"{name}","version":"1.2.0","author":"Ana"{extra}}}"#)
}

fn write_plugin(root: &Path, folder: &str, json: &str) -> PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::create_dir_all(dir.join("library/smart-masks")).unwrap();
    std::fs::write(dir.join(MANIFEST), json).unwrap();
    std::fs::write(dir.join("scripts/b_second.rhai"), "print(2);").unwrap();
    std::fs::write(dir.join("scripts/a-first.rhai"), "print(1);").unwrap();
    std::fs::write(dir.join("scripts/notes.txt"), "not a script").unwrap();
    std::fs::write(dir.join("library/smart-masks/x.scmask"), b"zip").unwrap();
    dir
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn manifest_is_checked() {
    let m = PluginManifest::parse(&manifest(ID, "Rust Pack", ""), V).unwrap();
    assert_eq!(m.name, "Rust Pack");
    assert_eq!(m.key(), ID);
    // Upper-case UUIDs mean the same plugin.
    assert_eq!(PluginManifest::parse(&manifest(&ID.to_uppercase(), "x", ""), V).unwrap().key(), ID);

    assert!(PluginManifest::parse("{", V).unwrap_err().contains("not valid"));
    assert!(PluginManifest::parse(&manifest("my-plugin", "x", ""), V).unwrap_err().contains("UUID"));
    assert!(PluginManifest::parse(&manifest(ID, " ", ""), V).unwrap_err().contains("name"));
    let wrong_format = manifest(ID, "x", "").replace("subcraft-plugin", "other");
    assert!(PluginManifest::parse(&wrong_format, V).unwrap_err().contains("format"));
    let newer_schema = manifest(ID, "x", "").replace("\"schema\":1", "\"schema\":2");
    assert!(PluginManifest::parse(&newer_schema, V).unwrap_err().contains("newer SubCraft"));

    let needs = manifest(ID, "x", r#","min_subcraft_version":"0.8.0""#);
    assert_eq!(PluginManifest::parse(&needs, V).unwrap_err(), "needs SubCraft 0.8.0 or newer (this is 0.7.0)");
    assert!(PluginManifest::parse(&needs, "0.8.0").is_ok());
    assert!(PluginManifest::parse(&manifest(ID, "x", r#","min_subcraft_version":"soon""#), V).is_err());
}

#[test]
fn plugins_are_discovered_with_scripts_and_assets() {
    let root = temp();
    write_plugin(&root, "zeta folder", &manifest(ID, "Alpha Pack", ""));
    write_plugin(&root, "beta", &manifest("0b8c2f8e-1f7c-4b8a-9a55-6b0d3c1f2e44", "Beta Tools", r#","min_subcraft_version":"9.0.0""#));
    std::fs::create_dir_all(root.join("broken")).unwrap();
    std::fs::write(root.join("broken").join(MANIFEST), "nope").unwrap();
    // Not plugins: a folder without a manifest, a hidden half-finished install, a loose script.
    std::fs::create_dir_all(root.join("just a folder")).unwrap();
    write_plugin(&root, ".installing-x", &manifest("6f1d3c9a-0000-4000-8000-000000000001", "Hidden", ""));
    std::fs::write(root.join("loose.rhai"), "").unwrap();

    let found = discover(&root, V);
    let names: Vec<String> = found.iter().map(Plugin::name).collect();
    assert_eq!(names, ["Alpha Pack", "Beta Tools", "broken"]);

    let alpha = &found[0];
    assert!(alpha.usable());
    let scripts: Vec<&str> = alpha.scripts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(scripts, ["a first", "b second"]);
    assert_eq!(alpha.contents_summary(), "2 scripts, 1 smart mask");
    let lib = alpha.library().expect("has assets");
    assert_eq!(lib.list(UserAssetKind::SmartMask).len(), 1);

    // Too new: listed by its name with the reason, and not usable.
    assert!(!found[1].usable());
    assert!(found[1].problem.as_deref().unwrap().contains("9.0.0"));
    // Broken manifest: listed by its folder name.
    assert!(found[2].manifest.is_none() && found[2].problem.is_some());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_duplicate_id_never_shadows_the_first_plugin() {
    let root = temp();
    write_plugin(&root, "one", &manifest(ID, "A", ""));
    write_plugin(&root, "two", &manifest(ID, "B", ""));
    let found = discover(&root, V);
    assert!(found[0].usable());
    assert!(found[1].problem.as_deref().unwrap().contains("same id"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn plugins_are_switched_off_by_id_not_by_folder() {
    let root = temp();
    let dir = write_plugin(&root, "first name", &manifest(ID, "A", ""));
    let p = Plugin::load(&dir, V);
    let mut disabled = Vec::new();
    assert!(is_enabled(&p, &disabled));
    set_enabled(&p, false, &mut disabled);
    set_enabled(&p, false, &mut disabled);
    assert_eq!(disabled, [ID]);
    // Renaming the folder keeps it off.
    std::fs::rename(&dir, root.join("renamed")).unwrap();
    assert!(!is_enabled(&Plugin::load(&root.join("renamed"), V), &disabled));
    set_enabled(&p, true, &mut disabled);
    assert!(disabled.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn zip_install_unpacks_and_updates_in_place() {
    let root = temp();
    let plugins = root.join("Plugins");
    let v1 = zip_of(&[
        ("Rust Pack/plugin.json", manifest(ID, "Rust Pack", "").as_bytes()),
        ("Rust Pack/scripts/rust.rhai", b"print(1);"),
        ("Rust Pack/library/smart-masks/rust.scmask", b"m"),
    ]);
    let p = install_zip_reader(std::io::Cursor::new(v1), &plugins, V).unwrap();
    assert_eq!(p.dir, plugins.join("Rust Pack"));
    assert_eq!(p.contents_summary(), "1 script, 1 smart mask");

    // Version 2, zipped without a top folder: replaces version 1 in the same folder.
    let v2_json = manifest(ID, "Rust Pack", "").replace("1.2.0", "2.0.0");
    let v2 = zip_of(&[("plugin.json", v2_json.as_bytes()), ("scripts/new.rhai", b"print(2);")]);
    let p2 = install_zip_reader(std::io::Cursor::new(v2), &plugins, V).unwrap();
    assert_eq!(p2.dir, p.dir);
    assert_eq!(p2.manifest.as_ref().unwrap().version, "2.0.0");
    assert_eq!(p2.contents_summary(), "1 script", "old files are gone");
    let left: Vec<_> = std::fs::read_dir(&plugins).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "no staging or backup folders left: {left:?}");

    // Another plugin with the same name gets its own folder.
    let other = zip_of(&[("plugin.json", manifest("0b8c2f8e-1f7c-4b8a-9a55-6b0d3c1f2e44", "Rust Pack", "").as_bytes())]);
    assert_eq!(install_zip_reader(std::io::Cursor::new(other), &plugins, V).unwrap().dir, plugins.join("Rust Pack (2)"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn bad_zips_are_refused_and_leave_nothing_behind() {
    let root = temp();
    let plugins = root.join("Plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let no_manifest = zip_of(&[("scripts/a.rhai", b"")]);
    assert!(install_zip_reader(std::io::Cursor::new(no_manifest), &plugins, V).unwrap_err().contains("no plugin.json"));
    let deep = zip_of(&[("a/b/plugin.json", manifest(ID, "x", "").as_bytes())]);
    assert!(install_zip_reader(std::io::Cursor::new(deep), &plugins, V).unwrap_err().contains("top folder"));
    let escape = zip_of(&[("plugin.json", manifest(ID, "x", "").as_bytes()), ("../evil.txt", b"x")]);
    assert!(install_zip_reader(std::io::Cursor::new(escape), &plugins, V).unwrap_err().contains("unsafe path"));
    assert!(!root.join("evil.txt").exists());
    let too_new = zip_of(&[("plugin.json", manifest(ID, "x", r#","min_subcraft_version":"99.0.0""#).as_bytes())]);
    assert!(install_zip_reader(std::io::Cursor::new(too_new), &plugins, V).unwrap_err().contains("99.0.0"));
    assert!(install_zip_reader(std::io::Cursor::new(b"not a zip".to_vec()), &plugins, V).unwrap_err().contains("not a zip"));
    assert_eq!(std::fs::read_dir(&plugins).unwrap().count(), 0);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn folder_install_create_and_uninstall() {
    let root = temp();
    let plugins = root.join("Plugins");
    let src = write_plugin(&root, "source", &manifest(ID, "Copy Me", ""));
    let p = install_folder(&src, &plugins, V).unwrap();
    assert_eq!(p.dir, plugins.join("Copy Me"));
    assert_eq!(p.contents_summary(), "2 scripts, 1 smart mask");

    let made = create_plugin("My: Plugin", "Ana", &plugins, V).unwrap();
    assert_eq!(made.dir, plugins.join("My_ Plugin"));
    assert!(made.usable());
    assert_eq!(made.contents_summary(), "1 script");
    assert!(made.dir.join("library/smart-materials").is_dir());

    // Only plugin folders directly inside the plugins folder can be removed.
    assert!(uninstall(&Plugin::load(&src, V), &plugins).is_err());
    assert!(src.exists());
    uninstall(&p, &plugins).unwrap();
    assert!(!p.dir.exists());
    assert_eq!(discover(&plugins, V).len(), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn install_files_refuses_paths_outside_the_plugin() {
    let root = temp();
    let json = manifest(ID, "x", "");
    let files: [(&str, &[u8]); 2] = [("plugin.json", json.as_bytes()), ("../out.txt", b"x")];
    assert!(install_files(&files, &root, V).is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    std::fs::remove_dir_all(&root).unwrap();
}
