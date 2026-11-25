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
    /// Part group of the texel (bake by mesh name): only triangles of this group in the
    /// `tri_group` list passed to [`RayBatchBackend::run`] are hit. [`NO_GROUP`] hits every triangle.
    pub group: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RayMode {
    /// Result per texel: `1 - occluded / counted` (1 when no direction faces out).
    Occlusion { distance: f32 },
    /// Result per texel: mean closest-hit distance over all rays divided by `max` (misses count
    /// as `max`).
    MeanDistance { max: f32 },
    /// Result per texel: three values, the sum of the directions that face out and reach
    /// `distance` unoccluded (zero when none do). The caller normalizes it into the bent normal.
    BentNormal { distance: f32 },
}

impl RayMode {
    /// Values per texel in the result of [`RayBatchBackend::run`].
    pub fn components(self) -> usize {
        match self {
            RayMode::BentNormal { .. } => 3,
            _ => 1,
        }
    }
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

/// Per-triangle data (indexed by mesh triangle) the curvature pass needs besides the BVH.
#[derive(Clone, Copy)]
pub struct CurvatureSurface<'a> {
    /// The three corner normals of each triangle: `n0.xyz, 0, n1.xyz, 0, n2.xyz, 0`.
    pub tri_normals: &'a [[f32; 12]],
    /// Connected element + object of each triangle, as one id: triangles with different ids are
    /// "foreign" to each other.
    pub tri_group: &'a [u32],
}

/// [`CurvatureTexel::group`] when the texel has no source triangle.
pub const NO_GROUP: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvatureTexel {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Group id of the triangle the texel was projected onto, or [`NO_GROUP`].
    pub group: u32,
}

/// One projection ray of the baker's cage: the closest hit in `(0, reach)` along `dir` (unit length)
/// from `origin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectionRay {
    pub origin: [f32; 3],
    pub dir: [f32; 3],
    pub reach: f32,
    /// Part group of the ray, as in [`RayTexel::group`]; [`NO_GROUP`] hits every triangle.
    pub group: u32,
}

/// The closest hit of a [`ProjectionRay`], like [`crate::bvh::RayHit`]: mesh triangle, distance and
/// the barycentric weights of vertices 1 and 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectionHit {
    pub tri: u32,
    pub t: f32,
    pub u: f32,
    pub v: f32,
}

pub trait RayBatchBackend: Send + Sync {
    /// Short human description, e.g. the adapter name.
    fn description(&self) -> String;
    /// [`RayMode::components`] results per texel `0..count`, in order; `texel(i)` describes texel `i` (called in
    /// order, in chunks, so no array of all records has to exist). Must return quickly between internal steps when
    /// `control.is_cancelled()`.
    /// Curvature of each texel `0..count` (see [`CurvatureSurface`]). The default refuses, so the
    /// caller uses the CPU.
    fn curvature(
        &self, _bvh: &Bvh, _surface: CurvatureSurface, _count: usize, _texel: &(dyn Fn(usize) -> CurvatureTexel + Sync), _radius: f32,
        _control: &dyn BatchControl,
    ) -> Result<Vec<f32>, RayBatchError> {
        Err(RayBatchError::Unsupported("this backend has no curvature pass".into()))
    }
    /// The closest hit of each ray `0..count` (see [`ProjectionRay`]), or `None` on a miss. With
    /// `front_only`, triangles the ray reaches from behind (`dot(geometric normal, dir) >= 0`) are
    /// skipped. `tri_group` (one per mesh triangle) filters hits by part, see [`ProjectionRay::group`].
    /// The default refuses, so the caller uses the CPU.
    #[allow(clippy::too_many_arguments)]
    fn project(
        &self, _bvh: &Bvh, _tri_group: Option<&[u32]>, _count: usize, _ray: &(dyn Fn(usize) -> ProjectionRay + Sync), _front_only: bool,
        _control: &dyn BatchControl,
    ) -> Result<Vec<Option<ProjectionHit>>, RayBatchError> {
        Err(RayBatchError::Unsupported("this backend has no projection pass".into()))
    }
    /// `tri_group` (one per mesh triangle) filters hits by part, see [`RayTexel::group`].
    #[allow(clippy::too_many_arguments)]
    fn run(&self, bvh: &Bvh, tri_group: Option<&[u32]>, count: usize, texel: &(dyn Fn(usize) -> RayTexel + Sync), rays: u32, mode: RayMode, control: &dyn BatchControl)
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

/// Creates the accelerator on one extra device, named by a key from the application's device
/// list (see `subcraft_render::ray_bake::bake_devices`).
type ExtraFactory = fn(&str) -> Option<Arc<dyn RayBatchBackend>>;

struct Extras {
    factory: Option<ExtraFactory>,
    /// Keys of the extra devices to bake on, besides the main one.
    wanted: Vec<String>,
    /// Devices already created (`None`: creation failed, not retried).
    created: Vec<(String, Option<Arc<dyn RayBatchBackend>>)>,
}

static EXTRAS: Mutex<Extras> = Mutex::new(Extras { factory: None, wanted: Vec::new(), created: Vec::new() });

/// Register how to create the accelerator on extra devices (multi-GPU baking, issue #731).
pub fn register_extra_ray_backends(factory: ExtraFactory) {
    EXTRAS.lock().expect("ray backend lock").factory = Some(factory);
}

/// Choose the extra devices bakes also use (keys of the application's device list). Devices
/// created before stay cached; those no longer wanted are simply not used.
pub fn set_extra_ray_devices(keys: Vec<String>) {
    EXTRAS.lock().expect("ray backend lock").wanted = keys;
}

/// The extra devices chosen with [`set_extra_ray_devices`].
pub fn extra_ray_devices() -> Vec<String> {
    EXTRAS.lock().expect("ray backend lock").wanted.clone()
}

/// Every accelerator a bake may use: the main one ([`ray_backend`]) first, then the extra devices
/// that could be created, in the order chosen.
pub fn ray_backends() -> Vec<Arc<dyn RayBatchBackend>> {
    let mut all: Vec<Arc<dyn RayBatchBackend>> = ray_backend().into_iter().collect();
    let (factory, wanted) = {
        let extras = EXTRAS.lock().expect("ray backend lock");
        (extras.factory, extras.wanted.clone())
    };
    let Some(factory) = factory else { return all };
    for key in wanted {
        let cached = EXTRAS.lock().expect("ray backend lock").created.iter().find(|(k, _)| *k == key).map(|(_, b)| b.clone());
        let backend = match cached {
            Some(b) => b,
            None => {
                // Create outside the lock: device creation can take a while.
                let b = factory(&key);
                EXTRAS.lock().expect("ray backend lock").created.push((key, b.clone()));
                b
            }
        };
        all.extend(backend);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake(String);

    impl RayBatchBackend for Fake {
        fn description(&self) -> String {
            self.0.clone()
        }
        fn run(&self, _: &Bvh, _: Option<&[u32]>, _: usize, _: &(dyn Fn(usize) -> RayTexel + Sync), _: u32, _: RayMode, _: &dyn BatchControl) -> Result<Vec<f32>, RayBatchError> {
            Err(RayBatchError::Unsupported("fake".into()))
        }
    }

    static CREATED: AtomicUsize = AtomicUsize::new(0);

    fn fake(key: &str) -> Option<Arc<dyn RayBatchBackend>> {
        CREATED.fetch_add(1, Ordering::Relaxed);
        (key != "broken").then(|| Arc::new(Fake(key.to_string())) as Arc<dyn RayBatchBackend>)
    }

    #[test]
    fn extra_devices_are_created_once_in_the_chosen_order_and_failures_are_skipped() {
        register_extra_ray_backends(fake);
        set_extra_ray_devices(vec!["b".into(), "broken".into(), "a".into()]);
        let names = |v: Vec<Arc<dyn RayBatchBackend>>| v.iter().map(|b| b.description()).filter(|d| d.len() <= 6).collect::<Vec<_>>();
        assert_eq!(names(ray_backends()), ["b", "a"]);
        assert_eq!(CREATED.load(Ordering::Relaxed), 3);
        assert_eq!(names(ray_backends()), ["b", "a"], "cached");
        assert_eq!(CREATED.load(Ordering::Relaxed), 3, "not created again, the broken one not retried");
        set_extra_ray_devices(vec!["a".into()]);
        assert_eq!(names(ray_backends()), ["a"]);
        assert_eq!(extra_ray_devices(), ["a"]);
    }
}
