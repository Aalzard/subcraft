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
    sh: array<vec4<f32>, 9>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var env_tex: texture_2d<f32>;
@group(0) @binding(2) var env_samp: sampler;
@group(1) @binding(0) var samp: sampler;
@group(1) @binding(1) var t_base: texture_2d<f32>;
@group(1) @binding(2) var t_rough: texture_2d<f32>;
@group(1) @binding(3) var t_metal: texture_2d<f32>;
@group(1) @binding(4) var t_normal: texture_2d<f32>;
@group(1) @binding(5) var t_emissive: texture_2d<f32>;
@group(1) @binding(6) var t_height: texture_2d<f32>;
@group(1) @binding(7) var t_ao: texture_2d<f32>;

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

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Data value shown as stored: pre-linearized so the sRGB target encodes it back to `c`.
fn as_stored(c: vec3<f32>) -> vec4<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v / 12.92;
    let hi = pow((v + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(hi, lo, v <= vec3<f32>(0.04045)), 1.0);
}

// Single-channel view modes: the channel unlit, as the 2D view shows it.
fn channel_view(mode: i32, uv: vec2<f32>) -> vec4<f32> {
    switch mode {
        case 1: { return vec4<f32>(textureSample(t_base, samp, uv).rgb, 1.0); }
        case 2: { return as_stored(textureSample(t_normal, samp, uv).rgb); }
        case 3: { return as_stored(vec3<f32>(textureSample(t_rough, samp, uv).r)); }
        case 4: { return as_stored(vec3<f32>(textureSample(t_metal, samp, uv).r)); }
        case 5: { return as_stored(vec3<f32>(textureSample(t_height, samp, uv).r)); }
        default: { return as_stored(vec3<f32>(textureSample(t_ao, samp, uv).r)); }
    }
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = i32(round(g.env_misc.w));
    if (mode > 0) {
        return channel_view(mode, in.uv);
    }
    let base = textureSample(t_base, samp, in.uv).rgb;
    let rough = clamp(textureSample(t_rough, samp, in.uv).r, 0.04, 1.0);
    let metal = clamp(textureSample(t_metal, samp, in.uv).r, 0.0, 1.0);
    let ntex = textureSample(t_normal, samp, in.uv).rgb * 2.0 - 1.0;
    let emissive = textureSample(t_emissive, samp, in.uv).rgb;
    let ao = clamp(textureSample(t_ao, samp, in.uv).r, 0.0, 1.0);

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

    let f0 = mix(vec3<f32>(0.04), base, metal);
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
    let direct = (kd + spec) * light * nol;

    let r = reflect(-v, n);
    let env_spec = environment(r, l, rough);
    // Ambient (environment) light is occluded by AO; specular occlusion approximated by AO too.
    let ambient = (diffuse * irradiance(n) + env_brdf(f0, rough, nov) * env_spec) * g.params.y * ao;

    let color = (direct + ambient + emissive) * g.params.x;
    return vec4<f32>(aces(color), 1.0);
}
