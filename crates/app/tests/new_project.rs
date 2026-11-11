//! New Project settings -> project: templates, resolution, split rules, UDIM tiles, summaries.
use glam::{Vec2, Vec3};
use subcraft_app::new_project::{NewProjectSettings, ProjectTemplate, SplitRule, plan_sets, summarize};
use subcraft_app::Session;
use subcraft_core::ChannelKind as K;
use subcraft_doc::NormalFormat;
use subcraft_mesh::{Corner, Mesh, MeshBuilder, primitives};

/// A quad at `origin` whose UVs fill the unit square shifted by `uv_offset` (stored UV space).
fn quad(b: &mut MeshBuilder, material: u32, origin: Vec3, uv_offset: Vec2) {
    let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(uv_offset + Vec2::new(x, z)) };
    b.push_triangle(material, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
    b.push_triangle(material, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
}

/// Two objects (Hull, Wing) over two materials (Metal, Glass); Hull = Metal + Glass, Wing = Metal.
fn two_objects() -> Mesh {
    let mut b = MeshBuilder::new("Ship");
    let (metal, glass) = (b.material("Metal"), b.material("Glass"));
    b.object("Hull");
    quad(&mut b, metal, Vec3::ZERO, Vec2::ZERO);
    quad(&mut b, glass, Vec3::new(2.0, 0.0, 0.0), Vec2::ZERO);
    b.object("Wing");
    quad(&mut b, metal, Vec3::new(4.0, 0.0, 0.0), Vec2::ZERO);
    b.build().unwrap()
}

/// One material over three UDIM tiles: 1001, 1002 (u 1..2) and 1011 (one tile up = v -1..0).
fn udim_mesh() -> Mesh {
    let mut b = MeshBuilder::new("Tiled");
    let m = b.material("Body");
    quad(&mut b, m, Vec3::ZERO, Vec2::ZERO);
    quad(&mut b, m, Vec3::new(2.0, 0.0, 0.0), Vec2::new(1.0, 0.0));
    quad(&mut b, m, Vec3::new(4.0, 0.0, 0.0), Vec2::new(0.0, -1.0));
    b.build().unwrap()
}

fn names(session: &Session) -> Vec<String> {
    session.document().texture_sets.iter().map(|t| t.name.clone()).collect()
}

#[test]
fn defaults_match_the_pbr_template_at_2048_and_build_a_ready_to_paint_project() {
    let settings = NewProjectSettings::default();
    assert_eq!(settings.template, ProjectTemplate::PbrMetalRough);
    assert_eq!(settings.resolution, 2048);
    assert_eq!(settings.split, SplitRule::ByMaterial);
    assert_eq!(settings.normal_format, NormalFormat::OpenGl);
    assert!(settings.high_poly.is_none());
    let s = Session::from_mesh_with(primitives::cube(2.0), &settings).unwrap();
    let set = &s.document().texture_sets[0];
    assert_eq!(set.resolution, 2048);
    let kinds: Vec<K> = set.channels.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [K::BaseColor, K::Height, K::Roughness, K::Metallic, K::Normal]);
    assert_eq!(set.layer_count(), 1, "an empty paint layer is selected, like Session::new");
    assert!(s.selected_layer().is_some());
    assert_eq!(s.settings().normal_format, Some(NormalFormat::OpenGl));
    assert_eq!(s.settings().export_template, "PBR Metallic Roughness");
}

#[test]
fn every_template_gives_its_channels_normal_format_and_export_template() {
    let want = [
        (ProjectTemplate::PbrMetalRough, 5, NormalFormat::OpenGl, false, "PBR Metallic Roughness"),
        (ProjectTemplate::UnrealOrm, 6, NormalFormat::DirectX, false, "Unreal Engine (ORM packed)"),
        (ProjectTemplate::Unity, 7, NormalFormat::OpenGl, true, "Unity URP (Metallic + Occlusion)"),
        (ProjectTemplate::Godot, 7, NormalFormat::OpenGl, true, "Godot 4 (ORM)"),
        (ProjectTemplate::Stylized, 1, NormalFormat::OpenGl, false, "PBR Metallic Roughness"),
    ];
    for (template, count, format, emissive, export) in want {
        let mut settings = NewProjectSettings::default();
        settings.apply_template(template);
        let s = Session::from_mesh_with(primitives::cube(2.0), &settings).unwrap();
        let set = &s.document().texture_sets[0];
        assert_eq!(set.channels.len(), count, "{template:?}");
        assert_eq!(set.has_channel(K::Emissive), emissive, "{template:?}");
        assert_eq!(set.has_channel(K::AmbientOcclusion), !matches!(template, ProjectTemplate::PbrMetalRough | ProjectTemplate::Stylized), "{template:?}");
        assert_eq!(s.settings().normal_format, Some(format), "{template:?}");
        assert_eq!(s.settings().export_template, export, "{template:?}");
        // Every template's paint layer knows every channel (so painting and fills work at once).
        let layer = s.selected_layer().unwrap();
        assert!(set.channels.iter().all(|c| layer.blend_for(c.kind).is_active()), "{template:?}");
    }
    // The export template names are real templates.
    let known: Vec<String> = subcraft_app::ExportTemplate::builtin().into_iter().map(|t| t.name).collect();
    for template in ProjectTemplate::ALL {
        assert!(known.contains(&template.export_template().to_string()), "{template:?}");
    }
}

#[test]
fn channels_can_be_edited_but_base_color_stays_and_emission_can_start_enabled() {
    let mut settings = NewProjectSettings::default();
    settings.set_channel(K::Emissive, true);
    settings.set_channel(K::Height, false);
    settings.set_channel(K::BaseColor, false);
    assert!(settings.has_channel(K::Emissive) && !settings.has_channel(K::Height) && settings.has_channel(K::BaseColor));
    let kinds: Vec<K> = settings.channels.clone();
    let mut sorted = kinds.clone();
    sorted.sort_by_key(|k| K::ALL.iter().position(|a| a == k));
    assert_eq!(kinds, sorted, "channels keep the standard order");
    let s = Session::from_mesh_with(primitives::cube(2.0), &settings).unwrap();
    assert!(s.document().texture_sets[0].has_channel(K::Emissive));
    settings.channels.clear();
    assert!(Session::from_mesh_with(primitives::cube(2.0), &settings).is_err());
}

#[test]
fn new_sessions_and_projects_default_to_2048_and_1024_stays_selectable() {
    assert_eq!(subcraft_app::DEFAULT_RESOLUTION, 2048);
    assert_eq!(NewProjectSettings::default().resolution, 2048);
    assert!(subcraft_app::new_project::PROJECT_RESOLUTIONS.contains(&1024));
    assert!(subcraft_doc::RESOLUTIONS.contains(&1024));
    let s = Session::sample();
    assert!(s.document().texture_sets.iter().all(|t| t.resolution == 2048));
}

#[test]
fn resolution_is_per_project_with_per_set_overrides_snapped_to_supported_sizes() {
    let mut settings = NewProjectSettings { resolution: 1024, ..Default::default() };
    settings.resolution_overrides.insert("Glass".into(), 512);
    settings.resolution_overrides.insert("Metal".into(), 3000);
    let s = Session::from_mesh_with(two_objects(), &settings).unwrap();
    let by_name: std::collections::BTreeMap<_, _> = s.document().texture_sets.iter().map(|t| (t.name.clone(), t.resolution)).collect();
    assert_eq!(by_name["Glass"], 512);
    assert_eq!(by_name["Metal"], 2048, "3000 snaps to the nearest supported size");
    let plain = Session::from_mesh_with(two_objects(), &NewProjectSettings { resolution: 256, ..Default::default() }).unwrap();
    assert!(plain.document().texture_sets.iter().all(|t| t.resolution == 256));
}

#[test]
fn split_rules_group_triangles_into_texture_sets_without_losing_any() {
    let mesh = two_objects();
    let total = mesh.triangle_count();
    let make = |split| {
        let settings = NewProjectSettings { split, ..Default::default() };
        let plans = plan_sets(&mesh, &settings);
        let s = Session::from_mesh_with(mesh.clone(), &settings).unwrap();
        let tris: usize = (0..s.scene().mesh.materials.len() as u32).map(|m| s.scene().mesh.material_triangles(m).len()).sum();
        assert_eq!(tris, total, "{split:?} keeps every triangle");
        assert!(s.scene().mesh.validate().is_empty());
        assert_eq!(plans.iter().map(|p| p.triangles).sum::<usize>(), total);
        names(&s)
    };
    assert_eq!(make(SplitRule::ByMaterial), ["Metal", "Glass"]);
    assert_eq!(make(SplitRule::ByObject), ["Hull", "Wing"]);
    assert_eq!(make(SplitRule::Single), ["Ship"]);
    // By material keeps an untouched mesh as it was imported (no rebuild).
    let s = Session::from_mesh_with(mesh.clone(), &NewProjectSettings::default()).unwrap();
    assert_eq!(s.scene().mesh.indices, mesh.indices);
    assert_eq!(s.scene().mesh.positions.len(), mesh.positions.len());
}

#[test]
fn udim_tiles_become_texture_sets_inside_the_unit_square_when_enabled() {
    let mesh = udim_mesh();
    let tris = mesh.triangle_count();
    let on = Session::from_mesh_with(mesh.clone(), &NewProjectSettings { udim: true, ..Default::default() }).unwrap();
    assert_eq!(names(&on), ["Body.1001", "Body.1002", "Body.1011"]);
    let tiles: Vec<_> = on.document().texture_sets.iter().map(|t| (t.udim_tile, t.udim_number(), t.base_name().to_string())).collect();
    assert_eq!(tiles, [(Some([0, 0]), Some(1001), "Body".to_string()), (Some([1, 0]), Some(1002), "Body".to_string()), (Some([0, -1]), Some(1011), "Body".to_string())]);
    let m = &on.scene().mesh;
    assert!(m.validate().is_empty());
    assert_eq!(m.triangle_count(), tris);
    let (lo, hi) = m.uv_bounds().unwrap();
    assert!(lo.min_element() >= -1e-5 && hi.max_element() <= 1.0 + 1e-5, "all tiles moved to 0..1: {lo:?} {hi:?}");
    assert!((0..3u32).all(|slot| m.material_triangles(slot).len() == 2));

    // Off: the mesh is used as it is (folded UVs), one set.
    let off = Session::from_mesh_with(mesh.clone(), &NewProjectSettings { udim: false, ..Default::default() }).unwrap();
    assert_eq!(names(&off), ["Body"]);
    assert!(off.document().texture_sets[0].udim_tile.is_none());
    // A single-tile model is never split, even with the option on.
    let flat = Session::from_mesh_with(primitives::plane(2.0, 1), &NewProjectSettings::default()).unwrap();
    assert_eq!(flat.document().texture_sets.len(), 1);
    assert!(flat.document().texture_sets[0].udim_tile.is_none());
}

#[test]
fn udim_tiles_share_the_material_resolution_and_save_reopen_keep_everything() {
    let mut settings = NewProjectSettings { resolution: 256, ..Default::default() };
    settings.resolution_overrides.insert("Body".into(), 512);
    let mut s = Session::from_mesh_with(udim_mesh(), &settings).unwrap();
    let path = std::env::temp_dir().join(format!("subcraft-udim-{}.subcraft", std::process::id()));
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    let sets = &reopened.document().texture_sets;
    assert_eq!(sets.iter().map(|t| t.udim_tile).collect::<Vec<_>>(), [Some([0, 0]), Some([1, 0]), Some([0, -1])]);
    assert!(sets.iter().all(|t| t.resolution == 512), "one resolution per material, whatever the tile");
    assert_eq!(reopened.settings(), s.settings());
    assert_eq!(reopened.settings().normal_format, Some(NormalFormat::OpenGl));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_model_without_uvs_is_refused_and_explained() {
    let mut b = MeshBuilder::new("Bare");
    let m = b.material("M");
    let c = |x: f32, z: f32| Corner { position: Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: None };
    b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
    let mesh = b.build_for_baking().unwrap();
    let summary = summarize(&mesh, &[]);
    assert!(!summary.has_uvs && !summary.paintable());
    assert!(summary.warnings.iter().any(|w| w.contains("no UV")), "{:?}", summary.warnings);
    assert!(Session::from_mesh_with(mesh, &NewProjectSettings::default()).err().is_some_and(|e| e.contains("UV")));
}

#[test]
fn the_summary_lists_counts_objects_materials_tiles_and_warnings() {
    let s = summarize(&two_objects(), &["imported with a warning".to_string()]);
    assert_eq!((s.triangles, s.name.as_str()), (6, "Ship"));
    assert_eq!(s.objects, [("Hull".to_string(), 4), ("Wing".to_string(), 2)]);
    assert_eq!(s.materials, [("Metal".to_string(), 4), ("Glass".to_string(), 2)]);
    assert!(s.paintable() && !s.uses_udim());
    assert_eq!(s.tiles.len(), 1);
    assert_eq!(s.warnings[0], "imported with a warning");
    // Hull's metal quad and Wing's metal quad share the same UVs: overlapping islands are reported.
    assert!(s.overlap > 0.5, "{}", s.overlap);
    assert!(s.warnings.iter().any(|w| w.contains("more than one triangle")), "{:?}", s.warnings);

    let t = summarize(&udim_mesh(), &[]);
    assert!(t.uses_udim());
    assert_eq!(t.tiles.iter().map(|x| (x.number, x.triangles)).collect::<Vec<_>>(), [(1001, 2), (1002, 2), (1011, 2)]);
    assert!(t.warnings.iter().any(|w| w.contains("1001, 1002, 1011")), "{:?}", t.warnings);
    assert!(t.overlap < 0.02, "tiles do not overlap each other: {}", t.overlap);
    let (lo, hi) = t.uv_bounds.unwrap();
    assert!(lo.y < 0.0 && hi.x > 1.0);
}

#[test]
fn the_last_choices_round_trip_and_old_or_partial_data_gets_defaults() {
    let mut settings = NewProjectSettings::default();
    settings.apply_template(ProjectTemplate::Godot);
    settings.resolution = 4096;
    settings.split = SplitRule::ByObject;
    settings.udim = false;
    settings.resolution_overrides.insert("Hull".into(), 1024);
    settings.high_poly = Some("C:/models/high.obj".into());
    let json = serde_json::to_string(&settings).unwrap();
    assert_eq!(serde_json::from_str::<NewProjectSettings>(&json).unwrap(), settings);
    assert_eq!(serde_json::from_str::<NewProjectSettings>("{}").unwrap(), NewProjectSettings::default());
    let partial: NewProjectSettings = serde_json::from_str(r#"{"resolution":512,"split":"single"}"#).unwrap();
    assert_eq!((partial.resolution, partial.split, partial.template), (512, SplitRule::Single, ProjectTemplate::PbrMetalRough));
}

/// QA 2026-11: "A", "A", "A 2" became "A", "A 2", "A 2": two texture sets with one name.
#[test]
fn numbered_duplicate_names_do_not_collide_with_given_names() {
    let mut b = MeshBuilder::new("Model");
    let m = b.material("Mat");
    for (i, name) in ["A", "A", "A 2", "A"].into_iter().enumerate() {
        b.object(name);
        quad(&mut b, m, Vec3::new(2.0 * i as f32, 0.0, 0.0), Vec2::ZERO);
    }
    let mesh = b.build().unwrap();
    let names: Vec<String> = plan_sets(&mesh, &NewProjectSettings { split: SplitRule::ByObject, ..Default::default() }).into_iter().map(|p| p.name).collect();
    let unique: std::collections::BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    assert!(names.contains(&"A".to_string()) && names.contains(&"A 2".to_string()), "given names are kept: {names:?}");
}

/// QA 2026-11: a project made without "one texture set per UDIM tile" has one set per material;
/// reloading its mesh only tried the per-tile names, matched nothing and stranded all the paint.
#[test]
fn reloading_a_project_made_without_udim_sets_matches_its_set() {
    let mesh = udim_mesh();
    let s = Session::from_mesh_with(mesh.clone(), &NewProjectSettings { udim: false, ..Default::default() }).unwrap();
    assert_eq!(names(&s), ["Body"]);
    let prepared = s.check_mesh_reload(std::path::Path::new("tiled.obj"), mesh).unwrap();
    assert_eq!(prepared.matched(), 1, "added {:?}, unmatched {:?}", prepared.added(), prepared.unmatched_old);
    assert!(prepared.added().is_empty() && prepared.unmatched_old.is_empty());
}

/// QA 2026-11: a single-set project is named after the model file, so reloading the edited
/// "ship_v2" file matched nothing: new empty sets, and the painted set left without geometry.
#[test]
fn reloading_a_renamed_file_keeps_a_single_set_project_together() {
    let mesh = two_objects();
    let s = Session::from_mesh_with(mesh.clone(), &NewProjectSettings { split: SplitRule::Single, ..Default::default() }).unwrap();
    assert_eq!(names(&s), ["Ship"]);
    let mut v2 = mesh;
    v2.name = "Ship_v2".into();
    let prepared = s.check_mesh_reload(std::path::Path::new("Ship_v2.obj"), v2).unwrap();
    assert_eq!(prepared.matched(), 1, "added {:?}, unmatched {:?}", prepared.added(), prepared.unmatched_old);
    assert!(prepared.added().is_empty() && prepared.unmatched_old.is_empty());
}
