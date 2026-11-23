// Packs the extra material channels (coat, sheen, anisotropy, subsurface, transmission,
// displacement) into the layers of one RGBA16F array texture, so the viewport shader reads
// them through a single binding. One draw writes one group (layer) at one mip level; every
// source is read at the mip level whose texel size matches the destination's.

struct Pack {
    // x: group (see `extended.rs`), y: destination width at this level
    p: vec4<f32>,
};

@group(0) @binding(0) var s: sampler;
@group(0) @binding(1) var coat_color: texture_2d<f32>;
@group(0) @binding(2) var coat_opacity: texture_2d<f32>;
@group(0) @binding(3) var coat_rough: texture_2d<f32>;
@group(0) @binding(4) var coat_spec: texture_2d<f32>;
@group(0) @binding(5) var coat_normal: texture_2d<f32>;
@group(0) @binding(6) var sheen_color: texture_2d<f32>;
@group(0) @binding(7) var sheen_opacity: texture_2d<f32>;
@group(0) @binding(8) var sheen_rough: texture_2d<f32>;
@group(0) @binding(9) var aniso_level: texture_2d<f32>;
@group(0) @binding(10) var aniso_angle: texture_2d<f32>;
@group(0) @binding(11) var translucency: texture_2d<f32>;
@group(0) @binding(12) var scatter_color: texture_2d<f32>;
@group(0) @binding(13) var scattering: texture_2d<f32>;
@group(0) @binding(14) var transmissive: texture_2d<f32>;
@group(0) @binding(15) var ior: texture_2d<f32>;
@group(0) @binding(16) var displacement: texture_2d<f32>;
@group(0) @binding(17) var<uniform> pack: Pack;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

// A source at the level matching the destination texel size (sources may be smaller or larger
// than the packed texture; a 1x1 default reads its only texel).
fn at(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let lod = max(log2(f32(textureDimensions(t).x) / pack.p.y), 0.0);
    return textureSampleLevel(t, s, uv, lod);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / pack.p.y;
    switch i32(round(pack.p.x)) {
        case 0: { return vec4<f32>(at(coat_color, uv).rgb, at(coat_opacity, uv).r); }
        case 1: {
            let n = at(coat_normal, uv).rg * 2.0 - 1.0;
            return vec4<f32>(at(coat_rough, uv).r, at(coat_spec, uv).r, n);
        }
        case 2: { return vec4<f32>(at(sheen_color, uv).rgb, at(sheen_opacity, uv).r); }
        case 3: {
            // Anisotropy as a vector at twice the angle: a direction and its opposite are the same
            // stretch, and the vector filters (mipmaps, bilinear) without wrapping artefacts.
            let a = at(aniso_angle, uv).r * 6.28318531 * 2.0;
            let level = at(aniso_level, uv).r;
            return vec4<f32>(at(sheen_rough, uv).r, level * cos(a), level * sin(a), at(translucency, uv).r);
        }
        case 4: { return vec4<f32>(at(scatter_color, uv).rgb, at(scattering, uv).r); }
        case 5: { return vec4<f32>(at(transmissive, uv).rgb, at(ior, uv).r); }
        default: { return vec4<f32>(at(displacement, uv).r, 0.0, 0.0, 0.0); }
    }
}
