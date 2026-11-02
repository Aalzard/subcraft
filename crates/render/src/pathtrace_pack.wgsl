// Packs one material slot's channel textures into the path tracer's texture-array layers:
// base color + roughness, normal + metallic, emission + height (resampled to the array size,
// from the mip level closest to it).

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var t_base: texture_2d<f32>;
@group(0) @binding(2) var t_rough: texture_2d<f32>;
@group(0) @binding(3) var t_metal: texture_2d<f32>;
@group(0) @binding(4) var t_normal: texture_2d<f32>;
@group(0) @binding(5) var t_emissive: texture_2d<f32>;
@group(0) @binding(6) var t_height: texture_2d<f32>;
@group(0) @binding(7) var out_base_rough: texture_storage_2d<rgba16float, write>;
@group(0) @binding(8) var out_normal_metal: texture_storage_2d<rgba16float, write>;
@group(0) @binding(9) var out_emissive_height: texture_storage_2d<rgba16float, write>;

fn fetch(t: texture_2d<f32>, uv: vec2<f32>, size: f32) -> vec4<f32> {
    let lod = max(log2(f32(textureDimensions(t).x) / size), 0.0);
    return textureSampleLevel(t, samp, uv, lod);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(out_base_rough);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let size = f32(dims.x);
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(dims);
    let c = vec2<i32>(gid.xy);
    textureStore(out_base_rough, c, vec4<f32>(fetch(t_base, uv, size).rgb, fetch(t_rough, uv, size).r));
    textureStore(out_normal_metal, c, vec4<f32>(fetch(t_normal, uv, size).rgb, fetch(t_metal, uv, size).r));
    textureStore(out_emissive_height, c, vec4<f32>(fetch(t_emissive, uv, size).rgb, fetch(t_height, uv, size).r));
}
