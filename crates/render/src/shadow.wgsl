// Depth of the model seen from the key light: the shadow map the viewport shader compares against.

struct Light {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> light: Light;

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    return light.view_proj * vec4<f32>(pos, 1.0);
}
