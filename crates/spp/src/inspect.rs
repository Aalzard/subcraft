//! Read-only `.spp` report for debugging and the web viewer (no full `resources/` scan).

use std::path::Path;

use hdf5_reader::Hdf5File;
use serde::Serialize;

use crate::build_stack_from_document;
use crate::count_hbo_features;
use crate::generators_decodable;
use crate::native::{
    probe_texture_cache, read_container, read_container_layers_only, read_magic_only,
    resolve_spp_path, texture_cache_decodable, TextureCacheProbe,
};
use crate::HboFeatureCounts;

#[derive(Debug, Clone, Serialize)]
pub struct DecodeStatus {
    /// Container layout (HDF5 paths, Painter version).
    pub container: bool,
    /// HBO registry `document.bin` → layer/material tree.
    pub document_hbo: bool,
    /// `scene.assbin` → mesh (needs Assimp or assimp-sys).
    pub mesh_assbin: bool,
    /// Painted tiles from `texture-cache/` (not implemented).
    pub painted_pixels: bool,
    /// Procedural / generator graphs → SubCraft fills (partial).
    pub procedural_generators: bool,
}

impl DecodeStatus {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.container {
            parts.push("Container");
        }
        if self.document_hbo {
            parts.push("Layer-Baum (HBO)");
        }
        if self.mesh_assbin {
            parts.push("Mesh (Assbin)");
        }
        if self.painted_pixels {
            parts.push("texture-cache (SVT)");
        }
        if self.procedural_generators {
            parts.push("Generatoren/Masken (HBO)");
        }
        if self.painted_pixels && self.procedural_generators {
            return format!(
                "Struktur entschlüsselt: {} — Raster-Import in SubCraft folgt",
                parts.join(", ")
            );
        }
        let mut missing = Vec::new();
        if !self.painted_pixels {
            missing.push("texture-cache");
        }
        if !self.procedural_generators {
            missing.push("Generatoren");
        }
        if missing.is_empty() {
            parts.join(", ")
        } else {
            format!("Teilweise: {} (offen: {})", parts.join(", "), missing.join(", "))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialSummary {
    pub scene_material: String,
    pub resolution: u32,
    pub texture_set: String,
    pub root_layers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextureCacheSummary {
    pub present: bool,
    pub header_bytes: usize,
    pub header_version: Option<u32>,
    pub material_caches: usize,
    pub tile_datasets: usize,
    pub total_tile_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HboGeneratorSummary {
    pub procedural_nodes: usize,
    pub mask_or_filter_nodes: usize,
    pub distinct_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SppInspectReport {
    pub source_path: String,
    pub file_bytes: u64,
    pub painter_major: u32,
    pub hbo_format: String,
    pub hbo_data_version: u32,
    pub document_bin_bytes: usize,
    pub scene_assbin_bytes: usize,
    pub materials: Vec<MaterialSummary>,
    pub total_layers: usize,
    pub hdf5_top_level: Vec<String>,
    pub decode: DecodeStatus,
    pub decode_summary: String,
    pub texture_cache: TextureCacheSummary,
    pub hbo_generators: HboGeneratorSummary,
    pub notes: Vec<String>,
}

fn texture_cache_summary(probe: &TextureCacheProbe) -> TextureCacheSummary {
    TextureCacheSummary {
        present: probe.present,
        header_bytes: probe.header_bytes,
        header_version: probe.header_version,
        material_caches: probe.material_caches,
        tile_datasets: probe.tile_datasets,
        total_tile_bytes: probe.total_tile_bytes,
    }
}

fn hbo_generator_summary(counts: &HboFeatureCounts) -> HboGeneratorSummary {
    HboGeneratorSummary {
        procedural_nodes: counts.procedural_nodes,
        mask_or_filter_nodes: counts.mask_or_filter_nodes,
        distinct_types: counts.distinct_types.clone(),
    }
}

pub fn painter_major_from_settings(settings: &[u8]) -> u32 {
    painter_major(settings)
}

fn painter_major(settings: &[u8]) -> u32 {
    if let Some(v) = painter_major_from_text(&String::from_utf8_lossy(settings)) {
        return v;
    }
    // Painter stores JSON inside UTF-16LE `projectsettings.ini`.
    let u16_text: String = settings
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .filter(|&u| u != 0)
        .filter_map(|u| char::from_u32(u as u32))
        .collect();
    painter_major_from_text(&u16_text).unwrap_or(0)
}

/// Painter 11 `document.bin` commonly uses HBO data_version 82.
fn infer_painter_major(hbo_data_version: u32) -> u32 {
    match hbo_data_version {
        82 => 11,
        _ => 0,
    }
}

fn painter_major_from_text(text: &str) -> Option<u32> {
    text
        .split("\"version\"")
        .nth(1)
        .and_then(|rest| rest.split(':').nth(1))
        .and_then(|n| n.trim().trim_matches([',', '"'].as_ref()).parse().ok())
}

fn count_layers(nodes: &[crate::SppLayerNode]) -> usize {
    nodes.iter().map(|n| 1 + count_layers(&n.children)).sum()
}

fn hdf5_top_level(path: &Path) -> Vec<String> {
    let file = match Hdf5File::open(path) {
        Ok(f) => f,
        Err(e) => return vec![format!("HDF5 open error: {e}")],
    };
    let root = match file.root_group() {
        Ok(g) => g,
        Err(e) => return vec![format!("root group: {e}")],
    };
    let mut out = Vec::new();
    if let Ok((groups, datasets)) = root.members() {
        for g in groups {
            out.push(format!("group /{}", g.name()));
        }
        for d in datasets {
            out.push(format!("dataset /{}", d.name()));
        }
    }
    out.sort();
    out
}

/// Inspect a `.spp` without extracting SBSAR or reading `texture-cache`.
/// Set `include_mesh` to read `paint/scene.assbin` (~2 MB) for viewers / Assimp.
pub fn inspect(spp: &Path, include_mesh: bool) -> Result<SppInspectReport, String> {
    read_magic_only(spp)?;
    let file_bytes = std::fs::metadata(spp).map(|m| m.len()).unwrap_or(0);
    let local = resolve_spp_path(spp);
    let container = if include_mesh {
        read_container(spp)?
    } else {
        read_container_layers_only(spp)?
    };
    let (stack, hbo_format, data_version) = build_stack_from_document(&container.document_bin)?;
    let painter_major = painter_major(&container.settings_ini).max(infer_painter_major(data_version));

    let mut materials = Vec::new();
    let mut total_layers = 0usize;
    for mat in &stack.materials {
        for ts in &mat.texture_sets {
            let n = count_layers(&ts.layers);
            total_layers += n;
            materials.push(MaterialSummary {
                scene_material: mat.scene_material.clone(),
                resolution: mat.resolution,
                texture_set: ts.name.clone(),
                root_layers: n,
            });
        }
    }

    let tc_probe = probe_texture_cache(&local)?;
    let hbo_counts = count_hbo_features(&container.document_bin)?;
    let painted_pixels = texture_cache_decodable(&tc_probe);
    let procedural_generators = generators_decodable(&hbo_counts);

    let decode = DecodeStatus {
        container: true,
        document_hbo: stack.materials.iter().any(|m| !m.texture_sets.is_empty()),
        mesh_assbin: container.scene_assbin.len() > 100,
        painted_pixels,
        procedural_generators,
    };

    let notes = vec![
        "HDF5: projectsettings.ini, paint/scene.assbin, paint/document.bin, resources/, texture-cache/.".into(),
        "HBO v11 registry in document.bin — Layer-Namen/Ordner decodierbar (siehe crates/spp/src/hbo).".into(),
        if painted_pixels {
            format!(
                "texture-cache: SVT-Header v{} — {} Cache-Einträge, {} Tile-Datasets (~{} MiB Rohdaten); Import in TiledImage folgt.",
                tc_probe.header_version.unwrap_or(0),
                tc_probe.material_caches,
                tc_probe.tile_datasets,
                tc_probe.total_tile_bytes / (1024 * 1024)
            )
        } else {
            "texture-cache: Gruppe fehlt oder Header/Tiles nicht lesbar.".into()
        },
        if procedural_generators {
            format!(
                "Generatoren/Masken: {} prozedurale + {} Mask/Filter-HBO-Knoten ({} Typen) — SubCraft-Mapping folgt.",
                hbo_counts.procedural_nodes,
                hbo_counts.mask_or_filter_nodes,
                hbo_counts.distinct_types.len()
            )
        } else {
            "Generatoren/Masken: keine erkannten HBO-Knoten im document.bin.".into()
        },
    ];

    Ok(SppInspectReport {
        source_path: spp.display().to_string(),
        file_bytes,
        painter_major,
        hbo_format: hbo_format.to_string(),
        hbo_data_version: data_version,
        document_bin_bytes: container.document_bin.len(),
        scene_assbin_bytes: container.scene_assbin.len(),
        materials,
        total_layers,
        hdf5_top_level: hdf5_top_level(&local),
        decode_summary: decode.summary(),
        decode,
        texture_cache: texture_cache_summary(&tc_probe),
        hbo_generators: hbo_generator_summary(&hbo_counts),
        notes,
    })
}
