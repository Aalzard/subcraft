//! Mesh reload: replace the project's model with an edited version of it and keep the painted
//! work (ADR-0042).
//!
//! Two steps, both off the UI thread:
//! 1. [`prepare`] imports the file, groups it into texture-set slots like a new project would
//!    (material / object / single, UDIM tiles), matches the slots to the existing texture sets by
//!    name and measures whether each matched layout moved (to suggest a mode).
//! 2. [`transfer`] builds the new document. **Keep UVs** reuses every raster untouched (only the
//!    material-slot index changes). **Reproject** maps every texel of the new layout to the old
//!    surface (`subcraft_mesh::transfer`) and resamples every paint raster (paint channels, paint
//!    effects, mask bases and mask paint effects) and the baked mesh maps from it. Fill layers and
//!    procedurals are evaluated from geometry and need nothing.
//!
//! [`crate::Session::finish_mesh_reload`] installs the result. The mesh is not part of the undo
//! history, so a reload clears it (the UI asks first).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use subcraft_core::{MeshMapKind, TextureSetId};
use subcraft_doc::{Document, Layer, LayerContent, TextureSet};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_mesh::transfer::{CorrespondenceOptions, TransferSource, TransferTarget, UvCorrespondence, build_correspondence, uv_coverage};
use subcraft_mesh::{Bvh, MaterialSlot, Mesh};

use crate::new_project::{NewProjectSettings, SetPlan, SplitRule, group_mesh, plan_sets};

/// How painted rasters reach the new mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadMode {
    /// The UV layout is unchanged: rasters stay exactly as they are.
    KeepUvs,
    /// The UV layout changed: rasters are moved through the 3D surface onto the new layout.
    Reproject,
}

impl ReloadMode {
    pub fn label(self) -> &'static str {
        match self {
            ReloadMode::KeepUvs => "Keep UVs",
            ReloadMode::Reproject => "Reproject",
        }
    }
}

/// One texture set of the reloaded model.
#[derive(Clone, Debug, PartialEq)]
pub struct SetMatch {
    pub name: String,
    /// The existing texture set it continues (matched by name), `None` for a new material.
    pub old: Option<TextureSetId>,
    /// Matched sets: share (0..1) of the surface whose UVs moved compared with the old model.
    pub uv_moved: Option<f32>,
}

/// An imported replacement mesh, matched against the current project and ready to transfer.
pub struct PreparedReload {
    pub path: PathBuf,
    mesh: Arc<Mesh>,
    plans: Vec<SetPlan>,
    /// The project mesh this was prepared against.
    base_mesh: Arc<Mesh>,
    pub triangles: usize,
    /// Importer and grouping notes.
    pub warnings: Vec<String>,
    /// Per new slot, in slot order.
    pub sets: Vec<SetMatch>,
    /// Existing texture sets no slot of the new model matches (kept, without geometry).
    pub unmatched_old: Vec<String>,
    pub suggested: ReloadMode,
}

impl PreparedReload {
    pub fn matched(&self) -> usize {
        self.sets.iter().filter(|s| s.old.is_some()).count()
    }

    pub fn added(&self) -> Vec<&str> {
        self.sets.iter().filter(|s| s.old.is_none()).map(|s| s.name.as_str()).collect()
    }

    /// Matched sets whose UV layout moved noticeably.
    pub fn moved(&self) -> Vec<&str> {
        self.sets.iter().filter(|s| s.uv_moved.is_some_and(|m| m > MOVED_SHARE)).map(|s| s.name.as_str()).collect()
    }
}

/// What a finished reload did (shown to the user).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshReloadReport {
    pub mode: Option<ReloadMode>,
    pub matched: Vec<String>,
    /// New materials that got a new, empty texture set.
    pub added: Vec<String>,
    /// Texture sets with no matching material: kept, but nothing shows them any more.
    pub kept_without_geometry: Vec<String>,
    /// Paint rasters moved onto the new layout (Reproject).
    pub rasters_transferred: usize,
    /// Pen paths turned into ordinary paint (their anchors refer to the old UVs).
    pub paths_converted: usize,
    /// Layers restricted to mesh parts (part numbers may differ on the new model).
    pub part_layers: usize,
    /// Texture sets whose baked maps should be baked again.
    pub stale_maps: Vec<String>,
}

impl MeshReloadReport {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Mesh reloaded ({}): {} texture set(s) matched",
            self.mode.map_or("", |m| m.label()),
            self.matched.len()
        );
        if self.rasters_transferred > 0 {
            s += &format!(", {} raster(s) reprojected", self.rasters_transferred);
        }
        if !self.added.is_empty() {
            s += &format!(", new: {}", self.added.join(", "));
        }
        if !self.kept_without_geometry.is_empty() {
            s += &format!(", kept without geometry: {}", self.kept_without_geometry.join(", "));
        }
        if self.paths_converted > 0 {
            s += &format!(", {} pen path(s) converted to paint", self.paths_converted);
        }
        if self.part_layers > 0 {
            s += &format!(", {} layer(s) use part visibility (check them)", self.part_layers);
        }
        if !self.stale_maps.is_empty() {
            s += ". Rebake mesh maps";
        }
        s
    }
}

/// Progress and cancellation shared with a background reload step.
#[derive(Default)]
pub struct ReloadProgress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
    stage: Mutex<String>,
}

impl ReloadProgress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 { 0.0 } else { (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0) }
    }

    pub fn stage(&self) -> String {
        self.stage.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub(crate) fn set_stage(&self, s: impl Into<String>) {
        if let Ok(mut stage) = self.stage.lock() {
            *stage = s.into();
        }
    }
}

/// A reload step running on its own thread. Dropping it cancels the work.
pub struct MeshReloadJob<T> {
    rx: Receiver<Result<T, String>>,
    progress: Arc<ReloadProgress>,
}

impl<T: Send + 'static> MeshReloadJob<T> {
    pub(crate) fn spawn(f: impl FnOnce(&ReloadProgress) -> Result<T, String> + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let progress = Arc::new(ReloadProgress::default());
        let p = progress.clone();
        subcraft_core::task::spawn(move || {
            let _ = tx.send(f(&p));
        });
        Self { rx, progress }
    }

    pub fn progress(&self) -> &ReloadProgress {
        &self.progress
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    /// The result once the step finished.
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("mesh reload stopped unexpectedly".into())),
        }
    }

    /// Block until the step finished.
    pub fn wait(self) -> Result<T, String> {
        self.rx.recv().map_err(|_| "mesh reload stopped unexpectedly".to_string())?
    }
}

impl<T> Drop for MeshReloadJob<T> {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// Share of a set's surface whose UVs moved above which Reproject is suggested.
const MOVED_SHARE: f32 = 0.02;
/// Resolution of the quick layout comparison in [`prepare`].
const PROBE_RESOLUTION: u32 = 64;

/// Name an existing texture set is matched by: its own name, and its material slot's name.
fn old_names<'a>(old_mesh: &'a Mesh, ts: &'a TextureSet) -> [&'a str; 2] {
    [ts.name.as_str(), old_mesh.materials.get(ts.material_slot as usize).map_or(ts.name.as_str(), |m| m.name.as_str())]
}

/// Import `path` and match it against the project (see the module docs).
pub fn prepare(path: &Path, old_mesh: Arc<Mesh>, old_bvh: &Bvh, old_doc: &Document, progress: &ReloadProgress) -> Result<PreparedReload, String> {
    progress.set_stage(format!("Importing {}", path.display()));
    let report = subcraft_mesh::import_mesh(path).map_err(|e| e.to_string())?;
    prepare_mesh(path, report.mesh, report.warnings, old_mesh, old_bvh, old_doc, progress)
}

/// [`prepare`] for an already imported mesh.
pub fn prepare_mesh(
    path: &Path,
    mesh: Mesh,
    mut warnings: Vec<String>,
    old_mesh: Arc<Mesh>,
    old_bvh: &Bvh,
    old_doc: &Document,
    progress: &ReloadProgress,
) -> Result<PreparedReload, String> {
    if mesh.triangle_count() == 0 {
        return Err("the model has no triangles".into());
    }
    if mesh.uv_bounds().is_none_or(|(lo, hi)| (hi - lo).length() <= 1e-6) {
        return Err("the model has no UV coordinates, so it cannot be painted".into());
    }
    let is_old = |name: &str| old_doc.texture_sets.iter().any(|ts| old_names(&old_mesh, ts).contains(&name));
    // The project does not remember how it was split into texture sets: use the split whose set
    // names match the existing ones best (by material on a tie).
    let mut best = (0usize, NewProjectSettings::default());
    for split in [SplitRule::ByMaterial, SplitRule::ByObject, SplitRule::Single] {
        let settings = NewProjectSettings { split, udim: true, ..Default::default() };
        let score = plan_sets(&mesh, &settings).iter().filter(|p| is_old(&p.name)).count();
        if score > best.0 {
            best = (score, settings);
        }
    }
    if best.1.split != SplitRule::ByMaterial {
        warnings.push(format!("Texture sets were matched {} like the existing project.", best.1.split.label().to_lowercase()));
    }
    let (mesh, plans) = group_mesh(mesh, &best.1);
    let mesh = Arc::new(mesh);
    let mut used = Vec::new();
    let mut sets = Vec::new();
    let diagonal = old_mesh.bounds.diagonal().max(mesh.bounds.diagonal());
    progress.total.store(plans.len(), Ordering::Relaxed);
    progress.set_stage("Comparing UV layouts");
    for (slot, plan) in plans.iter().enumerate() {
        let old = old_doc.texture_sets.iter().find(|ts| !used.contains(&ts.id) && old_names(&old_mesh, ts).contains(&plan.name.as_str()));
        let uv_moved = old.map(|ts| {
            used.push(ts.id);
            let source = TransferSource { mesh: &old_mesh, bvh: old_bvh, material: ts.material_slot };
            let target = TransferTarget { mesh: &mesh, triangles: mesh.material_triangles(slot as u32), resolution: PROBE_RESOLUTION };
            // Inside texels only: rim texels on seams can resolve to the neighbouring island.
            let options = CorrespondenceOptions { tile_size: TILE_SIZE, expand: 0.0, max_distance: diagonal * 0.05 };
            build_correspondence(source, target, options, &AtomicUsize::new(0), &progress.cancel).map_or(0.0, |c| c.moved_fraction(1.0))
        });
        progress.done.fetch_add(1, Ordering::Relaxed);
        sets.push(SetMatch { name: plan.name.clone(), old: old.map(|ts| ts.id), uv_moved });
    }
    if progress.is_cancelled() {
        return Err("mesh reload cancelled".into());
    }
    let unmatched_old = old_doc.texture_sets.iter().filter(|ts| !used.contains(&ts.id)).map(|ts| ts.name.clone()).collect();
    let suggested = if sets.iter().any(|s| s.uv_moved.is_some_and(|m| m > MOVED_SHARE)) { ReloadMode::Reproject } else { ReloadMode::KeepUvs };
    Ok(PreparedReload { path: path.to_path_buf(), triangles: mesh.triangle_count(), mesh, plans, base_mesh: old_mesh, warnings, sets, unmatched_old, suggested })
}

/// Baked mesh maps per texture set.
pub type MapSet = HashMap<TextureSetId, BTreeMap<MeshMapKind, TiledImage>>;

/// The new project state, ready for [`crate::Session::finish_mesh_reload`].
pub struct ReloadOutput {
    pub(crate) mesh: Mesh,
    pub(crate) document: Document,
    pub(crate) maps: MapSet,
    pub(crate) stale: Vec<TextureSetId>,
    pub(crate) base_mesh: Arc<Mesh>,
    pub(crate) base_doc: Document,
    pub(crate) source: PathBuf,
    pub report: MeshReloadReport,
}

/// Build the reloaded document (see the module docs).
pub fn transfer(prepared: &PreparedReload, mode: ReloadMode, old_bvh: &Bvh, old_doc: &Document, old_maps: &MapSet, progress: &ReloadProgress) -> Result<ReloadOutput, String> {
    let old_mesh = &prepared.base_mesh;
    let mut mesh = (*prepared.mesh).clone();
    let mut report = MeshReloadReport { mode: Some(mode), ..Default::default() };
    let template = old_doc.texture_sets.first();
    let mut sets = Vec::new();
    let mut maps = MapSet::new();
    let mut stale = Vec::new();
    // Progress: correspondence tiles of every matched set (Reproject), else one step per set.
    let total: usize = prepared
        .sets
        .iter()
        .filter_map(|s| s.old.and_then(|id| old_doc.texture_set(id)))
        .map(|ts| if mode == ReloadMode::Reproject { (ts.resolution.div_ceil(TILE_SIZE) as usize).pow(2) } else { 1 })
        .sum();
    progress.total.store(total.max(1), Ordering::Relaxed);
    let diagonal = old_mesh.bounds.diagonal().max(mesh.bounds.diagonal());
    for (slot, (m, plan)) in prepared.sets.iter().zip(&prepared.plans).enumerate() {
        let Some(old) = m.old.and_then(|id| old_doc.texture_set(id)) else {
            // A new material: a fresh set like New Project makes, shaped like the existing ones.
            let resolution = template.map_or(crate::DEFAULT_RESOLUTION, |t| t.resolution);
            let mut ts = TextureSet::new(plan.name.clone(), slot as u32, resolution);
            if let Some(t) = template {
                ts.channels = t.channels.clone();
            }
            ts.udim_tile = plan.tile;
            let tc = ts.clone();
            ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
            report.added.push(ts.name.clone());
            sets.push(ts);
            continue;
        };
        let mut ts = old.clone();
        ts.material_slot = slot as u32;
        ts.udim_tile = plan.tile;
        report.matched.push(ts.name.clone());
        ts.walk(|l, _| {
            if l.parts.is_some() {
                report.part_layers += 1;
            }
        });
        let old_set_maps = old_maps.get(&ts.id).filter(|m| !m.is_empty());
        if mode == ReloadMode::Reproject {
            progress.set_stage(format!("Reprojecting {}", ts.name));
            let source = TransferSource { mesh: old_mesh, bvh: old_bvh, material: old.material_slot };
            let options = CorrespondenceOptions { tile_size: TILE_SIZE, expand: 1.5, max_distance: diagonal * 0.05 };
            let mut mappers: HashMap<u32, Mapper> = HashMap::new();
            let mut resolutions = vec![ts.resolution];
            resolutions.extend(old_set_maps.into_iter().flat_map(|m| m.values().map(|i| i.width())));
            for r in resolutions {
                if mappers.contains_key(&r) {
                    continue;
                }
                let target = TransferTarget { mesh: &mesh, triangles: mesh.material_triangles(slot as u32), resolution: r };
                // Only the set's own resolution counts towards progress (bake sizes are extra).
                let scratch = AtomicUsize::new(0);
                let done = if r == ts.resolution { &progress.done } else { &scratch };
                let corr = build_correspondence(source, target, options, done, &progress.cancel).ok_or("mesh reload cancelled")?;
                let coverage = uv_coverage(old_mesh, old_mesh.material_triangles(old.material_slot), r, 1.0);
                mappers.insert(r, Mapper { corr, coverage });
            }
            let main = &mappers[&ts.resolution];
            let count = AtomicUsize::new(0);
            for layer in ts.layers.iter_mut() {
                Arc::make_mut(layer).map_rasters(&|img| {
                    count.fetch_add(1, Ordering::Relaxed);
                    match mappers.get(&img.width()) {
                        Some(m) if img.height() == img.width() => m.apply(img),
                        _ => main.apply(&img.resampled(ts.resolution, ts.resolution)),
                    }
                });
            }
            report.rasters_transferred += count.into_inner();
            report.paths_converted += drop_pen_paths(&mut ts.layers);
            if let Some(old_set_maps) = old_set_maps {
                maps.insert(ts.id, old_set_maps.iter().map(|(k, img)| (*k, mappers[&img.width()].apply(img))).collect());
            }
        } else {
            progress.done.fetch_add(1, Ordering::Relaxed);
            if let Some(m) = old_set_maps {
                maps.insert(ts.id, m.clone());
            }
        }
        if old_set_maps.is_some() {
            stale.push(ts.id);
            report.stale_maps.push(ts.name.clone());
        }
        if progress.is_cancelled() {
            return Err("mesh reload cancelled".into());
        }
        sets.push(ts);
    }
    // Sets without a matching material keep their layers on a material slot of their own that
    // has no triangles, so nothing is lost and a later reload can still match them by name.
    for ts in &old_doc.texture_sets {
        if prepared.sets.iter().any(|s| s.old == Some(ts.id)) {
            continue;
        }
        let mut ts = ts.clone();
        ts.material_slot = mesh.materials.len() as u32;
        mesh.materials.push(MaterialSlot { name: ts.name.clone() });
        if let Some(m) = old_maps.get(&ts.id) {
            maps.insert(ts.id, m.clone());
        }
        report.kept_without_geometry.push(ts.name.clone());
        sets.push(ts);
    }
    Ok(ReloadOutput {
        mesh,
        document: Document { texture_sets: sets },
        maps,
        stale,
        base_mesh: old_mesh.clone(),
        base_doc: old_doc.clone(),
        source: prepared.path.clone(),
        report,
    })
}

/// Turn pen-path and text layers into ordinary paint layers (their anchors are UVs of the old layout;
/// the transferred raster is kept).
fn drop_pen_paths(layers: &mut [Arc<Layer>]) -> usize {
    let mut n = 0;
    for layer in layers {
        let has = match &layer.content {
            LayerContent::Paint(p) => p.path.is_some() || p.text.is_some(),
            LayerContent::Group(g) => !g.children.is_empty(),
            LayerContent::Fill(_) => false,
        };
        if !has {
            continue;
        }
        let l = Arc::make_mut(layer);
        match &mut l.content {
            LayerContent::Paint(p) => {
                p.path = None;
                p.text = None; // text decals are placed in the old UV layout: keep the raster only
                n += 1;
            }
            LayerContent::Group(g) => n += drop_pen_paths(&mut g.children),
            LayerContent::Fill(_) => {}
        }
    }
    n
}

/// Correspondence at one resolution plus the old layout's coverage at that resolution.
struct Mapper {
    corr: UvCorrespondence,
    coverage: Vec<bool>,
}

impl Mapper {
    fn apply(&self, src: &TiledImage) -> TiledImage {
        transfer_raster(&self.corr, src, &self.coverage)
    }
}

/// Resample `src` (old layout) onto the layout of `corr`. Bilinear, using only source texels
/// inside the old islands (`coverage`, `src`-sized) when any are, so seams never pull in gutter
/// values. Only tiles that receive painted data are allocated; the rest stay at the fill value.
pub fn transfer_raster(corr: &UvCorrespondence, src: &TiledImage, coverage: &[bool]) -> TiledImage {
    let res = corr.resolution;
    let comps = src.comps();
    let fill = src.fill();
    let mut out = TiledImage::new(res, res, comps, fill);
    if src.allocated_tiles() == 0 {
        return out;
    }
    debug_assert_eq!(corr.tile_size, TILE_SIZE);
    debug_assert_eq!(coverage.len(), (src.width() * src.height()) as usize);
    let written: Vec<(usize, Vec<f32>)> = (0..corr.tile_count())
        .into_par_iter()
        .filter_map(|i| {
            let uvs = corr.tile(i)?;
            let mut data: Vec<f32> = (0..TILE_TEXELS * comps).map(|k| fill[k % comps]).collect();
            let mut any = false;
            let mut px = [0.0f32; 4];
            for (k, uv) in uvs.iter().enumerate() {
                // Texels whose taps hold only the fill value (even inside an allocated tile)
                // stay exactly at the fill, so tiles without paint are never allocated.
                if !uv[0].is_nan() && sample(src, coverage, *uv, &mut px) && (0..comps).any(|c| (px[c] - fill[c]).abs() > 1e-4) {
                    data[k * comps..(k + 1) * comps].copy_from_slice(&px[..comps]);
                    any = true;
                }
            }
            any.then_some((i, data))
        })
        .collect();
    for (i, data) in written {
        out.write_tile(i, &data);
    }
    out
}

/// Seam-aware bilinear read at `uv`. Returns false when every tap is unpainted (fill).
fn sample(src: &TiledImage, coverage: &[bool], uv: [f32; 2], out: &mut [f32; 4]) -> bool {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let fx = uv[0] * w as f32 - 0.5;
    let fy = uv[1] * h as f32 - 0.5;
    let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let mut taps = [(x0, y0, (1.0 - tx) * (1.0 - ty)), (x0 + 1, y0, tx * (1.0 - ty)), (x0, y0 + 1, (1.0 - tx) * ty), (x0 + 1, y0 + 1, tx * ty)];
    for t in &mut taps {
        t.0 = t.0.clamp(0, w - 1);
        t.1 = t.1.clamp(0, h - 1);
    }
    let inside = |t: &(i32, i32, f32)| coverage[(t.1 * w + t.0) as usize];
    if taps.iter().any(|t| t.2 > 0.0 && inside(t)) {
        for t in &mut taps {
            if !inside(t) {
                t.2 = 0.0;
            }
        }
    }
    let total: f32 = taps.iter().map(|t| t.2).sum();
    if total <= 0.0 {
        return false;
    }
    let comps = src.comps();
    let fill = src.fill();
    *out = [0.0; 4];
    let mut painted = false;
    let size = TILE_SIZE as i32;
    for &(x, y, wgt) in &taps {
        if wgt <= 0.0 {
            continue;
        }
        let index = src.tile_index((x / size) as u32, (y / size) as u32);
        let wgt = wgt / total;
        match src.tile(index) {
            Some(tile) => {
                painted = true;
                let o = (((y % size) * size + (x % size)) as usize) * comps;
                for (acc, value) in out.iter_mut().zip(&tile.data[o..o + comps]) {
                    *acc += value.to_f32() * wgt;
                }
            }
            None => {
                for c in 0..comps {
                    out[c] += fill[c] * wgt;
                }
            }
        }
    }
    painted
}
