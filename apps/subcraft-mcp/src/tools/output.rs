//! Baking, export and pictures.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use subcraft_app::{BakeRequest, BakeSettings, Session};
use subcraft_core::{BakeMatch, BakeMatchMode, ChannelKind, MeshMapKind};
use subcraft_export::scene::SceneFormat;
use subcraft_export::{ExportSettings, ExportTemplate, Padding};
use subcraft_image::FileFormat;
use subcraft_render::ViewMode;
use subcraft_ui::orbit::CameraView;

use crate::params::{Args, Res, channel, mesh_map, name_of};
use crate::paths;
use crate::render::{self, FlatKind, Shot};
use crate::server::{self, Reply, Server};
use crate::tools::layers::find_named;
use crate::tools::paint::{camera_view, view_size};

// ----- bake ---------------------------------------------------------------------------------------

pub fn bake(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let mut settings = BakeSettings { use_gpu: false, ..BakeSettings::default() };
    if let Some(maps) = args.strings("maps")? {
        settings.maps = maps.iter().map(|m| mesh_map(m)).collect::<Res<_>>()?;
    }
    if let Some(v) = args.u32("ao_samples")? { settings.ao_samples = v.clamp(1, 1024); }
    if let Some(v) = args.f32("ao_distance")? { settings.ao_distance = v.max(1e-4); }
    if let Some(v) = args.f32("curvature_radius")? { settings.curvature_radius = v.max(1e-4); }
    if let Some(v) = args.f32("cage_front")? { settings.cage_front = v.max(0.0); }
    if let Some(v) = args.f32("cage_rear")? { settings.cage_rear = v.max(0.0); }
    if args.bool("match_by_name")?.unwrap_or(false) {
        settings.name_match = BakeMatch { mode: BakeMatchMode::ByName, ..BakeMatch::default() };
    }
    let high_poly = args.str("high_poly")?.map(paths::input);
    if let Some(h) = &high_poly {
        if !h.exists() {
            return Err(format!("high-poly file not found: {}", paths::output(h)));
        }
    }
    let wanted = match args.strings("texture_sets")? {
        Some(list) => list.iter().map(|t| server::texture_set(session, Some(t))).collect::<Res<Vec<_>>>()?,
        None => session.document().texture_sets.iter().map(|t| t.id).collect(),
    };
    let size_arg = args.u32("size")?;
    let started = Instant::now();
    let mut results = Vec::new();
    // One job per resolution group, so every set bakes at its own texture resolution.
    let mut groups: std::collections::BTreeMap<u32, Vec<(subcraft_core::TextureSetId, u32)>> = Default::default();
    for id in &wanted {
        let ts = server::ts_ref(session, *id)?;
        let size = size_arg.unwrap_or(ts.resolution).clamp(64, 4096);
        groups.entry(size).or_default().push((ts.id, ts.material_slot));
    }
    let mut notices = Vec::new();
    for (size, sets) in groups {
        let request = BakeRequest { settings: BakeSettings { size, ..settings.clone() }, high_poly: high_poly.clone(), texture_sets: sets.clone() };
        let mut job = session.start_bake(request);
        let deadline = Instant::now() + Duration::from_secs(60 * 60);
        let mut last = Instant::now();
        let outcome = loop {
            if let Some(r) = job.poll(session) {
                break r;
            }
            if Instant::now() > deadline {
                job.cancel();
                break Err("bake took longer than an hour and was cancelled".into());
            }
            if last.elapsed() > Duration::from_secs(5) {
                eprintln!("[subcraft-mcp] bake {:.0}%: {}", job.fraction() * 100.0, job.stage());
                last = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(40));
        };
        notices.extend(job.notices().iter().cloned());
        let summary = job.summary();
        outcome.map_err(|e| format!("bake failed: {e}"))?;
        results.push(json!({ "size": size, "texture_sets": sets.iter().filter_map(|(id, _)| session.document().texture_set(*id).map(|t| t.name.clone())).collect::<Vec<_>>(), "summary": summary }));
    }
    let maps: Vec<Value> = wanted.iter().filter_map(|id| session.document().texture_set(*id)).map(|ts| json!({
        "texture_set": ts.name,
        "baked_maps": session.mesh_maps(ts.id).map(|m| m.iter().map(|(k, _)| name_of(&k)).collect::<Vec<_>>()).unwrap_or_default(),
    })).collect();
    Ok(Reply::json(json!({ "seconds": started.elapsed().as_secs_f32().round(), "jobs": results, "result": maps, "notices": notices })))
}

// ----- export -------------------------------------------------------------------------------------

pub fn templates() -> Res<Reply> {
    let list: Vec<Value> = ExportTemplate::builtin().iter().map(|t| json!({
        "name": t.name,
        "maps": t.maps.iter().map(|m| json!({ "suffix": m.suffix, "format": format!("{:?}", m.format).to_lowercase() })).collect::<Vec<_>>(),
    })).collect();
    Ok(Reply::json(json!({ "templates": list })))
}

fn template(session: &Session, name: Option<&str>) -> Res<ExportTemplate> {
    let all = ExportTemplate::builtin();
    let key = name.map(str::to_string).unwrap_or_else(|| session.settings().export_template.clone());
    if key.trim().is_empty() {
        return Ok(ExportTemplate::pbr_metal_rough());
    }
    match find_named(&all, |t| &t.name, &key, "export template") {
        Ok(t) => Ok(t.clone()),
        Err(e) if name.is_none() => {
            eprintln!("[subcraft-mcp] project template `{key}` not found ({e}); using PBR Metallic Roughness");
            Ok(ExportTemplate::pbr_metal_rough())
        }
        Err(e) => Err(e),
    }
}

/// `format_key`: the argument naming the texture file format ("format" for export_textures,
/// "texture_format" for export_scene, whose "format" is the scene format).
fn export_settings(session: &Session, args: &Args, format_key: &str) -> Res<ExportSettings> {
    let mut s = ExportSettings { template: template(session, args.str("template")?)?, ..ExportSettings::default() };
    if let Some(f) = args.str(format_key)? {
        s.format_override = Some(match f.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
            "png" | "png8" => FileFormat::Png8,
            "png16" => FileFormat::Png16,
            "tga" | "tga8" => FileFormat::Tga8,
            "jpg" | "jpeg" => FileFormat::Jpeg,
            "exr" | "exr32" => FileFormat::Exr32,
            other => return Err(format!("unknown format `{other}`: png8, png16, tga8, jpeg, exr32")),
        });
    }
    match args.get("padding") {
        None => {}
        Some(Value::String(p)) if p.eq_ignore_ascii_case("infinite") => s.padding = Padding::Infinite,
        Some(Value::String(p)) if p.eq_ignore_ascii_case("none") => s.padding = Padding::None,
        Some(Value::Number(n)) => s.padding = Padding::Texels(n.as_u64().unwrap_or(8) as u32),
        Some(v) => return Err(format!("padding must be \"infinite\", \"none\" or a number, got {v}")),
    }
    if let Some(n) = args.str("normal_format")? {
        s.normal_directx = Some(crate::tools::project::normal_format(n)? == subcraft_doc::NormalFormat::DirectX);
    }
    if let Some(h) = args.f32("height_to_normal")? {
        s.height_to_normal = h.max(0.0);
    }
    if let Some(u) = args.bool("udim_folders")? {
        s.udim_folders = u;
    }
    Ok(s)
}

fn out_dir(args: &Args) -> Res<std::path::PathBuf> {
    let dir = paths::input(args.req_str("dir")?);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", paths::output(&dir)))?;
    Ok(dir)
}

/// The session applies the project's normal format and UDIM-folder choice to every export;
/// explicit arguments override them for this export only.
fn with_project_overrides<T>(session: &mut Session, settings: &ExportSettings, args: &Args, f: impl FnOnce(&mut Session) -> Res<T>) -> Res<T> {
    let saved = session.settings().clone();
    let mut tmp = saved.clone();
    if let Some(dx) = settings.normal_directx.filter(|_| args.get("normal_format").is_some()) {
        tmp.normal_format = Some(if dx { subcraft_doc::NormalFormat::DirectX } else { subcraft_doc::NormalFormat::OpenGl });
    }
    if let Some(u) = args.bool("udim_folders")? {
        tmp.udim_folders = u;
    }
    let changed = tmp != saved;
    if changed {
        session.set_settings(tmp);
    }
    let out = f(session);
    if changed {
        session.set_settings(saved);
    }
    out
}

pub fn export_textures(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let settings = export_settings(session, args, "format")?;
    let dir = out_dir(args)?;
    let files = with_project_overrides(session, &settings, args, |s| s.export_textures(&dir, &settings))?;
    Ok(Reply::json(json!({ "template": settings.template.name, "folder": paths::output(&dir), "files": files.iter().map(|f| paths::output(f)).collect::<Vec<_>>() })))
}

pub fn export_scene(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let settings = export_settings(session, args, "texture_format")?;
    let format = match args.req_str("format")?.to_ascii_lowercase().as_str() {
        "obj" | "obj_mtl" => SceneFormat::ObjMtl,
        "gltf" => SceneFormat::Gltf,
        "glb" => SceneFormat::Glb,
        "usda" | "usd" => SceneFormat::Usda,
        other => return Err(format!("format must be obj, gltf, glb or usda, got `{other}`")),
    };
    let dir = out_dir(args)?;
    let files = with_project_overrides(session, &settings, args, |s| s.export_scene(&dir, &settings, format))?;
    Ok(Reply::json(json!({ "folder": paths::output(&dir), "files": files.iter().map(|f| paths::output(f)).collect::<Vec<_>>() })))
}

// ----- pictures ------------------------------------------------------------------------------------

fn named_view(name: &str) -> Res<CameraView> {
    let (yaw, pitch) = match name.to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "front" => (0.0, 0.0),
        "back" => (180.0, 0.0),
        "right" => (90.0, 0.0),
        "left" => (-90.0, 0.0),
        "top" => (0.0, 89.0),
        "bottom" => (0.0, -89.0),
        "three_quarter" | "3/4" | "default" => (35.0, 22.0),
        other => return Err(format!("unknown view `{other}`: front, back, left, right, top, bottom, three_quarter")),
    };
    Ok(CameraView { yaw_deg: yaw, pitch_deg: pitch, ..render::default_view() })
}

fn view_mode(s: &str) -> Res<ViewMode> {
    Ok(match crate::params::snake(s).as_str() {
        "material" | "shaded" | "pbr" => ViewMode::Material,
        "base_color" | "color" | "albedo" => ViewMode::BaseColor,
        "normal" => ViewMode::Normal,
        "roughness" => ViewMode::Roughness,
        "metallic" => ViewMode::Metallic,
        "height" => ViewMode::Height,
        "ambient_occlusion" | "ao" => ViewMode::AmbientOcclusion,
        "baked_normal" => ViewMode::BakedNormal,
        "baked_normal_map" => ViewMode::BakedNormalMap,
        other => return Err(format!("unknown view `{other}`")),
    })
}

fn hex_bg(s: &str) -> Res<[f32; 3]> {
    let v = crate::params::channel_value(ChannelKind::BaseColor, &Value::String(s.into()), false)?;
    Ok(v.0)
}

fn save_png(path: &std::path::Path, png: &[u8]) -> Res<String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", paths::output(d)))?;
    }
    std::fs::write(path, png).map_err(|e| format!("{}: {e}", paths::output(path)))?;
    Ok(paths::output(path))
}

pub fn render_preview(server: &mut Server, args: &Args) -> Res<Reply> {
    let size = view_size(args)?;
    let mode = view_mode(args.str("view")?.unwrap_or("material"))?;
    let env_name = args.str("environment")?.map(str::to_string);
    let base = Shot {
        camera: camera_view(args)?,
        size,
        view_mode: mode,
        environment: 0,
        environment_rotation_deg: args.f32("environment_rotation")?.unwrap_or(0.0),
        environment_intensity: args.f32("environment_intensity")?.unwrap_or(1.0),
        exposure: args.f32("exposure")?.unwrap_or(1.0),
        background: args.str("background")?.map(hex_bg).transpose()?,
        wireframe: args.bool("wireframe")?.unwrap_or(false),
    };
    let views: Vec<(String, CameraView)> = match args.strings("views")? {
        Some(v) if !v.is_empty() => v.iter().map(|n| named_view(n).map(|c| (n.to_string(), CameraView { zoom: base.camera.zoom, offset: base.camera.offset, ..c }))).collect::<Res<_>>()?,
        _ => vec![(String::new(), base.camera)],
    };
    if views.len() as u64 * size[0] as u64 * size[1] as u64 > 6 * 1024 * 1024 {
        return Err("too many pixels in one call: use fewer views or a smaller size".into());
    }
    let save = args.str("save_path")?.map(paths::input);
    let (session, library, gpu) = server.render_parts()?;
    let environment = match &env_name {
        Some(n) => library.environments.iter().position(|e| e.name.eq_ignore_ascii_case(n.trim()))
            .or_else(|| library.environments.iter().position(|e| e.name.to_lowercase().contains(&n.trim().to_lowercase())))
            .ok_or_else(|| format!("no environment `{n}`: {}", library.environments.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")))?,
        None => 0,
    };
    let adapter = gpu.adapter.clone();
    let mut reply = Reply::default();
    let mut saved = Vec::new();
    for (name, cam) in &views {
        let shot = Shot { camera: *cam, environment, ..base_clone(&base) };
        let started = Instant::now();
        let pixels = render::shaded(gpu, session, library, &shot)?;
        let png = render::png(&pixels, size)?;
        eprintln!("[subcraft-mcp] rendered {name} {}x{} in {:.1}s", size[0], size[1], started.elapsed().as_secs_f32());
        if let Some(p) = &save {
            let target = if views.len() > 1 { with_suffix(p, name) } else { p.clone() };
            saved.push(save_png(&target, &png)?);
        }
        reply.push_png(&png);
    }
    reply.push_json(json!({
        "views": views.iter().map(|(n, c)| json!({ "name": if n.is_empty() { None } else { Some(n) }, "yaw": c.yaw_deg, "pitch": c.pitch_deg, "zoom": c.zoom })).collect::<Vec<_>>(),
        "size": size,
        "environment": library.environments.get(environment).map(|e| e.name.clone()),
        "adapter": adapter,
        "saved": saved,
        "tip": "paint_stroke / pick with space \"view\" and the same yaw, pitch, zoom, width, height map pixels in this image to the model",
    }));
    Ok(reply)
}

fn base_clone(s: &Shot) -> Shot {
    Shot { camera: s.camera, size: s.size, view_mode: s.view_mode, environment: s.environment, environment_rotation_deg: s.environment_rotation_deg, environment_intensity: s.environment_intensity, exposure: s.exposure, background: s.background, wireframe: s.wireframe }
}

fn with_suffix(p: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("render");
    p.with_file_name(format!("{stem}_{suffix}.png"))
}

pub fn render_texture(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let ts = server::texture_set(session, args.str("texture_set")?)?;
    let max = args.u32("max_size")?.unwrap_or(1024).clamp(16, 4096);
    session.update_composites();
    let (img, kind, label) = if let Some(m) = args.str("mesh_map")? {
        let k: MeshMapKind = mesh_map(m)?;
        let img = session.mesh_maps(ts).and_then(|maps| maps.get(k)).ok_or_else(|| format!("no baked {} map for this texture set: bake_mesh_maps first", name_of(&k)))?.clone();
        (img, FlatKind::Data, format!("mesh map {}", name_of(&k)))
    } else {
        let k = channel(args.str("channel")?.unwrap_or("base_color"))?;
        let c = session.composite(ts).ok_or("no composite yet")?;
        let img = c.channel(k).ok_or_else(|| format!("the texture set has no {} channel", k.display_name()))?.clone();
        (img, FlatKind::for_channel(k), format!("channel {}", name_of(&k)))
    };
    let (pixels, size) = render::flat(&img, kind, max);
    let png = render::png(&pixels, size)?;
    let mut reply = Reply::default();
    reply.push_png(&png);
    let saved = args.str("save_path")?.map(|p| save_png(&paths::input(p), &png)).transpose()?;
    let name = server::ts_ref(session, ts)?.name.clone();
    reply.push_json(json!({ "texture_set": name, "shows": label, "source_resolution": [img.width(), img.height()], "image_size": size, "orientation": "UV space, origin top-left (u right, v down)", "saved": saved }));
    Ok(reply)
}
