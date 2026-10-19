//! Background mesh-map baking: runs on its own thread, reports progress, can be cancelled, and
//! hands results back to the [`Session`] when polled.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{MeshMapKind, TextureSetId};
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Mesh, import_mesh_for_baking};

use crate::Session;

/// What to bake.
#[derive(Clone, Debug)]
pub struct BakeRequest {
    pub settings: BakeSettings,
    /// Optional high-poly mesh file; without it the low poly bakes onto itself.
    pub high_poly: Option<PathBuf>,
    /// Texture sets (id, material slot) to bake, in order.
    pub texture_sets: Vec<(TextureSetId, u32)>,
}

enum Message {
    Baked(TextureSetId, BTreeMap<MeshMapKind, TiledImage>),
    Failed(String),
    Finished,
}

pub struct BakeJob {
    progress: Arc<Progress>,
    sets_done: Arc<AtomicUsize>,
    sets_total: usize,
    stage: Arc<Mutex<String>>,
    rx: Receiver<Message>,
    applied: usize,
    error: Option<String>,
    high_poly: bool,
}

impl BakeJob {
    pub(crate) fn start(low: Arc<Mesh>, low_bvh: Arc<Bvh>, request: BakeRequest) -> Self {
        let (tx, rx) = channel();
        let high_poly = request.high_poly.is_some();
        let sets_total = request.texture_sets.len();
        let progress = Arc::new(Progress::new());
        let sets_done = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(Mutex::new("Starting".to_string()));
        let (p, done, st) = (progress.clone(), sets_done.clone(), stage.clone());
        subcraft_core::task::spawn(move || {
            let high = match &request.high_poly {
                Some(path) => {
                    *st.lock().expect("stage lock") = format!("Loading high poly {}", path.display());
                    match import_mesh_for_baking(path) {
                        Ok(r) => {
                            *st.lock().expect("stage lock") = "Building high-poly BVH".into();
                            let bvh = Bvh::build(&r.mesh);
                            Some((r.mesh, bvh))
                        }
                        Err(e) => {
                            let _ = tx.send(Message::Failed(format!("high poly: {e}")));
                            let _ = tx.send(Message::Finished);
                            return;
                        }
                    }
                }
                None => None,
            };
            for (i, (ts, slot)) in request.texture_sets.iter().enumerate() {
                *st.lock().expect("stage lock") = format!("Baking texture set {}/{}", i + 1, request.texture_sets.len());
                let input = BakeInput {
                    low: Geometry { mesh: &low, bvh: &low_bvh },
                    high: high.as_ref().map(|(m, b)| Geometry { mesh: m, bvh: b }),
                    material_slot: *slot,
                };
                match bake(&input, &request.settings, &p) {
                    Ok(maps) => {
                        let _ = tx.send(Message::Baked(*ts, maps));
                    }
                    Err(e) => {
                        let _ = tx.send(Message::Failed(e.to_string()));
                        if p.is_cancelled() {
                            break;
                        }
                    }
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            let _ = tx.send(Message::Finished);
        });
        Self { progress, sets_done, sets_total, stage, rx, applied: 0, error: None, high_poly }
    }

    /// Overall progress 0..1.
    pub fn fraction(&self) -> f32 {
        let total = self.sets_total.max(1) as f32;
        ((self.sets_done.load(Ordering::Relaxed) as f32 + self.progress.fraction()) / total).min(1.0)
    }

    pub fn stage(&self) -> String {
        self.stage.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    /// Apply finished texture sets to the session. Returns `Some(result)` once the job is over:
    /// the number of texture sets baked, or the first error.
    pub fn poll(&mut self, session: &mut Session) -> Option<Result<usize, String>> {
        // Installing the display layer must not commit a partially drawn brush stroke.
        if session.is_stroking() { return None; }
        loop {
            match self.rx.try_recv() {
                Ok(Message::Baked(ts, maps)) => {
                    let normal = self.high_poly && maps.contains_key(&MeshMapKind::Normal);
                    session.set_mesh_maps(ts, maps);
                    if normal { session.show_baked_normal(ts); }
                    self.applied += 1;
                }
                Ok(Message::Failed(e)) => {
                    self.error.get_or_insert(e);
                }
                Ok(Message::Finished) => {
                    return Some(match self.error.take() {
                        Some(e) => Err(format!("{e} ({} of {} texture sets baked)", self.applied, self.sets_total)),
                        _ => Ok(self.applied),
                    });
                }
                Err(TryRecvError::Disconnected) => return Some(Err("bake worker stopped unexpectedly; check the application log".into())),
                Err(TryRecvError::Empty) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::ChannelKind;
    use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent};

    #[test]
    fn an_existing_normal_mesh_map_reference_is_preserved() {
        let mut session = Session::new(subcraft_mesh::primitives::plane(2., 1), 128);
        let ts = session.selection().texture_set;
        let mut layer = Layer::new_fill("Authored bake connection", session.selected_texture_set());
        if let LayerContent::Fill(fill) = &mut layer.content {
            fill.channels.insert(ChannelKind::Normal, FillChannel::new(FillSource::MeshMap { map: MeshMapKind::Normal }));
        }
        session.execute(crate::Command::InsertLayer { layer: Box::new(layer), label: "Connect bake".into() }).unwrap();
        let before = session.document().clone();
        session.show_baked_normal(ts);
        assert_eq!(session.document(), &before, "do not double-apply existing mesh-map links");
    }

    #[test]
    fn unexpected_worker_disconnect_is_failure() {
        let mut session = Session::new(subcraft_mesh::primitives::plane(2., 1), 128);
        let (tx, rx) = channel();
        drop(tx);
        let mut job = BakeJob { progress: Arc::new(Progress::new()), sets_done: Arc::new(AtomicUsize::new(0)),
            sets_total: 1, stage: Arc::new(Mutex::new(String::new())), rx, applied: 0, error: None, high_poly: true };
        assert!(job.poll(&mut session).unwrap().unwrap_err().contains("stopped unexpectedly"));
    }
}
