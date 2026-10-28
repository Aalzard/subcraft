// Curvature pass (ADR-0039): mirrors crates/bake/src/curvature.rs `curvature_on_surface`.
// One thread per texel; `ray_start..ray_end` is the texel sub-range of this dispatch and
// `distance` the query radius. Helpers and bindings 0..4 are in ray_common.wgsl.

@group(0) @binding(5) var<storage, read> tri_normals: array<vec4<f32>>; // 3 per triangle (leaf order)
@group(0) @binding(6) var<storage, read> tri_group: array<u32>;         // leaf order

// Ericson, Real-Time Collision Detection 5.1.5: closest point on (a, b, c) to p; w = barycentrics.
fn closest_point(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> array<vec3<f32>, 2> {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if (d1 <= 0.0 && d2 <= 0.0) { return array<vec3<f32>, 2>(a, vec3<f32>(1.0, 0.0, 0.0)); }
    let bp = p - b;
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if (d3 >= 0.0 && d4 <= d3) { return array<vec3<f32>, 2>(b, vec3<f32>(0.0, 1.0, 0.0)); }
    let vc = d1 * d4 - d3 * d2;
    if (vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0) {
        let v = d1 / (d1 - d3);
        return array<vec3<f32>, 2>(a + ab * v, vec3<f32>(1.0 - v, v, 0.0));
    }
    let cp = p - c;
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if (d6 >= 0.0 && d5 <= d6) { return array<vec3<f32>, 2>(c, vec3<f32>(0.0, 0.0, 1.0)); }
    let vb = d5 * d2 - d1 * d6;
    if (vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0) {
        let w = d2 / (d2 - d6);
        return array<vec3<f32>, 2>(a + ac * w, vec3<f32>(1.0 - w, 0.0, w));
    }
    let va = d3 * d6 - d5 * d4;
    if (va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0) {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return array<vec3<f32>, 2>(b + (c - b) * w, vec3<f32>(0.0, 1.0 - w, w));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    return array<vec3<f32>, 2>(a + ab * v + ac * w, vec3<f32>(1.0 - v - w, v, w));
}

fn normalize_or_zero(v: vec3<f32>) -> vec3<f32> {
    let l2 = dot(v, v);
    return select(vec3<f32>(0.0), v * inverseSqrt(l2), l2 > 1e-30);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let k = gid.x + gid.y * nwg.x * 64u;
    let i = params.ray_start + k;
    if (i >= params.ray_end || i >= params.count) { return; }
    let t0 = texels[i * 3u];
    let p = t0.xyz;
    let has_source = t0.w >= 0.0;  // group id as an exact float, -1 = no source triangle
    let src = u32(max(t0.w, 0.0));
    let n = texels[i * 3u + 1u].xyz;
    let radius = params.distance;
    let r2 = radius * radius;

    var all_sum = 0.0; var all_w = 0.0; var all_near = 0.0;
    var own_sum = 0.0; var own_w = 0.0; var own_near = 0.0;

    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    while (sp > 0u) {
        sp = sp - 1u;
        let ni = stack[sp];
        let na = nodes[ni * 2u];
        let nb = nodes[ni * 2u + 1u];
        let bmin = bitcast<vec3<f32>>(na.xyz);
        let bmax = bitcast<vec3<f32>>(nb.xyz);
        let dc = clamp(p, bmin, bmax) - p;
        if (dot(dc, dc) > r2) { continue; }
        if (nb.w == 0u) {
            stack[sp] = na.w;
            stack[sp + 1u] = na.w + 1u;
            sp = sp + 2u;
            continue;
        }
        for (var j = 0u; j < nb.w; j = j + 1u) {
            let ti = na.w + j;
            let a = tris[ti * 3u].xyz;
            let b = a + tris[ti * 3u + 1u].xyz;
            let c = a + tris[ti * 3u + 2u].xyz;
            let cpb = closest_point(p, a, b, c);
            let q = cpb[0];
            let bary = cpb[1];
            let d = q - p;
            let dist = length(d);
            if (dist >= radius) { continue; }
            let nq = normalize_or_zero(tri_normals[ti * 3u].xyz * bary.x + tri_normals[ti * 3u + 1u].xyz * bary.y + tri_normals[ti * 3u + 2u].xyz * bary.z);
            let dn = nq - n;
            if (dot(dn, dn) < 1e-6) { continue; }
            let foreign = has_source && tri_group[ti] != src;
            if (foreign && dot(d, n) < -radius * 1e-3) { continue; }
            let crease = cross(n, nq);
            let cl2 = dot(crease, crease);
            var across = d;
            if (cl2 > 1e-6) { across = d - crease * (dot(d, crease) / cl2); }
            if (dot(across, across) <= (radius * 1e-3) * (radius * 1e-3)) {
                let m = (a + b + c) / 3.0 - p;
                across = m;
                if (cl2 > 1e-6) { across = m - crease * (dot(m, crease) / cl2); }
            }
            if (dot(across, across) < 1e-24) { continue; }
            let w = 1.0 - max(dist, radius * 1e-3) / radius;
            let value = dot(dn, normalize(across));
            all_sum = all_sum + w * value; all_w = all_w + w; all_near = max(all_near, w);
            if (has_source && !foreign) {
                own_sum = own_sum + w * value; own_w = own_w + w; own_near = max(own_near, w);
            }
        }
    }
    var result = 0.5;
    if (own_w > 0.0) {
        result = 0.5 + 0.5 * clamp(own_sum / own_w, -1.0, 1.0) * own_near;
    } else if (all_w > 0.0) {
        result = 0.5 + 0.5 * clamp(all_sum / all_w, -1.0, 1.0) * all_near;
    }
    accum[i] = vec2<f32>(result, 0.0);
}
