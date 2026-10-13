// SubCraft viewport: metal/roughness PBR with a key light and a procedural sky environment.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    // x: exposure, y: environment intensity, z: light intensity
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var samp: sampler;
@group(1) @binding(1) var t_base: texture_2d<f32>;
@group(1) @binding(2) var t_rough: texture_2d<f32>;
@group(1) @binding(3) var t_metal: texture_2d<f32>;
@group(1) @binding(4) var t_normal: texture_2d<f32>;
@group(1) @binding(5) var t_emissive: texture_2d<f32>;

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
    let ground = vec3<f32>(0.16, 0.14, 0.12);
    let horizon = vec3<f32>(0.62, 0.64, 0.68);
    let zenith = vec3<f32>(0.26, 0.38, 0.62);
    let y = clamp(d.y, -1.0, 1.0);
    if (y < 0.0) {
        return mix(horizon, ground, smoothstep(0.0, 0.35, -y));
    }
    return mix(horizon, zenith, smoothstep(0.0, 0.8, y));
}

// Cosine-weighted irradiance of the sky above, approximated per normal.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let up = vec3<f32>(0.42, 0.48, 0.58);
    let down = vec3<f32>(0.20, 0.18, 0.16);
    return mix(down, up, n.y * 0.5 + 0.5);
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

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let base = textureSample(t_base, samp, in.uv).rgb;
    let rough = clamp(textureSample(t_rough, samp, in.uv).r, 0.04, 1.0);
    let metal = clamp(textureSample(t_metal, samp, in.uv).r, 0.0, 1.0);
    let ntex = textureSample(t_normal, samp, in.uv).rgb * 2.0 - 1.0;
    let emissive = textureSample(t_emissive, samp, in.uv).rgb;

    var n0 = normalize(in.normal);
    if (!front) {
        n0 = -n0;
    }
    let t = normalize(in.tangent.xyz - n0 * dot(n0, in.tangent.xyz));
    let b = cross(n0, t) * in.tangent.w;
    let n = normalize(t * ntex.x + b * ntex.y + n0 * max(ntex.z, 1e-3));

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
    let light = vec3<f32>(1.0, 0.96, 0.9) * g.params.z;
    let direct = (kd + spec) * light * nol;

    let r = reflect(-v, n);
    let env_spec = mix(sky(r), irradiance(r), rough);
    let ambient = (diffuse * irradiance(n) + env_brdf(f0, rough, nov) * env_spec) * g.params.y;

    let color = (direct + ambient + emissive) * g.params.x;
    return vec4<f32>(aces(color), 1.0);
}
