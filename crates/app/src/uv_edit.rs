//! Edit UV islands: move, rotate, scale and flip whole islands of the selected texture set and
//! move the painted work with them. The geometry is `subcraft_mesh::uv_edit`; the transfer is the
//! Reproject path of the mesh reload (ADR-0042), like Auto UV unwrap, so paint and masks stay on
//! the surface.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec2;
use subcraft_mesh::topology::uv_islands;
use subcraft_mesh::uv_edit::{UvTransform, preview, transform_islands};

use crate::mesh_reload::{MeshReloadJob, ReloadMode, ReloadOutput, prepare_mesh, transfer};
use crate::session::Session;

/// A finished background island edit, ready for [`Session::finish_uv_edit`].
pub struct UvEditOutput {
    reload: ReloadOutput,
    islands: usize,
}

impl Session {
    /// The UV island of every triangle of the model (ids are shared by the whole model). The 2D
    /// view keeps this while islands are edited and passes it back to the queries below.
    pub fn uv_islands(&self) -> Vec<u32> {
        uv_islands(&self.scene.mesh)
    }

    /// The island under a 2D-view texture position (UV 0..1) of the selected texture set.
    pub fn uv_island_at(&self, islands: &[u32], uv: Vec2) -> Option<u32> {
        let t = self.triangle_at_uv(uv)?;
        islands.get(t as usize).copied()
    }

    /// Islands of the selected texture set with a triangle inside the UV box `[a, b]`.
    pub fn uv_islands_in_box(&self, islands: &[u32], a: Vec2, b: Vec2) -> BTreeSet<u32> {
        let slot = self.selected_texture_set().material_slot;
        subcraft_paint::select::triangles_in_uv_box(&self.scene.mesh, slot, a.min(b), a.max(b)).into_iter().filter_map(|t| islands.get(t as usize).copied()).collect()
    }

    /// Every island of the selected texture set.
    pub fn uv_islands_of_texture_set(&self, islands: &[u32]) -> BTreeSet<u32> {
        let slot = self.selected_texture_set().material_slot;
        self.scene.mesh.material_triangles(slot).filter_map(|t| islands.get(t).copied()).collect()
    }

    /// The selected islands' UV triangles now (`t` = `None`) or after `t`, in the 2D view's tile
    /// (each triangle keeps the tile it starts in, so a move by a whole tile shows it leaving).
    pub fn uv_edit_triangles(&self, islands: &[u32], selected: &BTreeSet<u32>, t: Option<&UvTransform>) -> Vec<[Vec2; 3]> {
        let mesh = &self.scene.mesh;
        let slot = self.selected_texture_set().material_slot;
        // Islands of other texture sets are not edited from this one.
        let in_set = self.uv_islands_of_texture_set(islands);
        let keep: BTreeSet<u32> = selected.intersection(&in_set).copied().collect();
        let moved = preview(mesh, islands, &keep, t.unwrap_or(&UvTransform::default()));
        let tris = (0..mesh.triangle_count()).filter(|&tri| keep.contains(&islands[tri]));
        tris.zip(moved)
            .filter(|(tri, _)| mesh.tri_material.get(*tri).is_none_or(|m| *m == slot))
            .map(|(tri, uv)| {
                let (_, tile) = subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(tri));
                let off = Vec2::new(tile[0] as f32, tile[1] as f32);
                uv.map(|p| p - off)
            })
            .collect()
    }

    /// Apply `t` to the `selected` islands of the selected texture set in the background and
    /// reproject the painted rasters onto the edited layout.
    pub fn start_uv_edit(&self, selected: BTreeSet<u32>, t: UvTransform) -> Result<MeshReloadJob<UvEditOutput>, String> {
        if t.is_identity() {
            return Err("the transform changes nothing".into());
        }
        let islands = uv_islands(&self.scene.mesh);
        let in_set = self.uv_islands_of_texture_set(&islands);
        let selected: BTreeSet<u32> = selected.into_iter().filter(|i| in_set.contains(i)).collect();
        if selected.is_empty() {
            return Err("select at least one UV island of this texture set".into());
        }
        let (mesh, bvh, doc, maps) = (self.scene.mesh.clone(), self.scene.bvh.clone(), self.history.current().clone(), self.mesh_map_set());
        let source = self.mesh_source().map_or_else(|| PathBuf::from("uv-edit"), |p| p.to_path_buf());
        Ok(MeshReloadJob::spawn(move |p| {
            p.set_stage("Editing UV islands");
            let edited = transform_islands(&mesh, &islands, &selected, &t).map_err(|e| e.to_string())?;
            p.set_stage("Matching texture sets");
            let prepared = prepare_mesh(&source, edited, Vec::new(), Arc::clone(&mesh), &bvh, &doc, p)?;
            let reload = transfer(&prepared, ReloadMode::Reproject, &bvh, &doc, &maps, p)?;
            Ok(UvEditOutput { reload, islands: selected.len() })
        }))
    }

    /// Install a finished island edit. Not undoable (the history is cleared like any mesh reload).
    /// The remembered model file stays the original one.
    pub fn finish_uv_edit(&mut self, output: UvEditOutput) -> Result<String, String> {
        let source = self.settings.mesh_source.clone();
        let islands = output.islands;
        let report = self.finish_mesh_reload(output.reload);
        self.settings.mesh_source = source;
        let report = report?;
        log::info!("action: Edit UV islands ({islands} island(s))");
        let mut s = format!("Edited {islands} UV island(s)");
        if report.rasters_transferred > 0 {
            s += &format!(", {} raster(s) reprojected", report.rasters_transferred);
        }
        if !report.stale_maps.is_empty() {
            s += ". Rebake mesh maps";
        }
        Ok(s)
    }

    /// Both steps on the calling thread (tests, scripts).
    pub fn edit_uv_islands(&mut self, selected: BTreeSet<u32>, t: UvTransform) -> Result<String, String> {
        let output = self.start_uv_edit(selected, t)?.wait()?;
        self.finish_uv_edit(output)
    }
}
