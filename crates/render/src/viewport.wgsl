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
    // x: filter radius in texels
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
};
@group(1) @binding(10) var<uniform> mat: MaterialParams;
// Specular level: the reflectance of non-metals (0.5 = 4%).
@group(1) @binding(11) var t_specular: texture_2d<f32>;

// How much key light reaches `world` (a point on a surface with geometric normal `n`): 1 lit,
// down to 1 - opacity in full shadow. Percentage-closer filter over the shadow map's texels.
fn shadow_visibility(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (g.shadow.x < 0.5) { return 1.0; }
    // Offset along the normal so a lit surface does not shadow itself.
    let c = g.shadow_mat * vec4<f32>(world + n * g.shadow.w * 1.5, 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) { return 1.0; }
    let r = i32(g.shadow_filter.x);
    var lit = 0.0;
    var taps = 0.0;
    for (var y = -r; y <= r; y = y + 1) {
        for (var x = -r; x <= r; x = x + 1) {
            lit += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(x), f32(y)) * g.shadow.z, ndc.z);
            taps += 1.0;
        }
    }
    return 1.0 - clamp(g.shadow.y, 0.0, 1.0) * (1.0 - lit / taps);
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
        return textureSampleLevel(env_tex, env_samp, uv, rough * g.env_misc.z).rgb * g.env_misc.y;
    }
    return mix(sky(r), irradiance(r), rough) + studio(r, key, rough);
}

// Analytic split-sum environment BRDF (Karis, "Physically Based Shading on Mobile").
fn env_brdf(f0: vec3<f32>, roughness: f32, nov: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nov)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
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
fn channel_raw(mode: i32, uv: vec2<f32>) -> vec4<f32> {
    switch mode {
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

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
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
        return channel_view(mode, in.uv, shade);
    }
    let base = material_sample(t_base, albedo_samp, in.uv).rgb;
    let rough = clamp(material_sample(t_rough, samp, in.uv).r, 0.04, 1.0);
    let metal = clamp(material_sample(t_metal, samp, in.uv).r, 0.0, 1.0);
    let ntex = material_sample(t_normal, samp, in.uv).rgb * 2.0 - 1.0;
    let emissive = material_sample(t_emissive, samp, in.uv).rgb * mat.shader.y;
    // AO intensity blends the map toward no occlusion.
    let ao = mix(1.0, clamp(material_sample(t_ao, samp, in.uv).r, 0.0, 1.0), clamp(mat.shader.x, 0.0, 1.0));

    // Height -> tangent-space bump normal (central differences, one texel apart). UV v grows
    // downward while the bitangent points up the image (OpenGL convention), hence +dh/dv for y.
    let texel = 1.0 / vec2<f32>(textureDimensions(t_height));
    let h_l = textureSample(t_height, samp, in.uv - vec2<f32>(texel.x, 0.0)).r;
    let h_r = textureSample(t_height, samp, in.uv + vec2<f32>(texel.x, 0.0)).r;
    let h_u = textureSample(t_height, samp, in.uv - vec2<f32>(0.0, texel.y)).r;
    let h_d = textureSample(t_height, samp, in.uv + vec2<f32>(0.0, texel.y)).r;
    let dhdu = (h_r - h_l) / (2.0 * texel.x);
    let dhdv = (h_d - h_u) / (2.0 * texel.y);
    let bump = normalize(vec3<f32>(-dhdu * g.params.w, dhdv * g.params.w, 1.0));
    // Combine with the normal map (whiteout).
    let nts = normalize(vec3<f32>(ntex.xy + bump.xy, max(ntex.z, 1e-3) * bump.z));

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
    let direct = (kd + spec) * light * nol * shadow_visibility(in.world, n0);

    let r = reflect(-v, n);
    // Horizon fading: a normal map can tilt the reflection below the geometric surface; such
    // reflections would see through the mesh, so they fade out (Painter's Horizon Fading).
    let horizon = clamp(1.0 + mat.shader.z * dot(r, n0), 0.0, 1.0);
    let env_spec = environment(r, l, rough) * horizon * horizon;
    // Ambient (environment) light is occluded by AO; specular occlusion approximated by AO too.
    let ambient = (diffuse * irradiance(n) + env_brdf(f0, rough, nov) * env_spec) * g.params.y * ao;

    let color = (direct + ambient + emissive) * g.params.x;
    let alpha = surface_alpha(in.uv, in.clip.xy);
    return vec4<f32>(tonemap(color, i32(round(g.post.x)), g.post.y), alpha);
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
    return vec4<f32>(mix(g.background.rgb, env, g.background.a), 1.0);
}

// Emission only, in linear HDR: feeds the glow and glare passes of the viewport.
@fragment
fn fs_emissive(in: VsOut) -> @location(0) vec4<f32> {
    let e = textureSample(t_emissive, samp, in.uv).rgb * mat.shader.y * g.params.x;
    // Cut-out texels emit nothing; blended ones glow by their opacity.
    let alpha = surface_alpha(in.uv, in.clip.xy);
    return vec4<f32>(e * alpha, 1.0);
}
