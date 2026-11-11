//! GPU ray casting and curvature for the baker (ADR-0039): WGSL compute over storage buffers
//! built from the `subcraft_mesh` SAH BVH. Implements [`RayBatchBackend`] on its own headless
//! device so the viewport device is never stalled and a long dispatch cannot freeze the UI.
//!
//! Work is split by texel chunks and by work units per pass (rays for AO / thickness, texels for
//! curvature); every dispatch is submitted and awaited on its own and sized from the measured
//! speed to stay near [`TARGET_PASS`], far below the Windows GPU watchdog (TDR, about 2 s).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use rayon::prelude::*;
use subcraft_mesh::Bvh;
use subcraft_mesh::ray_batch::{BatchControl, CurvatureSurface, CurvatureTexel, NO_GROUP, RayBatchBackend, RayBatchError, RayMode, RayTexel};

/// Texels uploaded per chunk (48 bytes each).
const CHUNK: usize = 1 << 16;
/// Wanted duration of one dispatch, and the longest one we accept before shrinking.
const TARGET_PASS: Duration = Duration::from_millis(100);
const MAX_PASS: Duration = Duration::from_millis(250);
const STACK_DEPTH: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    mode: u32,
    ray_start: u32,
    ray_end: u32,
    ray_total: u32,
    count: u32,
    distance: f32,
    node_count: u32,
    pad: u32,
}

struct Pass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

pub struct GpuRayBaker {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    rays: Pass,
    curvature: Pass,
    /// One bake at a time per device keeps dispatches short and memory bounded.
    busy: Mutex<()>,
}

/// Registered with `subcraft_mesh::ray_batch::register_ray_backend` by the application.
pub fn create_backend() -> Option<Arc<dyn RayBatchBackend>> {
    let (adapter, device, queue) = crate::headless::device()?;
    crate::caps::install_worker_error_handlers(&device, "baking");
    Some(Arc::new(GpuRayBaker::new(&adapter, device, queue)))
}

/// What one chunk's dispatches iterate over.
#[derive(Clone, Copy)]
enum Units {
    /// `n` rays for every texel of the chunk.
    Rays(u32),
    /// The texels of the chunk themselves.
    Texels,
}

struct Job<'a> {
    pass: &'a Pass,
    nodes: &'a wgpu::Buffer,
    tris: &'a wgpu::Buffer,
    /// Extra read-only buffers bound from binding 5.
    extra: Vec<&'a wgpu::Buffer>,
    count: usize,
    record: &'a (dyn Fn(usize) -> [f32; 12] + Sync),
    units: Units,
    mode: u32,
    distance: f32,
    finish: &'a (dyn Fn([f32; 2]) -> f32 + Sync),
}

impl GpuRayBaker {
    pub fn new(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let common = include_str!("ray_common.wgsl");
        let module = |label: &str, main: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(format!("{common}\n{main}").into()),
            })
        };
        let rays_module = module("ray bake", include_str!("ray_bake.wgsl"));
        let curvature_module = module("curvature bake", include_str!("curvature.wgsl"));
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let make = |label: &str, module: &wgpu::ShaderModule, extra: usize| {
            let mut entries = vec![storage(0, true), storage(1, true), storage(2, true), storage(3, false), uniform];
            entries.extend((0..extra).map(|i| storage(5 + i as u32, true)));
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            Pass { pipeline, layout }
        };
        let rays = make("ray bake", &rays_module, 0);
        let curvature = make("curvature bake", &curvature_module, 2);
        Self { device, queue, adapter_name: adapter.get_info().name, rays, curvature, busy: Mutex::new(()) }
    }

    fn buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
    }

    fn wait(&self) -> Result<(), RayBatchError> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| RayBatchError::Unsupported(format!("GPU wait failed: {e}")))
    }

    /// Size check against the device limits; `Err` means "use the CPU".
    fn check_size(&self, what: &str, bytes: u64) -> Result<(), RayBatchError> {
        let limits = self.device.limits();
        let max = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        if bytes > max {
            return Err(RayBatchError::Unsupported(format!("{what} needs {} MB of GPU buffer, this GPU allows {} MB per buffer", bytes >> 20, max >> 20)));
        }
        Ok(())
    }

    fn storage_buffer(&self, label: &str, data: &[u8]) -> wgpu::Buffer {
        let buf = self.buffer(label, (data.len() as u64).max(16), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        self.queue.write_buffer(&buf, 0, data);
        buf
    }

    /// Run `job` over all texels in chunks; one value per texel.
    fn drive(&self, job: &Job, control: &dyn BatchControl) -> Result<Vec<f32>, RayBatchError> {
        let limits = self.device.limits();
        let max_groups = limits.max_compute_workgroups_per_dimension;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let chunk = CHUNK.min(job.count);
        let texel_buf = self.buffer("bake texels", (chunk * 48) as u64, storage);
        let accum_buf = self.buffer("bake accum", (chunk * 8) as u64, storage | wgpu::BufferUsages::COPY_SRC);
        let read_buf = self.buffer("bake readback", (chunk * 8) as u64, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let param_buf = self.buffer("bake params", std::mem::size_of::<Params>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: job.nodes.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: job.tris.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: texel_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: accum_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: param_buf.as_entire_binding() },
        ];
        entries.extend(job.extra.iter().enumerate().map(|(i, b)| wgpu::BindGroupEntry { binding: 5 + i as u32, resource: b.as_entire_binding() }));
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("bake"), layout: &job.pass.layout, entries: &entries });

        let run_started = Instant::now();
        let (mut prep_time, mut gpu_time, mut passes) = (Duration::ZERO, Duration::ZERO, 0u32);
        let mut out = Vec::with_capacity(job.count);
        // Work units per dispatch, adapted from measured speed (starts tiny so a slow adapter is safe).
        let mut per_pass = 1u32;
        let mut records: Vec<[f32; 12]> = Vec::with_capacity(chunk);
        for ci in 0..job.count.div_ceil(chunk) {
            if control.is_cancelled() {
                return Err(RayBatchError::Cancelled);
            }
            let first = ci * chunk;
            let part_len = chunk.min(job.count - first);
            let prep = Instant::now();
            (first..first + part_len).into_par_iter().map(job.record).collect_into_vec(&mut records);
            self.queue.write_buffer(&texel_buf, 0, bytemuck::cast_slice(&records));
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.clear_buffer(&accum_buf, 0, None);
            self.queue.submit([enc.finish()]);
            prep_time += prep.elapsed();

            let total_units = match job.units {
                Units::Rays(r) => r,
                Units::Texels => part_len as u32,
            };
            let mut done = 0u32;
            while done < total_units {
                if control.is_cancelled() {
                    return Err(RayBatchError::Cancelled);
                }
                let end = (done + per_pass).min(total_units);
                let threads = match job.units {
                    Units::Rays(_) => part_len as u32,
                    Units::Texels => end - done,
                };
                let groups = threads.div_ceil(64);
                let (gx, gy) = if groups <= max_groups { (groups, 1) } else { (max_groups, groups.div_ceil(max_groups)) };
                let params = Params {
                    mode: job.mode,
                    ray_start: done,
                    ray_end: end,
                    ray_total: total_units,
                    count: part_len as u32,
                    distance: job.distance,
                    node_count: 0,
                    pad: 0,
                };
                self.queue.write_buffer(&param_buf, 0, bytemuck::bytes_of(&params));
                let mut enc = self.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&job.pass.pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups(gx, gy, 1);
                }
                let started = Instant::now();
                self.queue.submit([enc.finish()]);
                self.wait()?;
                let took = started.elapsed();
                gpu_time += took;
                passes += 1;
                let n = end - done;
                done = end;
                // Next pass: scale toward the target time (at most 4x growth; shrink at once).
                let per_unit = took.as_secs_f64().max(1e-5) / n as f64;
                let fit = (TARGET_PASS.as_secs_f64() / per_unit).min(u32::MAX as f64) as u32;
                per_pass = if took > MAX_PASS { fit.max(1) } else { fit.clamp(1, n.saturating_mul(4).max(1)) };
                let frac = first as f32 + part_len as f32 * done as f32 / total_units as f32;
                control.progress(frac / job.count as f32);
            }

            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&accum_buf, 0, &read_buf, 0, (part_len * 8) as u64);
            self.queue.submit([enc.finish()]);
            let slice = read_buf.slice(..(part_len * 8) as u64);
            // A failed map (device lost, driver reset) must not panic: `get_mapped_range` below then
            // errors and the bake falls back to the CPU (issue #119).
            slice.map_async(wgpu::MapMode::Read, |r| if let Err(e) = r { log::warn!("GPU bake readback failed: {e}") });
            self.wait()?;
            {
                let data = slice.get_mapped_range().map_err(|e| RayBatchError::Unsupported(format!("GPU readback failed: {e}")))?;
                let acc: &[[f32; 2]] = bytemuck::cast_slice(&data);
                out.extend(acc.iter().map(|a| (job.finish)(*a)));
            }
            read_buf.unmap();
        }
        log::info!(
            "GPU bake pass: {} texels, total {:.2}s, records {:.2}s, dispatches {:.2}s in {passes} passes",
            job.count,
            run_started.elapsed().as_secs_f32(),
            prep_time.as_secs_f32(),
            gpu_time.as_secs_f32()
        );
        Ok(out)
    }

    /// Upload the tree. `Err` when it does not fit the device or is too deep for the shader stack.
    fn upload_bvh(&self, bvh: &Bvh) -> Result<(wgpu::Buffer, wgpu::Buffer, Vec<u32>), RayBatchError> {
        let flat = bvh.flatten();
        if tree_depth(&flat.nodes) + 2 >= STACK_DEPTH {
            return Err(RayBatchError::Unsupported("BVH is too deep for the GPU traversal stack".into()));
        }
        self.check_size("the BVH nodes", (flat.nodes.len() * 32) as u64)?;
        self.check_size("the BVH triangles", (flat.tris.len() * 48) as u64)?;
        let nodes = self.storage_buffer("bvh nodes", bytemuck::cast_slice(&flat.nodes));
        let tris = self.storage_buffer("bvh triangles", bytemuck::cast_slice(&flat.tris));
        Ok((nodes, tris, flat.tri_ids))
    }
}

/// Deepest root-to-leaf path (nodes). The shader stack holds [`STACK_DEPTH`] entries.
fn tree_depth(nodes: &[[u32; 8]]) -> usize {
    let mut deepest = 0;
    let mut todo = vec![(0usize, 1usize)];
    while let Some((i, d)) = todo.pop() {
        deepest = deepest.max(d);
        if nodes[i][7] == 0 {
            let l = nodes[i][3] as usize;
            todo.push((l, d + 1));
            todo.push((l + 1, d + 1));
        }
    }
    deepest
}

impl RayBatchBackend for GpuRayBaker {
    fn description(&self) -> String {
        format!("GPU ({})", self.adapter_name)
    }

    fn run(
        &self, bvh: &Bvh, count: usize, texel: &(dyn Fn(usize) -> RayTexel + Sync), rays: u32, mode: RayMode, control: &dyn BatchControl,
    ) -> Result<Vec<f32>, RayBatchError> {
        let rays = rays.max(1);
        let (mode_id, distance) = match mode {
            RayMode::Occlusion { distance } => (0u32, distance),
            RayMode::MeanDistance { max } => (1u32, max),
        };
        if bvh.is_empty() || count == 0 {
            // Nothing to hit: occlusion is 1, and every thickness ray travels the full distance.
            let value = match mode {
                RayMode::Occlusion { .. } => 1.0,
                RayMode::MeanDistance { .. } => 0.0,
            };
            return Ok(vec![value; count]);
        }
        let _guard = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let (nodes, tris, _) = self.upload_bvh(bvh)?;
        let record = |i: usize| {
            let t = texel(i);
            [t.origin[0], t.origin[1], t.origin[2], t.rotation, t.axis[0], t.axis[1], t.axis[2], 0.0, t.offset[0], t.offset[1], t.offset[2], 0.0]
        };
        let finish = |acc: [f32; 2]| match mode {
            RayMode::Occlusion { .. } => {
                if acc[1] <= 0.0 { 1.0 } else { 1.0 - acc[0] / acc[1] }
            }
            RayMode::MeanDistance { max } => 1.0 - acc[0] / rays as f32 / max,
        };
        self.drive(
            &Job { pass: &self.rays, nodes: &nodes, tris: &tris, extra: vec![], count, record: &record, units: Units::Rays(rays), mode: mode_id, distance, finish: &finish },
            control,
        )
    }

    fn curvature(
        &self, bvh: &Bvh, surface: CurvatureSurface, count: usize, texel: &(dyn Fn(usize) -> CurvatureTexel + Sync), radius: f32, control: &dyn BatchControl,
    ) -> Result<Vec<f32>, RayBatchError> {
        if bvh.is_empty() || count == 0 {
            return Ok(vec![0.5; count]);
        }
        let _guard = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let (nodes, tris, tri_ids) = self.upload_bvh(bvh)?;
        // Per-triangle data in leaf order, like the triangles.
        let normals: Vec<[f32; 12]> = tri_ids.iter().map(|&t| surface.tri_normals[t as usize]).collect();
        let groups: Vec<u32> = tri_ids.iter().map(|&t| surface.tri_group[t as usize]).collect();
        self.check_size("the triangle normals", (normals.len() * 48) as u64)?;
        let normal_buf = self.storage_buffer("triangle normals", bytemuck::cast_slice(&normals));
        let group_buf = self.storage_buffer("triangle groups", bytemuck::cast_slice(&groups));
        drop((normals, groups));
        let record = |i: usize| {
            let t = texel(i);
            [t.position[0], t.position[1], t.position[2], if t.group == NO_GROUP { -1.0 } else { t.group as f32 }, t.normal[0], t.normal[1], t.normal[2], 0.0, 0.0, 0.0, 0.0, 0.0]
        };
        let finish = |acc: [f32; 2]| acc[0];
        self.drive(
            &Job {
                pass: &self.curvature,
                nodes: &nodes,
                tris: &tris,
                extra: vec![&normal_buf, &group_buf],
                count,
                record: &record,
                units: Units::Texels,
                mode: 0,
                distance: radius,
                finish: &finish,
            },
            control,
        )
    }
}
