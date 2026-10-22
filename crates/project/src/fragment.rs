//! Library fragments: a saved smart material (layer subtree, `.scsmart`) or smart mask (mask
//! effect stack, `.scmask`). Same zip container idea and the same layer / effect schema as
//! projects, so both evolve together:
//!
//! ```text
//! fragment.json        format, schema version, kind, name, source resolution, layers / effects
//! rasters/NNNNN.tiles  paint channels, mask bases, mask paint effects
//! resources/<id>.<ext> images used by fills and image masks (original bytes)
//! ```

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use subcraft_doc::{Layer, MaskEffect, MaskEffectKind, ResourceStore};
use zip::write::SimpleFileOptions;

use crate::schema::{LayerV1, MaskEffectV1, ResourceV1, SCHEMA_VERSION};
use crate::{Loader, ProjectError, Writer, pack_resources, to_json, unpack_resources, write_payload};

pub const FRAGMENT_FORMAT: &str = "subcraft-fragment";
pub const SMART_MATERIAL_EXTENSION: &str = "scsmart";
pub const SMART_MASK_EXTENSION: &str = "scmask";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentKind {
    SmartMaterial,
    SmartMask,
}

impl FragmentKind {
    pub fn extension(self) -> &'static str {
        match self {
            FragmentKind::SmartMaterial => SMART_MATERIAL_EXTENSION,
            FragmentKind::SmartMask => SMART_MASK_EXTENSION,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct FragmentFile {
    format: String,
    schema_version: u32,
    writer: String,
    kind: FragmentKind,
    name: String,
    /// Resolution of the rasters (the texture set it was saved from).
    resolution: u32,
    #[serde(default)]
    layers: Vec<LayerV1>,
    #[serde(default)]
    mask_effects: Vec<MaskEffectV1>,
    #[serde(default)]
    resources: Vec<ResourceV1>,
}

/// A smart material (`layers`, usually one folder) or a smart mask (`mask_effects`).
#[derive(Clone, Debug)]
pub struct Fragment {
    pub kind: FragmentKind,
    pub name: String,
    pub resolution: u32,
    pub layers: Vec<Layer>,
    pub mask_effects: Vec<MaskEffect>,
    /// Images used by the layers / effects.
    pub resources: ResourceStore,
}

impl Fragment {
    /// Resources referenced by the layers and effects, in order.
    pub fn referenced_resources(&self) -> Vec<subcraft_core::ResourceId> {
        let mut out = Vec::new();
        for l in &self.layers {
            l.collect_resources(&mut out);
        }
        for e in &self.mask_effects {
            if let MaskEffectKind::Image { resource, .. } = e.kind
                && !out.contains(&resource)
            {
                out.push(resource);
            }
        }
        out
    }
}

pub fn write_fragment<W: Write + Seek>(w: W, f: &Fragment) -> Result<W, ProjectError> {
    let mut writer = Writer { rasters: Vec::new() };
    let layers = f.layers.iter().map(|l| writer.layer(l)).collect();
    let mask_effects = f.mask_effects.iter().map(|e| writer.mask_effect(e)).collect();
    let (resources, res_files) = pack_resources(&f.referenced_resources(), &f.resources);
    let file = FragmentFile {
        format: FRAGMENT_FORMAT.into(),
        schema_version: SCHEMA_VERSION,
        writer: format!("SubCraft {}", env!("CARGO_PKG_VERSION")),
        kind: f.kind,
        name: f.name.clone(),
        resolution: f.resolution,
        layers,
        mask_effects,
        resources,
    };
    let mut zip = zip::ZipWriter::new(w);
    zip.start_file("fragment.json", SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
    zip.write_all(to_json(&file).as_bytes())?;
    write_payload(&mut zip, &writer.rasters, &res_files)?;
    Ok(zip.finish()?)
}

/// Save atomically (temp file + rename).
pub fn save_fragment(path: &Path, f: &Fragment) -> Result<(), ProjectError> {
    let tmp = crate::temp_path(path);
    let result = (|| -> Result<(), ProjectError> {
        let file = std::fs::File::create(&tmp)?;
        let buf = write_fragment(std::io::BufWriter::new(file), f)?;
        let file = buf.into_inner().map_err(|e| ProjectError::Io(e.into_error()))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn read_fragment<R: Read + Seek>(r: R) -> Result<Fragment, ProjectError> {
    let mut zip = zip::ZipArchive::new(r)?;
    let mut loader = Loader { zip: &mut zip, cache: BTreeMap::new() };
    let json = loader.bytes("fragment.json").map_err(|_| ProjectError::NotAProject)?;
    let file: FragmentFile = serde_json::from_slice(&json).map_err(|e| ProjectError::Invalid(format!("fragment.json: {e}")))?;
    if file.format != FRAGMENT_FORMAT {
        return Err(ProjectError::NotAProject);
    }
    if file.schema_version > SCHEMA_VERSION || file.schema_version == 0 {
        return Err(ProjectError::UnsupportedVersion(file.schema_version));
    }
    if !subcraft_doc::RESOLUTIONS.contains(&file.resolution) {
        return Err(ProjectError::Invalid(format!("unsupported resolution {}", file.resolution)));
    }
    let layers = file.layers.iter().map(|l| loader.layer(l, file.resolution)).collect::<Result<Vec<_>, _>>()?;
    let mask_effects = file.mask_effects.iter().map(|e| loader.mask_effect(e, file.resolution)).collect::<Result<Vec<_>, _>>()?;
    let resources = unpack_resources(&mut loader, &file.resources)?;
    Ok(Fragment { kind: file.kind, name: file.name, resolution: file.resolution, layers, mask_effects, resources })
}

pub fn load_fragment(path: &Path) -> Result<Fragment, ProjectError> {
    return read_fragment(std::io::BufReader::new(std::fs::File::open(path)?));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use subcraft_core::{ChannelKind, ResourceId};
    use subcraft_doc::{Generator, GeneratorKind, LayerContent, Mask, Projection, Resource, ResourceKind, TextureSet};

    use super::*;

    fn png_bytes() -> Vec<u8> {
        let b = subcraft_image::Bitmap::new_u8(2, 2, 3, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]);
        subcraft_image::encode_bitmap(&b).unwrap().0
    }

    #[test]
    fn smart_material_roundtrip_with_paint_mask_and_image() {
        let mut ts = TextureSet::new("Body", 0, 256);
        let tc = ts.clone();
        let mut folder = Layer::new_folder("Painted Metal", &tc);
        let mut paint = Layer::new_paint("Scratches", &tc);
        if let LayerContent::Paint(p) = &mut paint.content {
            p.channel_mut(ChannelKind::Roughness, 256).set(10, 10, [0.9, 1.0, 0.0, 0.0]);
        }
        let img = ResourceId::new();
        let mut mask = Mask::new(256, false);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)))));
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Image { resource: img, projection: Projection::default(), invert: false })));
        paint.mask = Some(mask);
        if let LayerContent::Group(g) = &mut folder.content {
            g.children.push(Arc::new(paint));
        }
        ts.insert_above(None, folder.clone());

        let mut resources = ResourceStore::default();
        let bytes = png_bytes();
        resources.insert(Resource {
            id: img,
            name: "logo.png".into(),
            kind: ResourceKind::Texture,
            bitmap: Arc::new(subcraft_image::io::decode_bitmap(&bytes, None).unwrap()),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        });
        let unused = ResourceId::new();
        resources.insert(Resource {
            id: unused,
            name: "unused.png".into(),
            kind: ResourceKind::Texture,
            bitmap: Arc::new(subcraft_image::io::decode_bitmap(&png_bytes(), None).unwrap()),
            source_path: None,
            encoded: Some(Arc::new(png_bytes())),
        });

        let f = Fragment {
            kind: FragmentKind::SmartMaterial,
            name: "Painted Metal".into(),
            resolution: 256,
            layers: vec![folder.clone()],
            mask_effects: Vec::new(),
            resources,
        };
        let buf = write_fragment(Cursor::new(Vec::new()), &f).unwrap().into_inner();
        let back = read_fragment(Cursor::new(buf)).unwrap();
        assert_eq!(back.kind, FragmentKind::SmartMaterial);
        assert_eq!(back.name, "Painted Metal");
        assert_eq!(back.resolution, 256);
        assert_eq!(back.layers, vec![folder], "layer tree, rasters and mask round-trip exactly");
        assert!(back.resources.get(img).is_some(), "image travels with the smart material");
        assert!(back.resources.get(unused).is_none(), "only referenced images are stored");
    }

    #[test]
    fn smart_mask_roundtrip_and_rejections() {
        let mut g = Generator::new(GeneratorKind::Cavity);
        g.level = 0.3;
        let effects = vec![MaskEffect::new(MaskEffectKind::Generator(g)), MaskEffect::new(MaskEffectKind::Invert)];
        let f = Fragment {
            kind: FragmentKind::SmartMask,
            name: "Dirt".into(),
            resolution: 128,
            layers: Vec::new(),
            mask_effects: effects.clone(),
            resources: ResourceStore::default(),
        };
        let buf = write_fragment(Cursor::new(Vec::new()), &f).unwrap().into_inner();
        let back = read_fragment(Cursor::new(buf)).unwrap();
        assert_eq!(back.mask_effects, effects);

        // A project file is not a fragment; garbage is an error, not a panic.
        let doc = subcraft_doc::Document { texture_sets: vec![TextureSet::new("T", 0, 128)] };
        let project = crate::write_project(
            Cursor::new(Vec::new()),
            &subcraft_mesh::primitives::cube(1.0),
            &doc,
            &ResourceStore::default(),
            &crate::MeshMapSet::new(),
        )
        .unwrap()
        .into_inner();
        assert!(matches!(read_fragment(Cursor::new(project)), Err(ProjectError::NotAProject)));
        assert!(read_fragment(Cursor::new(b"nope".to_vec())).is_err());
    }
}
