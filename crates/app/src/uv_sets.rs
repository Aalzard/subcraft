//! Several UV sets: a model can carry more than one UV layout (glTF `TEXCOORD_1`, FBX UV layers).
//! Each texture set is painted with one of them, set 0 by default. Switching uses the Reproject
//! path of the mesh reload (ADR-0042), like Auto UV, so painted work stays on the surface.

use std::path::PathBuf;
use std::sync::Arc;

use subcraft_core::TextureSetId;

use crate::mesh_reload::{MeshReloadJob, ReloadMode, ReloadOutput, prepare_mesh, transfer};
use crate::session::Session;

/// A finished background switch, ready for [`Session::finish_uv_set_switch`].
pub struct UvSetOutput {
    reload: ReloadOutput,
    texture_set: String,
    set_name: String,
}

/// The UV sets a texture set can be painted with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UvSetChoice {
    /// Names of every set of the model, in file order.
    pub names: Vec<String>,
    /// The set the texture set is painted with now.
    pub current: usize,
}

impl Session {
    /// The UV sets of texture set `ts`, or `None` when the model has only one (nothing to pick).
    pub fn uv_set_choice(&self, ts: TextureSetId) -> Option<UvSetChoice> {
        let mesh = &self.scene.mesh;
        if mesh.uv_set_count() < 2 {
            return None;
        }
        let slot = self.document().texture_set(ts)?.material_slot;
        Some(UvSetChoice { names: (0..mesh.uv_set_count()).map(|i| mesh.uv_set_name(i)).collect(), current: mesh.slot_uv_set(slot)? })
    }

    /// Paint texture set `ts` with UV set `set`, in the background. Its painted layers are
    /// reprojected onto the new layout so they stay where they were on the model.
    pub fn start_uv_set_switch(&self, ts: TextureSetId, set: usize) -> Result<MeshReloadJob<UvSetOutput>, String> {
        let choice = self.uv_set_choice(ts).ok_or("the model has only one UV set")?;
        if set >= choice.names.len() {
            return Err(format!("the model has no UV set {set}"));
        }
        let texture_set = self.document().texture_set(ts).ok_or("no such texture set")?;
        let (slot, name) = (texture_set.material_slot, texture_set.name.clone());
        let switched = self.scene.mesh.with_uv_set(&[slot], set).ok_or("the model's UV sets are incomplete")?;
        if switched.uvs.iter().all(|uv| *uv == switched.uvs[0]) {
            return Err(format!("UV set {} is empty", choice.names[set]));
        }
        let (mesh, bvh, doc, maps) = (self.scene.mesh.clone(), self.scene.bvh.clone(), self.history.current().clone(), self.mesh_map_set());
        let source = self.mesh_source().map_or_else(|| PathBuf::from("uv-set"), |p| p.to_path_buf());
        let set_name = choice.names[set].clone();
        Ok(MeshReloadJob::spawn(move |p| {
            p.set_stage("Matching texture sets");
            let prepared = prepare_mesh(&source, switched, Vec::new(), Arc::clone(&mesh), &bvh, &doc, p)?;
            let reload = transfer(&prepared, ReloadMode::Reproject, &bvh, &doc, &maps, p)?;
            Ok(UvSetOutput { reload, texture_set: name, set_name })
        }))
    }

    /// Install a finished switch. Not undoable (the history is cleared like any mesh reload).
    pub fn finish_uv_set_switch(&mut self, output: UvSetOutput) -> Result<String, String> {
        let source = self.settings.mesh_source.clone();
        let report = self.finish_mesh_reload(output.reload);
        self.settings.mesh_source = source;
        let report = report?;
        log::info!("action: UV set of {} -> {}", output.texture_set, output.set_name);
        let mut s = format!("{} is painted with UV set {}", output.texture_set, output.set_name);
        if report.rasters_transferred > 0 {
            s += &format!(", {} raster(s) reprojected", report.rasters_transferred);
        }
        if !report.stale_maps.is_empty() {
            s += ". Rebake mesh maps";
        }
        Ok(s)
    }

    /// Both steps on the calling thread (tests, scripts).
    pub fn set_uv_set(&mut self, ts: TextureSetId, set: usize) -> Result<String, String> {
        let output = self.start_uv_set_switch(ts, set)?.wait()?;
        self.finish_uv_set_switch(output)
    }
}
