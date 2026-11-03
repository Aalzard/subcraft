//! Auto UV unwrap: replace the project's UV layout with a generated one and move the painted
//! work onto it. The unwrapping itself is `subcraft_mesh::unwrap`; the transfer is the Reproject
//! path of the mesh reload (ADR-0042), so paint, masks and baked maps follow the surface.

use std::path::PathBuf;
use std::sync::Arc;

use subcraft_mesh::unwrap::{UnwrapSettings, UnwrapStats, UvReport, analyze_uvs, unwrap};

use crate::mesh_reload::{MeshReloadJob, ReloadMode, ReloadOutput, prepare_mesh, transfer};
use crate::session::Session;

/// A finished background unwrap, ready for [`Session::finish_auto_uv`].
pub struct AutoUvOutput {
    reload: ReloadOutput,
    pub stats: UnwrapStats,
    /// Quality of the UVs that were replaced.
    pub before: UvReport,
}

impl Session {
    /// Quality of the current UV layout (overlap, flips, range, stretch).
    pub fn uv_report(&self) -> UvReport {
        analyze_uvs(&self.scene.mesh)
    }

    /// Unwrap the whole mesh in the background and reproject the painted rasters onto the result.
    pub fn start_auto_uv(&self, settings: UnwrapSettings) -> MeshReloadJob<AutoUvOutput> {
        let (mesh, bvh, doc, maps) = (self.scene.mesh.clone(), self.scene.bvh.clone(), self.history.current().clone(), self.mesh_map_set());
        let source = self.mesh_source().map_or_else(|| PathBuf::from("auto-uv"), |p| p.to_path_buf());
        MeshReloadJob::spawn(move |p| {
            p.set_stage("Unwrapping");
            let before = analyze_uvs(&mesh);
            let (unwrapped, stats) = unwrap(&mesh, &settings).map_err(|e| e.to_string())?;
            p.set_stage("Matching texture sets");
            let prepared = prepare_mesh(&source, unwrapped, Vec::new(), Arc::clone(&mesh), &bvh, &doc, p)?;
            let reload = transfer(&prepared, ReloadMode::Reproject, &bvh, &doc, &maps, p)?;
            Ok(AutoUvOutput { reload, stats, before })
        })
    }

    /// Install a finished unwrap. Not undoable (the history is cleared like any mesh reload).
    /// The remembered model file stays the original one.
    pub fn finish_auto_uv(&mut self, output: AutoUvOutput) -> Result<String, String> {
        let source = self.settings.mesh_source.clone();
        let (stats, before) = (output.stats, output.before);
        let report = self.finish_mesh_reload(output.reload);
        self.settings.mesh_source = source;
        let report = report?;
        log::info!("action: Auto UV Unwrap ({})", stats.summary());
        let mut s = format!("Auto UV unwrap: {}", stats.summary());
        if before.has_uvs {
            s += &format!(" (before: {})", before.summary());
        }
        if report.rasters_transferred > 0 {
            s += &format!(", {} raster(s) reprojected", report.rasters_transferred);
        }
        if !report.stale_maps.is_empty() {
            s += ". Rebake mesh maps";
        }
        Ok(s)
    }

    /// Both steps on the calling thread (tests, scripts).
    pub fn auto_uv_unwrap(&mut self, settings: UnwrapSettings) -> Result<String, String> {
        let output = self.start_auto_uv(settings).wait()?;
        self.finish_auto_uv(output)
    }
}
