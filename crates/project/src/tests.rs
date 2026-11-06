use std::io::Cursor;
use std::sync::Arc;

use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    BlendMode, Document, FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Levels, Mask, MaskEffect, MaskEffectKind,
    ProceduralKind, ProceduralParams, ResourceStore, TextureSet,
};
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;

use super::*;

#[test]
fn legacy_paint_schema_defaults_to_raster_only() {
    let paint: ContentV1 = serde_json::from_str(r#"{"type":"paint","channels":[]}"#).unwrap();
    assert!(matches!(paint, ContentV1::Paint { path: None, .. }));
}

#[test]
fn generator_defaults_and_schema_7_preserve_legacy_noise_and_roundtrip_new_controls() {
    let mut g = Generator::new(GeneratorKind::Edges);
    assert_eq!(g.noise_amount, 0.3);
    assert!(g.organic_breakup);
    g.set_pattern_size(25.); g.noise.seed = 47;
    assert_eq!(g.noise.scale, 4.);
    assert_eq!(g.pattern_size(), 25.);
    let mut ts = TextureSet::new("Body", 0, 128);
    let mut layer = Layer::new_fill("Edge wear", &ts);
    let mut mask = Mask::new(128, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    layer.mask = Some(mask); ts.insert_above(None, layer);
    let doc = Document { texture_sets: vec![ts] };
    let bytes = write_project(Cursor::new(Vec::new()), &primitives::cube(2.), &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    assert_eq!(read_project(Cursor::new(bytes.clone())).unwrap().document, doc);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap(); let name = f.name().unwrap().to_string(); let mut data = Vec::new(); f.read_to_end(&mut data).unwrap();
        if name == "manifest.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap(); json["schema_version"] = 6.into(); data = serde_json::to_vec(&json).unwrap();
        } else if name == "document.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
            json["texture_sets"][0]["layers"][0]["mask"]["effects"][0]["kind"]["generator"].as_object_mut().unwrap().remove("organic_breakup");
            json["texture_sets"][0]["layers"][0]["mask"]["effects"][0]["kind"]["generator"].as_object_mut().unwrap().remove("edge_spread");
            data = serde_json::to_vec(&json).unwrap();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap(); out.write_all(&data).unwrap();
    }
    let legacy = read_project(Cursor::new(out.finish().unwrap().into_inner())).unwrap();
    let MaskEffectKind::Generator(back) = legacy.document.texture_sets[0].layers[0].mask.as_ref().unwrap().effects[0].kind else { panic!() };
    assert!(!back.organic_breakup); assert_eq!((back.noise.scale, back.noise.seed, back.noise_amount), (4., 47, 0.3));
    assert!(back.edge_spread.is_none());
}

#[test]
fn schema_8_spread_roundtrips_and_schema_7_preserves_legacy_evaluation() {
    let mut g = Generator::new(GeneratorKind::Edges);
    g.edge_spread = Some(subcraft_doc::EdgeSpread { distance: 4.25, variation: 0.86, level_reach: false });
    assert_eq!(serde_json::to_string(&g.edge_spread).unwrap(), r#"{"distance":4.25,"variation":0.86}"#);
    let mut ts = TextureSet::new("Body", 0, 128);
    let mut layer = Layer::new_fill("Spread", &ts);
    layer.mask = Some(Mask::new(128, false));
    layer.mask.as_mut().unwrap().effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    ts.insert_above(None, layer);
    let doc = Document { texture_sets: vec![ts] };
    let bytes = write_project(Cursor::new(Vec::new()), &primitives::sample_crate(), &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    assert_eq!(read_project(Cursor::new(bytes.clone())).unwrap().document, doc);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap(); let name = f.name().unwrap().to_string(); let mut data = Vec::new(); f.read_to_end(&mut data).unwrap();
        if name == "manifest.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap(); json["schema_version"] = 7.into(); data = serde_json::to_vec(&json).unwrap();
        } else if name == "document.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
            json["texture_sets"][0]["layers"][0]["mask"]["effects"][0]["kind"]["generator"].as_object_mut().unwrap().remove("edge_spread");
            data = serde_json::to_vec(&json).unwrap();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap(); out.write_all(&data).unwrap();
    }
    let old = read_project(Cursor::new(out.finish().unwrap().into_inner())).unwrap();
    let MaskEffectKind::Generator(back) = old.document.texture_sets[0].layers[0].mask.as_ref().unwrap().effects[0].kind else { panic!() };
    assert_eq!(back, Generator { edge_spread: None, ..g });
}

#[test]
fn pen_schema_has_explicit_stable_fields_and_rejects_duplicate_anchors() {
    let id = uuid::Uuid::new_v4();
    let path = PenPathV4 { anchors: vec![PenAnchorV4 { id, position: [0.2, 0.3], incoming: [0.0, -0.1], outgoing: [0.0, 0.1] }], closed: false, radius: 0.02, hardness: 0.5, opacity: 1.0, values: vec![PenChannelV4 { channel: ChannelKind::BaseColor, value: [1.0, 0.0, 0.0] }] };
    let json = serde_json::to_value(&path).unwrap();
    assert_eq!(json["anchors"][0]["id"], id.to_string()); assert_eq!(json["anchors"][0]["outgoing"], serde_json::json!([0.0, 0.1f32]));
    assert_eq!(json["values"][0]["channel"], "base_color");
    let mut layer = Layer::new_paint("Pen", &TextureSet::new("Body", 0, 128));
    let LayerContent::Paint(p) = &mut layer.content else { panic!() };
    let mut source = subcraft_doc::PenPath { anchors: vec![subcraft_doc::PenAnchor { id: subcraft_core::AnchorId(id), position: [0.2, 0.3].into(), incoming: [0.0, -0.1].into(), outgoing: [0.0, 0.1].into() }], closed: false, radius: 0.02, hardness: 0.5, opacity: 1.0, values: [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into() };
    source.anchors.push(source.anchors[0].clone()); p.path = Some(source);
    let mut ts = TextureSet::new("Body", 0, 128); ts.insert_above(None, layer);
    let bytes = write_project(Cursor::new(Vec::new()), &primitives::cube(2.0), &Document { texture_sets: vec![ts] }, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    assert!(matches!(read_project(Cursor::new(bytes)), Err(ProjectError::Invalid(_))));
}

#[test]
fn old_part_masks_default_to_connected_and_new_domains_have_stable_names() {
    let parts: PartsV1 = serde_json::from_str(r#"{"enabled":true,"parts":[0,2]}"#).unwrap();
    assert_eq!(parts.kind, subcraft_core::PartKind::Connected);
    for (kind, name) in subcraft_core::PartKind::ALL.into_iter().zip(["connected", "uv_island", "object", "material"]) {
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{name}\""));
    }
}

fn rich_document() -> Document {
    let mut ts = TextureSet::new("Body", 0, 128);
    let tc = ts.clone();
    // Fill with procedural base color and a masked generator.
    let mut fill = Layer::new_fill("Fill", &tc);
    if let LayerContent::Fill(f) = &mut fill.content {
        f.channels.insert(
            ChannelKind::BaseColor,
            FillChannel {
                strength: 0.6,
                ..FillChannel::new(FillSource::Procedural {
                    params: ProceduralParams::new(ProceduralKind::Cells),
                    low: ChannelValue::rgb(0.1, 0.2, 0.3),
                    high: ChannelValue::rgb(0.9, 0.8, 0.7),
                })
            },
        );
        f.projection.mode = subcraft_doc::ProjectionMode::Triplanar;
    }
    let mut mask = Mask::new(128, false);
    mask.base.set(3, 4, [0.75; 4]);
    let mut paint_fx = TiledImage::new(128, 128, 2, [0.0; 4]);
    paint_fx.set(100, 100, [1.0, 1.0, 0.0, 0.0]);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Paint { image: paint_fx })));
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(Generator::new(GeneratorKind::Cavity)))));
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(Levels { gamma: 2.0, ..Levels::default() }))));
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Invert)));
    fill.mask = Some(mask);
    ts.insert_above(None, fill);
    // Folder with a painted layer.
    let folder = ts.insert_above(None, Layer::new_folder("Folder", &tc));
    let mut paint = Layer::new_paint("Paint", &tc);
    paint.blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Multiply;
    paint.blend.get_mut(&ChannelKind::Roughness).unwrap().opacity = 0.4;
    paint.visible = false;
    if let LayerContent::Paint(p) = &mut paint.content {
        p.channel_mut(ChannelKind::BaseColor, 128).set(70, 7, [0.2, 0.4, 0.6, 0.5]);
        p.channel_mut(ChannelKind::Height, 128).set(1, 120, [-0.25, 1.0, 0.0, 0.0]);
    }
    ts.insert(Some(folder), 0, paint).unwrap();
    let trim = TextureSet::new("Trim", 1, 256);
    Document { texture_sets: vec![ts, trim] }
}

fn roundtrip(doc: &Document) -> ProjectData {
    let mesh = primitives::sample_crate();
    let buf = write_project(Cursor::new(Vec::new()), &mesh, doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap();
    read_project(Cursor::new(buf.into_inner())).unwrap()
}

#[test]
fn document_roundtrips_exactly() {
    let doc = rich_document();
    let back = roundtrip(&doc);
    assert_eq!(back.document, doc, "layer tree, ids, blends, masks, effects and rasters survive");
    assert_eq!(back.mesh.positions, primitives::sample_crate().positions);
    assert_eq!(back.mesh.materials.len(), 2);
}

#[test]
fn baked_mesh_maps_roundtrip() {
    let doc = rich_document();
    let mut maps = MeshMapSet::new();
    let mut ao = TiledImage::new(256, 256, 1, [1.0; 4]);
    ao.set(10, 10, [0.25; 4]);
    let mut inner = std::collections::BTreeMap::new();
    inner.insert(subcraft_core::MeshMapKind::AmbientOcclusion, ao);
    inner.insert(subcraft_core::MeshMapKind::WorldSpaceNormal, TiledImage::new(256, 256, 3, [0.5, 1.0, 0.5, 0.0]));
    maps.insert(doc.texture_sets[0].id, inner);
    let mesh = primitives::sample_crate();
    let buf = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &maps).unwrap();
    let back = read_project(Cursor::new(buf.into_inner())).unwrap();
    assert_eq!(back.mesh_maps, maps);
    assert_eq!(back.document, doc);
}

#[test]
fn file_save_is_atomic_and_overwrites() {
    let dir = std::env::temp_dir().join(format!("subcraft-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crate.subcraft");
    let mesh = primitives::sample_crate();
    let mut doc = rich_document();
    save_project(&path, &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap();
    doc.texture_sets[0].name = "Renamed".into();
    save_project(&path, &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap();
    assert!(!temp_path(&path).exists(), "temporary file cleaned up");
    let back = load_project(&path).unwrap();
    assert_eq!(back.document.texture_sets[0].name, "Renamed");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rejects_newer_schema_and_non_projects() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("manifest.json", SimpleFileOptions::default()).unwrap();
    zip.write_all(br#"{"format":"subcraft-project","schema_version":99,"writer":"x","mesh":"m","document":"d"}"#).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(matches!(read_project(Cursor::new(bytes)), Err(ProjectError::UnsupportedVersion(99))));

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("readme.txt", SimpleFileOptions::default()).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(matches!(read_project(Cursor::new(bytes)), Err(ProjectError::NotAProject)));
    assert!(matches!(read_project(Cursor::new(b"definitely not a zip".to_vec())), Err(ProjectError::Container(_))));
}

#[test]
fn corrupt_raster_is_an_error_not_a_panic() {
    let doc = rich_document();
    let mesh = primitives::sample_crate();
    let good = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    // Rebuild the container with one raster truncated.
    let mut src = zip::ZipArchive::new(Cursor::new(good)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let name = f.name().unwrap().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == "rasters/00001.tiles" {
            data.truncate(data.len() / 2);
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    let bytes = out.finish().unwrap().into_inner();
    let err = read_project(Cursor::new(bytes)).unwrap_err();
    assert!(matches!(err, ProjectError::Invalid(ref m) if m.contains("truncated")), "{err}");
}

/// Pins the JSON of value types embedded in the schema: changing them is a format change.
#[test]
fn embedded_value_types_have_stable_json() {
    let src = FillSource::Procedural {
        params: ProceduralParams::new(ProceduralKind::WoodRings),
        low: ChannelValue::scalar(0.0),
        high: ChannelValue::rgb(1.0, 0.5, 0.25),
    };
    assert_eq!(
        serde_json::to_string(&src).unwrap(),
        r#"{"type":"procedural","params":{"kind":"wood_rings","scale":4.0,"seed":1,"balance":0.5,"contrast":0.0,"invert":false},"low":[0.0,0.0,0.0],"high":[1.0,0.5,0.25]}"#
    );
    assert_eq!(serde_json::to_string(&BlendMode::NormalMapCombine).unwrap(), r#""normal_map_combine""#);
    assert_eq!(serde_json::to_string(&ChannelKind::AmbientOcclusion).unwrap(), r#""ambient_occlusion""#);
    let p = subcraft_doc::Projection::default();
    assert_eq!(
        serde_json::to_string(&p).unwrap(),
        r#"{"mode":"uv","scale":[1.0,1.0],"rotation_deg":0.0,"offset":[0.0,0.0],"triplanar_sharpness":8.0,"planar_axis":2}"#
    );
}

/// Rebuild a container, transforming `manifest.json` with `f`.
fn with_manifest(bytes: Vec<u8>, f: impl Fn(String) -> String) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut file = src.by_index(i).unwrap();
        let name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == "manifest.json" {
            data = f(String::from_utf8(data).unwrap()).into_bytes();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn schema_1_projects_still_load() {
    let doc = rich_document();
    let mesh = primitives::sample_crate();
    let bytes = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    let v1 = with_manifest(bytes, |m| m.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"), "\"schema_version\": 1"));
    let back = read_project(Cursor::new(v1)).unwrap();
    assert_eq!(back.document, doc);
}

#[test]
fn schema_6_albedo_filter_roundtrips_and_legacy_sets_migrate_to_bilinear() {
    let mut doc = rich_document();
    doc.texture_sets[0].albedo_filter = subcraft_image::TextureFilter::Nearest;
    let mesh = primitives::sample_crate();
    let bytes = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    assert_eq!(read_project(Cursor::new(bytes.clone())).unwrap().document, doc);
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut file = input.by_index(i).unwrap();
        let name = file.name().unwrap().to_string();
        let mut data = Vec::new(); file.read_to_end(&mut data).unwrap();
        if name == "document.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
            for ts in json["texture_sets"].as_array_mut().unwrap() { ts.as_object_mut().unwrap().remove("albedo_filter"); }
            data = serde_json::to_vec(&json).unwrap();
        } else if name == "manifest.json" {
            let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap(); json["schema_version"] = 5.into();
            data = serde_json::to_vec(&json).unwrap();
        }
        output.start_file(name, SimpleFileOptions::default()).unwrap(); output.write_all(&data).unwrap();
    }
    let migrated = read_project(Cursor::new(output.finish().unwrap().into_inner())).unwrap();
    assert!(migrated.document.texture_sets.iter().all(|ts| ts.albedo_filter == subcraft_image::TextureFilter::Bilinear));
    assert_eq!(serde_json::to_string(&subcraft_image::TextureFilter::Nearest).unwrap(), "\"nearest\"");
}

#[test]
fn image_mask_effect_keeps_its_image() {
    use subcraft_core::ResourceId;
    use subcraft_doc::{Projection, Resource, ResourceKind};

    let png = {
        let dir = std::env::temp_dir().join(format!("subcraft-proj-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("logo.png");
        subcraft_image::save_image(&p, 2, 2, 2, &[0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.0, 1.0], subcraft_image::FileFormat::Png8).unwrap();
        let b = std::fs::read(&p).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        b
    };
    let id = ResourceId::new();
    let mut res = ResourceStore::default();
    res.insert(Resource {
        id,
        name: "logo.png".into(),
        kind: ResourceKind::Texture,
        bitmap: Arc::new(subcraft_image::io::decode_bitmap(&png, None).unwrap()),
        source_path: None,
        encoded: Some(Arc::new(png)),
    });
    let mut ts = TextureSet::new("Body", 0, 128);
    let mut layer = Layer::new_paint("Decal", &ts);
    let mut mask = Mask::new(128, false);
    let mut projection = Projection::default();
    projection.scale = [3.0, 3.0];
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Image { resource: id, projection, invert: true })));
    layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let doc = Document { texture_sets: vec![ts] };

    let bytes = write_project(Cursor::new(Vec::new()), &primitives::cube(1.0), &doc, &res, &MeshMapSet::new()).unwrap().into_inner();
    let back = read_project(Cursor::new(bytes)).unwrap();
    assert_eq!(back.document, doc);
    let r = back.resources.get(id).expect("mask image saved with the project");
    assert_eq!((r.bitmap.width, r.bitmap.channels), (2, 2));
}

fn effect_document() -> Document {
    use subcraft_doc::{ContentEffect, ContentEffectKind, FillContent};
    let mut ts = TextureSet::new("Body", 0, 128);
    let tc = ts.clone();
    let mut layer = Layer::new_fill("Fill", &tc);
    let mut paint = std::collections::BTreeMap::new();
    let mut img = subcraft_doc::empty_paint_image(ChannelKind::BaseColor, 128);
    img.set(9, 90, [0.25, 0.5, 0.75, 0.5]);
    paint.insert(ChannelKind::BaseColor, img);
    let mut fx_paint = ContentEffect::new(ContentEffectKind::Paint { channels: paint }, &tc);
    fx_paint.blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.3;
    let fill = FillContent {
        projection: subcraft_doc::Projection::default(),
        channels: [(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.8))))].into(),
    };
    let mut fx_fill = ContentEffect::new(ContentEffectKind::Fill(fill), &tc);
    fx_fill.visible = false;
    fx_fill.name = "Rough coat".into();
    let fx_levels = ContentEffect::new(ContentEffectKind::Levels { channel: ChannelKind::Roughness, levels: Levels { out_low: 0.2, ..Levels::default() } }, &tc);
    layer.content_effects = vec![Arc::new(fx_paint), Arc::new(fx_fill), Arc::new(fx_levels)];
    ts.insert_above(None, layer);
    Document { texture_sets: vec![ts] }
}

#[test]
fn content_effects_roundtrip_exactly_with_schema_5() {
    let doc = effect_document();
    let mesh = primitives::sample_crate();
    let bytes = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let mut json = String::new();
    zip.by_name("document.json").unwrap().read_to_string(&mut json).unwrap();
    assert!(json.contains("\"content_effects\"") && json.contains("\"type\": \"levels\""), "{json}");
    assert_eq!(SCHEMA_VERSION, 16);
    assert_eq!(read_project(Cursor::new(bytes)).unwrap().document, doc, "effect ids, order, blends, rasters survive");
}

#[test]
fn schema_4_layers_have_no_content_effects_and_bad_effects_are_rejected() {
    // A schema-4 layer has no `content_effects` key.
    let v4: LayerV1 = serde_json::from_str(
        r#"{"id":"00000000-0000-0000-0000-000000000001","name":"L","visible":true,"blend":[],"content":{"type":"paint","channels":[]}}"#,
    )
    .unwrap();
    assert!(v4.content_effects.is_empty());
    // Files without effects do not even write the key (older field layout unchanged).
    let plain = serde_json::to_string(&v4).unwrap();
    assert!(!plain.contains("content_effects"));

    let mesh = primitives::sample_crate();
    let save = |doc: &Document| write_project(Cursor::new(Vec::new()), &mesh, doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    // Levels on the normal channel is invalid data, not something to evaluate.
    let mut doc = effect_document();
    let layer = Arc::make_mut(&mut doc.texture_sets[0].layers[0]);
    Arc::make_mut(&mut layer.content_effects[2]).kind =
        subcraft_doc::ContentEffectKind::Levels { channel: ChannelKind::Normal, levels: Levels::default() };
    assert!(matches!(read_project(Cursor::new(save(&doc))), Err(ProjectError::Invalid(_))));
    // Folder source effects remain invalid (adjustments are supported in schema 9).
    let mut doc = effect_document();
    let effects = doc.texture_sets[0].layers[0].content_effects.clone();
    let tc = doc.texture_sets[0].clone();
    let mut folder = Layer::new_folder("F", &tc);
    folder.content_effects = effects;
    doc.texture_sets[0].insert_above(None, folder);
    assert!(matches!(read_project(Cursor::new(save(&doc))), Err(ProjectError::Invalid(_))));
}

#[test]
fn content_effect_kinds_have_stable_json() {
    let levels = ContentEffectKindV5::Levels { channel: ChannelKind::Roughness, levels: Levels::default() };
    assert_eq!(
        serde_json::to_string(&levels).unwrap(),
        r#"{"type":"levels","channel":"roughness","levels":{"in_low":0.0,"in_high":1.0,"gamma":1.0,"out_low":0.0,"out_high":1.0}}"#
    );
    let paint = ContentEffectKindV5::Paint { channels: vec![PaintChannelV1 { channel: ChannelKind::BaseColor, raster: "r".into() }] };
    assert_eq!(serde_json::to_string(&paint).unwrap(), r#"{"type":"paint","channels":[{"channel":"base_color","raster":"r"}]}"#);
}

#[test]
fn schema_12_filters_and_folder_adjustments_roundtrip_and_older_projects_migrate_unchanged() {
    use subcraft_doc::{ContentEffect, ContentEffectKind, Filter};
    let mut ts=TextureSet::new("T",0,128); let tc=ts.clone();
    let mut folder=Layer::new_folder("Filters",&tc);
    for filter in Filter::ALL.into_iter().filter(|f|f.supports_channel(ChannelKind::BaseColor)) {
        folder.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter},&tc)));
    }
    let mut mask=Mask::new(128,false);
    for filter in Filter::ALL.into_iter().filter(|f|f.supports_mask()) { mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(filter)))); }
    folder.mask=Some(mask); ts.insert_above(None,folder);
    let doc=Document {texture_sets:vec![ts]}; let mesh=primitives::cube(2.0);
    let save=|doc:&Document|write_project(Cursor::new(Vec::new()),&mesh,doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    let bytes=save(&doc); assert_eq!(read_project(Cursor::new(bytes)).unwrap().document,doc);
    assert_eq!(serde_json::to_string(&ContentEffectKindV5::Filter {channel:ChannelKind::BaseColor,filter:Filter::Threshold {cutoff:0.4}}).unwrap(),
        r#"{"type":"filter","channel":"base_color","filter":{"kind":"threshold","cutoff":0.4}}"#);
    let old=effect_document(); let bytes=save(&old);
    for schema in 5..=11 {
        let migrated=with_manifest(bytes.clone(),|m|m.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"),&format!("\"schema_version\": {schema}")));
        assert_eq!(read_project(Cursor::new(migrated)).unwrap().document,old);
    }
    let fragment=crate::fragment::Fragment {kind:crate::fragment::FragmentKind::SmartMaterial,name:"Filters".into(),resolution:128,
        layers:vec![doc.texture_sets[0].layers[0].as_ref().clone()],mask_effects:vec![],resources:ResourceStore::default()};
    let bytes=crate::fragment::write_fragment(Cursor::new(Vec::new()),&fragment).unwrap().into_inner();
    assert_eq!(crate::fragment::read_fragment(Cursor::new(bytes)).unwrap().layers,fragment.layers);
}

#[test]
fn loader_rejects_filters_with_invalid_domains_or_parameters() {
    use subcraft_doc::{ContentEffect, ContentEffectKind, Filter};
    let mesh=primitives::cube(2.0);
    for (channel,filter) in [(ChannelKind::Normal,Filter::Threshold {cutoff:0.5}),
        (ChannelKind::Roughness,Filter::ALL[1]), (ChannelKind::BaseColor,Filter::Blur {radius:33.0}),
        (ChannelKind::Height,Filter::Sharpen {radius:2.0,amount:-1.0}),
        (ChannelKind::BaseColor,Filter::DirectionalBlur {radius:4.0,angle_deg:181.0}), (ChannelKind::Height,Filter::Highpass {radius:4.0,amount:5.0}),(ChannelKind::BaseColor,Filter::Clamp {low:0.8,high:0.2})] {
        let mut ts=TextureSet::new("T",0,128); let mut layer=Layer::new_fill("F",&ts);
        layer.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel,filter},&ts)));
        ts.insert_above(None,layer); let doc=Document {texture_sets:vec![ts]};
        let bytes=write_project(Cursor::new(Vec::new()),&mesh,&doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
        assert!(matches!(read_project(Cursor::new(bytes)),Err(ProjectError::Invalid(_))));
    }
    let mut ts=TextureSet::new("T",0,128); let mut layer=Layer::new_fill("F",&ts);
    let mut mask=Mask::new(128,false);mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::ALL[1]))));layer.mask=Some(mask);
    ts.insert_above(None,layer);let doc=Document {texture_sets:vec![ts]};
    let bytes=write_project(Cursor::new(Vec::new()),&mesh,&doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    assert!(matches!(read_project(Cursor::new(bytes)),Err(ProjectError::Invalid(_))));
}

#[test]
fn neighborhood_filters_keep_their_original_json() {
    use subcraft_doc::Filter;
    assert_eq!(SCHEMA_VERSION,16);
    assert_eq!(serde_json::to_string(&Filter::Blur {radius:4.0}).unwrap(),r#"{"kind":"blur","radius":4.0}"#);
    assert_eq!(serde_json::to_string(&Filter::Sharpen {radius:2.0,amount:1.5}).unwrap(),r#"{"kind":"sharpen","radius":2.0,"amount":1.5}"#);
}

#[test]
fn schema_10_neighborhood_stacks_migrate_and_new_filters_have_stable_json() {
    use subcraft_doc::{ContentEffect,ContentEffectKind,Filter};
    let mut ts=TextureSet::new("Legacy filters",0,128);let mut l=Layer::new_fill("F",&ts);
    let mut mask=Mask::new(128,true);
    for f in [Filter::Blur {radius:8.0},Filter::Sharpen {radius:3.0,amount:0.5}] {
        l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter:f},&ts)));
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(f))));
    }
    l.mask=Some(mask);ts.insert_above(None,l);let doc=Document {texture_sets:vec![ts]};
    let bytes=write_project(Cursor::new(Vec::new()),&primitives::cube(2.0),&doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    let old=with_manifest(bytes,|s|s.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"),"\"schema_version\": 10"));
    assert_eq!(read_project(Cursor::new(old)).unwrap().document,doc);
    assert_eq!(serde_json::to_string(&Filter::DirectionalBlur {radius:8.0,angle_deg:45.0}).unwrap(),r#"{"kind":"directional_blur","radius":8.0,"angle_deg":45.0}"#);
    assert_eq!(serde_json::to_string(&Filter::Highpass {radius:4.0,amount:1.5}).unwrap(),r#"{"kind":"highpass","radius":4.0,"amount":1.5}"#);
}

#[test]
fn schema_11_directional_stacks_migrate_and_contours_have_stable_json() {
    use subcraft_doc::{ContentEffect,ContentEffectKind,Filter,OutlinePosition};
    let mut ts=TextureSet::new("Legacy",0,128);let mut l=Layer::new_fill("F",&ts);
    let mut mask=Mask::new(128,true);
    for f in [Filter::DirectionalBlur {radius:8.0,angle_deg:35.0},Filter::Highpass {radius:4.0,amount:0.5}] {
        l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter:f},&ts)));
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(f))));
    }
    l.mask=Some(mask);ts.insert_above(None,l);let doc=Document {texture_sets:vec![ts]};
    let bytes=write_project(Cursor::new(Vec::new()),&primitives::cube(2.0),&doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    let old=with_manifest(bytes,|s|s.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"),"\"schema_version\": 11"));
    assert_eq!(read_project(Cursor::new(old)).unwrap().document,doc);
    assert_eq!(serde_json::to_string(&Filter::Bevel {radius:8.0,amount:-0.2,threshold:0.5}).unwrap(),r#"{"kind":"bevel","radius":8.0,"amount":-0.2,"threshold":0.5}"#);
    assert_eq!(serde_json::to_string(&Filter::MaskOutline {radius:4.0,threshold:0.5,position:OutlinePosition::Outer}).unwrap(),r#"{"kind":"mask_outline","radius":4.0,"threshold":0.5,"position":"outer"}"#);
}

#[test]
fn schema_13_stacks_open_and_gradient_map_has_stable_json_and_survives_roundtrip() {
    use subcraft_doc::{ContentEffect,ContentEffectKind,Filter,Gradient,GradientStop};
    let st=|position,color|GradientStop {position,color};
    let gradient=Gradient::new(&[st(0.0,[0.0,0.0,0.0]),st(0.25,[0.5,0.0,0.0]),st(1.0,[1.0,1.0,0.5])]).unwrap();
    let new=Filter::GradientMap {gradient,to_gray:false};
    let mut ts=TextureSet::new("Gradients",0,128);let mut l=Layer::new_fill("F",&ts);
    // A pre-14 stack: only filters that existed in schema 13.
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter:Filter::Blur {radius:4.0}},&ts)));
    ts.insert_above(None,l);let old_doc=Document {texture_sets:vec![ts.clone()]};
    let bytes=write_project(Cursor::new(Vec::new()),&primitives::cube(2.0),&old_doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    let old=with_manifest(bytes,|s|s.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"),"\"schema_version\": 13"));
    assert_eq!(read_project(Cursor::new(old)).unwrap().document,old_doc);
    // New: the stop list serializes as a plain array, unused slots are never written.
    assert_eq!(serde_json::to_string(&new).unwrap(),r#"{"kind":"gradient_map","gradient":[{"position":0.0,"color":[0.0,0.0,0.0]},{"position":0.25,"color":[0.5,0.0,0.0]},{"position":1.0,"color":[1.0,1.0,0.5]}],"to_gray":false}"#);
    assert_eq!(serde_json::from_str::<Filter>(&serde_json::to_string(&new).unwrap()).unwrap(),new);
    assert!(serde_json::from_str::<Filter>(r#"{"kind":"gradient_map","gradient":[],"to_gray":true}"#).is_err());
    let nine=(0..9).map(|i|format!(r#"{{"position":{},"color":[0,0,0]}}"#,i as f32/10.0)).collect::<Vec<_>>().join(",");
    assert!(serde_json::from_str::<Filter>(&format!(r#"{{"kind":"gradient_map","gradient":[{nine}],"to_gray":true}}"#)).is_err());
    // Full project round trip as a content effect and as a mask effect.
    let mut ts=TextureSet::new("Gradients",0,128);let mut l=Layer::new_fill("F",&ts);
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter:new},&ts)));
    let mut mask=Mask::new(128,true);mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::GradientMap {gradient:Gradient::preset(1).unwrap(),to_gray:true}))));
    l.mask=Some(mask);ts.insert_above(None,l);let doc=Document {texture_sets:vec![ts]};
    let bytes=write_project(Cursor::new(Vec::new()),&primitives::cube(2.0),&doc,&ResourceStore::default(),&MeshMapSet::new()).unwrap().into_inner();
    assert_eq!(read_project(Cursor::new(bytes)).unwrap().document,doc);
}

#[test]
fn histogram_filters_have_stable_json() {
    use subcraft_doc::Filter;
    assert_eq!(serde_json::to_string(&Filter::HistogramScan {position:0.5,contrast:0.25,invert:true}).unwrap(),r#"{"kind":"histogram_scan","position":0.5,"contrast":0.25,"invert":true}"#);
    assert_eq!(serde_json::to_string(&Filter::HistogramRange {position:0.5,range:0.5,softness:0.0,invert:false}).unwrap(),r#"{"kind":"histogram_range","position":0.5,"range":0.5,"softness":0.0,"invert":false}"#);
}

#[test]
fn median_filter_has_stable_json() {
    use subcraft_doc::Filter;
    assert_eq!(serde_json::to_string(&Filter::Median {radius:3.0}).unwrap(),r#"{"kind":"median","radius":3.0}"#);
}

#[test]
fn warp_filter_has_stable_json() {
    use subcraft_doc::Filter;
    assert_eq!(serde_json::to_string(&Filter::Warp {intensity:8.0,scale:4,seed:7}).unwrap(),r#"{"kind":"warp","intensity":8.0,"scale":4,"seed":7}"#);
}

#[test]
fn the_trusted_loader_keeps_filter_parameters_the_checked_loader_rejects() {
    use subcraft_doc::Filter;
    let mesh = primitives::cube(2.0);
    let mut ts = TextureSet::new("T", 0, 128);
    let mut layer = Layer::new_fill("F", &ts);
    let mut mask = Mask::new(128, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Blur { radius: 90.0 }))));
    layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let doc = Document { texture_sets: vec![ts] };
    let bytes = write_project(Cursor::new(Vec::new()), &mesh, &doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    assert!(matches!(read_project(Cursor::new(bytes.clone())), Err(ProjectError::Invalid(_))));
    assert_eq!(read_project_checked(Cursor::new(bytes), true).unwrap().document, doc);
}

#[test]
fn schema_15_id_color_mask_roundtrips_has_stable_json_and_older_projects_migrate() {
    use subcraft_doc::IdColorMask;
    let id = IdColorMask { colors: vec![[0.9, 0.3, 0.25], [0.25, 0.5, 0.9]], tolerance: 0.08, softness: 0.02, invert: true };
    assert_eq!(
        serde_json::to_string(&MaskEffectKindV1::IdColor { id_color: id.clone() }).unwrap(),
        r#"{"type":"id_color","id_color":{"colors":[[0.9,0.3,0.25],[0.25,0.5,0.9]],"tolerance":0.08,"softness":0.02,"invert":true}}"#
    );
    let mesh = primitives::cube(2.0);
    let save = |doc: &Document| write_project(Cursor::new(Vec::new()), &mesh, doc, &ResourceStore::default(), &MeshMapSet::new()).unwrap().into_inner();
    let with_mask = |kind: MaskEffectKind| {
        let mut ts = TextureSet::new("T", 0, 128);
        let mut layer = Layer::new_fill("F", &ts);
        let mut mask = Mask::new(128, false);
        let mut effect = MaskEffect::new(kind);
        effect.opacity = 0.75;
        mask.effects.push(Arc::new(effect));
        layer.mask = Some(mask);
        ts.insert_above(None, layer);
        Document { texture_sets: vec![ts] }
    };
    let doc = with_mask(MaskEffectKind::IdColor(id.clone()));
    assert_eq!(read_project(Cursor::new(save(&doc))).unwrap().document, doc, "colors, tolerance, softness, invert survive");

    // A Smart Mask fragment carries it too.
    let fragment = crate::fragment::Fragment { kind: crate::fragment::FragmentKind::SmartMask, name: "ID".into(), resolution: 128,
        layers: vec![], mask_effects: vec![MaskEffect::new(MaskEffectKind::IdColor(id.clone()))], resources: ResourceStore::default() };
    let bytes = crate::fragment::write_fragment(Cursor::new(Vec::new()), &fragment).unwrap().into_inner();
    assert_eq!(crate::fragment::read_fragment(Cursor::new(bytes)).unwrap().mask_effects, fragment.mask_effects);

    // Out-of-range parameters are invalid data.
    let bad = with_mask(MaskEffectKind::IdColor(IdColorMask { tolerance: 3.0, ..id }));
    assert!(matches!(read_project(Cursor::new(save(&bad))), Err(ProjectError::Invalid(_))));

    // Schema 13 and 14 projects (no ID color masks) load unchanged.
    let old = effect_document();
    for version in [13, 14] {
        let bytes = with_manifest(save(&old), |m| m.replace(&format!("\"schema_version\": {SCHEMA_VERSION}"), &format!("\"schema_version\": {version}")));
        assert_eq!(read_project(Cursor::new(bytes)).unwrap().document, old, "schema {version}");
    }
}

#[test]
fn procedural_params_without_scratch_settings_round_trip_unchanged() {
    use subcraft_doc::{ProceduralKind, ProceduralParams, ScratchPreset};
    let a = ProceduralParams::new(ProceduralKind::Scratches);
    let json = serde_json::to_string(&a).unwrap();
    assert!(!json.contains("scratch\""), "{json}");
    let old = r#"{"kind":"scratches","scale":4.0,"seed":1,"balance":0.5,"contrast":0.0,"invert":false}"#;
    assert_eq!(serde_json::from_str::<ProceduralParams>(old).unwrap(), a);
    let mut g = ProceduralParams::new(ProceduralKind::Scratch);
    ScratchPreset::Cracked.apply(&mut g);
    assert_eq!(serde_json::from_str::<ProceduralParams>(&serde_json::to_string(&g).unwrap()).unwrap(), g);
}

#[test]
fn texturecraft_parity_filters_have_stable_json() {
    use subcraft_doc::Filter;
    let json = |f: Filter| serde_json::to_string(&f).unwrap();
    assert_eq!(json(Filter::Grayscale), r#"{"kind":"grayscale"}"#);
    assert_eq!(json(Filter::Invert), r#"{"kind":"invert"}"#);
    assert_eq!(json(Filter::Posterize {levels:4}), r#"{"kind":"posterize","levels":4}"#);
    assert_eq!(json(Filter::HistogramShift {shift:0.25}), r#"{"kind":"histogram_shift","shift":0.25}"#);
    assert_eq!(json(Filter::ColorBalance {shadows:[0.0,0.0,0.5],midtones:[0.0;3],highlights:[0.25,0.0,0.0]}),
        r#"{"kind":"color_balance","shadows":[0.0,0.0,0.5],"midtones":[0.0,0.0,0.0],"highlights":[0.25,0.0,0.0]}"#);
    assert_eq!(json(Filter::ReplaceColor {from:[1.0,0.0,0.0],to:[0.0,1.0,0.0],tolerance:0.25,softness:0.5}),
        r#"{"kind":"replace_color","from":[1.0,0.0,0.0],"to":[0.0,1.0,0.0],"tolerance":0.25,"softness":0.5}"#);
    assert_eq!(json(Filter::NormalInvert {flip_x:false,flip_y:true,flip_z:false}), r#"{"kind":"normal_invert","flip_x":false,"flip_y":true,"flip_z":false}"#);
    assert_eq!(json(Filter::EdgeDetect {radius:1.0,strength:2.0}), r#"{"kind":"edge_detect","radius":1.0,"strength":2.0}"#);
    assert_eq!(json(Filter::Glow {radius:8.0,amount:1.0,threshold:0.5}), r#"{"kind":"glow","radius":8.0,"amount":1.0,"threshold":0.5}"#);
    assert_eq!(json(Filter::DirectionalWarp {intensity:8.0,angle_deg:45.0}), r#"{"kind":"directional_warp","intensity":8.0,"angle_deg":45.0}"#);
    assert_eq!(json(Filter::Mosaic {size:8}), r#"{"kind":"mosaic","size":8}"#);
    for f in Filter::ALL { assert_eq!(serde_json::from_str::<Filter>(&json(f)).unwrap(), f); }
}

#[test]
fn generator_axis_and_concave_kind_round_trip_and_old_files_default_to_y() {
    use subcraft_doc::{Generator, GeneratorKind, GradientAxis};
    let mut g = Generator::new(GeneratorKind::PositionGradient);
    let y = serde_json::to_value(g).unwrap();
    assert!(y.get("axis").is_none(), "Y is the default and is not written");
    assert_eq!(serde_json::from_value::<Generator>(y).unwrap().axis, GradientAxis::Y);
    g.axis = GradientAxis::Z;
    let z = serde_json::to_value(g).unwrap();
    assert_eq!(z["axis"], "z");
    assert_eq!(serde_json::from_value::<Generator>(z).unwrap(), g);
    let c = Generator::new(GeneratorKind::Concave);
    assert_eq!(serde_json::to_value(c).unwrap()["kind"], "concave");
    assert_eq!(serde_json::from_value::<Generator>(serde_json::to_value(c).unwrap()).unwrap(), c);
}

#[test]
fn text_layers_roundtrip_with_embedded_font_and_old_schemas_have_none() {
    use subcraft_doc::{Resource, ResourceKind, TextDecal};
    use subcraft_text::{Align, Direction, FontRef};
    let mut ts = TextureSet::new("Body", 0, 128);
    let mut layer = Layer::new_paint("Text: Hello", &ts);
    let font_id = subcraft_core::ResourceId::new();
    let mut resources = ResourceStore::default();
    let bytes = Arc::new(subcraft_text::BUNDLED_FONTS[2].1.to_vec());
    resources.insert(Resource { id: font_id, name: "DejaVu Serif (x.ttf)".into(), kind: ResourceKind::Font, bitmap: Arc::new(subcraft_image::Bitmap::new_f32(1, 1, 1, vec![0.0])), source_path: None, encoded: Some(bytes.clone()) });
    let mut decal = TextDecal::new("Hello\nمرحبا", glam::Vec2::new(0.3, 0.6), [(ChannelKind::BaseColor, ChannelValue::rgb(0.1, 0.2, 0.3))].into());
    decal.font = FontRef::Embedded { resource: font_id };
    decal.style.align = Align::Center;
    decal.style.direction = Direction::Rtl;
    decal.style.letter_spacing = 0.1;
    decal.size = 0.2; decal.rotation = 0.5; decal.opacity = 0.75;
    if let LayerContent::Paint(p) = &mut layer.content { p.text = Some(decal.clone()); }
    ts.insert_above(None, layer);
    let doc = Document { texture_sets: vec![ts] };
    let bytes_out = write_project(Cursor::new(Vec::new()), &primitives::cube(2.), &doc, &resources, &MeshMapSet::new()).unwrap().into_inner();
    let loaded = read_project(Cursor::new(bytes_out.clone())).unwrap();
    assert_eq!(loaded.document, doc);
    let font = loaded.resources.get(font_id).expect("font resource saved");
    assert_eq!((font.kind, font.encoded.as_deref()), (ResourceKind::Font, Some(&*bytes)));
    // The container stores the font as a normal file entry.
    let zip = zip::ZipArchive::new(Cursor::new(bytes_out)).unwrap();
    assert!(zip.file_names().any(|n| n.is_ok_and(|n| n == format!("resources/{}.ttf", font_id.0).as_str())));
    // JSON from schemas without text has no text layers; unknown future fields are not required.
    let paint: ContentV1 = serde_json::from_str(r#"{"type":"paint","channels":[],"path":null}"#).unwrap();
    assert!(matches!(paint, ContentV1::Paint { text: None, .. }));
}

#[test]
fn text_layer_schema_is_pinned_and_rejects_bad_values() {
    use subcraft_text::{Align, Direction, FontRef};
    let t = TextLayerV16 {
        text: "Hi".into(), font: FontRef::Bundled { name: "DejaVu Sans".into() }, letter_spacing: 0.0, line_spacing: 1.0,
        align: Align::Left, direction: Direction::Auto, center: [0.5, 0.5], size: 0.1, rotation: 0.0, opacity: 1.0,
        values: vec![PenChannelV4 { channel: ChannelKind::BaseColor, value: [1.0, 1.0, 1.0] }],
    };
    let json = serde_json::to_string(&t).unwrap();
    assert!(json.contains(r#""font":{"type":"bundled","name":"DejaVu Sans"}"#), "{json}");
    assert!(json.contains(r#""align":"left""#) && json.contains(r#""direction":"auto""#), "{json}");
    assert_eq!(serde_json::from_str::<TextLayerV16>(&json).unwrap(), t);
    let embedded = serde_json::to_string(&FontRef::System { family: "Arial".into(), bold: true, italic: false }).unwrap();
    assert_eq!(embedded, r#"{"type":"system","family":"Arial","bold":true,"italic":false}"#);
    // Bad parameters are rejected by validation, which the loader runs on every text layer.
    let mut decal = subcraft_doc::TextDecal::new("x", glam::Vec2::splat(0.5), [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 1.0, 1.0))].into());
    assert!(decal.validate().is_ok());
    decal.size = 9.0;
    assert!(decal.validate().is_err());
    decal.size = 0.1;
    decal.opacity = f32::NAN;
    assert!(decal.validate().is_err());
}

#[test]
fn painter_generator_controls_round_trip_and_are_absent_in_older_files() {
    let mut g = Generator::new(GeneratorKind::Cavity);
    assert!(serde_json::to_value(g).unwrap().get("painter").is_none(), "none is not written, so older files are unchanged");
    g.painter = Some(subcraft_doc::PainterControls { edges_masking: 0.5, ao_masking: 0.25, curvature_weight: 0.75, smoothness: 0.5, grunge_scale: 4.0, grunge_floor: 0.2 });
    let back: Generator = serde_json::from_value(serde_json::to_value(g).unwrap()).unwrap();
    assert_eq!(back, g);
    let mut json = serde_json::to_value(g).unwrap();
    json.as_object_mut().unwrap().remove("painter");
    assert_eq!(serde_json::from_value::<Generator>(json).unwrap().painter, None);
}
