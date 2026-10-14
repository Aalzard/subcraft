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
