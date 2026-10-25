use std::path::{Path, PathBuf};

use subcraft_spp::{inspect, log_step, open_pack, SppExtractManifest};

use crate::session::{Session, DEFAULT_RESOLUTION};

/// Same decode flags as the web viewer (`SppInspectReport.decode`).
#[derive(Debug, Clone, Default)]
pub struct SppFormatProbe {
    pub painted_pixels_decodable: bool,
    pub procedural_decodable: bool,
    pub texture_cache_tiles: usize,
    pub texture_cache_mib: u64,
    pub procedural_nodes: usize,
    pub mask_or_filter_nodes: usize,
}

#[derive(Debug)]
pub struct SppOpenSummary {
    pub painter_major: u32,
    pub mesh_assbin: PathBuf,
    pub mesh_glb: Option<PathBuf>,
    pub sbsar_paths: Vec<PathBuf>,
    pub limitations: Vec<String>,
    pub mesh_loaded: bool,
    pub layers_imported: u32,
    pub format_probe: Option<SppFormatProbe>,
}

pub struct SppOpenResult {
    pub session: Option<Session>,
    pub summary: SppOpenSummary,
}

/// Cap Painter 8192² stacks for SubCraft import (env `SUBCRAFT_SPP_MAX_RESOLUTION`).
/// Defaults to `2048` for 1–8 texture sets and `1024` for larger scenes so CPU compositing
/// and layer toggling remain interactive unless overridden via `SUBCRAFT_SPP_MAX_RESOLUTION`.
pub fn max_texture_resolution_for_sets(painter_res: u32, material_count: usize) -> u32 {
    let default_cap = if material_count <= 8 {
        2048
    } else {
        1024
    };
    let cap = std::env::var("SUBCRAFT_SPP_MAX_RESOLUTION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_cap);
    painter_res.clamp(128, cap)
}

pub fn max_texture_resolution(painter_res: u32) -> u32 {
    max_texture_resolution_for_sets(painter_res, 1)
}

/// Returns `true` if `mat` has more than an empty default `"Layer 1"` placeholder.
pub fn is_active_spp_material(mat: &subcraft_spp::SppMaterialStack) -> bool {
    let layers = mat
        .texture_sets
        .first()
        .map(|ts| ts.layers.as_slice())
        .unwrap_or(&[]);
    if layers.len() > 1 {
        return true;
    }
    let Some(first) = layers.first() else {
        return false;
    };
    !first.children.is_empty()
        || first.base_color.is_some()
        || first.roughness.is_some()
        || first.metallic.is_some()
        || first.height.is_some()
        || !first.mask_effects.is_empty()
        || !first.mask_decal_hits.is_empty()
        || !first.mask_paint_strokes.is_empty()
}

impl Session {
    /// Extract `.spp`, load mesh (GLB), and rebuild layer stacks from Painter `document.bin`.
    pub fn open_from_spp(spp: &Path) -> Result<SppOpenResult, String> {
        log::info!("Opening SPP: {}", spp.display());
        log_step(&format!("SubCraft: open_from_spp {}", spp.display()));
        let out_dir = std::env::temp_dir().join(format!(
            "subcraft-spp-{}",
            spp.file_stem().and_then(|s| s.to_str()).unwrap_or("project")
        ));
        log_step("SubCraft: open_pack (HDF5 + HBO layers)");
        let (stack, manifest) = open_pack(spp, &out_dir)?;
        let painter_res = stack
            .materials
            .iter()
            .map(|m| m.resolution)
            .max()
            .unwrap_or(DEFAULT_RESOLUTION);
        let active_sets = stack
            .materials
            .iter()
            .filter(|m| is_active_spp_material(m))
            .count()
            .max(1);
        let resolution = max_texture_resolution_for_sets(painter_res, active_sets);
        log_step(&format!(
            "SubCraft: Painter resolution {}² → import {}² ({active_sets}/{} active sets, SUBCRAFT_SPP_MAX_RESOLUTION)",
            painter_res,
            resolution,
            stack.materials.len()
        ));

        let mut summary = SppOpenSummary::from_manifest(manifest);
        let mut real_mesh = false;
        let prefer_glb = std::env::var("SUBCRAFT_SPP_USE_GLB")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let glb_path = summary.mesh_glb.as_ref().filter(|p| p.is_file());
        let use_glb_first = prefer_glb && glb_path.is_some();
        let mut session = if use_glb_first {
            let glb = glb_path.unwrap();
            let (s, warnings) = Session::open_mesh(glb, resolution)?;
            for w in warnings {
                log::warn!("SPP mesh: {w}");
            }
            real_mesh = true;
            Some(s)
        } else if summary.mesh_assbin.is_file() {
            let bytes = std::fs::read(&summary.mesh_assbin)
                .map_err(|e| format!("{}: {e}", summary.mesh_assbin.display()))?;
            match subcraft_mesh::import_assbin_bytes(&bytes) {
                Ok(report) => {
                    log::info!("SPP: mesh loaded from in-process assbin decoder");
                    real_mesh = true;
                    for w in report.warnings {
                        log::warn!("SPP mesh: {w}");
                    }
                    Some(Session::new(report.mesh, resolution))
                }
                Err(e) => {
                    log::info!("SPP native assbin: {e} — placeholder mesh");
                    Some(Session::new(
                        subcraft_mesh::primitives::sample_crate(),
                        resolution,
                    ))
                }
            }
        } else if let Some(glb) = glb_path {
            let (s, warnings) = Session::open_mesh(glb, resolution)?;
            for w in warnings {
                log::warn!("SPP mesh: {w}");
            }
            real_mesh = true;
            Some(s)
        } else {
            log::info!("SPP: using placeholder mesh (layers still import)");
            Some(Session::new(
                subcraft_mesh::primitives::sample_crate(),
                resolution,
            ))
        };

        let mut layers_imported = 0u32;
        if let Some(ref mut s) = session {
            log_step("SubCraft: import layer tree into document");
            layers_imported = s.import_painter_layer_stack(&stack, resolution)?;
            log_step("SubCraft: decode Painter texture-cache channels");
            match subcraft_spp::decode_texture_cache(spp, resolution) {
                Ok(caches) if !caches.is_empty() => {
                    let n = s.import_painter_texture_cache(&caches);
                    log::info!("SPP: applied texture-cache channels to {n} texture sets");
                }
                Ok(_) => log::info!("SPP: no texture-cache channels found"),
                Err(e) => log::warn!("SPP texture-cache decode failed: {e}"),
            }
            log_step("SubCraft: import Painter baked mesh maps");
            match subcraft_spp::decode_painter_mesh_maps(spp, resolution) {
                Ok(maps) if !maps.is_empty() => {
                    let n = s.import_painter_mesh_maps(&maps);
                    log::info!("SPP: imported {n} Painter mesh maps");
                }
                Ok(_) => log::info!("SPP: no Painter mesh maps found"),
                Err(e) => log::warn!("SPP mesh-map decode failed: {e}"),
            }
            if real_mesh {
                log_step("SubCraft: bake missing Curvature/AO mesh maps for imported mask generators");
                s.bake_fast_mesh_maps_for_imported_generators();
            }
            s.set_loaded_path(spp.to_path_buf());
            log_step("SubCraft: pre-evaluate composites on background worker");
            s.update_composites();
        }
        summary.mesh_loaded = real_mesh;
        summary.layers_imported = layers_imported;
        if env_inspect_on_open() {
            log_step("SubCraft: optional format inspect (SUBCRAFT_SPP_INSPECT=1)");
            summary.format_probe = probe_spp_format(spp);
        }
        log_step("SubCraft: SPP open finished");

        Ok(SppOpenResult { session, summary })
    }
}

fn env_inspect_on_open() -> bool {
    std::env::var("SUBCRAFT_SPP_INSPECT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn probe_spp_format(spp: &Path) -> Option<SppFormatProbe> {
    let report = match inspect(spp, false) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("SPP format probe: {e}");
            return None;
        }
    };
    Some(SppFormatProbe {
        painted_pixels_decodable: report.decode.painted_pixels,
        procedural_decodable: report.decode.procedural_generators,
        texture_cache_tiles: report.texture_cache.tile_datasets,
        texture_cache_mib: report.texture_cache.total_tile_bytes / (1024 * 1024),
        procedural_nodes: report.hbo_generators.procedural_nodes,
        mask_or_filter_nodes: report.hbo_generators.mask_or_filter_nodes,
    })
}

impl SppOpenSummary {
    fn from_manifest(m: SppExtractManifest) -> Self {
        Self {
            painter_major: m.painter_version.major,
            mesh_assbin: m.mesh_assbin,
            mesh_glb: m.mesh_glb,
            sbsar_paths: m.embedded_sbsar,
            limitations: m.limitations,
            mesh_loaded: false,
            layers_imported: 0,
            format_probe: None,
        }
    }

    pub fn status_message(&self) -> String {
        let mut s = format!("Opened Painter {}.x project. ", self.painter_major);
        if self.mesh_loaded {
            s.push_str("Mesh loaded. ");
        } else {
            s.push_str(
                "Placeholder mesh (install Painter for assimp.dll, or native assbin port in progress). ",
            );
        }
        if self.layers_imported > 0 {
            s.push_str(&format!(
                "{} layer(s) imported at {}² (Painter may be higher; set SUBCRAFT_SPP_MAX_RESOLUTION). ",
                self.layers_imported,
                std::env::var("SUBCRAFT_SPP_MAX_RESOLUTION")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4096)
            ));
        }
        if let Some(p) = &self.format_probe {
            if p.painted_pixels_decodable {
                s.push_str(&format!(
                    "Texture-cache readable ({} tiles, ~{} MiB); painted pixels not in viewport yet. ",
                    p.texture_cache_tiles,
                    p.texture_cache_mib
                ));
            }
            if p.procedural_decodable {
                s.push_str(&format!(
                    "Generators/masks in document.bin ({} procedural, {} mask/filter nodes); SubCraft mapping pending. ",
                    p.procedural_nodes,
                    p.mask_or_filter_nodes
                ));
            }
        }
        if !self.sbsar_paths.is_empty() {
            s.push_str(&format!("{} .sbsar in Shelf → Materials. ", self.sbsar_paths.len()));
        }
        s.push_str("Optional: SUBCRAFT_SPP_SBSAR=1.");
        s
    }
}
