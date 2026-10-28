// Entry point of the AO / thickness pass; helpers are in ray_common.wgsl.

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
