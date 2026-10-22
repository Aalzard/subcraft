// Post-processing for the viewport: screen-space ambient occlusion, bloom, and the final composite.

struct Post {
    inv_proj: mat4x4<f32>,
    // x: AO radius (object units), y: AO strength, z: bloom strength, w: 1 = orthographic
    params: vec4<f32>,
    // x: width, y: height, z: 1 / width, w: 1 / height (of the target being drawn)
    size: vec4<f32>,
    // xy: blur direction in texels, z: bloom threshold, w: proj[1][1]
    misc: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: Post;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var aux: texture_2d<f32>;
@group(0) @binding(4) var depth_tex: texture_depth_2d;
@group(0) @binding(5) var ao_tex: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn view_pos(px: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) * p.size.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let v = p.inv_proj * vec4<f32>(ndc, depth, 1.0);
    return v.xyz / v.w;
}

fn load_depth(px: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    return textureLoad(depth_tex, clamp(px, vec2<i32>(0), dims - vec2<i32>(1)), 0);
}

fn gradient_noise(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Alchemy-style SSAO from depth alone: normals come from the nearest depth neighbours.
@fragment
fn fs_ao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let d = load_depth(px);
    if (d >= 1.0) {
        return vec4<f32>(1.0);
    }
    let pos = view_pos(px, d);
    let pl = view_pos(px + vec2<i32>(-1, 0), load_depth(px + vec2<i32>(-1, 0)));
    let pr = view_pos(px + vec2<i32>(1, 0), load_depth(px + vec2<i32>(1, 0)));
    let pu = view_pos(px + vec2<i32>(0, -1), load_depth(px + vec2<i32>(0, -1)));
    let pd = view_pos(px + vec2<i32>(0, 1), load_depth(px + vec2<i32>(0, 1)));
    var dx = pr - pos;
    if (abs(pl.z - pos.z) < abs(dx.z)) { dx = pos - pl; }
    var dy = pd - pos;
    if (abs(pu.z - pos.z) < abs(dy.z)) { dy = pos - pu; }
    var n = normalize(cross(dx, dy));
    // Face the camera (view space looks down -Z).
    if (dot(n, -pos) < 0.0) { n = -n; }

    let radius = p.params.x;
    // Screen radius of the world radius: perspective shrinks with distance, orthographic does not.
    let focal = 0.5 * p.size.y * p.misc.w;
    var r_px = radius * focal;
    if (p.params.w < 0.5) { r_px = r_px / max(-pos.z, 1e-4); }
    r_px = clamp(r_px, 2.0, 96.0);
    let rot = gradient_noise(frag.xy) * 6.2831853;
    var occ = 0.0;
    let count = 16;
    for (var i = 0; i < count; i = i + 1) {
        let t = (f32(i) + 0.5) / f32(count);
        let a = rot + f32(i) * 2.3999632;
        let off = vec2<f32>(cos(a), sin(a)) * (r_px * (0.15 + 0.85 * t));
        let spx = px + vec2<i32>(round(off));
        let sd = load_depth(spx);
        if (sd >= 1.0) { continue; }
        let sp = view_pos(spx, sd);
        let v = sp - pos;
        let dist2 = dot(v, v);
        // Range check keeps far-away surfaces from darkening the point.
        let range = smoothstep(0.0, 1.0, radius / max(sqrt(dist2), 1e-4));
        occ = occ + max(0.0, dot(n, v) - 0.02 * radius) / (dist2 + 0.01 * radius * radius) * range;
    }
    let ao = clamp(1.0 - occ * radius * 2.0 / f32(count), 0.0, 1.0);
    return vec4<f32>(ao);
}

// Bright pass at half resolution: soft knee around the threshold, in linear light.
@fragment
fn fs_bright(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let c = textureSample(src, samp, uv).rgb;
    let lum = max(max(c.r, c.g), c.b);
    let knee = 0.15;
    let soft = clamp((lum - p.misc.z + knee) / (2.0 * knee), 0.0, 1.0);
    let w = max(soft * soft, select(0.0, 1.0, lum > p.misc.z + knee)) ;
    return vec4<f32>(c * w, 1.0);
}

// 9-tap Gaussian along p.misc.xy texels.
@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let step = p.misc.xy * p.size.zw;
    var w = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var acc = textureSample(src, samp, uv).rgb * w[0];
    for (var i = 1; i < 5; i = i + 1) {
        let o = step * f32(i);
        acc = acc + textureSample(src, samp, uv + o).rgb * w[i] + textureSample(src, samp, uv - o).rgb * w[i];
    }
    return vec4<f32>(acc, 1.0);
}

// Scene (already tone mapped) times the AO factor plus the glow.
@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    var color = textureSample(src, samp, uv).rgb;
    if (p.params.y > 0.0) {
        let ao = textureSample(ao_tex, samp, uv).r;
        color = color * mix(1.0, ao, clamp(p.params.y, 0.0, 1.0));
    }
    if (p.params.z > 0.0) {
        color = color + textureSample(aux, samp, uv).rgb * p.params.z;
    }
    return vec4<f32>(color, 1.0);
}

// Plain bilinear copy (used to bring a full-resolution image down to half resolution).
@fragment
fn fs_copy(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    return vec4<f32>(textureSample(src, samp, uv).rgb, 1.0);
}

// src scaled by p.params.z, to be added into the effects buffer.
@fragment
fn fs_scale(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    return vec4<f32>(textureSample(src, samp, uv).rgb * p.params.z, 1.0);
}

// One stage of a glare streak: [1 2 1] / 4 along p.misc.xy texels. Stages at strides 1, 2, 4, 8...
// multiply into a box and then a smooth tent, so a long streak needs only a few cheap passes.
@fragment
fn fs_tent(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let o = p.misc.xy * p.size.zw;
    let c = textureSample(src, samp, uv).rgb * 0.5;
    let a = textureSample(src, samp, uv + o).rgb * 0.25;
    let b = textureSample(src, samp, uv - o).rgb * 0.25;
    return vec4<f32>(c + a + b, 1.0);
}
