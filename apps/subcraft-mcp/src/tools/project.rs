//! Project-level tools: open / create / save, texture sets, history.

use serde_json::{Value, json};
use subcraft_app::new_project::{NewProjectSettings, ProjectTemplate, SplitRule};
use subcraft_app::{Command, Session};
use subcraft_doc::NormalFormat;

use crate::describe;
use crate::params::{Args, Res, channel, enum_from, name_of, names};
use crate::paths;
use crate::server::{self, Reply, Server, exec};

pub fn status(server: &mut Server) -> Res<Reply> {
    let project = server.session.as_ref().map(|s| json!({
        "file": s.project_path().map(paths::output),
        "model": s.scene().mesh.name,
        "texture_sets": s.document().texture_sets.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
        "unsaved_changes": s.is_modified(),
    }));
    let gpu = server.gpu_name().map(str::to_string);
    let lib = server.library();
    let counts = json!({
        "materials": lib.materials.len(), "smart_materials": lib.smart_materials.len(), "smart_masks": lib.smart_masks.len(),
        "brushes": lib.alphas.len(), "environments": lib.environments.len(),
    });
    Ok(Reply::json(json!({
        "server": "subcraft-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        "wsl": std::env::var("WSL_DISTRO_NAME").ok(),
        "project": project,
        "render_adapter": gpu.unwrap_or_else(|| "not started yet (first render_preview starts it)".into()),
        "shelf": counts,
        "library_notes": server.library_notes,
        "model_formats": subcraft_mesh::SUPPORTED_EXTENSIONS,
    })))
}

fn import(path: &std::path::Path) -> Res<(subcraft_mesh::Mesh, Vec<String>)> {
    if !path.exists() {
        return Err(format!("file not found: {}", paths::output(path)));
    }
    let mut report = match subcraft_mesh::import_mesh(path) {
        Err(subcraft_mesh::ImportError::Build(subcraft_mesh::BuildError::NoUvs)) => subcraft_mesh::import_mesh_for_baking(path).map_err(|e| e.to_string())?,
        other => other.map_err(|e| e.to_string())?,
    };
    report.mesh = subcraft_mesh::unwrap::unwrap_if_missing(std::mem::take(&mut report.mesh), &mut report.warnings);
    Ok((report.mesh, report.warnings))
}

pub fn inspect_model(args: &Args) -> Res<Reply> {
    let path = paths::input(args.req_str("path")?);
    let (mesh, warnings) = import(&path)?;
    let s = subcraft_app::new_project::summarize(&mesh, &warnings);
    Ok(Reply::json(json!({
        "name": s.name,
        "triangles": s.triangles,
        "vertices": s.vertices,
        "objects": s.objects.iter().map(|(n, t)| json!({ "name": n, "triangles": t })).collect::<Vec<_>>(),
        "materials": s.materials.iter().map(|(n, t)| json!({ "name": n, "triangles": t })).collect::<Vec<_>>(),
        "has_uvs": s.has_uvs,
        "uv_bounds": s.uv_bounds.map(|(lo, hi)| json!([lo.to_array(), hi.to_array()])),
        "udim_tiles": s.tiles.iter().map(|t| json!({ "tile": t.number, "triangles": t.triangles })).collect::<Vec<_>>(),
        "uv_overlap": s.overlap,
        "paintable": s.paintable(),
        "warnings": s.warnings,
        "bounds_min": mesh.bounds.min.to_array(),
        "bounds_max": mesh.bounds.max.to_array(),
    })))
}

fn guard_unsaved(server: &Server, args: &Args) -> Res<()> {
    if let Some(s) = &server.session
        && s.is_modified() && !args.bool("discard_unsaved")?.unwrap_or(false) {
            return Err("the open project has unsaved changes: project_save first, or pass discard_unsaved: true".into());
        }
    Ok(())
}

fn opened(server: &mut Server, session: Session, warnings: Vec<String>) -> Res<Reply> {
    server.session = Some(session);
    let s = server.session.as_ref().expect("just set");
    let mut info = describe::project(s);
    info["warnings"] = json!(warnings);
    info["next_steps"] = json!("bake_mesh_maps for smart materials and edge/cavity masks; layers_list; render_preview to see the model");
    Ok(Reply::json(info))
}

/// The `resolution` argument, refused unless it is one SubCraft offers. project_new used to drop
/// an unsupported value silently (the sample kept 1024, a model snapped to the nearest size).
fn resolution_arg(args: &Args) -> Res<Option<u32>> {
    match args.u32("resolution")? {
        Some(r) if !subcraft_doc::RESOLUTIONS.contains(&r) => Err(format!("resolution must be one of {:?}", subcraft_doc::RESOLUTIONS)),
        r => Ok(r),
    }
}

pub fn new(server: &mut Server, args: &Args) -> Res<Reply> {
    guard_unsaved(server, args)?;
    let raw = args.req_str("model_path")?;
    let resolution = resolution_arg(args)?;
    if raw.trim().eq_ignore_ascii_case("sample") {
        let mut session = Session::sample();
        if let Some(r) = resolution {
            exec(&mut session, Command::SetResolution { texture_set: None, resolution: r })?;
        }
        return opened(server, session, vec!["SubCraft's built-in sample crate".into()]);
    }
    let path = paths::input(raw);
    let mut settings = NewProjectSettings::default();
    if let Some(t) = args.str("template")? {
        settings.apply_template(enum_from::<ProjectTemplate>("template", t, &names(&ProjectTemplate::ALL))?);
    }
    if let Some(r) = resolution {
        settings.resolution = r;
    }
    if let Some(s) = args.str("split")? {
        settings.split = enum_from::<SplitRule>("split", s, &names(&SplitRule::ALL))?;
    }
    if let Some(u) = args.bool("udim")? {
        settings.udim = u;
    }
    if let Some(chs) = args.strings("channels")? {
        settings.channels = chs.iter().map(|c| channel(c)).collect::<Res<Vec<_>>>()?;
        if !settings.channels.contains(&subcraft_core::ChannelKind::BaseColor) {
            settings.channels.insert(0, subcraft_core::ChannelKind::BaseColor);
        }
    }
    if let Some(n) = args.str("normal_format")? {
        settings.normal_format = normal_format(n)?;
    }
    let is_spp = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp"));
    let (session, warnings) = if is_spp {
        Session::open_mesh(&path, settings.resolution)?
    } else {
        let (mesh, warnings) = import(&path)?;
        let mut session = Session::from_mesh_with(mesh, &settings)?;
        session.set_mesh_source(&path);
        (session, warnings)
    };
    opened(server, session, warnings)
}

pub fn normal_format(s: &str) -> Res<NormalFormat> {
    match s.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
        "opengl" | "gl" => Ok(NormalFormat::OpenGl),
        "directx" | "dx" | "d3d" => Ok(NormalFormat::DirectX),
        _ => Err(format!("normal_format must be opengl or directx, got `{s}`")),
    }
}

pub fn open(server: &mut Server, args: &Args) -> Res<Reply> {
    guard_unsaved(server, args)?;
    let path = paths::input(args.req_str("path")?);
    if !path.exists() {
        return Err(format!("file not found: {}", paths::output(&path)));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let (session, warnings) = match ext.as_str() {
        "subcraft" => (Session::open_project(&path)?, Vec::new()),
        "spp" => Session::open_mesh(&path, subcraft_app::DEFAULT_RESOLUTION)?,
        _ => return Err(format!("`{}` is not a .subcraft project; use project_new to start a project from a model", paths::output(&path))),
    };
    opened(server, session, warnings)
}

pub fn save(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let path = match args.str("path")? {
        Some(p) => {
            let mut p = paths::input(p);
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case(subcraft_app::PROJECT_EXTENSION)) {
                p.set_extension(subcraft_app::PROJECT_EXTENSION);
            }
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", paths::output(dir)))?;
            }
            Some(p)
        }
        None if session.project_path().is_none() => return Err("this project has never been saved: pass `path` (a .subcraft file)".into()),
        None => None,
    };
    let saved = session.save(path.as_deref())?;
    Ok(Reply::json(json!({ "saved": paths::output(&saved), "open_in_app": "File > Open project in SubCraft, or double-click the file" })))
}

pub fn info(server: &mut Server) -> Res<Reply> {
    Ok(Reply::json(describe::project(server.session()?)))
}

pub fn texture_set_update(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let ts = server::texture_set(session, args.str("texture_set")?)?;
    // Read every argument before changing anything: a bad resolution or channel name used to
    // fail after the rename (and any earlier change) had already been made.
    let name = args.str("name")?;
    let resolution = args.u32("resolution")?;
    if let Some(r) = resolution
        && !subcraft_doc::RESOLUTIONS.contains(&r) {
            return Err(format!("resolution must be one of {:?}", subcraft_doc::RESOLUTIONS));
        }
    let all = args.bool("all_texture_sets")?.unwrap_or(false);
    let add = args.strings("add_channels")?.map(|chs| chs.iter().map(|c| channel(c)).collect::<Res<Vec<_>>>()).transpose()?;
    let remove = args.strings("remove_channels")?.map(|chs| chs.iter().map(|c| channel(c)).collect::<Res<Vec<_>>>()).transpose()?;
    for &kind in remove.iter().flatten() {
        if !server::ts_ref(session, ts)?.has_channel(kind) && !add.iter().flatten().any(|&k| k == kind) {
            return Err(format!("the texture set has no {} channel", kind.display_name()));
        }
    }
    let hidden = args.bool("hidden")?;
    exec(session, Command::SelectTextureSet(ts))?;
    if let Some(name) = name {
        exec(session, Command::RenameTextureSet { texture_set: ts, name: name.to_string() })?;
    }
    if let Some(r) = resolution {
        exec(session, Command::SetResolution { texture_set: (!all).then_some(ts), resolution: r })?;
    }
    if let Some(kinds) = add {
        exec(session, Command::AddChannels { texture_set: ts, kinds })?;
    }
    for kind in remove.into_iter().flatten() {
        if server::ts_ref(session, ts)?.has_channel(kind) {
            exec(session, Command::RemoveChannel { texture_set: ts, kind })?;
        }
    }
    if let Some(h) = hidden {
        exec(session, Command::SetTextureSetHidden { texture_set: ts, hidden: h })?;
    }
    let t = server::ts_ref(session, ts)?;
    Ok(Reply::json(describe::texture_set(session, t)))
}

pub fn undo_redo(server: &mut Server, args: &Args, undo: bool) -> Res<Reply> {
    let session = server.session()?;
    let steps = args.u32("steps")?.unwrap_or(1).max(1);
    let mut done = Vec::new();
    for _ in 0..steps {
        let label = if undo { session.history().undo_label() } else { session.history().redo_label() }.map(str::to_string);
        let Some(label) = label else { break };
        exec(session, if undo { Command::Undo } else { Command::Redo })?;
        done.push(label);
    }
    if done.is_empty() {
        return Err(format!("nothing to {}", if undo { "undo" } else { "redo" }));
    }
    Ok(Reply::json(json!({ if undo { "undone" } else { "redone" }: done, "next_undo": session.history().undo_label(), "next_redo": session.history().redo_label() })))
}

pub fn history(server: &mut Server) -> Res<Reply> {
    let session = server.session()?;
    let h = session.history();
    let entries: Vec<Value> = h.entries().iter().enumerate().map(|(i, e)| json!({ "step": i, "label": e.label, "current": i == h.cursor() })).collect();
    Ok(Reply::json(json!({ "entries": entries, "normal_format": session.settings().normal_format.map(|f| name_of(&f)) })))
}
