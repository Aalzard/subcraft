// Displacement preview: moves each vertex of the subdivided mesh along its normal by
// (Height + Displacement) x scale, sampled at the vertex's UV. Vertices are 12 floats:
// position (3), normal (3), tangent (4), uv (2), as `gpu_mesh::Vertex`.

struct Params {
    // x: world units per unit of height, y: vertex count of this dispatch, z: displacement layer
    // in `ext` (-1 = none)
    p: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> ids: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var height: texture_2d<f32>;
@group(0) @binding(5) var ext: texture_2d_array<f32>;
@group(0) @binding(6) var s: sampler;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u32(params.p.y)) {
        return;
    }
    let o = ids[id.x] * 12u;
    let pos = vec3<f32>(src[o], src[o + 1u], src[o + 2u]);
    let n = vec3<f32>(src[o + 3u], src[o + 4u], src[o + 5u]);
    let uv = vec2<f32>(src[o + 10u], src[o + 11u]);
    var h = textureSampleLevel(height, s, uv, 0.0).r;
    let layer = i32(round(params.p.z));
    if (layer >= 0) {
        h += textureSampleLevel(ext, s, uv, layer, 0.0).r;
    }
    let moved = pos + n * h * params.p.x;
    dst[o] = moved.x;
    dst[o + 1u] = moved.y;
    dst[o + 2u] = moved.z;
}
