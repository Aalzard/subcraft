// Order-independent transparency, final step: the weighted average of every blended surface
// (accumulated in any order by `fs_oit` of viewport.wgsl) laid over the opaque image.

@group(0) @binding(0) var accum: texture_2d<f32>;
@group(0) @binding(1) var reveal: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let a = textureLoad(accum, px, 0);
    // How much of the background still shows through all the layers (1 = nothing blended here).
    let r = textureLoad(reveal, px, 0).r;
    if (r >= 0.9999) {
        discard;
    }
    let color = a.rgb / max(a.a, 1e-5);
    return vec4<f32>(color, 1.0 - r);
}
