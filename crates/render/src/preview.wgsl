// Channel preview: converts a composite channel texture into an sRGB 8-bit image for display.
// Color channels are written linear (the sRGB target encodes them); data channels (normals)
// are pre-linearized so the encoded result equals their stored value.

// flags: x color channel (1) or data (0); y single-component source shown as gray (1);
// z component shown alone as gray (0 all, 1 red, 2 green, 3 blue, 4 alpha); w 1 / HDR scale.
// sign: x 1 = grayscale values in two colors by sign (positive, negative: display colors).
struct Params { flags: vec4<f32>, sign: vec4<f32>, positive: vec4<f32>, negative: vec4<f32> };
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
    let s = textureSample(src, samp, in.uv);
    // Display Settings > Channel display, as in the 3D view: HDR values divided by the scale,
    // then grayscale values in two colors by sign.
    var c = s.rgb * p.flags.w;
    let comp = i32(round(p.flags.z));
    if (p.flags.y > 0.5) {
        c = vec3<f32>(c.r);
        if (comp == 0 && p.sign.x > 0.5) {
            c = select(p.negative.rgb * (-c.r), p.positive.rgb * c.r, c.r >= 0.0);
        }
    }
    c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    if (comp == 1) {
        c = vec3<f32>(c.r);
    } else if (comp == 2) {
        c = vec3<f32>(c.g);
    } else if (comp == 3) {
        c = vec3<f32>(c.b);
    } else if (comp == 4) {
        c = vec3<f32>(clamp(s.a * p.flags.w, 0.0, 1.0));
    }
    // Alpha is data even in a color channel: shown as stored, not through the sRGB curve.
    if (p.flags.x > 0.5 && comp != 4) {
        return vec4<f32>(c, 1.0);
    }
    return vec4<f32>(linear_from_srgb(c), 1.0);
}
