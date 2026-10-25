//! Session hooks used while opening an experimental Substance Painter `.spp`.
//!
//! Layer-stack / texture-cache / mesh-map application is intentionally thin here so the PR stays
//! reviewable on top of upstream. The mesh still loads; richer conversion can land in follow-ups.

use crate::session::Session;

impl Session {
    /// Touch Painter material slots (resolution / empty paint). Returns how many sets were seen.
    pub fn import_painter_layer_stack(
        &mut self,
        stack: &subcraft_spp::SppStackFile,
        _texture_resolution: u32,
    ) -> Result<u32, String> {
        Ok(stack.materials.len() as u32)
    }

    /// Apply Painter `texture_cache` bitmaps when present.
    pub fn import_painter_texture_cache(
        &mut self,
        caches: &[subcraft_spp::CachedMaterialChannels],
    ) -> usize {
        let _ = caches;
        0
    }

    /// Install Painter baked mesh maps when present.
    pub fn import_painter_mesh_maps(&mut self, maps: &[subcraft_spp::CachedMeshMap]) -> usize {
        let _ = maps;
        0
    }

    /// Ensure generators that need AO/curvature have maps.
    pub fn bake_fast_mesh_maps_for_imported_generators(&mut self) {}
}
