//! Offscreen preview scenes for shelf thumbnails: a material / smart material composited on a
//! small mesh (a sphere for materials; a sphere with baked curvature + AO for smart materials and
//! masks, so their generators have mesh maps to read). Pure CPU; the UI renders the returned
//! composite.

use std::sync::Arc;

use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::MeshMapKind;
use subcraft_doc::{Layer, Resource, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::{Bvh, Mesh, primitives};

/// Texture-set resolution of the preview sphere (the composite the shelf thumbnail samples).
pub const THUMB_RESOLUTION: u32 = 256;

/// Preview-only tiling boost: fills are laid out for a whole model, so on a small sphere their
/// pattern is a few huge blobs. Thumbnails repeat UV-mapped fills this many times more
/// (triplanar / planar fills use [`PREVIEW_PROJECTED_BOOST`]). The saved material is untouched.
pub const PREVIEW_UV_BOOST: f32 = 3.0;
pub const PREVIEW_PROJECTED_BOOST: f32 = 1.6;

/// Scale the tiling of every fill in `layer` (and its children) for a thumbnail.
pub fn zoom_out_tiling(layer: &mut Layer) {
    use subcraft_doc::{LayerContent, ProjectionMode};
    match &mut layer.content {
        LayerContent::Fill(f) => {
            let k = if f.projection.mode == ProjectionMode::Uv { PREVIEW_UV_BOOST } else { PREVIEW_PROJECTED_BOOST };
            f.projection.scale = [f.projection.scale[0] * k, f.projection.scale[1] * k];
        }
        LayerContent::Group(g) => {
            for child in &mut g.children {
                zoom_out_tiling(Arc::make_mut(child));
            }
        }
        _ => {}
    }
}

pub struct PreviewScene {
    pub mesh: Arc<Mesh>,
    geometry: TextureSetGeometry,
    maps: MeshMaps,
    template: TextureSet,
    resources: ResourceStore,
}

impl PreviewScene {
    fn new(mesh: Mesh, bake_maps: bool, resolution: u32) -> Self {
        let mesh = Arc::new(mesh);
        let maps = if bake_maps {
            let bvh = Bvh::build(&mesh);
            let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
            let settings = BakeSettings {
                size: resolution,
                maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion],
                ao_samples: 16,
                curvature_radius: 0.03,
                ..BakeSettings::default()
            };
            MeshMaps::from_map(bake(&input, &settings, &Progress::new()).unwrap_or_default())
        } else {
            MeshMaps::new()
        };
        Self {
            geometry: TextureSetGeometry::new(mesh.clone(), 0, resolution),
            template: TextureSet::new("Preview", 0, resolution),
            mesh,
            maps,
            resources: ResourceStore::default(),
        }
    }

    /// A sphere (material presets).
    pub fn sphere() -> Self {
        Self::sphere_at(THUMB_RESOLUTION)
    }

    pub fn sphere_at(resolution: u32) -> Self {
        Self::new(primitives::uv_sphere(1.0, 48, 24), false, resolution)
    }

    /// A sphere with baked curvature + AO (smart materials and masks).
    pub fn baked_sphere() -> Self {
        Self::baked_sphere_at(THUMB_RESOLUTION)
    }

    pub fn baked_sphere_at(resolution: u32) -> Self {
        Self::new(primitives::uv_sphere(1.0, 48, 24), true, resolution)
    }

    /// A cube with baked curvature + AO (edges for wear generators).
    pub fn cube() -> Self {
        Self::new(primitives::cube(1.6), true, THUMB_RESOLUTION)
    }

    /// Texture set to build preview layers against.
    pub fn texture_set(&self) -> &TextureSet {
        &self.template
    }

    /// Composite of `layer` alone on this scene.
    pub fn composite(&self, layer: Layer) -> Composite {
        self.composite_with(layer, &[])
    }

    /// Composite of `layer` with the images it uses (a saved smart material's textures; without
    /// them its image fills would sample nothing).
    pub fn composite_with(&self, layer: Layer, images: &[Arc<Resource>]) -> Composite {
        let mut ts = self.template.clone();
        ts.insert_above(None, layer);
        let mut resources = self.resources.clone();
        for r in images {
            resources.insert_shared(r.clone());
        }
        let ctx = EvalContext { geometry: &self.geometry, resources: &resources, mesh_maps: &self.maps };
        composite_full(&ts, &ctx)
    }
}
