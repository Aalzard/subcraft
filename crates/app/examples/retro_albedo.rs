//! Reproducible native/export fixture: imported pixel image, same material, two filters.
use std::{path::Path, sync::Arc};
use subcraft_app::{Command, Session};
use subcraft_core::{ChannelKind as C, ResourceId};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Resource, ResourceKind, ResourceStore};
use subcraft_image::{Bitmap, TextureFilter, encode_bitmap};

fn main() {
    let out = std::env::args().nth(1).expect("output directory");
    let out = Path::new(&out); std::fs::create_dir_all(out).unwrap();
    let mesh = subcraft_mesh::primitives::cube(2.);
    let mut doc = Session::new(mesh.clone(), 128).document().clone();
    let ts = &mut doc.texture_sets[0]; ts.layers.clear();
    let palette = [[32, 48, 80], [230, 144, 48], [72, 160, 152], [224, 216, 184]];
    let data = (0..16 * 16).flat_map(|i| palette[(i % 16 / 2 + i / 16 / 2) % 4]).collect();
    let bitmap = Bitmap::new_u8(16, 16, 3, data);
    let (bytes, _) = encode_bitmap(&bitmap).unwrap();
    let id = ResourceId::new(); let mut resources = ResourceStore::default();
    resources.insert(Resource { id, name: "Retro palette.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), encoded: Some(Arc::new(bytes)), source_path: None });
    let mut layer = Layer::new_fill("Imported pixel albedo", ts);
    if let LayerContent::Fill(fill) = &mut layer.content {
        for channel in fill.channels.values_mut() { channel.enabled = false; }
        fill.channels.insert(C::BaseColor, FillChannel::new(FillSource::Bitmap { resource: id }));
    }
    ts.layers.push(Arc::new(layer));
    let initial = out.join("Albedo-Bilinear.subcraft");
    subcraft_project::save_project(&initial, &mesh, &doc, &resources, &Default::default()).unwrap();
    let mut session = Session::open_project(&initial).unwrap();
    let ts = session.selection().texture_set;
    for filter in [TextureFilter::Bilinear, TextureFilter::Nearest] {
        session.execute(Command::SetAlbedoFilter { texture_set: ts, filter }).unwrap();
        let name = format!("Albedo-{}", filter.label());
        session.save(Some(&out.join(format!("{name}.subcraft")))).unwrap();
        session.export_textures(&out.join(&name), &Default::default()).unwrap();
        println!("{}", out.join(format!("{name}.subcraft")).display());
    }
}
