// Colour profile (Painter's Display Settings > Color profile): the finished image, read as the
// linear values the white point produced, shown raw or through a 3D LUT. The LUT is OCIO's
// `ociolutimage` layout: red fastest, then green, then blue, reshaped row by row into the image
// (pixel (x, y) is entry y * width + x). See color_profile.rs for the CPU reference.

struct Params {
    // x: cube size N (0 = no LUT: raw), y: image width.
    size: vec4<i32>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var lut: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn lut_entry(i: vec3<i32>) -> vec3<f32> {
    let n = p.size.x;
    let flat = i.x + n * (i.y + n * i.z);
    return textureLoad(lut, vec2<i32>(flat % p.size.y, flat / p.size.y), 0).rgb;
}

// Trilinear lookup of `c` (0..1 per component) in the cube.
fn apply_lut(c: vec3<f32>) -> vec3<f32> {
    let n = p.size.x;
    let pos = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n - 1);
    let i0 = vec3<i32>(floor(pos));
    let i1 = min(i0 + vec3<i32>(1), vec3<i32>(n - 1));
    let f = pos - floor(pos);
    let c00 = mix(lut_entry(vec3<i32>(i0.x, i0.y, i0.z)), lut_entry(vec3<i32>(i1.x, i0.y, i0.z)), f.x);
    let c10 = mix(lut_entry(vec3<i32>(i0.x, i1.y, i0.z)), lut_entry(vec3<i32>(i1.x, i1.y, i0.z)), f.x);
    let c01 = mix(lut_entry(vec3<i32>(i0.x, i0.y, i1.z)), lut_entry(vec3<i32>(i1.x, i0.y, i1.z)), f.x);
    let c11 = mix(lut_entry(vec3<i32>(i0.x, i1.y, i1.z)), lut_entry(vec3<i32>(i1.x, i1.y, i1.z)), f.x);
    return mix(mix(c00, c10, f.y), mix(c01, c11, f.y), f.z);
}

fn linear_from_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_profile(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // The sRGB view decodes: this is the linear value the shading wrote (clamped HDR / white point).
    let src = textureLoad(image, vec2<i32>(frag.xy), 0);
    var shown = clamp(src.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (p.size.x >= 2) {
        shown = apply_lut(shown);
    }
    // Written so the stored bytes are `shown` itself: raw, not sRGB-encoded.
    return vec4<f32>(linear_from_srgb(clamp(shown, vec3<f32>(0.0), vec3<f32>(1.0))), src.a);
}
