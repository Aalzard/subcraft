//! Issue #119: once the window's graphics device is reported lost, the secondary GPU devices
//! submit nothing more and the work falls back to the processor with the same result.
//! The lost flag is process-wide, so this is one test in its own binary. Skips without an adapter.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use subcraft_compositor::gpu::{self, take_gpu_stats};
use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::ChannelKind;
use subcraft_doc::{Layer, LayerContent, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;
use subcraft_render::gpu_composite::GpuCompositor;

const RES: u32 = 256;

static BACKEND: OnceLock<Arc<GpuCompositor>> = OnceLock::new();
static HOOK_CALLS: AtomicU32 = AtomicU32::new(0);

fn texture_set() -> TextureSet {
    let base = TextureSet::new("Set", 0, RES);
    let mut ts = base.clone();
    ts.insert_above(None, Layer::new_fill("Fill", &base));
    let mut paint = Layer::new_paint("Paint", &base);
    let LayerContent::Paint(p) = &mut paint.content else { unreachable!() };
    let img = p.channel_mut(ChannelKind::BaseColor, RES);
    for y in 20..200 {
        for x in 30..220 {
            img.set(x, y, [0.9, 0.3, 0.1, 0.5 + 0.5 * ((x + y) as f32 * 0.05).sin()]);
        }
    }
    ts.insert_above(None, paint);
    ts
}

fn same(a: &Composite, b: &Composite) -> bool {
    a.channels.iter().all(|(kind, ia)| {
        let ib = b.channel(*kind).unwrap();
        (0..a.resolution).all(|y| (0..a.resolution).all(|x| {
            let (p, q) = (ia.get(x, y), ib.get(x, y));
            (0..ia.comps()).all(|c| (p[c] - q[c]).abs() <= 2f32.powi(-9) * p[c].abs().max(1.0) * 8.0)
        }))
    })
}

#[test]
fn after_the_device_is_lost_nothing_is_submitted_and_the_cpu_finishes_the_work() {
    if subcraft_render::gpu_composite::create_any_backend().is_none() {
        eprintln!("no GPU adapter: skipping");
        return;
    }
    gpu::register_gpu_compositor(|| {
        let backend = subcraft_render::gpu_composite::create_any_backend()?;
        let _ = BACKEND.set(backend.clone());
        Some(backend as Arc<dyn gpu::GpuCompositeBackend>)
    });
    gpu::set_gpu_min_tiles(1);
    gpu::set_gpu_min_resolution(0);
    gpu::set_gpu_compositing_enabled(true);
    subcraft_render::caps::set_device_lost_hook(|_| {
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    });

    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    let ctx = EvalContext { geometry: &geo, resources: &res, mesh_maps: &maps };
    let ts = texture_set();

    // Healthy: the GPU does the work.
    let _ = take_gpu_stats();
    let on_gpu = composite_full(&ts, &ctx);
    let stats = take_gpu_stats();
    assert!(stats.tiles > 0 && stats.errors == 0, "the GPU path did not run: {stats:?}");
    let backend = BACKEND.get().expect("backend created");
    let submitted = backend.submissions();
    assert!(submitted > 0);

    // The driver resets (this is what wgpu's device-lost callback calls).
    assert!(subcraft_render::caps::device_lost().is_none());
    subcraft_render::caps::report_device_lost("Unknown: Device is lost");
    subcraft_render::caps::report_device_lost("Unknown: reported again by another device");
    assert_eq!(subcraft_render::caps::device_lost().as_deref(), Some("Unknown: Device is lost"));
    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 1, "the UI is told once");

    // The same work again: not one more submission, every tile done on the processor, same texels.
    let after = composite_full(&ts, &ctx);
    let stats = take_gpu_stats();
    assert_eq!(backend.submissions(), submitted, "nothing may be submitted on a lost device");
    assert_eq!(stats.tiles, 0, "{stats:?}");
    assert!(stats.errors > 0 && stats.fallback_tiles > 0, "{stats:?}");
    assert!(same(&on_gpu, &after), "the CPU fallback must give the same result");
}
