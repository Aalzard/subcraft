//! Batched ray queries over a [`Bvh`] that an accelerator (the GPU) can take over.
//!
//! The baker prepares one [`RayTexel`] per covered texel on the CPU and asks a registered
//! [`RayBatchBackend`] to cast the same deterministic cosine-weighted rays the CPU path casts.
//! This module has no GPU types: the implementation lives in `subcraft-render`, registered at
//! application start-up with [`register_ray_backend`].

use std::sync::{Arc, Mutex};

use crate::Bvh;

/// One texel's ray fan: `rays` cosine-weighted directions around `axis` (Hammersley points
/// rotated by `rotation`), cast from `origin`. Directions with `dot(dir, offset) <= 0` are
/// skipped in [`RayMode::Occlusion`] (they point into the surface).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayTexel {
    pub origin: [f32; 3],
    pub rotation: f32,
    pub axis: [f32; 3],
    pub offset: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RayMode {
    /// Result per texel: `1 - occluded / counted` (1 when no direction faces out).
    Occlusion { distance: f32 },
    /// Result per texel: mean closest-hit distance over all rays divided by `max` (misses count
    /// as `max`).
    MeanDistance { max: f32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RayBatchError {
    Cancelled,
    /// The accelerator cannot do this job (size limits, deep tree, lost device ...). The caller
    /// falls back to the CPU.
    Unsupported(String),
}

impl std::fmt::Display for RayBatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RayBatchError::Cancelled => f.write_str("cancelled"),
            RayBatchError::Unsupported(s) => f.write_str(s),
        }
    }
}

/// Cancellation and progress for a running batch.
pub trait BatchControl: Sync {
    fn is_cancelled(&self) -> bool;
    /// Fraction of the batch finished, 0..=1.
    fn progress(&self, fraction: f32);
}

pub trait RayBatchBackend: Send + Sync {
    /// Short human description, e.g. the adapter name.
    fn description(&self) -> String;
    /// One result per texel `0..count`, in order; `texel(i)` describes texel `i` (called in
    /// order, in chunks, so no array of all records has to exist). Must return quickly between internal steps when
    /// `control.is_cancelled()`.
    fn run(&self, bvh: &Bvh, count: usize, texel: &(dyn Fn(usize) -> RayTexel + Sync), rays: u32, mode: RayMode, control: &dyn BatchControl)
        -> Result<Vec<f32>, RayBatchError>;
}

type Factory = fn() -> Option<Arc<dyn RayBatchBackend>>;

enum Slot {
    None,
    Factory(Factory),
    Ready(Option<Arc<dyn RayBatchBackend>>),
}

static BACKEND: Mutex<Slot> = Mutex::new(Slot::None);

/// Register how to create the accelerator. Called once at application start-up; the backend is
/// created lazily by the first [`ray_backend`] call (on the baking thread).
pub fn register_ray_backend(factory: Factory) {
    *BACKEND.lock().expect("ray backend lock") = Slot::Factory(factory);
}

/// The accelerator if one is registered and could be created (cached after the first call).
pub fn ray_backend() -> Option<Arc<dyn RayBatchBackend>> {
    let factory = match &*BACKEND.lock().expect("ray backend lock") {
        Slot::None => return None,
        Slot::Ready(b) => return b.clone(),
        Slot::Factory(f) => *f,
    };
    // Create outside the lock: device creation can take a while.
    let created = factory();
    let mut slot = BACKEND.lock().expect("ray backend lock");
    if let Slot::Factory(_) = &*slot {
        *slot = Slot::Ready(created.clone());
    }
    created
}
