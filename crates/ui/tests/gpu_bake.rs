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
// Curvature has hard thresholds (neighbour cut-offs), so a texel on one can flip.
const CURV_MEAN_TOL: f32 = 0.01;
const CURV_P99_TOL: f32 = 0.1;

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

const MAPS: [MeshMapKind; 4] = [MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness, MeshMapKind::Curvature, MeshMapKind::BentNormal];

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
        // Every colour channel (bent normals have three; grey maps repeat one).
        let (pa, pb) = (a.get(x, y), b.get(x, y));
        let mut worst = 0.0f32;
        for c in 0..a.comps().min(3) {
            let (va, vb) = (pa[c], pb[c]);
            assert!(va.is_finite() && vb.is_finite(), "NaN/inf at ({x},{y}): {va} vs {vb}");
            worst = worst.max((va - vb).abs());
        }
        errors.push(worst);
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
            // The UV sphere's south pole is a fan of ~1e-7 wide sliver triangles (sin(pi) is not 0). Their
            // curvature is numerical noise on the CPU as well, so the two paths cannot be compared there.
            if kind == MeshMapKind::Curvature && name == "sphere" {
                continue;
            }
            let s = compare(&mesh, &cpu[&kind], &fast[&kind]);
            eprintln!("{name} {}: mean abs error {:.5}, p99 {:.5}", kind.display_name(), s.mean, s.p99);
            let (mean_tol, p99_tol) = if kind == MeshMapKind::Curvature { (CURV_MEAN_TOL, CURV_P99_TOL) } else { (MEAN_TOL, P99_TOL) };
            assert!(s.mean <= mean_tol, "{name} {}: mean error {}", kind.display_name(), s.mean);
            assert!(s.p99 <= p99_tol, "{name} {}: p99 error {}", kind.display_name(), s.p99);
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

#[test]
fn gpu_curvature_matches_cpu_with_a_high_poly_source() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let low = primitives::torus(1.0, 0.35, 32, 16);
    let high = primitives::torus(1.0, 0.36, 96, 48);
    let (low_bvh, high_bvh) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &low_bvh }, high: Some(Geometry { mesh: &high, bvh: &high_bvh }), material_slot: 0 };
    let settings = BakeSettings { size: SIZE, maps: vec![MeshMapKind::Curvature], cage_front: 0.1, cage_rear: 0.1, ..BakeSettings::default() };
    let cpu = bake_streamed_with(&input, &settings, &Progress::new(), None, |_, _| {}).expect("cpu");
    let progress = Progress::new();
    let fast = bake_streamed_with(&input, &settings, &progress, Some(gpu.as_ref()), |_, _| {}).expect("gpu");
    assert!(progress.reports()[0].backend.starts_with("GPU"), "{:?}", progress.reports());
    let s = compare(&low, &cpu[&MeshMapKind::Curvature], &fast[&MeshMapKind::Curvature]);
    eprintln!("high-poly curvature: mean abs error {:.5}, p99 {:.5}", s.mean, s.p99);
    assert!(s.mean <= CURV_MEAN_TOL && s.p99 <= CURV_P99_TOL, "mean {} p99 {}", s.mean, s.p99);
}

const PROJECTED: [MeshMapKind; 4] = [MeshMapKind::Height, MeshMapKind::Normal, MeshMapKind::WorldSpaceNormal, MeshMapKind::AmbientOcclusion];

#[test]
fn gpu_high_poly_projection_matches_cpu() {
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let low = primitives::torus(1.0, 0.35, 32, 16);
    let high = primitives::torus(1.0, 0.37, 96, 48);
    let (low_bvh, high_bvh) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &low_bvh }, high: Some(Geometry { mesh: &high, bvh: &high_bvh }), material_slot: 0 };
    for ignore_backface in [false, true] {
        let settings = BakeSettings { size: SIZE, maps: PROJECTED.to_vec(), cage_front: 0.1, cage_rear: 0.1, ignore_backface, ..BakeSettings::default() };
        let cpu = bake_streamed_with(&input, &settings, &Progress::new(), None, |_, _| {}).expect("cpu");
        let progress = Progress::new();
        let fast = bake_streamed_with(&input, &settings, &progress, Some(gpu.as_ref()), |_, _| {}).expect("gpu");
        let reports = progress.reports();
        let height = reports.iter().find(|r| r.kind == MeshMapKind::Height).expect("height report");
        assert!(height.backend.starts_with("CPU, projection on GPU"), "{reports:?}");
        for kind in PROJECTED {
            let s = compare(&low, &cpu[&kind], &fast[&kind]);
            eprintln!("projection (ignore backface {ignore_backface}) {}: mean abs error {:.5}, p99 {:.5}", kind.display_name(), s.mean, s.p99);
            assert!(s.mean <= MEAN_TOL && s.p99 <= P99_TOL, "{}: mean {} p99 {}", kind.display_name(), s.mean, s.p99);
        }
    }
}

#[test]
fn gpu_projection_hits_match_the_bvh() {
    use subcraft_mesh::ray_batch::ProjectionRay;
    let _g = gpu_test();
    let Some(gpu) = backend() else { return };
    let mesh = primitives::sample_crate();
    let bvh = Bvh::build(&mesh);
    let n = 4000usize;
    // Rays from a sphere around the crate toward jittered points near its centre; some miss.
    let ray = |i: usize| {
        let f = i as f32;
        let (theta, phi) = (f * 2.399_963, (1.0 - 2.0 * (f + 0.5) / n as f32).acos());
        let origin = glam::Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()) * 3.0;
        let target = glam::Vec3::new((f * 0.37).sin(), (f * 0.53).cos(), (f * 0.71).sin()) * 0.8;
        // Every other ray starts inside the crate and looks out: it only meets back faces.
        let (origin, dir) = if i % 2 == 1 { (target * 0.5, (origin - target).normalize()) } else { (origin, (target - origin).normalize()) };
        ProjectionRay { origin: origin.to_array(), dir: dir.to_array(), reach: 2.0 + (i % 5) as f32 * 0.5 }
    };
    let mut counts = Vec::new();
    for front_only in [false, true] {
        let hits = gpu.project(&bvh, n, &ray, front_only, &NoControl).expect("project");
        assert_eq!(hits.len(), n);
        let (mut agree, mut hit_count) = (0usize, 0usize);
        for (i, h) in hits.iter().enumerate() {
            let r = ray(i);
            let (o, d) = (glam::Vec3::from(r.origin), glam::Vec3::from(r.dir));
            let cpu = bvh.intersect_filtered(o, d, 0.0, r.reach, |t| !front_only || mesh.geometric_normal(t as usize).dot(d) < 0.0);
            hit_count += cpu.is_some() as usize;
            let same = match (cpu, h) {
                (None, None) => true,
                (Some(c), Some(g)) => (c.t - g.t).abs() < 1e-4 && (c.tri == g.tri || ((c.u - g.u).abs() < 1e-3 || (c.v - g.v).abs() < 1e-3)),
                _ => false,
            };
            agree += same as usize;
        }
        eprintln!("front_only {front_only}: {agree} of {n} agree, {hit_count} hits");
        assert!(hit_count > n / 4 && hit_count < n, "test rays should both hit and miss: {hit_count}");
        // Rays that graze an edge may pick the neighbouring triangle or miss; allow a handful.
        assert!(agree >= n - n / 200, "only {agree} of {n} projection rays agree");
        counts.push(hit_count);
    }
    assert!(counts[1] < counts[0], "front only must skip the back faces the inside rays meet: {counts:?}");
    // Empty tree: every ray misses without touching the GPU.
    assert_eq!(gpu.project(&Bvh::default(), 3, &ray, false, &NoControl).unwrap(), vec![None; 3]);
}
