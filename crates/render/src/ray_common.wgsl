// Ambient occlusion / thickness rays for the baker (ADR-0039). One thread per texel casts the
// rays `ray_start..ray_end` of that texel's fan and adds to its accumulator. Mirrors
// crates/bake/src/surface.rs (`cosine_directions`) and crates/mesh/src/bvh.rs.

struct Params {
    mode: u32,        // 0 = occlusion, 1 = mean distance, 2 = bent normal (sum of open directions)
    ray_start: u32,
    ray_end: u32,
    ray_total: u32,
    count: u32,       // texels in this chunk
    distance: f32,    // occlusion distance or thickness max
    node_count: u32,
    pad: u32,
}

@group(0) @binding(0) var<storage, read> nodes: array<vec4<u32>>;   // 2 per node
@group(0) @binding(1) var<storage, read> tris: array<vec4<f32>>;    // 3 per triangle: v0, e1, e2
@group(0) @binding(2) var<storage, read> texels: array<vec4<f32>>;  // 3 per texel: origin+rotation, axis, offset
@group(0) @binding(3) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(4) var<uniform> params: Params;

const TAU: f32 = 6.283185307179586;
const STACK: u32 = 64u;

fn safe_inv(d: vec3<f32>) -> vec3<f32> {
    let s = select(vec3<f32>(1.0), vec3<f32>(-1.0), d < vec3<f32>(0.0));
    return 1.0 / select(d, s * 1e-20, abs(d) < vec3<f32>(1e-20));
}

// Slab test: entry distance, or a negative number on a miss.
fn slab(bmin: vec3<f32>, bmax: vec3<f32>, o: vec3<f32>, inv: vec3<f32>, t_max: f32) -> f32 {
    let t0 = (bmin - o) * inv;
    let t1 = (bmax - o) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    let enter = max(max(max(tn.x, tn.y), tn.z), 0.0);
    let exit = min(min(min(tf.x, tf.y), tf.z), t_max);
    return select(-1.0, enter, enter <= exit);
}

// Moller-Trumbore, both sides. Distance or -1.
fn tri_hit(ti: u32, o: vec3<f32>, d: vec3<f32>, t_max: f32) -> f32 {
    let v0 = tris[ti * 3u].xyz;
    let e1 = tris[ti * 3u + 1u].xyz;
    let e2 = tris[ti * 3u + 2u].xyz;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if (abs(det) < 1e-14) { return -1.0; }
    let inv_det = 1.0 / det;
    let s = o - v0;
    let u = dot(s, p) * inv_det;
    if (u < 0.0 || u > 1.0) { return -1.0; }
    let q = cross(s, e1);
    let v = dot(d, q) * inv_det;
    if (v < 0.0 || u + v > 1.0) { return -1.0; }
    let t = dot(e2, q) * inv_det;
    if (t <= 0.0 || t >= t_max) { return -1.0; }
    return t;
}
