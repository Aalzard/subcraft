// Edge-aware à-trous denoiser for the path-traced preview (SVGF-lite, without temporal reuse:
// accumulation already averages a still camera). Illumination is demodulated by the albedo
// guide, filtered with a 5x5 B3-spline kernel at growing strides, stopped at normal, depth and
// luminance edges (luminance sigma from the per-pixel variance, which falls as samples
// accumulate, so the filter fades out on a converged image), then remodulated.

struct Params {
    size: vec4<u32>,   // width, height, stride, samples
};

@group(0) @binding(0) var<storage, read> accum: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> guide_albedo: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> guide_normal: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> src: array<vec4<f32>>;      // illumination rgb + variance
@group(0) @binding(4) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(5) var<uniform> p: Params;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn pix(x: i32, y: i32) -> u32 {
    let cx = clamp(x, 0, i32(p.size.x) - 1);
    let cy = clamp(y, 0, i32(p.size.y) - 1);
    return u32(cy) * p.size.x + u32(cx);
}

fn illumination(i: u32) -> vec3<f32> {
    let n = max(f32(p.size.w), 1.0);
    return accum[i].rgb / n / max(guide_albedo[i].rgb, vec3<f32>(0.02));
}

// Demodulate and estimate the variance of the per-pixel mean (temporal once enough samples
// exist, spatial over 3x3 before that).
@compute @workgroup_size(8, 8)
fn prepare(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) { return; }
    let i = gid.y * p.size.x + gid.x;
    let n = max(f32(p.size.w), 1.0);
    let c = illumination(i);
    var variance: f32;
    if (p.size.w >= 4u) {
        let a = max(luminance(guide_albedo[i].rgb), 0.02);
        let mean = luminance(accum[i].rgb / n);
        variance = max(accum[i].w / n - mean * mean, 0.0) / n / (a * a);
    } else {
        var s1 = 0.0;
        var s2 = 0.0;
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let l = luminance(illumination(pix(i32(gid.x) + dx, i32(gid.y) + dy)));
                s1 += l;
                s2 += l * l;
            }
        }
        s1 /= 9.0;
        variance = max(s2 / 9.0 - s1 * s1, 0.0) * 4.0;
    }
    dst[i] = vec4<f32>(c, variance);
}

@compute @workgroup_size(8, 8)
fn atrous(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) { return; }
    let i = gid.y * p.size.x + gid.x;
    let center = src[i];
    let np = guide_normal[i];
    let zp = guide_albedo[i].w;
    if (np.w == 0.0) {
        dst[i] = center;
        return;
    }
    let step = i32(p.size.z);
    // Blurred variance at p steadies the luminance edge stop.
    var var_blur = 0.0;
    var g3 = array<f32, 3>(0.25, 0.5, 0.25);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            var_blur += g3[dx + 1] * g3[dy + 1] * src[pix(i32(gid.x) + dx, i32(gid.y) + dy)].w;
        }
    }
    let lp = luminance(center.rgb);
    let sigma_l = 4.0 * sqrt(max(var_blur, 0.0)) + 1e-4;
    var h = array<f32, 5>(0.0625, 0.25, 0.375, 0.25, 0.0625);
    var sum = vec3<f32>(0.0);
    var sum_var = 0.0;
    var wsum = 0.0;
    for (var dy = -2; dy <= 2; dy = dy + 1) {
        for (var dx = -2; dx <= 2; dx = dx + 1) {
            let q = pix(i32(gid.x) + dx * step, i32(gid.y) + dy * step);
            let nq = guide_normal[q];
            if (nq.w == 0.0) { continue; }
            let sq = src[q];
            let zq = guide_albedo[q].w;
            let wn = pow(max(dot(np.xyz, nq.xyz), 0.0), 64.0);
            let wz = exp(-abs(zp - zq) / (0.01 * zp * f32(step) + 1e-6));
            let wl = exp(-abs(lp - luminance(sq.rgb)) / sigma_l);
            let w = h[dx + 2] * h[dy + 2] * wn * wz * wl;
            sum += sq.rgb * w;
            sum_var += sq.w * w * w;
            wsum += w;
        }
    }
    if (wsum <= 1e-12) {
        dst[i] = center;
        return;
    }
    dst[i] = vec4<f32>(sum / wsum, sum_var / (wsum * wsum));
}

// Remodulate by the albedo; pixels without a surface keep the plain mean.
@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) { return; }
    let i = gid.y * p.size.x + gid.x;
    let n = max(f32(p.size.w), 1.0);
    if (guide_normal[i].w == 0.0) {
        dst[i] = vec4<f32>(accum[i].rgb / n, 1.0);
        return;
    }
    dst[i] = vec4<f32>(src[i].rgb * max(guide_albedo[i].rgb, vec3<f32>(0.02)), 1.0);
}
