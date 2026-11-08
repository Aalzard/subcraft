//! Background mesh-map baking: runs on its own thread, reports progress, can be cancelled, and
//! hands results back to the [`Session`] when polled.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use subcraft_bake::name_match::PartMatch;
use subcraft_bake::{place_high, BakeInput, BakeSettings, Geometry, MapReport, Progress, bake_streamed};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BakeLogLevel { Info, Warning, Error }

/// One line of a bake's log: seconds since the bake started, a level and the text.
#[derive(Clone, Debug, PartialEq)]
pub struct BakeLogLine {
    pub at: f32,
    pub level: BakeLogLevel,
    pub text: String,
}

impl BakeLogLine {
    /// `[  1.2 s] warning: ...`, the form copied to the clipboard.
    pub fn plain(&self) -> String {
        let level = match self.level { BakeLogLevel::Info => "info", BakeLogLevel::Warning => "warning", BakeLogLevel::Error => "error" };
        format!("[{:6.1} s] {level}: {}", self.at, self.text)
    }
}

/// The whole log as plain text (for Copy).
pub fn log_text(lines: &[BakeLogLine]) -> String {
    lines.iter().map(BakeLogLine::plain).collect::<Vec<_>>().join("\n")
}

/// Where one map is in the running or last bake (for the Bake window's per-map rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapState { Waiting, Running, Done, Warning }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapProgress {
    pub kind: MeshMapKind,
    pub state: MapState,
    /// Seconds spent on this map so far (all texture sets); 0 while waiting.
    pub seconds: f32,
}

enum Message {
    Log(BakeLogLine),
    Map(TextureSetId, MeshMapKind, TiledImage),
    Baked,
    Reports(Vec<MapReport>),
    /// Something the owner should know that does not fail the bake (unmatched part names).
    Notice(String),
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
    low: Arc<Mesh>,
    started: std::time::Instant,
    elapsed: Option<std::time::Duration>,
    reports: Vec<MapReport>,
    /// Most recently installed complete map, for a transient viewport preview.
    pub latest_map: Option<(TextureSetId, MeshMapKind)>,
    notices: Vec<String>,
    log: Vec<BakeLogLine>,
    /// Maps seen finishing while the job runs (the per-set reports only arrive at the end of a set).
    live: LiveMaps,
}

#[derive(Default)]
struct LiveMaps {
    done: BTreeMap<MeshMapKind, f32>,
    current: Option<(MeshMapKind, std::time::Instant)>,
}

impl LiveMaps {
    fn observe(&mut self, now_current: Option<MeshMapKind>) {
        if self.current.map(|c| c.0) == now_current { return; }
        if let Some((kind, at)) = self.current.take() { self.done.insert(kind, at.elapsed().as_secs_f32()); }
        if let Some(kind) = now_current { self.current = Some((kind, std::time::Instant::now())); }
    }
}

impl Drop for BakeJob {
    fn drop(&mut self) { self.progress.cancel(); }
}

impl BakeJob {
    pub(crate) fn start(low: Arc<Mesh>, low_bvh: Arc<Bvh>, request: BakeRequest) -> Self {
        Self::start_prepared(low, low_bvh, request, None)
    }

    pub(crate) fn start_prepared(low: Arc<Mesh>, low_bvh: Arc<Bvh>, request: BakeRequest, prepared: Option<BakePreviewMesh>) -> Self {
        let (tx, rx) = channel();
        let high_poly = request.high_poly.is_some();
        let sets_total = request.texture_sets.len();
        let progress = Arc::new(Progress::new());
        let sets_done = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(Mutex::new("Starting".to_string()));
        let (p, done, st) = (progress.clone(), sets_done.clone(), stage.clone());
        let scene = low.clone();
        let started = std::time::Instant::now();
        subcraft_core::task::spawn(move || {
            // Every line goes to the Bake window's log and to the application log file.
            let say = |level: BakeLogLevel, text: String| {
                match level {
                    BakeLogLevel::Info => log::info!("bake: {text}"),
                    BakeLogLevel::Warning => log::warn!("bake: {text}"),
                    BakeLogLevel::Error => log::error!("bake: {text}"),
                }
                let _ = tx.send(Message::Log(BakeLogLine { at: started.elapsed().as_secs_f32(), level, text }));
            };
            {
                let s = &request.settings;
                let maps: Vec<_> = s.maps.iter().map(|k| k.display_name()).collect();
                say(BakeLogLevel::Info, format!("Baking {} texture set(s) at {} px: {}. GPU {}.", request.texture_sets.len(), s.size, maps.join(", "), if s.use_gpu { "requested" } else { "off (CPU)" }));
                match &request.high_poly {
                    Some(path) => say(BakeLogLevel::Info, format!("High poly: {}", path.display())),
                    None => say(BakeLogLevel::Info, "No high poly: the low poly bakes onto itself.".into()),
                }
            }
            let high_mesh = match &request.high_poly {
                Some(path) => {
                    *st.lock().expect("stage lock") = format!("Loading high poly {}", path.display());
                    match prepared.filter(|p| p.is_current(path)).map(|p| Ok(p.mesh))
                        .unwrap_or_else(|| import_mesh_for_baking(path).map(|r| Arc::new(r.mesh))) {
                        Ok(r) => {
                            Some(r)
                        }
                        Err(e) => {
                            say(BakeLogLevel::Error, format!("High poly could not be loaded: {e}"));
                            let _ = tx.send(Message::Failed(format!("high poly: {e}")));
                            let _ = tx.send(Message::Finished);
                            return;
                        }
                    }
                }
                None => None,
            };
            if p.is_cancelled() { say(BakeLogLevel::Warning, "Bake cancelled.".into()); let _ = tx.send(Message::Failed("bake cancelled".into())); let _ = tx.send(Message::Finished); return; }
            *st.lock().expect("stage lock") = "Building high-poly BVH".into();
            let high_mesh = high_mesh.map(|mesh| {
                let placement = &request.settings.placement;
                if !placement.is_identity() { say(BakeLogLevel::Info, format!("High-poly placement: offset {:?}, scale {} x {:?}, rotation {:?}°, pivot {:?}, anchor {:?}.", placement.offset, placement.scale, placement.axis_scale, placement.rotation_deg, placement.pivot, placement.anchor)); }
                place_high(&low, &mesh, placement)
            });
            let high = high_mesh.map(|mesh| { let bvh = Bvh::build(&mesh); (mesh, bvh) });
            if let Some((mesh, _)) = &high && let Some(parts) = PartMatch::new(&low, mesh, &request.settings.name_match) {
                log::info!("bake match by name: {} group(s) {:?}; unmatched high parts {:?}", parts.groups.len(), parts.groups, parts.unmatched_high);
                if let Some(report) = parts.report() {
                    say(BakeLogLevel::Warning, report.clone());
                    let _ = tx.send(Message::Notice(report));
                }
            }
            for (i, (ts, slot)) in request.texture_sets.iter().enumerate() {
                *st.lock().expect("stage lock") = format!("Baking texture set {}/{}", i + 1, request.texture_sets.len());
                say(BakeLogLevel::Info, format!("Texture set {}/{} (material slot {slot}) started.", i + 1, request.texture_sets.len()));
                let set_started = std::time::Instant::now();
                let input = BakeInput {
                    low: Geometry { mesh: &low, bvh: &low_bvh },
                    high: high.as_ref().map(|(m, b)| Geometry { mesh: m, bvh: b }),
                    material_slot: *slot,
                };
                match bake_streamed(&input, &request.settings, &p, |kind, image| {
                    let _ = tx.send(Message::Map(*ts, kind, image.clone()));
                }) {
                    Ok(_) => {
                        let reports = p.reports();
                        for r in &reports {
                            // `CPU`, `GPU (adapter)` or `CPU (GPU failed: reason)`.
                            let level = if r.backend.contains("failed") { BakeLogLevel::Warning } else { BakeLogLevel::Info };
                            say(level, format!("{}: {:.2} s on {}", r.kind.display_name(), r.seconds, r.backend));
                        }
                        for w in p.warnings() { say(BakeLogLevel::Warning, w); }
                        say(BakeLogLevel::Info, format!("Texture set {}/{} done in {:.1} s.", i + 1, request.texture_sets.len(), set_started.elapsed().as_secs_f32()));
                        let _ = tx.send(Message::Reports(reports));
                        let _ = tx.send(Message::Baked);
                    }
                    Err(e) => {
                        for w in p.warnings() { say(BakeLogLevel::Warning, w); }
                        if matches!(e, subcraft_bake::BakeError::NoTexels) {
                            say(BakeLogLevel::Warning, format!("Texture set {}/{} (material slot {slot}) skipped: {e}.", i + 1, request.texture_sets.len()));
                        } else {
                            say(BakeLogLevel::Error, format!("Texture set {}/{} (material slot {slot}) failed: {e}", i + 1, request.texture_sets.len()));
                        }
                        let _ = tx.send(Message::Failed(e.to_string()));
                        if p.is_cancelled() {
                            break;
                        }
                    }
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            say(BakeLogLevel::Info, format!("Finished in {:.1} s.", started.elapsed().as_secs_f32()));
            let _ = tx.send(Message::Finished);
        });
        Self { progress, sets_done, sets_total, stage, rx, applied: 0, error: None, high_poly, low: scene, started: std::time::Instant::now(), elapsed: None, reports: Vec::new(), latest_map: None, notices: Vec::new(), log: Vec::new(), live: LiveMaps::default() }
    }

    /// Overall progress 0..1.
    pub fn fraction(&self) -> f32 {
        let total = self.sets_total.max(1) as f32;
        ((self.sets_done.load(Ordering::Relaxed) as f32 + self.progress.fraction()) / total).min(1.0)
    }

    pub fn stage(&self) -> String {
        let stage = self.stage.lock().map(|s| s.clone()).unwrap_or_default();
        match self.progress.current_map() {
            Some(kind) => format!("{stage} · {}", kind.display_name()),
            None => stage,
        }
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    /// Which path made each map and how long it took, e.g. `Ambient Occlusion 1.2 s (GPU (NVIDIA ...))`;
    /// empty until a texture set has finished.
    pub fn summary(&self) -> String {
        let maps: Vec<String> = self.reports.iter().map(|r| format!("{} {:.1} s on {}", r.kind.display_name(), r.seconds, r.backend)).collect();
        let total = self.elapsed.unwrap_or_else(|| self.started.elapsed());
        if maps.is_empty() { String::new() } else { format!("{:.1} s total. {}", total.as_secs_f32(), maps.join("; ")) }
    }

    /// One row per requested map: waiting / running / done (or warning when the GPU path failed and
    /// the CPU took over), with the time spent. A finished job shows no running row.
    pub fn map_progress(&self, maps: &[MeshMapKind]) -> Vec<MapProgress> {
        let finished = self.elapsed.is_some();
        let current = if finished { None } else { self.progress.current_map() };
        maps.iter().map(|&kind| {
            let mine: Vec<&MapReport> = self.reports.iter().filter(|r| r.kind == kind).collect();
            let reported: f32 = mine.iter().map(|r| r.seconds).sum::<f32>().max(0.0);
            let live_done = self.live.done.get(&kind).copied();
            let running_for = self.live.current.filter(|c| c.0 == kind).map(|c| c.1.elapsed().as_secs_f32());
            let (state, seconds) = if mine.iter().any(|r| r.backend.contains("failed")) { (MapState::Warning, reported) }
                else if current == Some(kind) { (MapState::Running, running_for.unwrap_or(0.0)) }
                else if mine.len() >= self.sets_total.max(1) || (finished && !mine.is_empty()) { (MapState::Done, reported) }
                else if let Some(t) = live_done { (MapState::Done, t) }
                else { (MapState::Waiting, 0.0) };
            MapProgress { kind, state, seconds }
        }).collect()
    }

    /// The bake's log so far: what ran where and for how long, and every warning (cage misses,
    /// GPU fallbacks, unmatched parts, skipped texture sets) and error, in order.
    pub fn log(&self) -> &[BakeLogLine] {
        &self.log
    }

    /// Non-fatal notes received so far (e.g. low parts that found no high part by name).
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// Apply finished texture sets to the session. Returns `Some(result)` once the job is over:
    /// the number of texture sets baked, or the first error.
    pub fn poll(&mut self, session: &mut Session) -> Option<Result<usize, String>> {
        if !Arc::ptr_eq(&self.low, &session.scene().mesh) {
            self.cancel();
            return Some(Err("bake discarded: the project mesh changed".into()));
        }
        if self.elapsed.is_none() { self.live.observe(self.progress.current_map()); }
        // Installing the display layer must not commit a partially drawn brush stroke.
        if session.is_stroking() { return None; }
        loop {
            match self.rx.try_recv() {
                Ok(Message::Map(ts, kind, image)) => {
                    // Yield after one map so a fast worker cannot skip intermediate previews.
                    session.set_mesh_maps(ts, BTreeMap::from([(kind, image)]));
                    if self.high_poly && kind == MeshMapKind::Normal { session.show_baked_normal(ts); }
                    self.latest_map = Some((ts, kind));
                    if self.live.current.map(|c| c.0) == Some(kind) { self.live.observe(None); } else { self.live.done.entry(kind).or_insert(0.0); }
                    return None;
                }
                Ok(Message::Baked) => self.applied += 1,
                Ok(Message::Log(line)) => self.log.push(line),
                Ok(Message::Reports(r)) => self.reports.extend(r),
                Ok(Message::Notice(note)) => self.notices.push(note),
                Ok(Message::Failed(e)) => {
                    self.error.get_or_insert(e);
                }
                Ok(Message::Finished) => {
                    self.elapsed = Some(self.started.elapsed());
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

    fn queued_job(session: &Session, rx: Receiver<Message>) -> BakeJob {
        BakeJob { progress: Arc::new(Progress::new()), sets_done: Arc::new(AtomicUsize::new(0)),
            sets_total: 1, stage: Arc::new(Mutex::new(String::new())), rx, applied: 0, error: None, high_poly: false,
            low: session.scene().mesh.clone(), started: std::time::Instant::now(), elapsed: None, reports: Vec::new(), latest_map: None, notices: Vec::new(), log: Vec::new(), live: LiveMaps::default() }
    }

    #[test]
    fn complete_maps_arrive_independently_and_leave_authored_channels_untouched() {
        let mut session = Session::new(subcraft_mesh::primitives::plane(2., 1), 128);
        let before = session.document().clone();
        let ts = session.selection().texture_set;
        let (tx, rx) = channel();
        let mut job = queued_job(&session, rx);
        let normal = TiledImage::new(128, 128, 3, [0.5, 0.5, 1., 0.]);
        let ao = TiledImage::new(128, 128, 1, [0.25; 4]);
        tx.send(Message::Map(ts, MeshMapKind::Normal, normal.clone())).unwrap();
        tx.send(Message::Map(ts, MeshMapKind::AmbientOcclusion, ao.clone())).unwrap();
        tx.send(Message::Baked).unwrap(); tx.send(Message::Finished).unwrap();
        assert!(job.poll(&mut session).is_none());
        assert_eq!(job.latest_map, Some((ts, MeshMapKind::Normal)));
        assert_eq!(session.mesh_maps(ts).unwrap().get(MeshMapKind::Normal), Some(&normal));
        assert!(session.mesh_maps(ts).unwrap().get(MeshMapKind::AmbientOcclusion).is_none());
        assert!(job.poll(&mut session).is_none());
        assert_eq!(job.latest_map, Some((ts, MeshMapKind::AmbientOcclusion)));
        assert_eq!(session.mesh_maps(ts).unwrap().get(MeshMapKind::AmbientOcclusion), Some(&ao));
        assert_eq!(job.poll(&mut session), Some(Ok(1)));
        assert_eq!(session.document(), &before);
    }

    #[test]
    fn a_different_scene_rejects_queued_results() {
        let original = Session::new(subcraft_mesh::primitives::plane(2., 1), 128);
        let mut replacement = Session::new(subcraft_mesh::primitives::plane(2., 1), 128);
        let (tx, rx) = channel();
        let mut job = queued_job(&original, rx);
        tx.send(Message::Finished).unwrap();
        assert!(job.poll(&mut replacement).unwrap().unwrap_err().contains("project mesh changed"));
        assert!(job.progress.is_cancelled());
        assert!(replacement.mesh_maps(replacement.selection().texture_set).is_none());
    }

    #[test]
    fn preview_import_needs_no_uvs_and_preserves_exported_coordinates() {
        let path = std::env::temp_dir().join(format!("subcraft-bake-preview-{}.obj", uuid::Uuid::new_v4()));
        std::fs::write(&path, "v 20 0 0\nv 21 0 0\nv 20 1 0\nf 1 2 3\n").unwrap();
        let job = BakePreviewJob::start(path.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let source = loop {
            if let Some(result) = job.poll() { break result.unwrap(); }
            assert!(std::time::Instant::now() < deadline, "preview import timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(source.mesh.triangle_count(), 1);
        assert_eq!(source.mesh.bounds.min.x, 20.);
        assert_eq!(source.mesh.bounds.max.x, 21.);
        assert!(source.is_current(&path));
        std::fs::write(&path, "changed source").unwrap();
        assert!(!source.is_current(&path), "never rebake from a stale cached sculpt");
        std::fs::remove_file(path).unwrap();
    }

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
            sets_total: 1, stage: Arc::new(Mutex::new(String::new())), rx, applied: 0, error: None, high_poly: true,
            low: session.scene().mesh.clone(), started: std::time::Instant::now(), elapsed: None, reports: Vec::new(), latest_map: None, notices: Vec::new(), log: Vec::new(), live: LiveMaps::default() };
        assert!(job.poll(&mut session).unwrap().unwrap_err().contains("stopped unexpectedly"));
    }
}

/// Background import for the pre-bake overlay. Dropping an obsolete job discards its result;
/// the mesh stays in its imported coordinate frame and never modifies the project mesh.
#[derive(Clone)]
pub struct BakePreviewMesh {
    pub mesh: Arc<Mesh>,
    path: PathBuf,
    modified: Option<std::time::SystemTime>,
    len: u64,
}

impl BakePreviewMesh {
    /// A cached preview is only reusable while the imported source file is unchanged.
    pub fn is_current(&self, path: &std::path::Path) -> bool {
        path == self.path && std::fs::metadata(path).is_ok_and(|m| m.len() == self.len && m.modified().ok() == self.modified)
    }
}

pub struct BakePreviewJob(Receiver<Result<BakePreviewMesh, String>>);

impl BakePreviewJob {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = channel();
        subcraft_core::task::spawn(move || {
            let result = (|| {
                let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                let r = import_mesh_for_baking(&path).map_err(|e| e.to_string())?;
                let source = BakePreviewMesh { mesh: Arc::new(r.mesh), path: path.clone(), modified: metadata.modified().ok(), len: metadata.len() };
                if !source.is_current(&path) { return Err("high-poly file changed while loading; reload the preview".into()); }
                Ok(source)
            })();
            let _ = tx.send(result);
        });
        Self(rx)
    }
    pub fn poll(&self) -> Option<Result<BakePreviewMesh, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("high-poly preview worker stopped unexpectedly".into())),
        }
    }
}
