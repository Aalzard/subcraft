//! GPU ray casting for the baker (ADR-0039): WGSL compute over storage buffers built from the
//! `subcraft_mesh` SAH BVH. Implements [`RayBatchBackend`] on its own headless device so the
//! viewport device is never stalled and a long dispatch cannot freeze the UI.
//!
//! Work is split by texel chunks and by rays per pass; every dispatch is submitted and awaited
//! on its own and sized from the measured speed to stay near [`TARGET_PASS`], far below the
//! Windows GPU watchdog (TDR, about 2 s).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use subcraft_mesh::Bvh;
use subcraft_mesh::ray_batch::{BatchControl, RayBatchBackend, RayBatchError, RayMode, RayTexel};

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

pub struct GpuRayBaker {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// One bake at a time per device keeps dispatches short and memory bounded.
    busy: Mutex<()>,
}

/// Registered with `subcraft_mesh::ray_batch::register_ray_backend` by the application.
pub fn create_backend() -> Option<Arc<dyn RayBatchBackend>> {
    let (adapter, device, queue) = crate::headless::device()?;
    Some(Arc::new(GpuRayBaker::new(&adapter, device, queue)))
}

impl GpuRayBaker {
    pub fn new(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ray bake"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ray_bake.wgsl").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ray bake"),
            entries: &[
                entry(0, storage(true)),
                entry(1, storage(true)),
                entry(2, storage(true)),
                entry(3, storage(false)),
                entry(4, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ray bake"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ray bake"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { device, queue, adapter_name: adapter.get_info().name, pipeline, layout, busy: Mutex::new(()) }
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
        let finish = |acc: [f32; 2]| match mode {
            RayMode::Occlusion { .. } => {
                if acc[1] <= 0.0 { 1.0 } else { 1.0 - acc[0] / acc[1] }
            }
            RayMode::MeanDistance { max } => 1.0 - acc[0] / rays as f32 / max,
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
        let flat = bvh.flatten();
        if tree_depth(&flat.nodes) + 2 >= STACK_DEPTH {
            return Err(RayBatchError::Unsupported("BVH is too deep for the GPU traversal stack".into()));
        }
        let limits = self.device.limits();
        let node_bytes = (flat.nodes.len() * 32) as u64;
        let tri_bytes = (flat.tris.len() * 48) as u64;
        let max_binding = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        if node_bytes > max_binding || tri_bytes > max_binding {
            return Err(RayBatchError::Unsupported(format!(
                "mesh needs {} MB of GPU buffers, this GPU allows {} MB per buffer",
                node_bytes.max(tri_bytes) >> 20,
                max_binding >> 20
            )));
        }
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let node_buf = self.buffer("bvh nodes", node_bytes, storage);
        let tri_buf = self.buffer("bvh triangles", tri_bytes, storage);
        self.queue.write_buffer(&node_buf, 0, bytemuck::cast_slice(&flat.nodes));
        self.queue.write_buffer(&tri_buf, 0, bytemuck::cast_slice(&flat.tris));
        drop(flat);

        let chunk = CHUNK.min(count);
        let texel_buf = self.buffer("ray texels", (chunk * 48) as u64, storage);
        let accum_buf = self.buffer("ray accum", (chunk * 8) as u64, storage | wgpu::BufferUsages::COPY_SRC);
        let read_buf = self.buffer("ray readback", (chunk * 8) as u64, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let param_buf = self.buffer("ray params", std::mem::size_of::<Params>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray bake"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: node_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tri_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: texel_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: accum_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: param_buf.as_entire_binding() },
            ],
        });
        let max_groups = limits.max_compute_workgroups_per_dimension;

        let mut out = Vec::with_capacity(count);
        // Rays per dispatch, adapted from measured speed (starts tiny so a slow adapter is safe).
        let mut rays_per_pass = 1u32;
        let mut records: Vec<[f32; 12]> = Vec::with_capacity(chunk);
        for ci in 0..count.div_ceil(chunk) {
            let first = ci * chunk;
            let part_len = chunk.min(count - first);
            if control.is_cancelled() {
                return Err(RayBatchError::Cancelled);
            }
            records.clear();
            records.extend((first..first + part_len).map(texel).map(|t| {
                [t.origin[0], t.origin[1], t.origin[2], t.rotation, t.axis[0], t.axis[1], t.axis[2], 0.0, t.offset[0], t.offset[1], t.offset[2], 0.0]
            }));
            self.queue.write_buffer(&texel_buf, 0, bytemuck::cast_slice(&records));
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.clear_buffer(&accum_buf, 0, None);
            self.queue.submit([enc.finish()]);

            let n_texels = part_len as u32;
            let groups = n_texels.div_ceil(64);
            let (gx, gy) = if groups <= max_groups { (groups, 1) } else { (max_groups, groups.div_ceil(max_groups)) };
            let mut done = 0u32;
            while done < rays {
                if control.is_cancelled() {
                    return Err(RayBatchError::Cancelled);
                }
                let end = (done + rays_per_pass).min(rays);
                let params = Params { mode: mode_id, ray_start: done, ray_end: end, ray_total: rays, count: n_texels, distance, node_count: 0, pad: 0 };
                self.queue.write_buffer(&param_buf, 0, bytemuck::bytes_of(&params));
                let mut enc = self.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups(gx, gy, 1);
                }
                let started = Instant::now();
                self.queue.submit([enc.finish()]);
                self.wait()?;
                let took = started.elapsed();
                let n = end - done;
                done = end;
                // Next pass: scale toward the target time (at most 4x growth; shrink at once).
                let per_ray = took.as_secs_f64().max(1e-5) / n as f64;
                let fit = (TARGET_PASS.as_secs_f64() / per_ray) as u32;
                rays_per_pass = if took > MAX_PASS { fit.max(1) } else { fit.clamp(1, n.saturating_mul(4).max(1)) };
                let frac = (ci * chunk) as f32 + part_len as f32 * done as f32 / rays as f32;
                control.progress(frac / count as f32);
            }

            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&accum_buf, 0, &read_buf, 0, (part_len * 8) as u64);
            self.queue.submit([enc.finish()]);
            let slice = read_buf.slice(..(part_len * 8) as u64);
            slice.map_async(wgpu::MapMode::Read, |r| r.expect("map ray accumulators"));
            self.wait()?;
            {
                let data = slice.get_mapped_range().map_err(|e| RayBatchError::Unsupported(format!("GPU readback failed: {e}")))?;
                let acc: &[[f32; 2]] = bytemuck::cast_slice(&data);
                out.extend(acc.iter().map(|a| finish(*a)));
            }
            read_buf.unmap();
        }
        Ok(out)
    }
}
