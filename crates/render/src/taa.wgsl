// Temporal anti-aliasing: the running average of jittered frames (see taa.rs).

struct Params {
    // x: weight of the current frame (1 / frame number); 1 restarts the average.
    weight: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var current: texture_2d<f32>;
@group(0) @binding(2) var history: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_accum(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let now = textureLoad(current, px, 0);
    // The first frame replaces whatever the history held (never mixed with it).
    if p.weight.x >= 1.0 {
        return now;
    }
    return mix(textureLoad(history, px, 0), now, p.weight.x);
}

@fragment
fn fs_resolve(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(current, vec2<i32>(frag.xy), 0);
}
