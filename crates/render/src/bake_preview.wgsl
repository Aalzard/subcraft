struct Uniforms { view_proj: mat4x4<f32>, model: mat4x4<f32>, cage: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Uniforms;
// Ray-miss heatmap of the low poly's texture set (1 = the projection ray missed the high poly).
@group(0) @binding(1) var miss_map: texture_2d<f32>;
@group(0) @binding(2) var miss_sampler: sampler;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) line: f32,
    @location(3) uv: vec2<f32>,
    @location(4) low: f32,
};
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>, @location(3) uv_low: vec3<f32>) -> Out {
    let distance = select(select(0.0, -g.cage.y, color.w < 0.0), g.cage.x, color.w > 0.0);
    var out: Out;
    out.position = g.view_proj * g.model * vec4<f32>(position + normal * distance, 1.0);
    out.normal = (g.model * vec4<f32>(normal, 0.0)).xyz; out.color = color.rgb; out.line = abs(color.w);
    out.uv = uv_low.xy; out.low = uv_low.z;
    return out;
}
@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let shade = 0.5 + 0.5 * abs(dot(normalize(in.normal), normalize(vec3<f32>(0.4, 0.8, 0.6))));
    let miss = textureSample(miss_map, miss_sampler, in.uv).r;
    if (g.cage.w > 0.5 && in.low > 0.5 && in.line < 0.5) {
        // Heatmap: neutral gray where rays reach the high poly, bright red where they miss.
        let hit = vec3<f32>(0.42, 0.45, 0.48) * shade;
        let red = vec3<f32>(1.0, 0.05, 0.03) * (0.7 + 0.3 * shade);
        return vec4<f32>(mix(hit, red, miss), 1.0);
    }
    return vec4<f32>(in.color * select(shade, 1.0, in.line > 0.5), select(g.cage.z, 0.28, in.line > 0.5));
}
