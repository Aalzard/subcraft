@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    // Reading an sRGB texture decodes it. Restore encoded values for egui's UNORM input.
    let c = textureLoad(source, vec2<i32>(p.xy), 0);
    let hi = 1.055 * pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(hi, c.rgb * 12.92, c.rgb <= vec3<f32>(0.0031308)), c.a);
}
