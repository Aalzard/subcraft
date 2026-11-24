// High-poly projection pass for the baker: mirrors the cage ray in crates/bake/src/lib.rs
// (`bake_core`, `project`) and `Bvh::intersect_filtered`. One thread per texel finds the closest
// hit in (0, reach). `ray_start..ray_end` is the texel sub-range of this dispatch; `mode` 1 keeps
// only triangles the ray hits from the front (Ignore backface). Helpers and bindings are in
// ray_common.wgsl. Output per texel: (t, u, v, leaf index as an exact float), t = -1 on a miss.

// Part group of each triangle (leaf order); read only for rays with a group (w of the direction row).
@group(0) @binding(5) var<storage, read> tri_group: array<u32>;

// Moller-Trumbore keeping u, v; `front_only` rejects back faces (det <= 0), exactly where the CPU
// test `dot(geometric_normal, dir) < 0` fails. vec4(t, u, v, 1) or t = -1.
fn tri_hit_uv(ti: u32, o: vec3<f32>, d: vec3<f32>, t_max: f32, front_only: bool) -> vec4<f32> {
    let miss = vec4<f32>(-1.0, 0.0, 0.0, 0.0);
    let v0 = tris[ti * 3u].xyz;
    let e1 = tris[ti * 3u + 1u].xyz;
    let e2 = tris[ti * 3u + 2u].xyz;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if (abs(det) < 1e-14) { return miss; }
    if (front_only && det <= 0.0) { return miss; }
    let inv_det = 1.0 / det;
    let s = o - v0;
    let u = dot(s, p) * inv_det;
    if (u < 0.0 || u > 1.0) { return miss; }
    let q = cross(s, e1);
    let v = dot(d, q) * inv_det;
    if (v < 0.0 || u + v > 1.0) { return miss; }
    let t = dot(e2, q) * inv_det;
    if (t <= 0.0 || t >= t_max) { return miss; }
    return vec4<f32>(t, u, v, 1.0);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let k = gid.x + gid.y * nwg.x * 64u;
    let i = params.ray_start + k;
    if (i >= params.ray_end || i >= params.count) { return; }
    let t0 = texels[i * 3u];
    let o = t0.xyz;
    let reach = t0.w;
    let t1 = texels[i * 3u + 1u];
    let d = t1.xyz;
    let group = i32(t1.w);  // part group as an exact float, -1 = every triangle
    let front_only = params.mode == 1u;

    let inv = safe_inv(d);
    var best = vec4<f32>(-1.0, 0.0, 0.0, 0.0);
    var t_best = reach;
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    while (sp > 0u) {
        sp = sp - 1u;
        let ni = stack[sp];
        let a = nodes[ni * 2u];
        let b = nodes[ni * 2u + 1u];
        let count = b.w;
        if (slab(bitcast<vec3<f32>>(a.xyz), bitcast<vec3<f32>>(b.xyz), o, inv, t_best) < 0.0) { continue; }
        if (count > 0u) {
            for (var j = 0u; j < count; j = j + 1u) {
                if (group >= 0 && tri_group[a.w + j] != u32(group)) { continue; }
                let h = tri_hit_uv(a.w + j, o, d, t_best, front_only);
                if (h.x >= 0.0) {
                    t_best = h.x;
                    best = vec4<f32>(h.x, h.y, h.z, f32(a.w + j));
                }
            }
        } else {
            let l = a.w;
            let la = nodes[l * 2u];
            let lb = nodes[l * 2u + 1u];
            let ra = nodes[l * 2u + 2u];
            let rb = nodes[l * 2u + 3u];
            let dl = slab(bitcast<vec3<f32>>(la.xyz), bitcast<vec3<f32>>(lb.xyz), o, inv, t_best);
            let dr = slab(bitcast<vec3<f32>>(ra.xyz), bitcast<vec3<f32>>(rb.xyz), o, inv, t_best);
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
    accum[i] = best;
}
