// SubCraft viewport: metal/roughness PBR with a key light and a procedural sky environment.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    // x: exposure, y: environment intensity, z: light intensity, w: height bump scale
    params: vec4<f32>,
    light_color: vec4<f32>,
    ground: vec4<f32>,
    horizon: vec4<f32>,
    zenith: vec4<f32>,
    irr_up: vec4<f32>,
    irr_down: vec4<f32>,
    // x: ring panels, y: overhead light, z: key softbox, w: 1 = HDRI environment
    lights: vec4<f32>,
    // x: environment rotation (radians), y: HDRI intensity, z: last prefiltered mip level,
    // w: view mode (0 material, 1 base color, 2 normal, 3 roughness, 4 metallic, 5 height, 6 AO)
    env_misc: vec4<f32>,
    // x: tone map function (see tonemap.wgsl), y: mapping factor
    post: vec4<f32>,
    sh: array<vec4<f32>, 9>,
    inv_view_proj: mat4x4<f32>,
    // rgb: background color (linear), a: environment opacity behind the model
    background: vec4<f32>,
    // x: background blur 0..1
    env_view: vec4<f32>,
    // Single-channel display. x: 1 / HDR scale, y: 1 = two colors for the sign of grayscale
    // values, z: component (0 all, 1 R, 2 G, 3 B, 4 alpha), w: 1 = lit
    solo: vec4<f32>,
    // rgb: color of positive values, a: texture level bias of the material textures
    solo_positive: vec4<f32>,
    solo_negative: vec4<f32>,
    // Key light's view-projection for the shadow map.
    shadow_mat: mat4x4<f32>,
    // x: 1 = shadows on, y: opacity, z: one texel in map UV, w: one texel in world units
    shadow: vec4<f32>,
    // x: filter radius in texels, y: comparisons, z: TAA frame
    shadow_filter: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var env_tex: texture_2d<f32>;
@group(0) @binding(2) var env_samp: sampler;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_samp: sampler_comparison;
@group(1) @binding(0) var samp: sampler;
@group(1) @binding(1) var t_base: texture_2d<f32>;
@group(1) @binding(2) var t_rough: texture_2d<f32>;
@group(1) @binding(3) var t_metal: texture_2d<f32>;
@group(1) @binding(4) var t_normal: texture_2d<f32>;
@group(1) @binding(5) var t_emissive: texture_2d<f32>;
@group(1) @binding(6) var t_height: texture_2d<f32>;
@group(1) @binding(7) var t_ao: texture_2d<f32>;
@group(1) @binding(8) var albedo_samp: sampler;
// Opacity channel (1 where the texture set has none).
@group(1) @binding(9) var t_opacity: texture_2d<f32>;

// Per texture set (material) parameters. Add new fields after `alpha`.
struct MaterialParams {
    // x: alpha mode (0 opaque, 1 alpha test, 2 alpha blending), y: alpha test threshold,
    // z: 1 = alpha dithering (ordered dither instead of test / blend)
    alpha: vec4<f32>,
    // Shader Settings: x AO intensity, y emissive intensity, z horizon fading
    shader: vec4<f32>,
    // Layer in `t_ext` of the extended channel groups 0..3 and 4..6 (-1: the set has none of
    // the group's channels), see `extended.rs`.
    ext_a: vec4<f32>,
    ext_b: vec4<f32>,
    // x: anisotropy, y: subsurface scattering, z: translucency (1 = the set has the channel)
    ext_flags: vec4<f32>,
};
@group(1) @binding(10) var<uniform> mat: MaterialParams;
// Extended channels packed into array layers (`extended.wgsl`): 0 coat colour + opacity,
// 1 coat roughness, specular level, normal XY, 2 sheen colour + opacity, 3 sheen roughness,
// anisotropy vector (level at twice the angle), translucency, 4 scattering colour + amount,
// 5 transmissive + IOR, 6 displacement.
@group(1) @binding(13) var t_ext: texture_2d_array<f32>;
// Specular level: the reflectance of non-metals (0.5 = 4%).
@group(1) @binding(11) var t_specular: texture_2d<f32>;
// Part overlay (geometry mask editing, Polygon Fill): a UV-space mask over the texture set.
// R: geometry the mask leaves out, drawn grey with diagonal lines; G: the part or the fill
// element under the cursor, tinted. Empty (0, 0) where there is nothing to show.
@group(1) @binding(12) var t_overlay: texture_2d<f32>;

fn part_overlay(color: vec3<f32>, uv: vec2<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let o = textureSampleLevel(t_overlay, samp, uv, 0.0).rg;
    let stripe = step(0.72, fract((pixel.x + pixel.y) / 9.0));
    let grey = mix(vec3<f32>(0.40), vec3<f32>(0.14), stripe);
    let greyed = mix(color, grey, 0.85 * o.r);
    return mix(greyed, vec3<f32>(1.0, 0.62, 0.22), 0.4 * o.g);
}

// How much key light reaches `world` (a point on a surface with geometric normal `n`, seen at
// framebuffer position `frag`): 1 lit, down to 1 - opacity in full shadow. Percentage-closer
// filter over a disk of bilinear comparisons (Vogel spiral), rotated per pixel by interleaved
// gradient noise and per TAA frame, so the penumbra is smooth instead of a blocky box.
fn shadow_visibility(world: vec3<f32>, n: vec3<f32>, frag: vec2<f32>) -> f32 {
    if (g.shadow.x < 0.5) { return 1.0; }
    // Offset along the normal so a lit surface does not shadow itself; surfaces at a grazing
    // angle to the light and wider filters need more (slope-scaled).
    let nl = dot(n, normalize(g.light_dir.xyz));
    let sin_l = sqrt(clamp(1.0 - nl * nl, 0.0, 1.0));
    let radius = max(g.shadow_filter.x, 0.0);
    let c = g.shadow_mat * vec4<f32>(world + n * g.shadow.w * (1.5 + radius * 0.5 * sin_l), 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) { return 1.0; }
    let count = max(i32(g.shadow_filter.y), 1);
    let ign = fract(52.9829189 * fract(dot(frag, vec2<f32>(0.06711056, 0.00583715))));
    let rot = 6.2831853 * ign + g.shadow_filter.z * 2.39996323;
    var lit = 0.0;
    for (var i = 0; i < count; i = i + 1) {
        let rr = sqrt((f32(i) + 0.5) / f32(count)) * radius;
        let th = f32(i) * 2.39996323 + rot;
        lit += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(cos(th), sin(th)) * rr * g.shadow.z, ndc.z);
    }
    return 1.0 - clamp(g.shadow.y, 0.0, 1.0) * (1.0 - lit / f32(count));
}

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.tangent = v.tangent;
    o.uv = v.uv;
    return o;
}

const PI: f32 = 3.14159265;

fn sky(d: vec3<f32>) -> vec3<f32> {
    let y = clamp(d.y, -1.0, 1.0);
    if (y < 0.0) {
        return mix(g.horizon.rgb, g.ground.rgb, smoothstep(0.0, 0.35, -y));
    }
    return mix(g.horizon.rgb, g.zenith.rgb, smoothstep(0.0, 0.8, y));
}

fn rotate_env(d: vec3<f32>) -> vec3<f32> {
    let c = cos(g.env_misc.x);
    let s = sin(g.env_misc.x);
    return vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
}

fn equirect(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.z, d.x) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

fn sh_irradiance(n: vec3<f32>) -> vec3<f32> {
    let x = n.x;
    let y = n.y;
    let z = n.z;
    var e = g.sh[0].rgb * 0.282095;
    e += g.sh[1].rgb * (0.488603 * y) + g.sh[2].rgb * (0.488603 * z) + g.sh[3].rgb * (0.488603 * x);
    e += g.sh[4].rgb * (1.092548 * x * y) + g.sh[5].rgb * (1.092548 * y * z);
    e += g.sh[6].rgb * (0.315392 * (3.0 * z * z - 1.0)) + g.sh[7].rgb * (1.092548 * x * z);
    e += g.sh[8].rgb * (0.546274 * (x * x - y * y));
    return max(e, vec3<f32>(0.0));
}

// A round studio softbox seen in reflection: angular radius grows with roughness while total
// energy stays roughly constant, so smooth surfaces show a small bright highlight and rough
// surfaces a wide dim one.
fn softbox(r: vec3<f32>, dir: vec3<f32>, radius: f32, intensity: f32, rough: f32) -> f32 {
    let w = radius + rough * rough * 1.6;
    let ang = acos(clamp(dot(r, dir), -1.0, 1.0));
    let falloff = 1.0 - smoothstep(w * 0.6, w, ang);
    return intensity * (radius * radius) / (w * w) * falloff;
}

// Soft-edged band: 1 within `half` of the center, edges widened by `soft`.
fn band(d: f32, half: f32, soft: f32) -> f32 {
    return 1.0 - smoothstep(half - soft, half + soft, d);
}

// Studio environment seen in reflections: a ring of eight light panels around the horizon,
// a round overhead light and the key softbox. Edge softness grows with roughness and panel
// brightness drops as they spread, so every surface reflects crisp shapes when smooth and a
// soft wash when rough, from any viewing angle.
fn studio(r: vec3<f32>, key: vec3<f32>, rough: f32) -> vec3<f32> {
    let soft = 0.02 + rough * 0.75;
    let step = PI / 4.0;
    let az = atan2(r.z, r.x);
    let da = abs(az / step - round(az / step)) * step;
    let el = asin(clamp(r.y, -1.0, 1.0));
    let half_a = 0.27;
    let half_e = 0.36;
    let spread = (half_a * half_e) / ((half_a + soft) * (half_e + soft));
    let panels = band(da, half_a, soft) * band(abs(el - 0.05), half_e, soft) * g.lights.x * spread;
    let top_half = 0.35;
    let top = band(acos(clamp(r.y, -1.0, 1.0)), top_half, soft) * g.lights.y * (top_half * top_half) / ((top_half + soft) * (top_half + soft));
    let warm = vec3<f32>(1.0, 0.96, 0.9);
    return warm * (panels + top + softbox(r, key, 0.22, g.lights.z, rough));
}

// Diffuse environment light seen by a normal.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    if (g.lights.w > 0.5) {
        return sh_irradiance(rotate_env(n)) * g.env_misc.y;
    }
    return mix(g.irr_down.rgb, g.irr_up.rgb, n.y * 0.5 + 0.5);
}

// Specular environment light along a reflection direction.
fn environment(r: vec3<f32>, key: vec3<f32>, rough: f32) -> vec3<f32> {
    if (g.lights.w > 0.5) {
        let uv = equirect(rotate_env(r));
        return textureSampleLevel(env_tex, env_samp, uv, env_lod(rough)).rgb * g.env_misc.y;
    }
    return mix(sky(r), irradiance(r), rough) + studio(r, key, rough);
}

// Prefiltered HDRI level for a GGX lobe of perceptual roughness `rough`. Each level doubles the
// blur angle and the last one matches roughness 1 (see subcraft_assets::environment), so the
// level is last + log2(lobe width) with the lobe width ~ alpha = rough^2.
fn env_lod(rough: f32) -> f32 {
    return clamp(g.env_misc.z + 2.0 * log2(max(rough, 1e-3)), 0.0, g.env_misc.z);
}

// Analytic split-sum environment BRDF terms (Karis, "Physically Based Shading on Mobile"):
// reflectance = f0 * x + y.
fn dfg(roughness: f32, nov: f32) -> vec2<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nov)) * r.x + r.y;
    return vec2<f32>(-1.04, 1.04) * a004 + r.zw;
}

fn env_brdf(f0: vec3<f32>, roughness: f32, nov: f32) -> vec3<f32> {
    let ab = dfg(roughness, nov);
    return f0 * ab.x + ab.y;
}

// Multiple-scattering compensation (Fdez-Aguera; Filament): single-scatter GGX loses the light
// that bounces between microfacets, so rough metals render too dark. Scale the specular lobe by
// the lost energy, tinted by f0 (1 for smooth surfaces and dielectrics barely change).
fn spec_energy(f0: vec3<f32>, roughness: f32, nov: f32) -> vec3<f32> {
    let ab = dfg(roughness, nov);
    return vec3<f32>(1.0) + f0 * (1.0 / max(ab.x + ab.y, 0.05) - 1.0);
}

// Specular occlusion from ambient occlusion (Lagarde & de Rousiers 2014): a cavity hides
// reflections less than it hides diffuse light at normal incidence, more at grazing angles and
// for sharp reflections. 1 wherever AO is 1.
fn spec_occlusion(nov: f32, ao: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    return clamp(pow(nov + ao, exp2(-16.0 * a - 1.0)) - 1.0 + ao, 0.0, 1.0);
}

// Data value shown as stored: pre-linearized so the sRGB target encodes it back to `c`.
fn as_stored(c: vec3<f32>) -> vec4<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v / 12.92;
    let hi = pow((v + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(hi, lo, v <= vec3<f32>(0.04045)), 1.0);
}

// Single-channel view modes: the channel as the 2D view shows it. `t_base` is always read
// with `albedo_samp`: OpenGL allows one sampler per texture, and the viewport pipeline fails there
// if a texture is sampled with two (issue #91).
// Tangent-space bump normal of the Height channel at `uv` (central differences, one texel apart),
// scaled by the height scale. UV v grows downward while the bitangent points up the image
// (OpenGL convention), hence +dh/dv for y.
fn height_bump(uv: vec2<f32>) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_height));
    let h_l = textureSample(t_height, samp, uv - vec2<f32>(texel.x, 0.0)).r;
    let h_r = textureSample(t_height, samp, uv + vec2<f32>(texel.x, 0.0)).r;
    let h_u = textureSample(t_height, samp, uv - vec2<f32>(0.0, texel.y)).r;
    let h_d = textureSample(t_height, samp, uv + vec2<f32>(0.0, texel.y)).r;
    let dhdu = (h_r - h_l) / (2.0 * texel.x);
    let dhdv = (h_d - h_u) / (2.0 * texel.y);
    return normalize(vec3<f32>(-dhdu * g.params.w, dhdv * g.params.w, 1.0));
}

// The Normal channel combined with the height bump (whiteout), as the shading combines them.
fn combined_normal(ntex: vec3<f32>, bump: vec3<f32>) -> vec3<f32> {
    return normalize(vec3<f32>(ntex.xy + bump.xy, max(ntex.z, 1e-3) * bump.z));
}

fn channel_raw(mode: i32, uv: vec2<f32>) -> vec4<f32> {
    switch mode {
        case 9: {
            let ntex = textureSample(t_normal, samp, uv).rgb * 2.0 - 1.0;
            return vec4<f32>(combined_normal(ntex, height_bump(uv)) * 0.5 + 0.5, 1.0);
        }
        case 1, 7: { return textureSample(t_base, albedo_samp, uv); }
        case 2: { return vec4<f32>(textureSample(t_normal, samp, uv).rgb, 1.0); }
        case 3: { return vec4<f32>(vec3<f32>(textureSample(t_rough, samp, uv).r), 1.0); }
        case 4: { return vec4<f32>(vec3<f32>(textureSample(t_metal, samp, uv).r), 1.0); }
        case 5: { return vec4<f32>(vec3<f32>(textureSample(t_height, samp, uv).r), 1.0); }
        case 8: { return vec4<f32>(vec3<f32>(textureSample(t_base, albedo_samp, uv).r), 1.0); }
        default: { return vec4<f32>(vec3<f32>(textureSample(t_ao, samp, uv).r), 1.0); }
    }
}

// The channel display settings applied to the raw value: HDR scale, one component as gray, two
// colors for the sign of grayscale values, and `shade` (1 when unlit). Color channels (mode 1) are
// shown as colors; everything else as stored.
fn channel_view(mode: i32, uv: vec2<f32>, shade: f32) -> vec4<f32> {
    let raw = channel_raw(mode, uv);
    var c = raw.rgb * g.solo.x;
    let component = i32(round(g.solo.z));
    let grayscale = mode == 3 || mode == 4 || mode == 5 || mode == 6 || mode == 8;
    if (component == 1) {
        c = vec3<f32>(c.r);
    } else if (component == 2) {
        c = vec3<f32>(c.g);
    } else if (component == 3) {
        c = vec3<f32>(c.b);
    } else if (component == 4) {
        c = vec3<f32>(raw.a);
    } else if (grayscale && g.solo.y > 0.5) {
        c = select(g.solo_negative.rgb * (-c.r), g.solo_positive.rgb * c.r, c.r >= 0.0);
    }
    c = c * shade;
    if (mode == 1) {
        return vec4<f32>(c, 1.0);
    }
    return as_stored(c);
}

// A material texture, with the texture level bias of the Display Settings when one is set.
fn material_sample(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    if (g.solo_positive.a != 0.0) {
        return textureSampleBias(t, s, uv, g.solo_positive.a);
    }
    return textureSample(t, s, uv);
}

// 4x4 Bayer threshold of a pixel, in (0, 1).
fn bayer4(p: vec2<f32>) -> f32 {
    let x = u32(p.x) & 3u;
    let y = u32(p.y) & 3u;
    var m = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(m[y * 4u + x]) + 0.5) / 16.0;
}

// Effective opacity of the surface (Base Color coverage x Opacity channel) under the texture
// set's alpha mode: cut-out texels are discarded, the returned alpha is what blending uses
// (1 unless alpha blending without dithering). Sample first, decide after: no texture reads
// follow a discard.
fn surface_alpha(uv: vec2<f32>, frag: vec2<f32>) -> f32 {
    let a = clamp(textureSample(t_base, albedo_samp, uv).a * textureSample(t_opacity, samp, uv).r, 0.0, 1.0);
    let mode = i32(round(mat.alpha.x));
    if (mode == 0) {
        return 1.0;
    }
    if (mat.alpha.z > 0.5) {
        if (a < bayer4(frag)) {
            discard;
        }
        return 1.0;
    }
    if (mode == 1) {
        if (a < mat.alpha.y) {
            discard;
        }
        return 1.0;
    }
    return a;
}

// The environment seen through a transmissive surface along `d`: the HDRI, or the procedural
// sky as the background shows it (without the studio panels, which exist to be reflected).
fn background_radiance_lit(d: vec3<f32>, rough: f32) -> vec3<f32> {
    if (g.lights.w > 0.5) {
        let uv = equirect(rotate_env(d));
        return textureSampleLevel(env_tex, env_samp, uv, env_lod(rough)).rgb * g.env_misc.y;
    }
    return mix(sky(d), irradiance(d), rough);
}

// Layer of extended group `grp` in `t_ext`, -1 when absent.
fn ext_layer(grp: i32) -> i32 {
    switch grp {
        case 0: { return i32(round(mat.ext_a.x)); }
        case 1: { return i32(round(mat.ext_a.y)); }
        case 2: { return i32(round(mat.ext_a.z)); }
        case 3: { return i32(round(mat.ext_a.w)); }
        case 4: { return i32(round(mat.ext_b.x)); }
        case 5: { return i32(round(mat.ext_b.y)); }
        default: { return i32(round(mat.ext_b.z)); }
    }
}

// Extended group `grp` at `uv`, or `def` (the channels' defaults) when the set has none of its
// channels. Always samples (layer 0 of a 1x1 texture when absent) so derivatives stay in
// uniform control flow.
fn ext_sample(grp: i32, def: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    let layer = ext_layer(grp);
    var v: vec4<f32>;
    if (g.solo_positive.a != 0.0) {
        v = textureSampleBias(t_ext, samp, uv, max(layer, 0), g.solo_positive.a);
    } else {
        v = textureSample(t_ext, samp, uv, max(layer, 0));
    }
    return select(v, def, layer < 0);
}

// Anisotropic GGX (Burley 2012) with the height-correlated Smith visibility (Heitz 2014):
// roughness `at` along the tangent `ta`, `ab` along the bitangent `ba`.
fn aniso_ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, ta: vec3<f32>, ba: vec3<f32>, at: f32, ab: f32) -> f32 {
    let h = normalize(v + l);
    let noh = max(dot(n, h), 0.0);
    let toh = dot(ta, h);
    let boh = dot(ba, h);
    let a2 = at * ab;
    let dv = vec3<f32>(ab * toh, at * boh, a2 * noh);
    let w2 = a2 / max(dot(dv, dv), 1e-7);
    let d = a2 * w2 * w2 / PI;
    let nov = max(dot(n, v), 1e-4);
    let nol = max(dot(n, l), 0.0);
    let lv = nol * length(vec3<f32>(at * dot(ta, v), ab * dot(ba, v), nov));
    let ll = nov * length(vec3<f32>(at * dot(ta, l), ab * dot(ba, l), nol));
    let vis = 0.5 / max(lv + ll, 1e-5);
    return d * vis;
}

// Isotropic GGX distribution times Schlick-Smith visibility / (4 n.v n.l), as the base layer.
fn ggx(nol: f32, nov: f32, noh: f32, rough: f32) -> f32 {
    let a = rough * rough;
    let a2 = a * a;
    let dd = noh * noh * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * dd * dd);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let gv = nov / (nov * (1.0 - k) + k);
    let gl = nol / (nol * (1.0 - k) + k);
    return d * gv * gl / max(4.0 * nov * nol, 1e-4);
}

// "Charlie" sheen distribution (Estevez & Kulla 2017) with Neubelt's visibility.
fn sheen_brdf(nol: f32, nov: f32, noh: f32, rough: f32) -> f32 {
    let inv = 1.0 / (rough * rough);
    let sin2 = max(1.0 - noh * noh, 0.0);
    let d = (2.0 + inv) * pow(sin2, inv * 0.5) / (2.0 * PI);
    let vis = 1.0 / max(4.0 * (nol + nov - nol * nov), 1e-4);
    return d * vis;
}

// Directional albedo of the sheen lobe under uniform light (fit used by three.js), in the units
// of `irradiance()` (which already divides by pi).
fn sheen_albedo(nov: f32, rough: f32) -> f32 {
    let r2 = rough * rough;
    let lo = rough < 0.25;
    let a = select(-8.48 * r2 + 14.3 * rough - 9.95, -339.2 * r2 + 161.4 * rough - 25.9, lo);
    let b = select(1.97 * r2 - 3.27 * rough + 0.72, 44.0 * r2 - 23.7 * rough + 3.26, lo);
    let dg = exp(a * nov + b) + select(0.1 * (rough - 0.25), 0.0, lo);
    return clamp(dg, 0.0, PI);
}

// The surface's display colour and coverage (what `fs_main` draws and `fs_oit` accumulates).
fn shade(in: VsOut, front: bool) -> vec4<f32> {
    let mode = i32(round(g.env_misc.w));
    if (mode > 0) {
        var shade = 1.0;
        if (g.solo.w > 0.5) {
            // Lit solo view: the channel under the key light, so the model's shape stays readable.
            var facing = normalize(in.normal);
            if (!front) {
                facing = -facing;
            }
            shade = 0.35 + 0.65 * max(dot(facing, normalize(g.light_dir.xyz)), 0.0);
        }
        let shown_channel = channel_view(mode, in.uv, shade);
        return vec4<f32>(part_overlay(shown_channel.rgb, in.uv, in.clip.xy), shown_channel.a);
    }
    let base = material_sample(t_base, albedo_samp, in.uv).rgb;
    let rough = clamp(material_sample(t_rough, samp, in.uv).r, 0.04, 1.0);
    let metal = clamp(material_sample(t_metal, samp, in.uv).r, 0.0, 1.0);
    let ntex = material_sample(t_normal, samp, in.uv).rgb * 2.0 - 1.0;
    let emissive = material_sample(t_emissive, samp, in.uv).rgb * mat.shader.y;
    // AO intensity blends the map toward no occlusion.
    let ao = mix(1.0, clamp(material_sample(t_ao, samp, in.uv).r, 0.0, 1.0), clamp(mat.shader.x, 0.0, 1.0));

    // Height bump combined with the normal map (whiteout): the same helpers as the "Normal +
    // Height" view, so that view always shows what the shading uses.
    let nts = combined_normal(ntex, height_bump(in.uv));

    var n0 = normalize(in.normal);
    if (!front) {
        n0 = -n0;
    }
    let t = normalize(in.tangent.xyz - n0 * dot(n0, in.tangent.xyz));
    let b = cross(n0, t) * in.tangent.w;
    let n = normalize(t * nts.x + b * nts.y + n0 * nts.z);

    let v = normalize(g.eye.xyz - in.world);
    let l = normalize(g.light_dir.xyz);
    let h = normalize(v + l);
    let nol = max(dot(n, l), 0.0);
    let nov = max(dot(n, v), 1e-4);
    let noh = max(dot(n, h), 0.0);
    let voh = max(dot(v, h), 0.0);

    let spec_level = clamp(material_sample(t_specular, samp, in.uv).r, 0.0, 1.0);
    let f0 = mix(vec3<f32>(0.08 * spec_level), base, metal);
    let diffuse = base * (1.0 - metal);

    let a = rough * rough;
    let a2 = a * a;
    let dd = noh * noh * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * dd * dd);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let gv = nov / (nov * (1.0 - k) + k);
    let gl = nol / (nol * (1.0 - k) + k);
    let f = f0 + (1.0 - f0) * pow(1.0 - voh, 5.0);
    let spec = d * gv * gl * f / max(4.0 * nov * nol, 1e-4);
    let kd = (1.0 - f) * diffuse / PI;
    let light = g.light_color.rgb * g.params.z;
    let ec = spec_energy(f0, rough, nov);
    let direct = (kd + spec * ec) * light * nol * shadow_visibility(in.world, n0, in.clip.xy);

    let r = reflect(-v, n);
    // Horizon fading: a normal map can tilt the reflection below the geometric surface; such
    // reflections would see through the mesh, so they fade out (Painter's Horizon Fading).
    let horizon = clamp(1.0 + mat.shader.z * dot(r, n0), 0.0, 1.0);
    let env_spec = environment(r, l, rough) * horizon * horizon;
    // Ambient (environment) light: AO darkens the diffuse part, specular occlusion derived from
    // AO the reflections.
    let so = spec_occlusion(nov, ao, rough);
    let ambient = (diffuse * irradiance(n) * ao + env_brdf(f0, rough, nov) * ec * env_spec * so) * g.params.y;

    var color = (direct + ambient + emissive) * g.params.x;

    // ---- extended channels: anisotropy, subsurface, translucency, transmission, sheen, coat.
    // A set without any of them keeps the shading above exactly.
    let x_coat = ext_sample(0, vec4<f32>(1.0, 1.0, 1.0, 0.0), in.uv);
    let x_coat2 = ext_sample(1, vec4<f32>(0.1, 0.5, 0.0, 0.0), in.uv);
    let x_sheen = ext_sample(2, vec4<f32>(1.0, 1.0, 1.0, 0.0), in.uv);
    let x_misc = ext_sample(3, vec4<f32>(0.5, 0.0, 0.0, 0.0), in.uv);
    let x_sss = ext_sample(4, vec4<f32>(1.0, 1.0, 1.0, 0.0), in.uv);
    let x_trans = ext_sample(5, vec4<f32>(0.0, 0.0, 0.0, 0.5), in.uv);
    let coat_on = ext_layer(0) >= 0 || ext_layer(1) >= 0;
    let sheen_on = ext_layer(2) >= 0;
    let aniso_on = mat.ext_flags.x > 0.5;
    let sss_on = mat.ext_flags.y > 0.5;
    let transl_on = mat.ext_flags.z > 0.5;
    let trans_on = ext_layer(5) >= 0;
    if (coat_on || sheen_on || aniso_on || sss_on || transl_on || trans_on) {
        let vis = shadow_visibility(in.world, n0, in.clip.xy);
        let env_i = g.params.y;
        // Base specular, anisotropic when the set has Anisotropy Level.
        var spec_d = spec;
        var r_spec = r;
        let aniso_level = clamp(length(x_misc.yz), 0.0, 1.0);
        if (aniso_on && aniso_level > 1e-3) {
            let ang = 0.5 * atan2(x_misc.z, x_misc.y);
            let tr0 = t * cos(ang) + b * sin(ang);
            let ta = normalize(tr0 - n * dot(n, tr0));
            let ba = cross(n, ta);
            let at = max(a * (1.0 + aniso_level), 1e-3);
            let ab = max(a * (1.0 - aniso_level), 1e-3);
            spec_d = aniso_ggx(n, v, l, ta, ba, at, ab) * f;
            // Reflections stretched across the tangent (bent normal, as Filament does).
            let a_tan = cross(ba, v);
            let a_nrm = cross(a_tan, ba);
            let bent = normalize(mix(n, a_nrm, aniso_level * clamp(5.0 * rough, 0.0, 1.0)));
            r_spec = reflect(-v, bent);
        }
        let horizon_s = clamp(1.0 + mat.shader.z * dot(r_spec, n0), 0.0, 1.0);
        let env_spec_d = environment(r_spec, l, rough) * horizon_s * horizon_s;

        // Diffuse share left by transmission and translucency.
        let trans = clamp(x_trans.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * (1.0 - metal) * select(0.0, 1.0, trans_on);
        let transl = clamp(x_misc.w, 0.0, 1.0) * select(0.0, 1.0, transl_on);
        let dscale = (vec3<f32>(1.0) - trans) * (1.0 - 0.5 * transl);

        // Subsurface scattering: light spreads further for the scattering colour, so those
        // channels see a smoother normal (the geometric one) and wrap past the terminator, and
        // their shadow edges soften.
        let w = select(vec3<f32>(0.0), clamp(x_sss.a, 0.0, 1.0) * clamp(x_sss.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), sss_on);
        let nl = mix(vec3<f32>(dot(n, l)), vec3<f32>(dot(n0, l)), w);
        let wrap = w * 0.6;
        let diff_nl = clamp((nl + wrap) / (1.0 + wrap), vec3<f32>(0.0), vec3<f32>(1.0));
        let vis_s = mix(vec3<f32>(vis), vec3<f32>(sqrt(vis)), w);
        let irr_d = mix(irradiance(n), irradiance(n0), w);

        var layer = kd * dscale * light * diff_nl * vis_s
            + spec_d * ec * light * nol * vis
            + (diffuse * dscale * irr_d * ao + env_brdf(f0, rough, nov) * ec * env_spec_d * so) * env_i;

        // Translucency: light from behind passes through thin parts, tinted by the scattering
        // colour when there is one.
        if (transl_on) {
            let tint = diffuse * mix(vec3<f32>(1.0), x_sss.rgb, x_sss.a * select(0.0, 1.0, sss_on));
            // Diffuse light through the back face, and light scattered forward toward the viewer,
            // strongest at the silhouette where the surface is thinnest.
            let back = max((dot(-n, l) + 0.3) / 1.3, 0.0);
            let thin = (1.0 - nov) * (1.0 - nov);
            let forward = pow(max(dot(v, -l), 0.0), 4.0) * thin;
            layer += tint * transl * (light * (back * 0.3 + forward * 1.5) / PI + irradiance(-n) * env_i * 0.3);
        }

        // Transmission: the environment seen through the surface, refracted by the IOR channel
        // (0..1 = IOR 1..2; 0.5 = 1.5, glass), tinted by the base colour and blurred by roughness.
        if (trans_on) {
            // Refracted in, then out of the back of a solid body (Filament's solid sphere model).
            let ior = 1.0 + clamp(x_trans.a, 0.0, 1.0);
            var rr = refract(-v, n, 1.0 / ior);
            if (dot(rr, rr) < 1e-6) {
                rr = reflect(-v, n);
            }
            let n1 = normalize(dot(n, rr) * rr - n * 0.5);
            let out = refract(rr, n1, ior);
            if (dot(out, out) > 1e-6) {
                rr = out;
            }
            let through = background_radiance_lit(rr, rough) * env_i;
            layer += through * base * trans * (vec3<f32>(1.0) - env_brdf(f0, rough, nov));
        }

        // Sheen: a soft, rim-bright velvet lobe over the base (energy taken from the base).
        if (sheen_on) {
            let sheen_col = clamp(x_sheen.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * clamp(x_sheen.a, 0.0, 1.0);
            let sr = clamp(x_misc.x, 0.07, 1.0);
            let sheen_scale = 1.0 - 0.157 * max(sheen_col.r, max(sheen_col.g, sheen_col.b));
            layer = layer * sheen_scale
                + sheen_col * sheen_brdf(nol, nov, noh, sr) * light * nol * vis
                + sheen_col * irradiance(n) * sheen_albedo(nov, sr) * env_i * ao;
        }

        // Clear coat: a second, smooth specular layer with its own normal and roughness,
        // tinting and dimming what is under it (Fresnel of the coat).
        if (coat_on) {
            let amount = clamp(x_coat.a, 0.0, 1.0);
            let cr = clamp(x_coat2.x, 0.04, 1.0);
            let f0c = 0.08 * clamp(x_coat2.y, 0.0, 1.0);
            let cxy = x_coat2.zw;
            let cts = vec3<f32>(cxy, sqrt(max(1.0 - dot(cxy, cxy), 0.0)));
            let nc = normalize(t * cts.x + b * cts.y + n0 * cts.z);
            let novc = max(dot(nc, v), 1e-4);
            let nolc = max(dot(nc, l), 0.0);
            let nohc = max(dot(nc, h), 0.0);
            let fc = f0c + (1.0 - f0c) * pow(1.0 - voh, 5.0);
            let fc_view = f0c + (1.0 - f0c) * pow(1.0 - novc, 5.0);
            let rc = reflect(-v, nc);
            let horizon_c = clamp(1.0 + mat.shader.z * dot(rc, n0), 0.0, 1.0);
            let coat_env = environment(rc, l, cr) * horizon_c * horizon_c * env_brdf(vec3<f32>(f0c), cr, novc);
            let under = (1.0 - amount * fc_view) * mix(vec3<f32>(1.0), clamp(x_coat.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), amount);
            layer = layer * under
                + amount * (ggx(nolc, novc, nohc, cr) * fc * light * nolc * vis + coat_env * env_i * ao);
        }
        color = (layer + emissive) * g.params.x;
    }
    let alpha = surface_alpha(in.uv, in.clip.xy);
    // The highlighted texture set (under the cursor or hovered in the list) is tinted warm.
    let shown = mix(tonemap(color, i32(round(g.post.x)), g.post.y), vec3<f32>(1.0, 0.62, 0.22), 0.3 * mat.shader.w);
    return vec4<f32>(part_overlay(shown, in.uv, in.clip.xy), alpha);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return shade(in, front);
}

// Order-independent transparency (oit.rs): this layer's weighted colour and its coverage, summed
// in any order and averaged by the composite pass. Weight as `oit::layer_weight`.
struct OitOut {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: vec4<f32>,
};

@fragment
fn fs_oit(in: VsOut, @builtin(front_facing) front: bool) -> OitOut {
    let c = shade(in, front);
    let a = clamp(c.a, 0.0, 1.0);
    let z = clamp(in.clip.z, 0.0, 1.0);
    let w = clamp(a * max(1e-2, 3e3 * pow(1.0 - z, 3.0)), 1e-2, 3e3);
    var out: OitOut;
    out.accum = vec4<f32>(c.rgb * a, a) * w;
    out.reveal = vec4<f32>(a);
    return out;
}

// ---- environment behind the model --------------------------------------------------------------

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BgOut;
    o.ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    o.clip = vec4<f32>(o.ndc, 1.0, 1.0);
    return o;
}

// Light arriving from direction `d`, blurred by `blur` (0 sharp .. 1 fully diffused): the HDRI at
// a prefiltered level, or the procedural sky fading into its irradiance.
fn background_radiance(d: vec3<f32>, blur: f32) -> vec3<f32> {
    if (g.lights.w > 0.5) {
        let uv = equirect(rotate_env(d));
        return textureSampleLevel(env_tex, env_samp, uv, blur * g.env_misc.z).rgb * g.env_misc.y;
    }
    return mix(sky(d), irradiance(d), blur) * g.params.y;
}

// The environment seen along each pixel's view ray, exposed and tone mapped like the model, mixed
// over the background color by the environment opacity.
@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    let near = g.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let d = normalize(far.xyz / far.w - near.xyz / near.w);
    let env = tonemap(background_radiance(d, g.env_view.x) * g.params.x, i32(round(g.post.x)), g.post.y);
    // Vertical gradient from the bottom colour to the top one (equal by default).
    let plain = mix(g.background.rgb, g.env_view.yzw, clamp(in.ndc.y * 0.5 + 0.5, 0.0, 1.0));
    return vec4<f32>(mix(plain, env, g.background.a), 1.0);
}

// Octahedral encoding of a unit vector into [-1, 1]^2.
fn oct_encode(n: vec3<f32>) -> vec2<f32> {
    let p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if (n.z >= 0.0) {
        return p;
    }
    return (vec2<f32>(1.0) - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
}

struct SurfaceOut {
    // Octahedral world normal, roughness, reflectance at normal incidence.
    @location(0) surface: vec4<f32>,
    // Share of the pixel's light that comes straight from the key light (what contact shadows
    // may take away).
    @location(1) key_share: vec4<f32>,
};

// Surface pass for the screen-space effects (contact shadows, reflections; post.rs): the shading
// normal (normal map and height bump included), roughness, reflectance and the key light's
// share of the lighting (simplified shading, shadow map included). Cut-out texels are left out.
@fragment
fn fs_surface(in: VsOut, @builtin(front_facing) front: bool) -> SurfaceOut {
    if (surface_alpha(in.uv, in.clip.xy) < 0.5) {
        discard;
    }
    let base = material_sample(t_base, albedo_samp, in.uv).rgb;
    let rough = clamp(material_sample(t_rough, samp, in.uv).r, 0.04, 1.0);
    let metal = clamp(material_sample(t_metal, samp, in.uv).r, 0.0, 1.0);
    let ntex = material_sample(t_normal, samp, in.uv).rgb * 2.0 - 1.0;
    let nts = combined_normal(ntex, height_bump(in.uv));
    var n0 = normalize(in.normal);
    if (!front) {
        n0 = -n0;
    }
    let t = normalize(in.tangent.xyz - n0 * dot(n0, in.tangent.xyz));
    let b = cross(n0, t) * in.tangent.w;
    let n = normalize(t * nts.x + b * nts.y + n0 * nts.z);
    let spec_level = clamp(material_sample(t_specular, samp, in.uv).r, 0.0, 1.0);
    let f0 = mix(vec3<f32>(0.08 * spec_level), base, metal);
    let diffuse = base * (1.0 - metal);
    let v = normalize(g.eye.xyz - in.world);
    let l = normalize(g.light_dir.xyz);
    let nol = max(dot(n, l), 0.0);
    let nov = max(dot(n, v), 1e-4);
    let ao = mix(1.0, clamp(material_sample(t_ao, samp, in.uv).r, 0.0, 1.0), clamp(mat.shader.x, 0.0, 1.0));
    let direct = (diffuse + f0) / PI * g.light_color.rgb * g.params.z * nol * shadow_visibility(in.world, n0, in.clip.xy);
    let ambient = (diffuse * irradiance(n) * ao + env_brdf(f0, rough, nov) * environment(reflect(-v, n), l, rough)) * g.params.y;
    let lum = vec3<f32>(0.2126, 0.7152, 0.0722);
    let share = dot(direct, lum) / max(dot(direct + ambient, lum), 1e-5);
    var out: SurfaceOut;
    out.surface = vec4<f32>(oct_encode(n), rough, max(f0.r, max(f0.g, f0.b)));
    out.key_share = vec4<f32>(clamp(share, 0.0, 1.0));
    return out;
}

// Emission only, in linear HDR: feeds the glow and glare passes of the viewport.
@fragment
fn fs_emissive(in: VsOut) -> @location(0) vec4<f32> {
    let e = textureSample(t_emissive, samp, in.uv).rgb * mat.shader.y * g.params.x;
    // Cut-out texels emit nothing; blended ones glow by their opacity.
    let alpha = surface_alpha(in.uv, in.clip.xy);
    return vec4<f32>(e * alpha, 1.0);
}
