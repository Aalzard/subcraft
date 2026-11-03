//! GPU vs CPU compositing benchmark (ADR-0044).
//!
//! `cargo run --release -p subcraft-render --example gpu_composite_bench -- [resolution] [layers] [reps]`
//!
//! Scenes: (1) the `big_project_bench` stack as shipped (it has blur / sharpen / median filters,
//! so the GPU backend declines it), (2) the same stack with the neighborhood filters removed,
//! (3) a paint / uniform-fill / folder stack. Per scene: cold full composite, toggle a layer,
//! opacity change of the top layer and of a mid layer, each on the CPU and on the GPU, with the
//! GPU time split into CPU preparation, upload, GPU, readback and storing the result.
//! `SUBCRAFT_GPU_COMPOSITE_SOFTWARE=1` accepts a software adapter.
#![allow(dead_code)]
use std::sync::Arc;
use std::time::Instant;

use subcraft_compositor::gpu::{self, take_gpu_stats};
use subcraft_compositor::{CompositeCache, EvalContext};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{
    BlendMode, ContentEffectKind, FillChannel, FillSource, Layer, LayerContent, MaskEffectKind, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};

#[path = "../../app/examples/big_project_bench.rs"]
#[allow(clippy::all, unused)]
mod scenes;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

struct Scene {
    name: &'static str,
    ts: TextureSet,
    top: LayerId,
    mid: LayerId,
}

/// Remove blur-type filters (content and mask) so the set is supported by the GPU backend.
fn strip_spatial(ts: &mut TextureSet) {
    fn walk(list: &mut Vec<Arc<Layer>>) {
        for l in list.iter_mut() {
            let layer = Arc::make_mut(l);
            layer.content_effects.retain(|e| !matches!(&e.kind, ContentEffectKind::Filter { filter, .. } if filter.radius().is_some()));
            if let Some(m) = &mut layer.mask {
                m.effects.retain(|e| !matches!(&e.kind, MaskEffectKind::Filter(f) if f.radius().is_some()));
            }
            if let LayerContent::Group(g) = &mut layer.content {
                walk(&mut g.children);
            }
        }
    }
    walk(&mut ts.layers);
}

/// Paint rasters over about half of the tiles, uniform fills with masks, folders.
fn paint_stack(res: u32, layers: usize) -> Scene {
    let mut ts = TextureSet::new("T", 0, res);
    let base = ts.clone();
    let tiles = (res / 64) as usize;
    let mut ids = Vec::new();
    for i in 0..layers {
        let f = i as f32 / layers as f32;
        let mut layer = if i % 4 == 0 {
            let mut l = Layer::new_paint("P", &base);
            let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
            let kind = if i % 8 == 0 { ChannelKind::BaseColor } else { ChannelKind::Roughness };
            let img = p.channel_mut(kind, res);
            let comps = img.comps();
            let mut buf = vec![0.0f32; 4096 * comps];
            for t in 0..tiles * tiles {
                let (tx, ty) = (t % tiles, t / tiles);
                if (tx * 7 + ty * 3 + i) % 2 != 0 {
                    continue;
                }
                for (n, px) in buf.chunks_exact_mut(comps).enumerate() {
                    let a = 0.4 + 0.6 * (((n % 64) as f32 + (n / 64) as f32) / 128.0);
                    if comps == 4 {
                        px.copy_from_slice(&[0.2 + 0.7 * f, 0.6 - 0.4 * f, 0.3 + (tx as f32 / tiles as f32) * 0.5, a]);
                    } else {
                        px.copy_from_slice(&[0.2 + 0.7 * f, a]);
                    }
                }
                img.write_tile(t, &buf);
            }
            l
        } else {
            let mut l = Layer::new_fill("F", &base);
            let LayerContent::Fill(c) = &mut l.content else { unreachable!() };
            for (k, ch) in c.channels.iter_mut() {
                ch.enabled = false;
                if *k == ChannelKind::BaseColor {
                    *ch = FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.3 + 0.5 * f, 0.4, 0.8 - 0.5 * f)));
                } else if *k == ChannelKind::Roughness {
                    *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.2 + 0.7 * f)));
                }
            }
            l
        };
        let mode = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Overlay, BlendMode::Screen, BlendMode::SoftLight][i % 5];
        for b in layer.blend.values_mut() {
            b.mode = mode;
            b.opacity = 0.5 + 0.5 * ((i * 3 % 7) as f32 / 7.0);
        }
        if i % 3 == 1 {
            let mut m = subcraft_doc::Mask::new(res, i % 2 == 0);
            let img = &mut m.base;
            let buf = vec![0.5 + 0.5 * f; 4096];
            for t in (0..tiles * tiles).step_by(3) {
                img.write_tile(t, &buf);
            }
            layer.mask = Some(m);
        }
        ids.push(ts.insert_above(None, layer));
    }
    // Wrap layers 5..13 in a multiply folder.
    let folder = ts.insert_above(None, Layer::new_folder("Folder", &base));
    for id in ids[5..13.min(ids.len())].iter().rev() {
        let at = 0;
        ts.move_to(*id, Some(folder), at).unwrap();
    }
    for b in ts.find_mut(folder).unwrap().blend.values_mut() {
        b.mode = BlendMode::Multiply;
        b.opacity = 0.8;
    }
    let top = *ids.last().unwrap();
    Scene { name: "paint + uniform fills + folder", ts, top, mid: ids[ids.len() / 2] }
}

fn eval_context<'a>(geo: &'a TextureSetGeometry, res: &'a ResourceStore, maps: &'a MeshMaps) -> EvalContext<'a> {
    EvalContext { geometry: geo, resources: res, mesh_maps: maps }
}

struct Run {
    total: f64,
    tiles: usize,
    stats: gpu::GpuStats,
}

fn update(cache: &mut CompositeCache, ts: &TextureSet, ctx: &EvalContext, use_gpu: bool) -> Run {
    gpu::set_gpu_compositing_enabled(use_gpu);
    let _ = take_gpu_stats();
    let t = Instant::now();
    let tiles = cache.update(ts, ctx).len();
    Run { total: ms(t), tiles, stats: take_gpu_stats() }
}

fn describe(r: &Run) -> String {
    let s = &r.stats;
    if s.tiles == 0 {
        return format!("{:>8.0} ms, {} tiles (CPU)", r.total, r.tiles);
    }
    format!(
        "{:>8.0} ms, {} tiles ({} on GPU): prep {:.0} + upload {:.0} + gpu {:.0} (kernels {:.0}) + readback {:.0} + store {:.0}; uploaded {} MB, cache {}/{} hit",
        r.total, r.tiles, s.tiles, s.prep_ms, s.timing.upload_ms, s.timing.gpu_ms, s.timing.kernel_ms, s.timing.readback_ms, s.apply_ms,
        s.timing.uploaded_bytes >> 20, s.timing.cache_hits, s.timing.cache_hits + s.timing.cache_misses
    )
}

fn bench_scene(scene: &Scene, res: u32, reps: usize) {
    println!("\n== {} ({res}², {} top-level layers)", scene.name, scene.ts.layers.len());
    let mesh = subcraft_mesh::primitives::uv_sphere(1.0, 64, 64);
    let geo = TextureSetGeometry::new(Arc::new(mesh), 0, res);
    let (resources, maps) = (ResourceStore::default(), MeshMaps::new());
    let ctx = eval_context(&geo, &resources, &maps);
    for use_gpu in [false, true] {
        let label = if use_gpu { "GPU" } else { "CPU" };
        let mut cache = CompositeCache::new();
        let r = update(&mut cache, &scene.ts, &ctx, use_gpu);
        println!("{label} cold full composite : {}", describe(&r));
        // Toggle visibility of a mid layer (every tile re-evaluates), off then on.
        let mut ts = scene.ts.clone();
        let mut times = Vec::new();
        let mut last = None;
        for i in 0..reps * 2 {
            let l = ts.find_mut(scene.mid).unwrap();
            l.visible = !l.visible;
            let r = update(&mut cache, &ts, &ctx, use_gpu);
            if i % 2 == 0 || true {
                times.push(r.total);
            }
            last = Some(r);
        }
        println!("{label} toggle mid layer    : median {:>6.0} ms | last {}", median(times), describe(&last.unwrap()));
        let mut times = Vec::new();
        let mut last = None;
        for i in 0..reps {
            for b in ts.find_mut(scene.top).unwrap().blend.values_mut() {
                b.opacity = 0.9 - 0.05 * i as f32;
            }
            let r = update(&mut cache, &ts, &ctx, use_gpu);
            times.push(r.total);
            last = Some(r);
        }
        println!("{label} opacity, top layer  : median {:>6.0} ms | last {}", median(times), describe(&last.unwrap()));
        let mut times = Vec::new();
        let mut last = None;
        for i in 0..reps {
            for b in ts.find_mut(scene.mid).unwrap().blend.values_mut() {
                b.opacity = 0.9 - 0.04 * i as f32;
            }
            let r = update(&mut cache, &ts, &ctx, use_gpu);
            times.push(r.total);
            last = Some(r);
        }
        println!("{label} opacity, mid layer  : median {:>6.0} ms | last {}", median(times), describe(&last.unwrap()));
    }
    gpu::set_gpu_compositing_enabled(true);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let res: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4096);
    let layers: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(40);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(3);
    subcraft_compositor::gpu::register_gpu_compositor(subcraft_render::gpu_composite::create_backend);
    match gpu::gpu_backend_name() {
        Some(n) => println!("GPU backend: {n}"),
        None => println!("no usable GPU backend (software adapters are refused): GPU rows will show CPU"),
    }
    println!("threads: {}", rayon::current_num_threads());
    // big_project_bench stack, built through the real session commands.
    let mesh = subcraft_mesh::primitives::uv_sphere(1.0, 64, 64);
    let mut s = subcraft_app::Session::new(mesh, res);
    let ids = scenes::build_stack(&mut s, layers);
    let ts = s.document().texture_sets[0].clone();
    // `SUBCRAFT_BENCH_PAINT_ONLY=1` runs only the paint scene (quick re-measurement).
    if std::env::var_os("SUBCRAFT_BENCH_PAINT_ONLY").is_some() {
        drop(s);
        bench_scene(&paint_stack(res, layers), res, reps);
        return;
    }
    let scene = Scene { name: "big_project_bench stack as shipped", top: *ids.last().unwrap(), mid: ids[ids.len() / 2], ts: ts.clone() };
    bench_scene(&scene, res, reps);
    let mut stripped = ts;
    strip_spatial(&mut stripped);
    bench_scene(&Scene { name: "big_project_bench stack, neighborhood filters removed", ts: stripped, top: scene.top, mid: scene.mid }, res, reps);
    drop(s);
    bench_scene(&paint_stack(res, layers), res, reps);
}
