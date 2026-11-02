// Path-traced image to the viewport's sRGB output: mean of the accumulated samples (or the
// denoised image), exposure and the viewport's tone map.

struct Params {
    size: vec4<u32>,     // width, height, samples, 1 = read the denoised buffer
    look: vec4<f32>,     // exposure, tone map id
};

@group(0) @binding(0) var<storage, read> accum: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> denoised: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;

@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hable(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15; let b = 0.50; let c = 0.10; let d = 0.20; let e = 0.02; let f = 0.30;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn tonemap(color: vec3<f32>, mode: i32) -> vec3<f32> {
    switch mode {
        case 1: {
            let white = hable(vec3<f32>(11.2));
            return clamp(hable(color * 2.0) / white, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 2: { return clamp(color / (color + vec3<f32>(1.0)), vec3<f32>(0.0), vec3<f32>(1.0)); }
        case 3: { return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)); }
        default: { return aces(color); }
    }
}

@fragment fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = min(u32(pos.x), p.size.x - 1u);
    let y = min(u32(pos.y), p.size.y - 1u);
    let i = y * p.size.x + x;
    var c: vec3<f32>;
    if (p.size.w != 0u) {
        c = denoised[i].rgb;
    } else {
        c = accum[i].rgb / max(f32(p.size.z), 1.0);
    }
    return vec4<f32>(tonemap(max(c, vec3<f32>(0.0)) * p.look.x, i32(round(p.look.y))), 1.0);
}
