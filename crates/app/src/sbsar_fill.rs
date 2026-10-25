//! Build a fill layer + resources from baked texture paths (SBSAR shelf preview / import helpers).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_assets::texture_maps::{MapConversion, plan};
use subcraft_core::ResourceId;
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Resource, ResourceKind, ResourceStore, TextureSet};

fn load_bitmap_resource(path: &Path, conversion: MapConversion, resources: &mut ResourceStore) -> Result<ResourceId, String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bitmap = if conversion == MapConversion::None {
        subcraft_image::io::decode_bitmap(&bytes, Some(&name)).map_err(|e| format!("{name}: {e}"))?
    } else {
        let decoded = subcraft_image::io::decode_bitmap(&bytes, Some(&name)).map_err(|e| format!("{name}: {e}"))?;
        let converted = match conversion {
            MapConversion::Invert => decoded.map_color(|_, v| 1.0 - v),
            _ => decoded.map_color(|c, v| if c == 1 { 1.0 - v } else { v }),
        };
        converted
    };
    let id = ResourceId::new();
    resources.insert(Resource {
        id,
        name,
        kind: ResourceKind::Texture,
        bitmap: Arc::new(bitmap),
        source_path: Some(path.to_path_buf()),
        encoded: Some(Arc::new(bytes)),
    });
    Ok(id)
}

/// One fill layer using the texture-set channel plan (same rules as `import_texture_set`).
pub fn fill_layer_from_texture_paths(paths: &[PathBuf], ts: &TextureSet, resources: &mut ResourceStore) -> Result<Layer, String> {
    let plan = plan(paths);
    let mut layer = Layer::new_fill(plan.name.clone(), ts);
    let LayerContent::Fill(fill) = &mut layer.content else { unreachable!("new_fill") };
    for c in fill.channels.values_mut() {
        c.enabled = false;
    }
    let mut used = 0usize;
    for (path, m) in &plan.maps {
        if !ts.has_channel(m.channel) {
            continue;
        }
        let resource = load_bitmap_resource(path, m.conversion, resources)?;
        let channel_val = if let Some(res) = resources.get(resource) {
            crate::sbsar::fill_channel_for_baked_map(m.channel, &res.bitmap, resource)
        } else {
            FillChannel::new(FillSource::Bitmap { resource })
        };
        fill.channels.insert(m.channel, channel_val);
        used += 1;
    }
    if used == 0 {
        return Err("no texture maps recognized in SBSAR bake".into());
    }
    Ok(layer)
}
