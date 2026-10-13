// Channel preview: converts a composite channel texture into an sRGB 8-bit image for display.
// Color channels are written linear (the sRGB target encodes them); data channels (normals)
// are pre-linearized so the encoded result equals their stored value.

// x: color channel (1) or data (0); y: single-component source shown as gray (1).
struct Params { flags: vec4<f32> };
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<uniform> p: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn linear_from_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var c = clamp(textureSample(src, samp, in.uv).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (p.flags.y > 0.5) {
        c = vec3<f32>(c.r);
    }
    if (p.flags.x > 0.5) {
        return vec4<f32>(c, 1.0);
    }
    return vec4<f32>(linear_from_srgb(c), 1.0);
}
