//! Reproducible native viewport checks using the real texture-set import and screenshot paths.
//! prepare <texture-folder> <project.subcraft> [resolution]
//! capture <project.subcraft> <screenshot.png>
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("prepare <folder> <project> [resolution] | capture <project> <png>".into());
    }
    if args[0] == "prepare" {
        let paths: Vec<_> = std::fs::read_dir(&args[1])?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        let resolution = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(4096);
        let mut session =
            subcraft_app::Session::new(subcraft_mesh::primitives::plane(2., 1), resolution);
        let imported = session
            .import_texture_set(&paths)
            .map_err(std::io::Error::other)?;
        eprintln!(
            "{}: {:?}; skipped {:?}",
            imported.layer_name, imported.used, imported.skipped
        );
        session
            .save(Some(&PathBuf::from(&args[2])))
            .map_err(std::io::Error::other)?;
        return Ok(());
    }
    if args[0] != "capture" {
        return Err("unknown operation".into());
    }
    subcraft_ui::run(subcraft_ui::app::LaunchOptions {
        mesh: Some(args[1].clone().into()),
        screenshot: Some(args[2].clone().into()),
        ..Default::default()
    })
    .map_err(|e| format!("{e}").into())
}
