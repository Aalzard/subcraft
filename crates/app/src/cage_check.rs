//! Ray-miss check for the Bake window: which texels of each texture set would reach no high-poly
//! surface with the current cage, placement and name matching. Runs on a background thread
//! (the placed high poly's BVH is built once and reused while only the cage distances change).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use subcraft_bake::{BakeInput, BakeSettings, Geometry, cage_misses, place_high};
use subcraft_core::TextureSetId;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Mesh};

/// The high poly as placed for a check, with its BVH (shared between checks).
pub struct PlacedHigh {
    pub source: Arc<Mesh>,
    pub placement: subcraft_core::BakePlacement,
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
}

/// Everything a check result depends on; a new check is needed when this changes.
#[derive(Clone, Debug, PartialEq)]
pub struct CageCheckKey {
    pub low: usize,
    pub high: usize,
    pub cage: [u32; 2],
    pub placement: subcraft_core::BakePlacement,
    pub name_match: subcraft_core::BakeMatch,
    pub size: u32,
}

impl CageCheckKey {
    pub fn new(low: &Arc<Mesh>, high: &Arc<Mesh>, settings: &BakeSettings, size: u32) -> Self {
        Self {
            low: Arc::as_ptr(low) as usize,
            high: Arc::as_ptr(high) as usize,
            cage: [settings.cage_front.to_bits(), settings.cage_rear.to_bits()],
            placement: settings.placement.clone(),
            name_match: settings.name_match.clone(),
            size,
        }
    }
}

pub struct CageCheckResult {
    pub key: CageCheckKey,
    /// Miss map per (texture set, material slot); sets without texels are left out.
    pub maps: Vec<(TextureSetId, u32, TiledImage)>,
    pub covered: usize,
    pub missed: usize,
    pub placed: Arc<PlacedHigh>,
}

impl CageCheckResult {
    pub fn miss_fraction(&self) -> f32 {
        if self.covered == 0 { 0.0 } else { self.missed as f32 / self.covered as f32 }
    }
}

pub struct CageCheckJob {
    pub key: CageCheckKey,
    rx: Receiver<CageCheckResult>,
}

impl CageCheckJob {
    /// Check every set at `size`² (a preview resolution; misses are a property of the surface,
    /// not of the map size). `placed` is reused when it matches `high` and the placement.
    pub fn start(low: Arc<Mesh>, high: Arc<Mesh>, placed: Option<Arc<PlacedHigh>>, settings: BakeSettings, sets: Vec<(TextureSetId, u32)>, size: u32) -> Self {
        let key = CageCheckKey::new(&low, &high, &settings, size);
        let (tx, rx) = channel();
        let job_key = key.clone();
        subcraft_core::task::spawn(move || {
            let placed = placed
                .filter(|p| Arc::ptr_eq(&p.source, &high) && p.placement == settings.placement)
                .unwrap_or_else(|| {
                    let mesh = place_high(&low, &high, &settings.placement);
                    let bvh = Bvh::build(&mesh);
                    Arc::new(PlacedHigh { source: high.clone(), placement: settings.placement.clone(), mesh, bvh })
                });
            let low_bvh = Bvh::from_triangles(&[]);
            let mut maps = Vec::new();
            let (mut covered, mut missed) = (0, 0);
            for (ts, slot) in sets {
                let input = BakeInput {
                    low: Geometry { mesh: &low, bvh: &low_bvh },
                    high: Some(Geometry { mesh: &placed.mesh, bvh: &placed.bvh }),
                    material_slot: slot,
                };
                if let Ok(r) = cage_misses(&input, &settings, size) {
                    covered += r.covered;
                    missed += r.missed;
                    maps.push((ts, slot, r.image));
                }
            }
            let _ = tx.send(CageCheckResult { key: job_key, maps, covered, missed, placed });
        });
        Self { key, rx }
    }

    /// The result once it is ready (`Some(None)` when the worker died).
    pub fn poll(&self) -> Option<Option<CageCheckResult>> {
        match self.rx.try_recv() {
            Ok(r) => Some(Some(r)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_reports_misses_per_set_and_reuses_the_placed_high_poly() {
        let low = Arc::new(subcraft_mesh::primitives::cube(2.0));
        // The high poly is a larger cube around the low one (0.2 out, wider than the rim texels
        // reach past each face): a thin cage reaches nothing, a deep front cage reaches all of it.
        let high = Arc::new(subcraft_mesh::primitives::cube(2.4));
        let thin = BakeSettings { cage_front: 0.001, cage_rear: 0.001, ..Default::default() };
        let job = CageCheckJob::start(low.clone(), high.clone(), None, thin.clone(), vec![(TextureSetId::new(), 0)], 32);
        let r = loop { if let Some(r) = job.poll() { break r.unwrap(); } std::thread::sleep(std::time::Duration::from_millis(5)); };
        assert_eq!(r.maps.len(), 1);
        assert!(r.miss_fraction() > 0.99, "{}", r.miss_fraction());
        let deep = BakeSettings { cage_front: 0.1, cage_rear: 0.001, ..Default::default() };
        let job = CageCheckJob::start(low.clone(), high.clone(), Some(r.placed.clone()), deep.clone(), vec![(TextureSetId::new(), 0)], 32);
        let r2 = loop { if let Some(r) = job.poll() { break r.unwrap(); } std::thread::sleep(std::time::Duration::from_millis(5)); };
        assert!(Arc::ptr_eq(&r.placed, &r2.placed), "BVH reused while only the cage changed");
        assert!(r2.miss_fraction() < 0.01, "{}", r2.miss_fraction());
        assert_ne!(job.key, CageCheckKey::new(&low, &high, &thin, 32));
    }
}
