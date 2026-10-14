//! Offscreen preview scenes for shelf thumbnails: a material / smart material composited on a
//! small mesh (a sphere for materials, a baked cube for smart materials so edge wear and cavity
//! dirt show up). Pure CPU; the UI renders the returned composite.

use std::sync::Arc;

use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::MeshMapKind;
use subcraft_doc::{Layer, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::{Bvh, Mesh, primitives};

pub const THUMB_RESOLUTION: u32 = 128;

pub struct PreviewScene {
    pub mesh: Arc<Mesh>,
    geometry: TextureSetGeometry,
    maps: MeshMaps,
    template: TextureSet,
    resources: ResourceStore,
}

impl PreviewScene {
    fn new(mesh: Mesh, bake_maps: bool) -> Self {
        let mesh = Arc::new(mesh);
        let maps = if bake_maps {
            let bvh = Bvh::build(&mesh);
            let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
            let settings = BakeSettings {
                size: THUMB_RESOLUTION,
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
            geometry: TextureSetGeometry::new(mesh.clone(), 0, THUMB_RESOLUTION),
            template: TextureSet::new("Preview", 0, THUMB_RESOLUTION),
            mesh,
            maps,
            resources: ResourceStore::default(),
        }
    }

    /// A sphere (material presets).
    pub fn sphere() -> Self {
        Self::new(primitives::uv_sphere(1.0, 48, 24), false)
    }

    /// A cube with baked curvature + AO (smart materials and masks).
    pub fn cube() -> Self {
        Self::new(primitives::cube(1.6), true)
    }

    /// Texture set to build preview layers against.
    pub fn texture_set(&self) -> &TextureSet {
        &self.template
    }

    /// Composite of `layer` alone on this scene.
    pub fn composite(&self, layer: Layer) -> Composite {
        let mut ts = self.template.clone();
        ts.insert_above(None, layer);
        let ctx = EvalContext { geometry: &self.geometry, resources: &self.resources, mesh_maps: &self.maps };
        composite_full(&ts, &ctx)
    }
}
