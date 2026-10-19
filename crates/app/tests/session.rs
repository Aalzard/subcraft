//! Session behaviour: commands + history, stroke lifecycle, composites, actions.

use glam::{Vec2, Vec3};
use subcraft_app::{Action, Camera, Command, CommandError, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{BlendMode, FillChannel, FillSource, LayerContent, LayerKind};
use subcraft_mesh::primitives;

fn session() -> Session {
    Session::new(primitives::sample_crate(), 256)
}

#[test]
fn browser_archive_matches_the_export_plan_for_all_texture_sets() {
    let mut s=session();let settings=subcraft_export::ExportSettings::default();let plan=s.export_plan(&settings);let result=s.export_archive(&settings).unwrap();assert_eq!(result.files,plan.len());let zip=zip::ZipArchive::new(std::io::Cursor::new(result.bytes)).unwrap();let names:Vec<_>=zip.file_names().map(|n|n.unwrap().into_owned()).collect();for (_,name) in plan {assert!(names.contains(&name));}
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(1.5, 1.2, 3.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [800, 600])
}

#[test]
fn new_session_has_a_paint_layer_per_texture_set() {
    let s = session();
    let doc = s.document();
    assert_eq!(doc.texture_sets.len(), 2);
    assert_eq!(doc.texture_sets[0].name, "Body");
    for ts in &doc.texture_sets {
        assert_eq!(ts.layers.len(), 1);
        assert_eq!(ts.layers[0].kind(), LayerKind::Paint);
    }
    assert_eq!(s.selected_layer().unwrap().name, "Layer 1");
    assert!(!s.history().can_undo());
}

#[test]
fn add_layer_undo_redo_and_selection() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Fill);
    assert_eq!(s.selected_texture_set().layers.len(), 2);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert_ne!(s.selection().layer, Some(fill), "selection falls back to an existing layer");
    s.execute(Command::Redo).unwrap();
    assert!(s.selected_texture_set().find(fill).is_some());
    assert_eq!(s.history().undo_label(), Some("New Fill Layer"));
}

#[test]
fn opacity_drag_coalesces_until_sealed() {
    let mut s = session();
    let layer = s.selection().layer.unwrap();
    for o in [0.9, 0.7, 0.5] {
        s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity: o }).unwrap();
    }
    assert_eq!(s.history().entries().len(), 2);
    s.execute(Command::Seal).unwrap();
    s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity: 0.2 }).unwrap();
    assert_eq!(s.history().entries().len(), 3);
    assert_eq!(s.selected_layer().unwrap().blend_for(ChannelKind::BaseColor).opacity, 0.2);
}

#[test]
fn layer_edits_and_errors() {
    let mut s = session();
    let base = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::MoveDown(top)).unwrap();
    assert_eq!(s.selected_texture_set().layers[0].id, top);
    assert!(s.execute(Command::MoveDown(top)).is_err(), "already at bottom");
    s.execute(Command::SetBlendMode { layer: base, channel: ChannelKind::BaseColor, mode: BlendMode::Multiply }).unwrap();
    s.execute(Command::SetVisible { layer: base, visible: false }).unwrap();
    s.execute(Command::Rename { layer: base, name: "Dirt".into() }).unwrap();
    let l = s.selected_texture_set().find(base).unwrap();
    assert_eq!((l.name.as_str(), l.visible, l.blend_for(ChannelKind::BaseColor).mode), ("Dirt", false, BlendMode::Multiply));
    assert_eq!(
        s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.1))) }),
        Err(CommandError::NotAFillLayer)
    );
    s.execute(Command::AddMask { layer: base, white: false }).unwrap();
    assert!(s.selected_texture_set().find(base).unwrap().mask.is_some());
    s.execute(Command::DeleteLayer(top)).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert_eq!(s.selection().layer, Some(base));
}

#[test]
fn stroke_is_one_history_entry_and_updates_the_composite() {
    let mut s = session();
    let first = s.update_composites();
    assert_eq!(first.len(), 2, "both texture sets evaluated initially");
    assert!(s.update_composites().is_empty());

    let cam = camera();
    let center = cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate();
    s.brush.settings.size = 10.0;
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0));
    s.begin_stroke().unwrap();
    assert!(s.is_stroking());
    assert_eq!(s.execute(Command::AddLayer(NewLayer::Paint)), Err(CommandError::StrokeInProgress));
    let mut painted = 0;
    for k in 0..10 {
        painted += s.stroke_to(&cam, center + Vec2::new(k as f32 * 8.0, 0.0), false).unwrap().len();
    }
    assert!(painted > 0);
    s.end_stroke();
    assert_eq!(s.history().entries().len(), 2);
    assert_eq!(s.history().undo_label(), Some("Paint Stroke"));

    let updates = s.update_composites();
    assert_eq!(updates.len(), 1, "only the Body texture set changed");
    assert!(!updates[0].tiles.is_empty() && updates[0].tiles.len() < 16, "{:?}", updates[0].tiles);
    let body = s.document().texture_sets[0].id;
    let c = s.composite(body).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    s.execute(Command::Undo).unwrap();
    s.update_composites();
    assert_ne!(&c, s.composite(body).unwrap().channel(ChannelKind::BaseColor).unwrap());
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    assert!(p.channels.is_empty(), "undo removes the stroke");
}

#[test]
fn empty_stroke_adds_no_history_and_cancel_restores() {
    let mut s = session();
    s.begin_stroke().unwrap();
    s.stroke_to(&camera(), Vec2::new(1.0, 1.0), false).unwrap(); // misses the mesh
    s.end_stroke();
    assert_eq!(s.history().entries().len(), 1);

    let cam = camera();
    let p = cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate();
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, p, false).unwrap();
    assert_ne!(s.document(), s.history().current());
    s.cancel_stroke();
    assert_eq!(s.document(), s.history().current());
}

#[test]
fn actions_drive_the_session() {
    let mut s = session();
    let size = s.brush.settings.size;
    assert!(s.perform(Action::BrushLarger).unwrap());
    assert!(s.brush.settings.size > size);
    s.perform(Action::NewFillLayer).unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Fill);
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert!(!s.perform(Action::ResetCamera).unwrap(), "camera actions belong to the front end");
    s.perform(Action::ToggleSymmetryX).unwrap();
    assert!(s.brush.options.symmetry[0]);
}

#[test]
fn pick_reports_material_slot() {
    let s = session();
    let cam = camera();
    let p = s.pick(&cam, cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate()).unwrap();
    assert_eq!(p.material_slot, 0);
    assert!((p.position.z - 0.5).abs() < 1e-3);
    assert!(s.pick(&cam, Vec2::new(1.0, 1.0)).is_none());
}

#[test]
fn save_and_reopen_project() {
    let dir = std::env::temp_dir().join(format!("subcraft-app-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.subcraft");
    let mut s = session();
    assert!(!s.is_modified());
    assert!(s.save(None).is_err(), "no path yet");
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let cam = camera();
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate(), false).unwrap();
    s.end_stroke();
    assert!(s.is_modified());
    s.save(Some(&path)).unwrap();
    assert!(!s.is_modified());
    assert_eq!(s.project_path(), Some(path.as_path()));

    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    assert!(!reopened.is_modified());
    assert!(!reopened.history().can_undo(), "fresh history after open");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_writes_maps_for_every_texture_set() {
    let mut s = session();
    let settings = subcraft_app::ExportSettings::default();
    let plan = s.export_plan(&settings);
    // Body + Trim, each BaseColor/Roughness/Metallic/Normal/Height.
    assert_eq!(plan.len(), 10, "{plan:?}");
    let dir = std::env::temp_dir().join(format!("subcraft-app-export-{}", std::process::id()));
    let files = s.export_textures(&dir, &settings).unwrap();
    assert_eq!(files.len(), 10);
    assert!(files.iter().all(|f| f.exists()));
    assert!(files[0].file_name().unwrap().to_str().unwrap().starts_with("SampleCrate_Body_"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn background_bake_feeds_smart_mask_generators_and_survives_save() {
    use subcraft_app::{BakeRequest, BakeSettings, MeshMapKind};
    use subcraft_doc::{Generator, GeneratorKind, MaskEffectKind};
    let mut s = Session::new(primitives::cube(2.0), 128);
    let ts = s.document().texture_sets[0].id;
    // A white fill shown only where the Edges generator (baked curvature) says so.
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel {
        layer: fill,
        channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))),
    })
    .unwrap();
    s.execute(Command::AddMask { layer: fill, white: false }).unwrap();
    let mut g = Generator::new(GeneratorKind::Edges);
    g.level = 0.6;
    g.softness = 0.05;
    s.execute(Command::AddMaskEffect { layer: fill, kind: MaskEffectKind::Generator(g) }).unwrap();
    s.update_composites();
    let before = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();

    let req = BakeRequest {
        settings: BakeSettings { size: 128, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ao_samples: 8, ..BakeSettings::default() },
        high_poly: None,
        texture_sets: vec![(ts, 0)],
    };
    let mut job = s.start_bake(req);
    let result = loop {
        if let Some(r) = job.poll(&mut s) {
            break r;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(result, Ok(1));
    assert_eq!(s.mesh_maps(ts).unwrap().len(), 2);
    assert!(s.is_modified(), "baking marks the project modified");
    let updates = s.update_composites();
    assert!(!updates.is_empty(), "new mesh maps re-evaluate the composite");
    let after = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap();
    assert_ne!(&before, after, "Edges generator now uses real curvature");
    // Some texels became white (edges), the face centers did not.
    let dense = after.to_dense();
    let white = dense.chunks_exact(4).filter(|p| p[0] > 0.95).count();
    let gray = dense.chunks_exact(4).filter(|p| (p[0] - 0.5).abs() < 0.02).count();
    assert!(white > 100 && gray > white, "edges white ({white}), faces untouched ({gray})");

    let dir = std::env::temp_dir().join(format!("subcraft-bake-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("baked.subcraft");
    s.save(Some(&path)).unwrap();
    assert!(!s.is_modified());
    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.mesh_maps(ts).unwrap().len(), 2, "baked maps saved in the project");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn shelf_assets_become_undoable_layers_and_masks() {
    use subcraft_app::assets::Library;
    let lib = Library::builtin();
    let mut s = session();
    let gold = lib.materials.iter().find(|m| m.name == "Gold").unwrap();
    s.add_material(gold).unwrap();
    let l = s.selected_layer().unwrap();
    assert_eq!((l.name.as_str(), l.kind()), ("Gold", LayerKind::Fill));
    assert_eq!(s.history().undo_label(), Some("Material: Gold"));

    let smart = &lib.smart_materials[0];
    s.add_smart_material(smart).unwrap();
    let folder = s.selected_layer().unwrap();
    assert!(folder.is_folder() && folder.children().len() >= 2);
    assert_eq!(s.selected_texture_set().layers.len(), 3);

    s.execute(Command::SelectLayer(Some(s.selected_texture_set().layers[1].id))).unwrap();
    let edge = lib.smart_masks.iter().find(|m| m.name == "Edge Wear").unwrap();
    s.apply_smart_mask(edge).unwrap();
    let mask = s.selected_layer().unwrap().mask.as_ref().unwrap();
    assert_eq!(mask.effects.len(), 1);
    assert_eq!(mask.base.fill()[0], 0.0, "smart masks start from a black mask");
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none());

    // Brush alpha is used by strokes.
    s.set_brush_alpha(&lib.alphas[1]);
    assert_eq!(s.brush.alpha_name, "Square");
    assert!(s.brush.alpha.is_some());
    s.set_brush_alpha(&lib.alphas[0]);
    assert!(s.brush.alpha.is_none());
}

#[test]
fn thumbnail_scenes_composite_presets() {
    use subcraft_app::assets::Library;
    use subcraft_app::thumbnails::PreviewScene;
    let lib = Library::builtin();
    let sphere = PreviewScene::sphere();
    let gold = lib.materials.iter().find(|m| m.name == "Gold").unwrap();
    let c = sphere.composite(gold.build(sphere.texture_set()));
    let base = c.channel(ChannelKind::BaseColor).unwrap().get(64, 64);
    assert!((base[0] - 1.0).abs() < 1e-2 && base[2] < 0.4, "gold base color: {base:?}");
    assert!((c.channel(ChannelKind::Metallic).unwrap().get(64, 64)[0] - 1.0).abs() < 1e-3);
    // The smart-material cube has baked maps, so edge wear differs from face centers.
    let cube = PreviewScene::cube();
    let worn = lib.smart_materials.iter().find(|m| m.name == "Worn Painted Metal").unwrap();
    let c = cube.composite(worn.build(cube.texture_set()));
    let metal = c.channel(ChannelKind::Metallic).unwrap().to_dense();
    let bare = metal.iter().filter(|&&m| m > 0.9).count();
    let painted = metal.iter().filter(|&&m| m < 0.1).count();
    assert!(bare > 100 && painted > bare, "edges show bare steel ({bare}), faces are painted ({painted})");
}

#[test]
fn saved_smart_material_thumbnail_uses_its_images() {
    use std::sync::Arc;
    use subcraft_app::assets::smart::SmartMaterial;
    use subcraft_app::thumbnails::PreviewScene;
    use subcraft_doc::{Layer, Resource, ResourceKind};
    let red = subcraft_image::Bitmap::new_f32(4, 4, 3, [0.8f32, 0.1, 0.1].repeat(16));
    let image = Arc::new(Resource {
        id: subcraft_core::ResourceId::new(),
        name: "red.png".into(),
        kind: ResourceKind::Texture,
        bitmap: Arc::new(red),
        source_path: None,
        encoded: None,
    });
    let scene = PreviewScene::baked_sphere();
    let mut layer = Layer::new_fill("Bricks", scene.texture_set());
    let LayerContent::Fill(f) = &mut layer.content else { panic!("fill layer") };
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::Bitmap { resource: image.id }));
    let smart = SmartMaterial::saved("Bricks".into(), layer, 128, vec![image], None);

    let with = scene.composite_with(smart.build(scene.texture_set()), smart.resources());
    let base = with.channel(ChannelKind::BaseColor).unwrap().get(64, 64);
    assert!(base[0] > 0.4 && base[1] < 0.2, "the saved image shows: {base:?}");
    let without = scene.composite(smart.build(scene.texture_set()));
    assert_ne!(without.channel(ChannelKind::BaseColor).unwrap().get(64, 64), base, "without its images it can't");
}

#[test]
fn changing_texture_set_resolution_resamples_paint_in_one_undo_step() {
    use subcraft_doc::{Layer, TextureSet};
    use subcraft_image::TiledImage;
    let mut s = Session::new(primitives::sample_crate(), 128);
    let ts = s.selected_texture_set().id;
    // A paint layer with a red square in the left half.
    let mut layer = Layer::new_paint("Paint", &TextureSet::new("t", 0, 128));
    let mut red = TiledImage::new(128, 128, 4, [0.0; 4]);
    for y in 0..128 {
        for x in 0..64 {
            red.set(x, y, [1.0, 0.0, 0.0, 1.0]);
        }
    }
    let LayerContent::Paint(p) = &mut layer.content else { panic!("paint layer") };
    p.channels.insert(ChannelKind::BaseColor, red);
    s.execute(Command::InsertLayer { layer: Box::new(layer), label: "paint".into() }).unwrap();
    let before = s.document().clone();
    let entries = s.history().entries().len();

    s.execute(Command::SetResolution { texture_set: Some(ts), resolution: 512 }).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1);
    assert_eq!(s.selected_texture_set().resolution, 512);
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("paint layer") };
    let img = &p.channels[&ChannelKind::BaseColor];
    assert_eq!((img.width(), img.height()), (512, 512), "rasters follow the texture set");
    assert!(img.get(64, 256)[3] > 0.99 && img.get(448, 256)[3] < 0.01, "the stroke stays in the left half");
    s.update_composites();
    assert_eq!(s.composite(ts).unwrap().resolution, 512, "composite rebuilt at the new size");

    // Other texture sets keep theirs; "all" changes every set in one step.
    let other = s.document().texture_sets.iter().find(|t| t.id != ts).map(|t| t.resolution);
    assert_eq!(other, Some(128));
    s.execute(Command::SetResolution { texture_set: None, resolution: 256 }).unwrap();
    assert!(s.document().texture_sets.iter().all(|t| t.resolution == 256));

    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before, "undo restores the exact rasters");
    assert!(s.execute(Command::SetResolution { texture_set: Some(ts), resolution: 300 }).is_err(), "only supported sizes");
}

/// Interactive updates composite on the background worker; synchronous updates (export, bake,
/// save, tests) still see the exact composite of the newest document.
#[test]
fn background_compositing_converges_and_sync_updates_are_exact() {
    use std::time::{Duration, Instant};
    use subcraft_doc::{ProceduralKind, ProceduralParams};
    let wood = |s: &mut Session, scale: f32| {
        let layer = s.document().texture_sets[0].layers.iter().find(|l| l.kind() == LayerKind::Fill).unwrap().id;
        let source = FillSource::Procedural {
            params: ProceduralParams { scale, ..ProceduralParams::new(ProceduralKind::WoodRings) },
            low: ChannelValue::rgb(0.2, 0.1, 0.05),
            high: ChannelValue::rgb(0.5, 0.3, 0.1),
        };
        s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(source) }).unwrap();
    };
    let same = |a: &Session, b: &Session| {
        for (x, y) in a.document().texture_sets.iter().zip(&b.document().texture_sets) {
            assert_eq!(a.composite(x.id).unwrap().channels, b.composite(y.id).unwrap().channels, "{}", x.name);
        }
    };
    let (mut bg, mut inline) = (session(), session());
    for s in [&mut bg, &mut inline] {
        s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    }
    // Edits arrive every frame while the worker is busy with cold fields.
    for i in 0..5 {
        wood(&mut bg, 2.0 + i as f32);
        bg.update_composites_within(Some(Duration::from_millis(1)));
    }
    bg.update_composites();
    assert!(!bg.composites_pending());
    wood(&mut inline, 6.0);
    inline.update_composites();
    same(&bg, &inline);

    // Interactive frames converge by themselves and report what changed.
    wood(&mut bg, 2.5);
    wood(&mut inline, 2.5);
    inline.update_composites();
    let start = Instant::now();
    let mut changed = 0;
    while bg.composites_pending() {
        changed += bg.update_composites_within(Some(Duration::from_millis(1))).iter().map(|u| u.changed.len()).sum::<usize>();
        assert!(start.elapsed() < Duration::from_secs(120), "the worker converges");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(changed > 0);
    same(&bg, &inline);
    assert!(bg.update_composites_within(None).is_empty(), "nothing left once converged");

    // New baked maps re-evaluate every tile of that texture set on the worker too.
    let body = bg.document().texture_sets[0].id;
    bg.set_mesh_maps(body, Default::default());
    let updates = bg.update_composites();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].tiles.len(), 16);
}

#[test]
fn starting_background_compositing_retains_existing_sync_results() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.update_composites();
    let before: Vec<_> = s.document().texture_sets.iter().map(|ts| (ts.id, s.composite(ts.id).unwrap().clone())).collect();
    s.update_composites_within(Some(std::time::Duration::from_millis(1)));
    // The worker has no dirty tiles to publish. Existing results must remain available both
    // immediately and after a synchronous caller waits for this adopted snapshot.
    for (id, expected) in &before {
        assert_eq!(s.composite(*id), Some(expected));
    }
    s.update_composites();
    for (id, expected) in &before {
        assert_eq!(s.composite(*id), Some(expected));
    }
    assert!(!s.composites_pending());
}

#[test]
fn save_and_export_use_newest_document_while_worker_is_busy() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.update_composites_within(Some(std::time::Duration::from_millis(1)));
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel {
        layer,
        channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::Uniform { value: ChannelValue::rgb(0.13, 0.42, 0.71) }),
    }).unwrap();
    let path = std::env::temp_dir().join(format!("subcraft-worker-save-{}.subcraft", layer));
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(s.document(), reopened.document(), "save stores authored data, never a stale viewport composite");
    let archive = s.export_archive(&subcraft_export::ExportSettings::default()).unwrap();
    assert!(archive.files > 0);
    assert!(!s.composites_pending(), "export synchronously drains the newest snapshot");
    reopened.update_composites();
    for ts in &s.document().texture_sets {
        assert_eq!(s.composite(ts.id), reopened.composite(ts.id));
    }
}

#[test]
fn insert_layer_at_places_inside_folders_and_selects() {
    let mut s = session();
    let ts = s.selection().texture_set;
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let folder = s.selection().layer.unwrap();
    let tc = s.selected_texture_set().clone();
    let fill = subcraft_doc::Layer::new_fill("Inside", &tc);
    let id = fill.id;
    s.execute(Command::InsertLayerAt { texture_set: ts, parent: Some(folder), index: 99, layer: Box::new(fill), label: "Place".into() }).unwrap();
    assert_eq!(s.selection().layer, Some(id));
    assert_eq!(s.selected_texture_set().location(id).unwrap().parent, Some(folder));
    let bottom = subcraft_doc::Layer::new_paint("Bottom", &tc);
    let bottom_id = bottom.id;
    s.execute(Command::InsertLayerAt { texture_set: ts, parent: None, index: 0, layer: Box::new(bottom), label: "Place".into() }).unwrap();
    assert_eq!(s.selected_texture_set().layers[0].id, bottom_id);
    let paint = s.selected_texture_set().layers[1].id;
    let err = s.execute(Command::InsertLayerAt { texture_set: ts, parent: Some(paint), index: 0, layer: Box::new(subcraft_doc::Layer::new_paint("X", &tc)), label: "Place".into() });
    assert!(err.is_err(), "a paint layer is not a folder");
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(bottom_id).is_none());
}
