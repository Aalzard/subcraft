// Progressive path tracer for the Rendering mode (ADR-0001: wgpu compute, no hardware ray
// tracing). One thread per pixel adds one camera path per dispatch to the accumulation buffer.
// Traverses the same flattened SAH BVH as the GPU baker (crates/mesh/src/bvh.rs, `flatten`).
//
// Material: metal/roughness GGX (VNDF sampling, height-correlated Smith) + Lambert, normal map
// and height bump in the viewport's tangent convention, emission. Lighting: the viewport
// environment (HDRI or procedural sky; the studio panels only in specular reflections, as the
// raster viewport shows them), the key light as a directional light with shadow rays, and an
// optional gray ground disc. Russian roulette after the second bounce.

struct Params {
    inv_view_proj: mat4x4<f32>,
    eye: vec4<f32>,             // xyz eye, w = 1 for orthographic
    light_dir: vec4<f32>,       // xyz towards the key light, w = intensity
    light_color: vec4<f32>,     // rgb, w = environment intensity
    background: vec4<f32>,      // rgb, w = height bump scale
    ground: vec4<f32>,
    horizon: vec4<f32>,
    zenith: vec4<f32>,
    lights: vec4<f32>,          // ring panels, overhead, key softbox, 1 = HDRI
    env: vec4<f32>,             // rotation, HDRI intensity, ray epsilon, firefly clamp
    floor: vec4<f32>,           // xyz center, w = radius (0 = no ground)
    size: vec4<u32>,            // width, height, sample index, max bounces
    misc: vec4<u32>,            // frame seed, write guides, 0, 0
    ground_look: vec4<f32>,     // rgb reflectivity, w = glossiness
    ground_misc: vec4<f32>,     // key-light shadow intensity, 1 = visible from below, 1 = clear color, 0
    clear: vec4<f32>,           // rgb clear color seen by camera rays that miss
};

@group(0) @binding(0) var<storage, read> nodes: array<vec4<u32>>;     // 2 per node
@group(0) @binding(1) var<storage, read> tris: array<vec4<f32>>;      // 3 per triangle: v0, e1, e2
@group(0) @binding(2) var<storage, read> tri_info: array<vec4<u32>>;  // vertex indices + material layer
@group(0) @binding(3) var<storage, read> verts: array<vec4<f32>>;     // 3 per vertex: n+u, tangent, v
@group(0) @binding(4) var<storage, read_write> accum: array<vec4<f32>>; // rgb sum, w = sum of luminance²
@group(0) @binding(5) var<storage, read_write> guide_albedo: array<vec4<f32>>; // rgb albedo, w = depth
@group(0) @binding(6) var<storage, read_write> guide_normal: array<vec4<f32>>;
@group(0) @binding(7) var<uniform> p: Params;
@group(1) @binding(0) var samp: sampler;
@group(1) @binding(1) var t_base_rough: texture_2d_array<f32>;
@group(1) @binding(2) var t_normal_metal: texture_2d_array<f32>;
@group(1) @binding(3) var t_emissive_height: texture_2d_array<f32>;
@group(1) @binding(4) var env_tex: texture_2d<f32>;
@group(1) @binding(5) var env_samp: sampler;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
const FAR: f32 = 3.4e38;
const FLOOR_ALBEDO: f32 = 0.18;

// ----- random numbers (PCG) -----------------------------------------------------------------

var<private> rng: u32;

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand() -> f32 {
    rng = pcg(rng);
    return f32(rng >> 8u) * (1.0 / 16777216.0);
}

// ----- BVH traversal ---------------------------------------------------------------------------

fn safe_inv(d: vec3<f32>) -> vec3<f32> {
    let s = select(vec3<f32>(1.0), vec3<f32>(-1.0), d < vec3<f32>(0.0));
    return 1.0 / select(d, s * 1e-20, abs(d) < vec3<f32>(1e-20));
}

fn slab(bmin: vec3<f32>, bmax: vec3<f32>, o: vec3<f32>, inv: vec3<f32>, t_max: f32) -> f32 {
    let t0 = (bmin - o) * inv;
    let t1 = (bmax - o) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    let enter = max(max(max(tn.x, tn.y), tn.z), 0.0);
    let exit = min(min(min(tf.x, tf.y), tf.z), t_max);
    return select(-1.0, enter, enter <= exit);
}

struct Hit {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
};

// Closest hit (both sides) in (0, t_max); `hit.t == t_max` when nothing is hit. `any_hit` stops
// at the first hit.
fn trace(o: vec3<f32>, d: vec3<f32>, t_max: f32, any_hit: bool) -> Hit {
    var hit = Hit(t_max, 0u, 0.0, 0.0);
    if (arrayLength(&nodes) < 2u) { return hit; }
    let inv = safe_inv(d);
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    while (sp > 0u) {
        sp = sp - 1u;
        let ni = stack[sp];
        let a = nodes[ni * 2u];
        let b = nodes[ni * 2u + 1u];
        let count = b.w;
        if (slab(bitcast<vec3<f32>>(a.xyz), bitcast<vec3<f32>>(b.xyz), o, inv, hit.t) < 0.0) { continue; }
        if (count > 0u) {
            for (var k = 0u; k < count; k = k + 1u) {
                let ti = a.w + k;
                let v0 = tris[ti * 3u].xyz;
                let e1 = tris[ti * 3u + 1u].xyz;
                let e2 = tris[ti * 3u + 2u].xyz;
                let pv = cross(d, e2);
                let det = dot(e1, pv);
                if (abs(det) < 1e-14) { continue; }
                let inv_det = 1.0 / det;
                let s = o - v0;
                let u = dot(s, pv) * inv_det;
                if (u < 0.0 || u > 1.0) { continue; }
                let q = cross(s, e1);
                let v = dot(d, q) * inv_det;
                if (v < 0.0 || u + v > 1.0) { continue; }
                let t = dot(e2, q) * inv_det;
                if (t <= 0.0 || t >= hit.t) { continue; }
                hit = Hit(t, ti, u, v);
                if (any_hit) { return hit; }
            }
        } else {
            let l = a.w;
            let la = nodes[l * 2u];
            let lb = nodes[l * 2u + 1u];
            let ra = nodes[l * 2u + 2u];
            let rb = nodes[l * 2u + 3u];
            let dl = slab(bitcast<vec3<f32>>(la.xyz), bitcast<vec3<f32>>(lb.xyz), o, inv, hit.t);
            let dr = slab(bitcast<vec3<f32>>(ra.xyz), bitcast<vec3<f32>>(rb.xyz), o, inv, hit.t);
            if (dl >= 0.0 && dr >= 0.0) {
                if (dl <= dr) {
                    stack[sp] = l + 1u;
                    stack[sp + 1u] = l;
                } else {
                    stack[sp] = l;
                    stack[sp + 1u] = l + 1u;
                }
                sp = sp + 2u;
            } else if (dl >= 0.0) {
                stack[sp] = l;
                sp = sp + 1u;
            } else if (dr >= 0.0) {
                stack[sp] = l + 1u;
                sp = sp + 1u;
            }
        }
    }
    return hit;
}

// Ground disc (plane y = floor.y), distance or -1.
fn floor_hit(o: vec3<f32>, d: vec3<f32>, t_max: f32) -> f32 {
    if (p.floor.w <= 0.0 || abs(d.y) < 1e-8) { return -1.0; }
    // Seen from underneath (a ray going up) only when the ground is visible from below.
    if (d.y > 0.0 && p.ground_misc.y == 0.0) { return -1.0; }
    let t = (p.floor.y - o.y) / d.y;
    if (t <= 0.0 || t >= t_max) { return -1.0; }
    let q = o + d * t - p.floor.xyz;
    if (dot(q.xz, q.xz) > p.floor.w * p.floor.w) { return -1.0; }
    return t;
}

// What camera rays see where they miss the model: the clear color, or the view's background.
fn backdrop() -> vec3<f32> {
    return select(p.background.rgb, p.clear.rgb, p.ground_misc.z != 0.0);
}

fn occluded(o: vec3<f32>, d: vec3<f32>) -> bool {
    return trace(o, d, FAR, true).t < FAR || floor_hit(o, d, FAR) > 0.0;
}

// ----- environment (mirrors viewport.wgsl) ---------------------------------------------------------

fn sky(d: vec3<f32>) -> vec3<f32> {
    let y = clamp(d.y, -1.0, 1.0);
    if (y < 0.0) {
        return mix(p.horizon.rgb, p.ground.rgb, smoothstep(0.0, 0.35, -y));
    }
    return mix(p.horizon.rgb, p.zenith.rgb, smoothstep(0.0, 0.8, y));
}

fn rotate_env(d: vec3<f32>) -> vec3<f32> {
    let c = cos(p.env.x);
    let s = sin(p.env.x);
    return vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
}

fn equirect(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.z, d.x) / TAU + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

fn softbox(r: vec3<f32>, dir: vec3<f32>, radius: f32, intensity: f32, rough: f32) -> f32 {
    let w = radius + rough * rough * 1.6;
    let ang = acos(clamp(dot(r, dir), -1.0, 1.0));
    let falloff = 1.0 - smoothstep(w * 0.6, w, ang);
    return intensity * (radius * radius) / (w * w) * falloff;
}

fn band(d: f32, half: f32, soft: f32) -> f32 {
    return 1.0 - smoothstep(half - soft, half + soft, d);
}

fn studio(r: vec3<f32>, key: vec3<f32>, rough: f32) -> vec3<f32> {
    let soft = 0.02 + rough * 0.75;
    let step = PI / 4.0;
    let az = atan2(r.z, r.x);
    let da = abs(az / step - round(az / step)) * step;
    let el = asin(clamp(r.y, -1.0, 1.0));
    let half_a = 0.27;
    let half_e = 0.36;
    let spread = (half_a * half_e) / ((half_a + soft) * (half_e + soft));
    let panels = band(da, half_a, soft) * band(abs(el - 0.05), half_e, soft) * p.lights.x * spread;
    let top_half = 0.35;
    let top = band(acos(clamp(r.y, -1.0, 1.0)), top_half, soft) * p.lights.y * (top_half * top_half) / ((top_half + soft) * (top_half + soft));
    let warm = vec3<f32>(1.0, 0.96, 0.9);
    return warm * (panels + top + softbox(r, key, 0.22, p.lights.z, rough));
}

// Radiance arriving along `d`. The procedural studio panels are a reflection-only device in the
// raster viewport (diffuse light there is the sky irradiance); the path tracer keeps that look by
// showing them only to glossy (specular-sampled) rays, blurred like the viewport by roughness.
fn environment(d: vec3<f32>, glossy: bool, rough: f32) -> vec3<f32> {
    if (p.lights.w > 0.5) {
        return textureSampleLevel(env_tex, env_samp, equirect(rotate_env(d)), 0.0).rgb * p.env.y * p.light_color.w;
    }
    var c = sky(d);
    if (glossy) {
        c += studio(d, normalize(p.light_dir.xyz), rough);
    }
    return c * p.light_color.w;
}

// ----- material ----------------------------------------------------------------------------------

struct Surface {
    pos: vec3<f32>,
    ng: vec3<f32>,      // geometric normal facing the incoming ray
    n: vec3<f32>,       // shading normal
    base: vec3<f32>,
    rough: f32,
    metal: f32,
    emissive: vec3<f32>,
};

fn vert_n(i: u32) -> vec3<f32> { return verts[i * 3u].xyz; }
fn vert_t(i: u32) -> vec4<f32> { return verts[i * 3u + 1u]; }
fn vert_uv(i: u32) -> vec2<f32> { return vec2<f32>(verts[i * 3u].w, verts[i * 3u + 2u].x); }

fn surface(o: vec3<f32>, d: vec3<f32>, hit: Hit) -> Surface {
    var s: Surface;
    let info = tri_info[hit.tri];
    let w = vec3<f32>(1.0 - hit.u - hit.v, hit.u, hit.v);
    s.pos = o + d * hit.t;
    var ng = normalize(cross(tris[hit.tri * 3u + 1u].xyz, tris[hit.tri * 3u + 2u].xyz));
    var n0 = vert_n(info.x) * w.x + vert_n(info.y) * w.y + vert_n(info.z) * w.z;
    n0 = select(ng, normalize(n0), dot(n0, n0) > 1e-12);
    let t4 = vert_t(info.x) * w.x + vert_t(info.y) * w.y + vert_t(info.z) * w.z;
    let uv = vert_uv(info.x) * w.x + vert_uv(info.y) * w.y + vert_uv(info.z) * w.z;
    let layer = i32(info.w);
    let br = textureSampleLevel(t_base_rough, samp, uv, layer, 0.0);
    let nm = textureSampleLevel(t_normal_metal, samp, uv, layer, 0.0);
    let eh = textureSampleLevel(t_emissive_height, samp, uv, layer, 0.0);
    s.base = br.rgb;
    s.rough = clamp(br.a, 0.04, 1.0);
    s.metal = clamp(nm.a, 0.0, 1.0);
    s.emissive = eh.rgb;

    // Height bump (same construction as the raster viewport) combined with the normal map.
    let texel = 1.0 / vec2<f32>(textureDimensions(t_emissive_height).xy);
    let h_l = textureSampleLevel(t_emissive_height, samp, uv - vec2<f32>(texel.x, 0.0), layer, 0.0).a;
    let h_r = textureSampleLevel(t_emissive_height, samp, uv + vec2<f32>(texel.x, 0.0), layer, 0.0).a;
    let h_u = textureSampleLevel(t_emissive_height, samp, uv - vec2<f32>(0.0, texel.y), layer, 0.0).a;
    let h_d = textureSampleLevel(t_emissive_height, samp, uv + vec2<f32>(0.0, texel.y), layer, 0.0).a;
    let hs = p.background.w;
    let bump = normalize(vec3<f32>(-(h_r - h_l) / (2.0 * texel.x) * hs, (h_d - h_u) / (2.0 * texel.y) * hs, 1.0));
    let ntex = nm.xyz * 2.0 - 1.0;
    let nts = normalize(vec3<f32>(ntex.xy + bump.xy, max(ntex.z, 1e-3) * bump.z));

    // Face the incoming ray (meshes are rendered double-sided).
    if (dot(ng, d) > 0.0) {
        ng = -ng;
    }
    if (dot(n0, ng) < 0.0) {
        n0 = -n0;
    }
    let tl = dot(t4.xyz, t4.xyz);
    var n = n0;
    if (tl > 1e-12) {
        let t = normalize(t4.xyz - n0 * dot(n0, t4.xyz));
        let b = cross(n0, t) * select(-1.0, 1.0, t4.w >= 0.0);
        n = normalize(t * nts.x + b * nts.y + n0 * nts.z);
    }
    // Keep the shading normal on the visible side of the geometry.
    let v = -d;
    if (dot(n, v) < 1e-3) {
        n = normalize(n + v * (1e-3 - dot(n, v)));
    }
    s.ng = ng;
    s.n = n;
    return s;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
}

fn ggx_d(noh: f32, a2: f32) -> f32 {
    let dd = noh * noh * (a2 - 1.0) + 1.0;
    return a2 / (PI * dd * dd);
}

fn smith_lambda(c: f32, a2: f32) -> f32 {
    let c2 = max(c * c, 1e-8);
    return (sqrt(1.0 + a2 * (1.0 - c2) / c2) - 1.0) * 0.5;
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bt = vec3<f32>(b, s + n.y * n.y * a, -n.y);
    return mat3x3<f32>(t, bt, n);
}

// Visible-normal sample (Heitz 2018) in the local frame (z = normal).
fn sample_vndf(v: vec3<f32>, alpha: f32, u1: f32, u2: f32) -> vec3<f32> {
    let vh = normalize(vec3<f32>(alpha * v.x, alpha * v.y, v.z));
    let lensq = vh.x * vh.x + vh.y * vh.y;
    let t1 = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-vh.y, vh.x, 0.0) / sqrt(lensq), lensq > 1e-12);
    let t2 = cross(vh, t1);
    let r = sqrt(u1);
    let phi = TAU * u2;
    let p1 = r * cos(phi);
    let s = 0.5 * (1.0 + vh.z);
    let p2 = (1.0 - s) * sqrt(max(1.0 - p1 * p1, 0.0)) + s * r * sin(phi);
    let nh = p1 * t1 + p2 * t2 + sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2)) * vh;
    return normalize(vec3<f32>(alpha * nh.x, alpha * nh.y, max(1e-6, nh.z)));
}

struct Lobes {
    f: vec3<f32>,     // BSDF * cos
    pdf: f32,
};

fn spec_probability(s: Surface, nov: f32) -> f32 {
    let f0 = mix(vec3<f32>(0.04), s.base, s.metal);
    let fs = luminance(fresnel(f0, nov));
    let fd = luminance(s.base) * (1.0 - s.metal) * (1.0 - fs);
    return clamp(fs / max(fs + fd, 1e-6), 0.1, 0.9);
}

// BSDF * cos(l) and the one-sample mixture pdf for direction `l` (world space).
fn eval_bsdf(s: Surface, v: vec3<f32>, l: vec3<f32>, ps: f32) -> Lobes {
    let n = s.n;
    let nol = dot(n, l);
    let nov = max(dot(n, v), 1e-4);
    if (nol <= 0.0 || dot(s.ng, l) <= 0.0) {
        return Lobes(vec3<f32>(0.0), 0.0);
    }
    let h = normalize(v + l);
    let noh = max(dot(n, h), 0.0);
    let voh = max(dot(v, h), 0.0);
    let alpha = s.rough * s.rough;
    let a2 = alpha * alpha;
    let f0 = mix(vec3<f32>(0.04), s.base, s.metal);
    let d = ggx_d(noh, a2);
    let lv = smith_lambda(nov, a2);
    let ll = smith_lambda(nol, a2);
    let g2 = 1.0 / (1.0 + lv + ll);
    let g1 = 1.0 / (1.0 + lv);
    let f = fresnel(f0, voh);
    let spec = d * g2 * f / (4.0 * nov);
    let diffuse = s.base * (1.0 - s.metal) * (1.0 - fresnel(f0, nov)) * (nol / PI);
    let pdf_spec = g1 * d / (4.0 * nov);
    let pdf_diff = nol / PI;
    return Lobes(spec + diffuse, ps * pdf_spec + (1.0 - ps) * pdf_diff);
}

// ----- main ---------------------------------------------------------------------------------------------

fn camera_ray(px: vec2<f32>) -> array<vec3<f32>, 2> {
    let ndc = vec2<f32>(px.x / f32(p.size.x) * 2.0 - 1.0, 1.0 - px.y / f32(p.size.y) * 2.0);
    let near = p.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let far = p.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let a = near.xyz / near.w;
    let b = far.xyz / far.w;
    return array<vec3<f32>, 2>(a, normalize(b - a));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) { return; }
    let pixel = gid.y * p.size.x + gid.x;
    rng = pcg(pixel ^ pcg(p.misc.x * 9781u + 0x68bc21u));
    let eps = p.env.z;

    // Guide buffers for the denoiser: the unjittered primary hit, written on the first sample.
    if (p.misc.y != 0u) {
        let ray = camera_ray(vec2<f32>(gid.xy) + 0.5);
        let hit = trace(ray[0], ray[1], FAR, false);
        let tf = floor_hit(ray[0], ray[1], hit.t);
        if (tf > 0.0) {
            guide_albedo[pixel] = vec4<f32>(vec3<f32>(FLOOR_ALBEDO), tf);
            guide_normal[pixel] = vec4<f32>(0.0, 1.0, 0.0, 1.0);
        } else if (hit.t < FAR) {
            let s = surface(ray[0], ray[1], hit);
            guide_albedo[pixel] = vec4<f32>(max(s.base, vec3<f32>(0.02)), hit.t);
            guide_normal[pixel] = vec4<f32>(s.n, 1.0);
        } else {
            guide_albedo[pixel] = vec4<f32>(1.0, 1.0, 1.0, FAR);
            guide_normal[pixel] = vec4<f32>(0.0);
        }
    }

    let ray = camera_ray(vec2<f32>(gid.xy) + vec2<f32>(rand(), rand()));
    var o = ray[0];
    var d = ray[1];
    var throughput = vec3<f32>(1.0);
    var radiance = vec3<f32>(0.0);
    var glossy = false;
    var last_rough = 0.0;
    let key = normalize(p.light_dir.xyz);
    let key_light = p.light_color.rgb * p.light_dir.w;
    for (var depth = 0u; depth <= p.size.w; depth = depth + 1u) {
        let hit = trace(o, d, FAR, false);
        let tf = floor_hit(o, d, hit.t);
        if (tf > 0.0) {
            // Gray ground: diffuse, lit by the environment and the key light.
            let pos = o + d * tf;
            let up = select(vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), d.y < 0.0);
            // Fade into the background towards the rim (camera rays only).
            var fade = 1.0;
            if (depth == 0u) {
                let r = length((pos - p.floor.xyz).xz) / p.floor.w;
                fade = 1.0 - smoothstep(0.55, 1.0, r);
                radiance += backdrop() * (1.0 - fade);
            }
            // A glossy reflection tinted by the reflectivity color; the diffuse part keeps the rest.
            let reflectivity = clamp(p.ground_look.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * fade;
            let spec = max(reflectivity.x, max(reflectivity.y, reflectivity.z));
            let albedo = vec3<f32>(FLOOR_ALBEDO) * fade * (1.0 - spec);
            let origin = pos + up * eps;
            if (dot(up, key) > 0.0 && p.light_dir.w > 0.0) {
                // The shadow intensity sets how much of the key light a shadowed spot loses.
                let lit = select(1.0, 1.0 - p.ground_misc.x, occluded(origin, key));
                radiance += throughput * albedo / PI * key_light * dot(up, key) * lit;
            }
            if (depth == p.size.w) { break; }
            o = origin;
            if (spec > 0.0 && rand() < spec) {
                // Phong lobe around the mirror direction: glossiness 1 is a mirror.
                let gloss = clamp(p.ground_look.w, 0.0, 1.0);
                let n = mix(2.0, 4000.0, gloss * gloss * gloss);
                let cos_t = pow(max(rand(), 1e-6), 1.0 / (n + 1.0));
                let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
                let phi = TAU * rand();
                let mirror = reflect(d, up);
                d = normalize(basis(mirror) * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t));
                if (dot(d, up) <= 0.0) { break; }
                throughput *= reflectivity / spec;
                glossy = true;
                last_rough = 1.0 - gloss;
            } else {
                let m = basis(up);
                let r1 = rand();
                let phi = TAU * rand();
                let local = vec3<f32>(sqrt(r1) * cos(phi), sqrt(r1) * sin(phi), sqrt(max(0.0, 1.0 - r1)));
                d = normalize(m * local);
                throughput *= albedo / max(1.0 - spec, 1e-4);
                glossy = false;
            }
        } else if (hit.t >= FAR) {
            if (depth == 0u) {
                radiance += backdrop();
            } else {
                var e = environment(d, glossy, last_rough) * throughput;
                // Clamp rare bright indirect samples (HDRI suns seen through rough bounces).
                let le = luminance(e);
                if (depth > 1u && le > p.env.w) { e *= p.env.w / le; }
                radiance += e;
            }
            break;
        } else {
            let s = surface(o, d, hit);
            let v = -d;
            radiance += throughput * s.emissive;
            let origin = s.pos + s.ng * eps;
            let ps = spec_probability(s, max(dot(s.n, v), 1e-4));
            // Key light (directional, hard shadow) as next-event estimation.
            if (p.light_dir.w > 0.0 && dot(s.ng, key) > 0.0) {
                let b = eval_bsdf(s, v, key, ps);
                if (any(b.f > vec3<f32>(0.0)) && !occluded(origin, key)) {
                    radiance += throughput * b.f * key_light;
                }
            }
            if (depth == p.size.w) { break; }
            // Sample the next direction from the specular or diffuse lobe.
            let m = basis(s.n);
            let vl = transpose(m) * v;
            var l: vec3<f32>;
            if (rand() < ps) {
                let alpha = s.rough * s.rough;
                let h = m * sample_vndf(vl, alpha, rand(), rand());
                l = reflect(-v, h);
                glossy = true;
            } else {
                let r1 = rand();
                let phi = TAU * rand();
                l = m * vec3<f32>(sqrt(r1) * cos(phi), sqrt(r1) * sin(phi), sqrt(max(0.0, 1.0 - r1)));
                glossy = false;
            }
            let b = eval_bsdf(s, v, l, ps);
            if (b.pdf <= 1e-8 || all(b.f <= vec3<f32>(0.0))) { break; }
            throughput *= b.f / b.pdf;
            last_rough = s.rough;
            o = origin;
            d = l;
        }
        // Russian roulette after the second bounce.
        if (depth >= 2u) {
            let q = clamp(max(throughput.x, max(throughput.y, throughput.z)), 0.05, 0.95);
            if (rand() > q) { break; }
            throughput /= q;
        }
    }
    radiance = max(radiance, vec3<f32>(0.0));
    if (any(radiance != radiance)) { radiance = vec3<f32>(0.0); }
    let l = luminance(radiance);
    let prev = select(accum[pixel], vec4<f32>(0.0), p.size.z == 0u);
    accum[pixel] = prev + vec4<f32>(radiance, l * l);
}
