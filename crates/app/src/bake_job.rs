//! Background mesh-map baking: runs on its own thread, reports progress, can be cancelled, and
//! hands results back to the [`Session`] when polled.
//!
//! Every map is installed as soon as it is finished (not when the whole texture set is done), so
//! the viewport, the 2D view and the bake window show maps while the rest are still baking.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use subcraft_bake::matching::MatchReport;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, MatchMode, Progress, bake_each, name_groups};
use subcraft_core::{MeshMapKind, TextureSetId};
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, import_detail_mesh};

use crate::Session;

/// What to bake.
#[derive(Clone, Debug)]
pub struct BakeRequest {
    pub settings: BakeSettings,
    /// High-poly mesh files (merged into one detail mesh); empty = the low poly bakes onto itself.
    pub high_poly: Vec<PathBuf>,
    /// Texture sets (id, material slot) to bake, in order.
    pub texture_sets: Vec<(TextureSetId, u32)>,
}

/// State of one map of one texture set in a running (or finished) bake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapStatus {
    Queued,
    /// Baking now, with progress 0..1.
    Baking(f32),
    Done,
    /// Not baked (cancelled or failed before it was reached).
    Skipped,
}

enum Message {
    Map(TextureSetId, MeshMapKind, TiledImage),
    Matching(MatchReport),
    Failed(String),
    Finished,
}

pub struct BakeJob {
    progress: Arc<Progress>,
    sets_done: Arc<AtomicUsize>,
    texture_sets: Vec<TextureSetId>,
    maps: Vec<MeshMapKind>,
    stage: Arc<Mutex<String>>,
    rx: Receiver<Message>,
    /// Maps installed into the session so far, in arrival order.
    finished: Vec<(TextureSetId, MeshMapKind)>,
    matching: Option<MatchReport>,
    over: bool,
    applied_sets: usize,
    error: Option<String>,
}

/// Merge several meshes into one (object names and material names kept).
pub fn merge_meshes(mut meshes: Vec<Mesh>) -> Mesh {
    if meshes.len() == 1 {
        return meshes.pop().expect("one mesh");
    }
    let mut b = MeshBuilder::new("High poly").uvs_optional();
    for mesh in &meshes {
        let mut current: Option<Option<u32>> = None;
        for t in 0..mesh.triangle_count() {
            let object = mesh.tri_object.get(t).copied().flatten();
            if current != Some(object) {
                let name = object.and_then(|o| mesh.objects.get(o as usize)).map_or(mesh.name.as_str(), |o| o.name.as_str());
                b.object(name);
                current = Some(object);
            }
            let slot = mesh.tri_material.get(t).copied().unwrap_or(0) as usize;
            let material = b.material(mesh.materials.get(slot).map_or("DefaultMaterial", |m| m.name.as_str()));
            let corners = mesh.tri(t).map(|i| {
                let i = i as usize;
                Corner { position: mesh.positions[i], normal: mesh.normals.get(i).copied(), uv: mesh.uvs.get(i).copied() }
            });
            b.push_triangle(material, corners);
        }
    }
    b.build().unwrap_or_else(|_| meshes.swap_remove(0))
}

impl BakeJob {
    pub(crate) fn start(low: Arc<Mesh>, low_bvh: Arc<Bvh>, request: BakeRequest) -> Self {
        let (tx, rx) = channel();
        let progress = Arc::new(Progress::new());
        let sets_done = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(Mutex::new("Starting".to_string()));
        let texture_sets: Vec<TextureSetId> = request.texture_sets.iter().map(|(ts, _)| *ts).collect();
        let maps = request.settings.maps.clone();
        let (p, done, st) = (progress.clone(), sets_done.clone(), stage.clone());
        subcraft_core::task::spawn(move || {
            let set_stage = |s: String| *st.lock().expect("stage lock") = s;
            let mut loaded = Vec::new();
            for path in &request.high_poly {
                set_stage(format!("Loading high poly {}", path.display()));
                match import_detail_mesh(path) {
                    Ok(r) => loaded.push(r.mesh),
                    Err(e) => {
                        let _ = tx.send(Message::Failed(format!("high poly {}: {e}", path.display())));
                        let _ = tx.send(Message::Finished);
                        return;
                    }
                }
            }
            let high = if loaded.is_empty() {
                None
            } else {
                set_stage("Building high-poly BVH".into());
                let mesh = merge_meshes(loaded);
                let bvh = Bvh::build(&mesh);
                Some((mesh, bvh))
            };
            let geometry = |slot: u32| BakeInput {
                low: Geometry { mesh: &low, bvh: &low_bvh },
                high: high.as_ref().map(|(m, b)| Geometry { mesh: m, bvh: b }),
                material_slot: slot,
            };
            if high.is_some() && request.settings.match_mode == MatchMode::ByMeshName {
                set_stage("Matching meshes by name".into());
                if let Some(g) = name_groups(&geometry(0), &request.settings) {
                    let _ = tx.send(Message::Matching(g.report));
                }
            }
            for (i, (ts, slot)) in request.texture_sets.iter().enumerate() {
                set_stage(format!("Baking texture set {}/{}", i + 1, request.texture_sets.len()));
                let result = bake_each(&geometry(*slot), &request.settings, &p, |kind, img| {
                    let _ = tx.send(Message::Map(*ts, kind, img));
                });
                if let Err(e) = result {
                    let _ = tx.send(Message::Failed(e.to_string()));
                    if p.is_cancelled() {
                        break;
                    }
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            let _ = tx.send(Message::Finished);
        });
        Self {
            progress,
            sets_done,
            texture_sets,
            maps,
            stage,
            rx,
            finished: Vec::new(),
            matching: None,
            over: false,
            applied_sets: 0,
            error: None,
        }
    }

    /// Overall progress 0..1.
    pub fn fraction(&self) -> f32 {
        let total = self.texture_sets.len().max(1) as f32;
        ((self.sets_done.load(Ordering::Relaxed) as f32 + self.progress.fraction()) / total).min(1.0)
    }

    pub fn stage(&self) -> String {
        self.stage.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    /// Texture sets of this bake, in order.
    pub fn texture_sets(&self) -> &[TextureSetId] {
        &self.texture_sets
    }

    /// Maps of this bake, in order.
    pub fn maps(&self) -> &[MeshMapKind] {
        &self.maps
    }

    /// The texture set and map being baked right now.
    pub fn current(&self) -> Option<(TextureSetId, MeshMapKind)> {
        if self.over {
            return None;
        }
        let ts = *self.texture_sets.get(self.sets_done.load(Ordering::Relaxed))?;
        Some((ts, self.progress.current_map()?))
    }

    /// Maps installed so far, in arrival order.
    pub fn finished(&self) -> &[(TextureSetId, MeshMapKind)] {
        &self.finished
    }

    /// Name-matching result (only with a high poly and "By Mesh Name").
    pub fn matching(&self) -> Option<&MatchReport> {
        self.matching.as_ref()
    }

    pub fn status(&self, ts: TextureSetId, kind: MeshMapKind) -> MapStatus {
        if self.finished.contains(&(ts, kind)) {
            return MapStatus::Done;
        }
        if self.current() == Some((ts, kind)) {
            return MapStatus::Baking(self.progress.map_fraction());
        }
        if self.over || self.progress.is_cancelled() { MapStatus::Skipped } else { MapStatus::Queued }
    }

    /// Install finished maps into the session. Returns `Some(result)` once the job is over:
    /// the number of texture sets that received maps, or the first error.
    pub fn poll(&mut self, session: &mut Session) -> Option<Result<usize, String>> {
        loop {
            match self.rx.try_recv() {
                Ok(Message::Map(ts, kind, img)) => {
                    if !self.finished.iter().any(|(t, _)| *t == ts) {
                        self.applied_sets += 1;
                    }
                    session.set_mesh_maps(ts, BTreeMap::from([(kind, img)]));
                    self.finished.push((ts, kind));
                }
                Ok(Message::Matching(report)) => self.matching = Some(report),
                Ok(Message::Failed(e)) => {
                    self.error.get_or_insert(e);
                }
                Ok(Message::Finished) | Err(TryRecvError::Disconnected) => {
                    self.over = true;
                    return Some(match self.error.take() {
                        Some(e) if self.applied_sets == 0 => Err(e),
                        _ => Ok(self.applied_sets),
                    });
                }
                Err(TryRecvError::Empty) => return None,
            }
        }
    }
}

/// Small RGBA8 preview of a baked map (`size`² nearest samples): values shown as stored, scalar
/// maps as gray, the way an exported PNG of the map looks.
pub fn map_preview_rgba(img: &TiledImage, size: u32) -> Vec<u8> {
    let size = size.max(1);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    for y in 0..size {
        for x in 0..size {
            let sx = ((x as u64 * 2 + 1) * img.width() as u64 / (size as u64 * 2)) as u32;
            let sy = ((y as u64 * 2 + 1) * img.height() as u64 / (size as u64 * 2)) as u32;
            let c = img.get(sx.min(img.width() - 1), sy.min(img.height() - 1));
            if img.comps() == 1 {
                out.extend_from_slice(&[to8(c[0]), to8(c[0]), to8(c[0]), 255]);
            } else {
                out.extend_from_slice(&[to8(c[0]), to8(c[1]), to8(c[2]), 255]);
            }
        }
    }
    out
}
