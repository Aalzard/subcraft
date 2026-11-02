//! Background Painter import: the project opens as soon as the model and the flattened channels
//! are read, and the rebuilt layer stack is installed when it is done (see [`crate::spp_import`]).
//! Runs on its own thread, reports progress, can be cancelled, and hands results to the
//! [`Session`] when polled (the same shape as [`crate::bake_job`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use subcraft_core::TextureSetId;
use subcraft_doc::{Layer, ResourceStore};
use subcraft_mesh::Mesh;

use crate::Session;

/// Progress and cancellation shared between the import thread and whoever shows it.
#[derive(Default)]
pub struct ImportControl {
    cancel: AtomicBool,
    stage: Mutex<String>,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl ImportControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub(crate) fn set_stage(&self, stage: impl Into<String>) {
        *self.stage.lock().unwrap_or_else(|p| p.into_inner()) = stage.into();
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    /// Start counting `total` Painter layers for the current stage.
    pub(crate) fn begin_layers(&self, stage: impl Into<String>, total: usize) {
        self.set_stage(stage);
        self.total.store(total, Ordering::Relaxed);
    }

    pub(crate) fn layer_done(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// What the import is doing, e.g. `Rebuilding Painter layers (120 of 374)`.
    pub fn stage(&self) -> String {
        let stage = self.stage.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return stage;
        }
        format!("{stage} ({} of {total} layers)", self.done.load(Ordering::Relaxed).min(total))
    }

    /// Share of the current stage that is done, if it counts layers.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0))
    }
}

/// The rebuilt Painter stack of one texture set: the folder goes above the flattened layer, whose
/// visibility says which of the two is shown.
#[derive(Clone)]
pub struct SppStackUpdate {
    pub texture_set: TextureSetId,
    pub flat: Arc<Layer>,
    pub folder: Arc<Layer>,
}

/// Everything the second import phase produced.
#[derive(Clone)]
pub struct SppStack {
    pub updates: Vec<SppStackUpdate>,
    /// Textures the rebuilt layers use.
    pub resources: ResourceStore,
    pub notes: Vec<String>,
}

pub enum SppEvent {
    /// The model, the baked maps and the flattened channels: usable at once.
    Opened(Box<Session>, Vec<String>),
    /// The rebuilt layer stack, to be installed into the opened session.
    Stack(SppStack),
}

enum Message {
    Event(SppEvent),
    Failed(String),
    Finished,
}

/// What a poll found.
pub enum SppPoll {
    Idle,
    /// The project to show now; the stack may follow.
    Opened(Box<Session>, Vec<String>),
    /// The rebuilt stack was installed (notes: how close it is to Painter's result).
    StackInstalled(Vec<String>),
    /// The rebuilt stack was dropped because the session it belongs to is gone.
    Discarded,
    /// The import is over (`Err`: it failed or was cancelled, with the reason).
    Finished(Result<(), String>),
}

pub struct SppImportJob {
    pub path: PathBuf,
    control: Arc<ImportControl>,
    rx: Receiver<Message>,
    held: Option<Message>,
    mesh: Option<Arc<Mesh>>,
    opened: bool,
    started: std::time::Instant,
}

impl Drop for SppImportJob {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

impl SppImportJob {
    pub fn start(path: &Path) -> Self {
        let (tx, rx) = channel();
        let control = ImportControl::new();
        let (ctl, p, txc) = (control.clone(), path.to_path_buf(), tx.clone());
        subcraft_core::task::spawn(move || {
            let result = crate::spp_import::import_spp(&p, &ctl, &mut |event| {
                let _ = txc.send(Message::Event(event));
            });
            if let Err(e) = result {
                let _ = tx.send(Message::Failed(e));
            }
            let _ = tx.send(Message::Finished);
        });
        Self { path: path.to_path_buf(), control, rx, held: None, mesh: None, opened: false, started: std::time::Instant::now() }
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    pub fn control(&self) -> &Arc<ImportControl> {
        &self.control
    }

    /// True once the project has been handed out and only the layer stack is still being rebuilt.
    pub fn is_rebuilding(&self) -> bool {
        self.opened
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// Hand over the next result. `session` is the session showing this project (after
    /// [`SppPoll::Opened`]); a rebuilt stack is installed into it, unless a different project was
    /// opened meanwhile.
    pub fn poll(&mut self, session: &mut Session) -> SppPoll {
        let message = match self.held.take() {
            Some(m) => m,
            None => match self.rx.try_recv() {
                Ok(m) => m,
                Err(TryRecvError::Empty) => return SppPoll::Idle,
                Err(TryRecvError::Disconnected) => return SppPoll::Finished(Err("the Painter import stopped unexpectedly; check the application log".into())),
            },
        };
        match message {
            Message::Event(SppEvent::Opened(opened, notes)) => {
                self.mesh = Some(opened.scene().mesh.clone());
                self.opened = true;
                SppPoll::Opened(opened, notes)
            }
            Message::Event(SppEvent::Stack(stack)) => {
                if !self.mesh.as_ref().is_some_and(|m| Arc::ptr_eq(m, &session.scene().mesh)) {
                    self.cancel();
                    return SppPoll::Discarded;
                }
                // Installing layers must not commit a partially drawn brush stroke.
                if session.is_stroking() {
                    self.held = Some(Message::Event(SppEvent::Stack(stack)));
                    return SppPoll::Idle;
                }
                SppPoll::StackInstalled(session.apply_spp_stack(stack))
            }
            Message::Failed(e) => SppPoll::Finished(Err(e)),
            Message::Finished => SppPoll::Finished(Ok(())),
        }
    }
}

