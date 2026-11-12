struct Uniforms {
    view_proj: mat4x4<f32>,
    origin_radius: vec4<f32>,
    eye: vec4<f32>,
    axes: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Uniforms;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
};
@vertex fn vs_main(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>, @location(2) kind: u32) -> Out {
    var o: Out;
    o.clip = g.view_proj * vec4<f32>(pos, 1.0);
    // LineList cannot use pipeline depth bias. Nudge guide depth without moving its world
    // position or screen-space plane, so coplanar surface intersections remain visible.
    if kind == 4u { o.clip.z -= 0.000002 * o.clip.w; }
    o.world = pos;
    o.color = color;
    o.kind = kind;
    return o;
}
@fragment fn fs_main(v: Out) -> @location(0) vec4<f32> {
    var enabled = 1.0;
    if v.kind == 4u { return v.color; }
    if v.kind > 0u { enabled = g.axes[v.kind - 1u]; }
    let distance_fade = 1.0 - smoothstep(g.origin_radius.w * 0.4, g.origin_radius.w, distance(v.world, g.origin_radius.xyz));
    // Lines in the grid plane fade when the plane is seen edge-on; g.axes.w is the plane's
    // normal axis (0 X, 1 Y, 2 Z) and the line along it never fades.
    var angle_fade = 1.0;
    let up = u32(g.axes.w + 0.5);
    if v.kind != up + 1u {
        let view = normalize(g.eye.xyz - v.world);
        var facing = view.y;
        if up == 0u { facing = view.x; }
        if up == 2u { facing = view.z; }
        angle_fade = smoothstep(0.02, 0.20, abs(facing));
    }
    let alpha = v.color.a * enabled * distance_fade * angle_fade;
    if alpha < 0.003 { discard; }
    return vec4<f32>(v.color.rgb, alpha);
}
