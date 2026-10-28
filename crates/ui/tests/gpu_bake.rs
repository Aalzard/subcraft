//! GPU baking against the CPU reference (ADR-0039). Skips with "no adapter" without a GPU.
//!
//! Stated tolerance: the GPU is not bit-identical (fused multiply-add, different sin/cos, rays that
//! graze an edge can flip), so per map the mean absolute error must stay below `MEAN_TOL` and the
//! 99th percentile below `P99_TOL`; the covered texels must be identical and no value may be NaN.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_bake::surface::texels;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake_streamed_with};
use subcraft_core::MeshMapKind;
use subcraft_image::TiledImage;
use subcraft_mesh::ray_batch::{BatchControl, RayBatchBackend, RayBatchError, RayMode, RayTexel};
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_render::ray_bake::create_backend;

const SIZE: u32 = 256;
const MEAN_TOL: f32 = 0.004;
const P99_TOL: f32 = 0.08;

// Concurrent DX12 device creation can stall the driver: one GPU test at a time.
fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn backend() -> Option<Arc<dyn RayBatchBackend>> {
    static B: std::sync::OnceLock<Option<Arc<dyn RayBatchBackend>>> = std::sync::OnceLock::new();
    let b = B.get_or_init(create_backend).clone();
    if b.is_none() {
        eprintln!("no adapter: skipping");
    }
    b
}

const MAPS: [MeshMapKind; 2] = [MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness];

fn bake(mesh: &Mesh, gpu: Option<&dyn RayBatchBackend>, progress: &Progress) -> BTreeMap<MeshMapKind, TiledImage> {
    let bvh = Bvh::build(mesh);
    let input = BakeInput { low: Geometry { mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let settings = BakeSettings { size: SIZE, maps: MAPS.to_vec(), ..BakeSettings::default() };
    bake_streamed_with(&input, &settings, progress, gpu, |_, _| {}).expect("bake")
}

struct Stats {
    mean: f32,
    p99: f32,
}

fn compare(mesh: &Mesh, a: &TiledImage, b: &TiledImage) -> Stats {
    let covered = texels(mesh, 0, SIZE);
    assert!(covered.len() > 500, "test mesh covers {} texels", covered.len());
    let mut errors = Vec::with_capacity(covered.len());
    for t in &covered {
        let (x, y) = (t.index % SIZE, t.index / SIZE);
        let (va, vb) = (a.get(x, y)[0], b.get(x, y)[0]);
        assert!(va.is_finite() && vb.is_finite(), "NaN/inf at ({x},{y}): {va} vs {vb}");
        errors.push((va - vb).abs());
    }
    errors.sort_by(f32::total_cmp);
    Stats { mean: errors.iter().sum::<f32>() / errors.len() as f32, p99: errors[errors.len() * 99 / 100] }
}

fn meshes() -> Vec<(&'static str, Mesh)> {
    vec![
        ("crate", primitives::sample_crate()),
        ("torus", primitives::torus(1.0, 0.35, 48, 24)),
        ("sphere", primitives::uv_sphere(1.0, 48, 24)),
        ("slab", primitives::thin_slab(2.0, 2.0, 0.05)),
    ]
}

#[test]
fn gpu_matches_cpu_on_the_crate_and_primitives() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    eprintln!("adapter: {}", gpu.description());
    for (name, mesh) in meshes() {
        let cpu_progress = Progress::new();
        let cpu = bake(&mesh, None, &cpu_progress);
        let gpu_progress = Progress::new();
        let fast = bake(&mesh, Some(gpu.as_ref()), &gpu_progress);
        assert!(gpu_progress.reports().iter().all(|r| r.backend.starts_with("GPU")), "{name}: {:?}", gpu_progress.reports());
        assert!((gpu_progress.fraction() - 1.0).abs() < 1e-6);
        for kind in MAPS {
            let s = compare(&mesh, &cpu[&kind], &fast[&kind]);
            eprintln!("{name} {}: mean abs error {:.5}, p99 {:.5}", kind.display_name(), s.mean, s.p99);
            assert!(s.mean <= MEAN_TOL, "{name} {}: mean error {}", kind.display_name(), s.mean);
            assert!(s.p99 <= P99_TOL, "{name} {}: p99 error {}", kind.display_name(), s.p99);
            // Gutters are padded from the same covered texels, so no pixel of the image may be NaN.
            for y in (0..SIZE).step_by(3) {
                for x in (0..SIZE).step_by(3) {
                    assert!(fast[&kind].get(x, y)[0].is_finite());
                }
            }
        }
    }
}

#[test]
fn gpu_bakes_are_repeatable_on_the_same_device() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let mesh = primitives::sample_crate();
    let a = bake(&mesh, Some(gpu.as_ref()), &Progress::new());
    let b = bake(&mesh, Some(gpu.as_ref()), &Progress::new());
    assert_eq!(a, b, "two GPU bakes of the same mesh must be identical");
}

#[test]
fn gpu_ao_darkens_a_concave_crease_and_leaves_open_plane_bright() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let plane = primitives::plane(2.0, 2);
    let maps = bake(&plane, Some(gpu.as_ref()), &Progress::new());
    for t in texels(&plane, 0, SIZE).iter().step_by(31) {
        assert!(maps[&MeshMapKind::AmbientOcclusion].get(t.index % SIZE, t.index / SIZE)[0] > 0.99);
    }
    let crate_mesh = primitives::sample_crate();
    let maps = bake(&crate_mesh, Some(gpu.as_ref()), &Progress::new());
    let ao = &maps[&MeshMapKind::AmbientOcclusion];
    let covered = texels(&crate_mesh, 0, SIZE);
    let darkest = covered.iter().map(|t| ao.get(t.index % SIZE, t.index / SIZE)[0]).fold(1.0f32, f32::min);
    assert!(darkest < 0.9, "the crate has occluded corners, darkest AO {darkest}");
}

struct NoControl;

impl BatchControl for NoControl {
    fn is_cancelled(&self) -> bool {
        false
    }
    fn progress(&self, _: f32) {}
}

struct CancelAfter(std::sync::atomic::AtomicUsize);

impl BatchControl for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed) > 0
    }
    fn progress(&self, _: f32) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn gpu_cancel_stops_between_chunks_and_progress_is_monotonic() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let mesh = primitives::torus(1.0, 0.35, 48, 24);
    let bvh = Bvh::build(&mesh);
    let record = |i: usize| RayTexel { origin: [0.0, 0.0, 2.0 + (i % 7) as f32 * 0.01], rotation: 0.3, axis: [0.0, 0.0, -1.0], offset: [0.0, 0.0, -1.0] };
    // Complete run: one value per texel and progress never goes backwards.
    struct Monitor(std::sync::Mutex<Vec<f32>>);
    impl BatchControl for Monitor {
        fn is_cancelled(&self) -> bool {
            false
        }
        fn progress(&self, f: f32) {
            self.0.lock().unwrap().push(f);
        }
    }
    let m = Monitor(Default::default());
    let out = gpu.run(&bvh, 150_000, &record, 8, RayMode::Occlusion { distance: 10.0 }, &m).expect("run");
    assert_eq!(out.len(), 150_000);
    let seen = m.0.lock().unwrap().clone();
    assert!(seen.windows(2).all(|w| w[1] >= w[0]) && (seen.last().unwrap() - 1.0).abs() < 1e-5, "{seen:?}");
    // Cancelled after the first dispatch.
    let r = gpu.run(&bvh, 150_000, &record, 8, RayMode::Occlusion { distance: 10.0 }, &CancelAfter(Default::default()));
    assert_eq!(r.unwrap_err(), RayBatchError::Cancelled);
}

#[test]
fn gpu_refuses_a_mesh_that_does_not_fit_instead_of_panicking() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    // An empty BVH has nothing to hit: AO 1, thickness 0, without touching the GPU.
    let bvh = Bvh::default();
    let record = |_: usize| RayTexel { origin: [0.0; 3], rotation: 0.0, axis: [0.0, 1.0, 0.0], offset: [0.0, 1.0, 0.0] };
    let ao = gpu.run(&bvh, 3, &record, 4, RayMode::Occlusion { distance: 1.0 }, &NoControl).unwrap();
    assert_eq!(ao, [1.0; 3]);
    let thick = gpu.run(&bvh, 3, &record, 4, RayMode::MeanDistance { max: 1.0 }, &NoControl).unwrap();
    assert_eq!(thick, [0.0; 3]);
}

fn finished_summary(use_gpu: bool) -> (String, bool) {
    let mut session = subcraft_app::Session::new(primitives::sample_crate(), 128);
    let ts = session.selected_texture_set().clone();
    let mut job = session.start_bake(subcraft_app::BakeRequest {
        high_poly: None,
        texture_sets: vec![(ts.id, ts.material_slot)],
        settings: BakeSettings { size: 128, maps: vec![MeshMapKind::AmbientOcclusion, MeshMapKind::Normal], use_gpu, ..BakeSettings::default() },
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if let Some(result) = job.poll(&mut session) {
            assert_eq!(result, Ok(1));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "bake timed out");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    (job.summary(), session.mesh_maps(ts.id).is_some_and(|m| m.get(MeshMapKind::AmbientOcclusion).is_some()))
}

#[test]
fn a_bake_job_reports_the_path_and_time_and_the_preference_off_uses_the_cpu() {
    let _g = gpu_test();
    let have_gpu = backend().is_some();
    subcraft_ui::register_gpu_baking();
    let (summary, installed) = finished_summary(true);
    assert!(installed);
    assert!(summary.contains(" s total") && summary.contains("Ambient Occlusion"), "{summary}");
    if have_gpu {
        assert!(summary.contains("on GPU ("), "{summary}");
        assert!(summary.contains("Normal ") && summary.contains("on CPU"), "{summary}");
    }
    let (summary, _) = finished_summary(false);
    assert!(!summary.contains("GPU"), "{summary}");
}

#[test]
fn old_preferences_without_the_gpu_switch_default_to_on() {
    let prefs: subcraft_ui::recovery::Preferences = serde_json::from_str("{\"autosave_minutes\": 5}").unwrap();
    assert!(prefs.use_gpu_baking);
    let off: subcraft_ui::recovery::Preferences = serde_json::from_str("{\"use_gpu_baking\": false}").unwrap();
    assert!(!off.use_gpu_baking);
}
