//! Bake Painter procedural noise `.sbsar` graphs (Ratio Cells, BnW Spots, …) into mask bitmaps.
//!
//! SPSM / SPMSK fills store an optional [`subcraft_spp::SppSbsarRef`]. When the Substance bake
//! host and Painter starter assets are available, SubCraft replaces the Pattern approximation
//! with a real baked grayscale image, keeping Painter UV tiling on the projection.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_core::ResourceId;
use subcraft_doc::{
    Layer, LayerContent, MaskEffect, MaskEffectKind, Projection, ProjectionMode, Resource,
    ResourceKind,
};

/// Painter noise graph parameters used when baking a procedural `.sbsar` into a mask image.
#[derive(Clone, Debug, PartialEq)]
pub struct PainterNoiseParams {
    pub url: String,
    pub scale: i32,
    pub balance: f32,
    pub contrast: f32,
    pub disorder: f32,
    pub invert: bool,
    pub seed: i64,
}
use subcraft_sbsar::{bake_sbsar_with_overrides, baked_texture_paths, clamp_bake_size};
use subcraft_spp::{SppLayerNode, SppMaskEffect, SppMaskEffectKind, SppSbsarRef};

use crate::session::Session;

/// Resolve a Painter `urlToSbsRes` (e.g. `/noise_cells_1/ratio_cells_1?...`) to an on-disk `.sbsar`.
pub fn resolve_painter_procedural_sbsar(url: &str) -> Option<PathBuf> {
    let stem = url
        .trim_start_matches('/')
        .split(['/', '?'])
        .next()?
        .trim();
    if stem.is_empty() {
        return None;
    }
    let file_name = format!("{stem}.sbsar");
    for root in painter_procedurals_roots() {
        let direct = root.join("Noises").join(&file_name);
        if direct.is_file() {
            return Some(direct);
        }
        let grunge = root.join("Grunges").join(&file_name);
        if grunge.is_file() {
            return Some(grunge);
        }
        let patterns = root.join("Patterns").join(&file_name);
        if patterns.is_file() {
            return Some(patterns);
        }
        if let Some(found) = find_named_file(&root, &file_name) {
            return Some(found);
        }
    }
    None
}

fn painter_procedurals_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let candidates = [
        r"C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\procedurals",
        r"C:\Program Files\Allegorithmic\Substance Painter\resources\shelf\allegorithmic\procedurals",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_dir() {
            roots.push(p);
        }
    }
    if let Ok(extra) = std::env::var("SUBCRAFT_PAINTER_PROCEDURALS") {
        let p = PathBuf::from(extra);
        if p.is_dir() {
            roots.push(p);
        }
    }
    roots
}

fn find_named_file(dir: &Path, file_name: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            if let Some(found) = find_named_file(&path, file_name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(file_name))
        {
            return Some(path);
        }
    }
    None
}

fn bake_cache_key(sbsar_path: &Path, r: &PainterNoiseParams, size: u32) -> u64 {
    let mut h = DefaultHasher::new();
    sbsar_path.hash(&mut h);
    r.url.hash(&mut h);
    r.scale.hash(&mut h);
    r.balance.to_bits().hash(&mut h);
    r.contrast.to_bits().hash(&mut h);
    r.disorder.to_bits().hash(&mut h);
    r.invert.hash(&mut h);
    r.seed.hash(&mut h);
    size.hash(&mut h);
    h.finish()
}

fn painter_noise_from_spp(r: &SppSbsarRef) -> PainterNoiseParams {
    PainterNoiseParams {
        url: r.url.clone(),
        scale: r.scale,
        balance: r.balance,
        contrast: r.contrast,
        disorder: r.disorder,
        invert: r.invert,
        seed: r.seed,
    }
}

impl Session {
    /// Bake a Painter noise SBSAR into a grayscale texture resource (cached by params).
    pub fn bake_painter_procedural_mask(
        &mut self,
        r: &PainterNoiseParams,
        size_px: u32,
    ) -> Result<ResourceId, String> {
        let sbsar_path = resolve_painter_procedural_sbsar(&r.url).ok_or_else(|| {
            format!(
                "Painter procedural SBSAR not found for url {:?} (install Substance 3D Painter starter assets or set SUBCRAFT_PAINTER_PROCEDURALS)",
                r.url
            )
        })?;
        let size = clamp_bake_size(size_px.max(256).min(2048));
        let key = bake_cache_key(&sbsar_path, r, size);
        // `.png` grayscale — see note below about TGA/RGBA logo-mask semantics.
        let cache_name = format!("painter-proc-{key:016x}.png");

        if let Some(existing) = self
            .resources()
            .iter()
            .find(|res| res.kind == ResourceKind::Texture && res.name == cache_name)
        {
            return Ok(existing.id);
        }

        let out_dir = std::env::temp_dir().join(format!("subcraft-painter-proc-{key:016x}"));
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| format!("{}: {e}", out_dir.display()))?;

        let overrides = [
            ("scale".into(), r.scale.to_string()),
            ("balance".into(), format!("{:.6}", r.balance)),
            ("contrast".into(), format!("{:.6}", r.contrast)),
            ("disorder".into(), format!("{:.6}", r.disorder)),
            ("invert".into(), if r.invert { "1" } else { "0" }.into()),
            ("$randomseed".into(), r.seed.to_string()),
        ];

        let manifest = bake_sbsar_with_overrides(&sbsar_path, &out_dir, size, 0, &overrides)
            .map_err(|e| format!("bake {}: {e}", sbsar_path.display()))?;
        let paths = baked_texture_paths(&out_dir, &manifest);
        let tga = paths
            .first()
            .ok_or_else(|| format!("{}: bake produced no outputs", sbsar_path.display()))?;
        let bytes = std::fs::read(tga).map_err(|e| format!("{}: {e}", tga.display()))?;
        let _ = std::fs::remove_dir_all(&out_dir);

        // Bake host always writes 32-bit BGRA TGA. Mask Image effects treat RGBA as a logo
        // stamp (value=1 × alpha) — which turns opaque noise into a solid white mask. Convert
        // to 1-channel grayscale so the compositor uses luminance (Ratio Cells, BnW Spots, …).
        let rgba = subcraft_image::io::decode_bitmap(&bytes, Some("output.tga"))
            .map_err(|e| format!("{cache_name}: {e}"))?;
        let bitmap = rgba_to_mask_grayscale(&rgba);
        let (encoded, _) = subcraft_image::io::encode_bitmap(&bitmap)
            .map_err(|e| format!("{cache_name}: encode: {e}"))?;

        let id = ResourceId::new();
        self.resources_mut().insert(Resource {
            id,
            name: cache_name,
            kind: ResourceKind::Texture,
            bitmap: Arc::new(bitmap),
            source_path: Some(sbsar_path),
            encoded: Some(Arc::new(encoded)),
        });
        Ok(id)
    }

    /// Replace bakeable Painter procedural mask fills with SBSAR-baked [`MaskEffectKind::Image`]
    /// effects. Falls back to the Pattern approximation when the host / assets are missing.
    pub fn upgrade_spp_procedural_masks(
        &mut self,
        layer: &mut Layer,
        node: &SppLayerNode,
        bake_size: u32,
    ) {
        if let Some(mask) = layer.mask.as_mut() {
            for (eff_arc, spp_fx) in mask.effects.iter_mut().zip(node.mask_effects.iter()) {
                try_upgrade_one(self, Arc::make_mut(eff_arc), spp_fx, bake_size);
            }
        }
        if let LayerContent::Group(g) = &mut layer.content {
            for (child_arc, child_node) in g.children.iter_mut().zip(node.children.iter()) {
                let child = Arc::make_mut(child_arc);
                self.upgrade_spp_procedural_masks(child, child_node, bake_size);
            }
        }
    }

    /// Convert an SPSM layer tree, baking real Painter noise SBSAR masks when possible.
    pub fn spp_node_to_layer_baked(&mut self, node: &SppLayerNode, resolution: u32) -> Layer {
        let mut layer = crate::spp_convert::spp_node_to_layer(node, resolution);
        self.upgrade_spp_procedural_masks(&mut layer, node, resolution);
        layer
    }

    /// Convert SPMSK mask effects, baking noise SBSAR fills when possible.
    pub fn spp_effects_to_mask_effects_baked(
        &mut self,
        effects: &[SppMaskEffect],
        bake_size: u32,
    ) -> Vec<MaskEffect> {
        let mut out = crate::spp_convert::spp_effects_to_mask_effects(effects);
        for (eff, spp_fx) in out.iter_mut().zip(effects.iter()) {
            try_upgrade_one(self, eff, spp_fx, bake_size);
        }
        out
    }
}

/// Rec.709 luminance → single-channel **u8** mask (drops unused TGA alpha).
/// Must be u8 so [`encode_bitmap`] writes PNG; f32 would become EXR named `.png`.
fn rgba_to_mask_grayscale(bm: &subcraft_image::Bitmap) -> subcraft_image::Bitmap {
    let n = (bm.width * bm.height) as usize;
    let mut gray = Vec::with_capacity(n);
    for y in 0..bm.height {
        for x in 0..bm.width {
            let t = bm.texel(x, y, false);
            let lum = (0.2126 * t[0] + 0.7152 * t[1] + 0.0722 * t[2]).clamp(0.0, 1.0);
            gray.push((lum * 255.0).round() as u8);
        }
    }
    subcraft_image::Bitmap::new_u8(bm.width, bm.height, 1, gray)
}

fn try_upgrade_one(
    session: &mut Session,
    eff: &mut MaskEffect,
    spp_fx: &SppMaskEffect,
    bake_size: u32,
) {
    let SppMaskEffectKind::ProceduralFill {
        sbsar: Some(sbsar),
        uv_scale,
        triplanar,
        ..
    } = &spp_fx.kind
    else {
        return;
    };
    let painter = painter_noise_from_spp(sbsar);
    let Ok(resource) = session.bake_painter_procedural_mask(&painter, bake_size) else {
        return;
    };
    let mut projection = Projection::default();
    if *triplanar {
        projection.mode = ProjectionMode::Triplanar;
    }
    if let Some(s) = *uv_scale {
        projection.scale = [s, s];
    }
    // Invert is already applied in the Substance bake when `painter.invert` is set.
    let _ = painter;
    eff.kind = MaskEffectKind::Image {
        resource,
        projection,
        invert: false,
    };
}
