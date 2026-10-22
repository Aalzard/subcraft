struct Uniforms { view_proj: mat4x4<f32>, cage: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Uniforms;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) line: f32,
};
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
    let distance = select(select(0.0, -g.cage.y, color.w < 0.0), g.cage.x, color.w > 0.0);
    var out: Out;
    out.position = g.view_proj * vec4<f32>(position + normal * distance, 1.0);
    out.normal = normal; out.color = color.rgb; out.line = abs(color.w);
    return out;
}
@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let shade = 0.5 + 0.5 * abs(dot(normalize(in.normal), normalize(vec3<f32>(0.4, 0.8, 0.6))));
    return vec4<f32>(in.color * select(shade, 1.0, in.line > 0.5), select(1.0, 0.28, in.line > 0.5));
}
