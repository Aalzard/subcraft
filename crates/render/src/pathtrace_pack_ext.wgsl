// Packs one material slot's extra channels for the path tracer: from the viewport's packed
// extended-channel array (see extended.rs / extended.wgsl), the Opacity and the Specular level
// channels, into three rgba32uint array layers holding two half floats per component:
//   ext0: (coat r, g) (coat b, coat opacity) (coat roughness, sheen roughness) (anisotropy x, y)
//   ext1: (sheen r, g) (sheen b, sheen opacity) (translucency, opacity) (IOR channel, coat specular level)
//   ext2: (scatter r, g) (scatter b, scattering) (transmissive r, g) (transmissive b, specular level)
// A group the set lacks reads the viewport's default (everything off).

struct Pack {
    layers0: vec4<i32>,   // layer of groups 0..3 (-1 = none)
    layers1: vec4<i32>,   // groups 4..6
};

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var t_ext: texture_2d_array<f32>;
@group(0) @binding(2) var t_opacity: texture_2d<f32>;
@group(0) @binding(3) var t_specular: texture_2d<f32>;
@group(0) @binding(4) var<uniform> pack: Pack;
@group(0) @binding(5) var out0: texture_storage_2d<rgba32uint, write>;
@group(0) @binding(6) var out1: texture_storage_2d<rgba32uint, write>;
@group(0) @binding(7) var out2: texture_storage_2d<rgba32uint, write>;

fn group_layer(g: i32) -> i32 {
    if (g < 4) { return pack.layers0[g]; }
    return pack.layers1[g - 4];
}

fn ext(g: i32, def: vec4<f32>, uv: vec2<f32>, size: f32) -> vec4<f32> {
    let layer = group_layer(g);
    if (layer < 0) { return def; }
    let lod = max(log2(f32(textureDimensions(t_ext).x) / size), 0.0);
    return textureSampleLevel(t_ext, samp, uv, layer, lod);
}

fn fetch(t: texture_2d<f32>, uv: vec2<f32>, size: f32) -> vec4<f32> {
    let lod = max(log2(f32(textureDimensions(t).x) / size), 0.0);
    return textureSampleLevel(t, samp, uv, lod);
}

fn h2(a: f32, b: f32) -> u32 {
    return pack2x16float(vec2<f32>(a, b));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(out0);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let size = f32(dims.x);
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(dims);
    let c = vec2<i32>(gid.xy);
    // Defaults as in viewport.wgsl `shade`.
    let coat = ext(0, vec4<f32>(1.0, 1.0, 1.0, 0.0), uv, size);
    let coat2 = ext(1, vec4<f32>(0.1, 0.5, 0.0, 0.0), uv, size);
    let sheen = ext(2, vec4<f32>(1.0, 1.0, 1.0, 0.0), uv, size);
    let misc = ext(3, vec4<f32>(0.5, 0.0, 0.0, 0.0), uv, size);
    let sss = ext(4, vec4<f32>(1.0, 1.0, 1.0, 0.0), uv, size);
    let trans = ext(5, vec4<f32>(0.0, 0.0, 0.0, 0.5), uv, size);
    let opacity = fetch(t_opacity, uv, size).r;
    let spec = fetch(t_specular, uv, size).r;
    textureStore(out0, c, vec4<u32>(h2(coat.r, coat.g), h2(coat.b, coat.a), h2(coat2.x, misc.x), h2(misc.y, misc.z)));
    textureStore(out1, c, vec4<u32>(h2(sheen.r, sheen.g), h2(sheen.b, sheen.a), h2(misc.w, opacity), h2(trans.a, coat2.y)));
    textureStore(out2, c, vec4<u32>(h2(sss.r, sss.g), h2(sss.b, sss.a), h2(trans.r, trans.g), h2(trans.b, spec)));
}
