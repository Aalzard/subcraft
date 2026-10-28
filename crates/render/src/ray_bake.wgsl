// Ambient occlusion / thickness rays for the baker (ADR-0039). One thread per texel casts the
// rays `ray_start..ray_end` of that texel's fan and adds to its accumulator. Mirrors
// crates/bake/src/surface.rs (`cosine_directions`) and crates/mesh/src/bvh.rs.

struct Params {
    mode: u32,        // 0 = occlusion, 1 = mean distance
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
@group(0) @binding(3) var<storage, read_write> accum: array<vec2<f32>>;
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

// Closest hit distance in (0, t_max), or t_max when nothing is hit. `any_hit` stops at the
// first hit (returns its distance).
fn trace(o: vec3<f32>, d: vec3<f32>, t_max: f32, any_hit: bool) -> f32 {
    let inv = safe_inv(d);
    var best = t_max;
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    while (sp > 0u) {
        sp = sp - 1u;
        let ni = stack[sp];
        let a = nodes[ni * 2u];
        let b = nodes[ni * 2u + 1u];
        let count = b.w;
        if (slab(bitcast<vec3<f32>>(a.xyz), bitcast<vec3<f32>>(b.xyz), o, inv, best) < 0.0) { continue; }
        if (count > 0u) {
            for (var k = 0u; k < count; k = k + 1u) {
                let t = tri_hit(a.w + k, o, d, best);
                if (t >= 0.0) {
                    best = t;
                    if (any_hit) { return best; }
                }
            }
        } else {
            let l = a.w;
            let la = nodes[l * 2u];
            let lb = nodes[l * 2u + 1u];
            let ra = nodes[l * 2u + 2u];
            let rb = nodes[l * 2u + 3u];
            let dl = slab(bitcast<vec3<f32>>(la.xyz), bitcast<vec3<f32>>(lb.xyz), o, inv, best);
            let dr = slab(bitcast<vec3<f32>>(ra.xyz), bitcast<vec3<f32>>(rb.xyz), o, inv, best);
            if (dl >= 0.0 && dr >= 0.0) {
                if (dl <= dr) {
                    stack[sp] = l + 1u;
                    stack[sp + 1u] = l;
                } else {
                    stack[sp] = l;
                    stack[sp + 1u] = l + 1u;
                }
                sp = sp + 2u;
            } else if (dl >= 0.0) {
                stack[sp] = l;
                sp = sp + 1u;
            } else if (dr >= 0.0) {
                stack[sp] = l + 1u;
                sp = sp + 1u;
            }
        }
    }
    return best;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let i = gid.x + gid.y * nwg.x * 64u;
    if (i >= params.count) { return; }
    let t0 = texels[i * 3u];
    let origin = t0.xyz;
    let rotation = t0.w;
    let n = texels[i * 3u + 1u].xyz;
    let offset = texels[i * 3u + 2u].xyz;

    // glam Vec3::any_orthonormal_pair
    let sgn = select(1.0, -1.0, n.z < 0.0);
    let a = -1.0 / (sgn + n.z);
    let b = n.x * n.y * a;
    let tangent = vec3<f32>(1.0 + sgn * n.x * n.x * a, sgn * b, -sgn * n.x);
    let bitan = vec3<f32>(b, sgn + n.y * n.y * a, -n.y);

    var acc = accum[i];
    let total = f32(params.ray_total);
    for (var r = params.ray_start; r < params.ray_end; r = r + 1u) {
        let u = (f32(r) + 0.5) / total;
        let v = fract(f32(reverseBits(r)) / 4294967296.0 + rotation);
        let rad = sqrt(u);
        let phi = TAU * v;
        var dir = tangent * (rad * cos(phi)) + bitan * (rad * sin(phi)) + n * sqrt(max(1.0 - u, 0.0));
        let len = length(dir);
        dir = select(n, dir / len, len > 1e-20);
        if (params.mode == 0u) {
            if (dot(dir, offset) <= 0.0) { continue; }
            acc.y = acc.y + 1.0;
            if (trace(origin, dir, params.distance, true) < params.distance) { acc.x = acc.x + 1.0; }
        } else {
            acc.x = acc.x + trace(origin, dir, params.distance, false);
        }
    }
    accum[i] = acc;
}
