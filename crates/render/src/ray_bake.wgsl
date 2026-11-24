// Entry point of the AO / thickness / bent normal pass; helpers are in ray_common.wgsl.

// Part group of each triangle (leaf order); read only for texels with a group (w of the axis row).
@group(0) @binding(5) var<storage, read> tri_group: array<u32>;

// Closest hit distance in (0, t_max), or t_max when nothing is hit. `any_hit` stops at the
// first hit (returns its distance). With `group` >= 0 only triangles of that part group count
// (bake by mesh name; mirrors `PartMatch::accepts`).
fn trace(o: vec3<f32>, d: vec3<f32>, t_max: f32, any_hit: bool, group: i32) -> f32 {
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
                if (group >= 0 && tri_group[a.w + k] != u32(group)) { continue; }
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
    let t1 = texels[i * 3u + 1u];
    let n = t1.xyz;
    let group = i32(t1.w);  // part group as an exact float, -1 = every triangle
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
            if (trace(origin, dir, params.distance, true, group) < params.distance) { acc.x = acc.x + 1.0; }
        } else if (params.mode == 2u) {
            if (dot(dir, offset) <= 0.0) { continue; }
            if (trace(origin, dir, params.distance, true, group) >= params.distance) { acc = acc + vec4<f32>(dir, 0.0); }
        } else {
            acc.x = acc.x + trace(origin, dir, params.distance, false, group);
        }
    }
    accum[i] = acc;
}
