// Post-processing for the viewport: screen-space ambient occlusion, bloom, and the final composite.

struct Post {
    inv_proj: mat4x4<f32>,
    // x: AO radius (object units), y: AO strength, z: bloom strength, w: 1 = orthographic
    params: vec4<f32>,
    // x: width, y: height, z: 1 / width, w: 1 / height (of the target being drawn)
    size: vec4<f32>,
    // xy: blur direction in texels, z: bloom threshold, w: proj[1][1]
    misc: vec4<f32>,
    // Lens and color stage (fs_grade):
    // [0] aberration amount, radius, softness, ratio
    // [1] aberration centre x, y, 1 = achromatic, vignette amount
    // [2] sharpen amount, sharpen diameter (px), grain amount, grain size (px)
    // [3] 1 = colored grain, 1 / gamma, sepia ratio, 1 = color correction on
    grade: mat4x4<f32>,
    // Lens distortion: power (0 = off), zoom, edge roundness, edge smoothness
    lens: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: Post;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var aux: texture_2d<f32>;
// The depth buffer, bound as an unfilterable float texture: GLSL cannot `textureLoad` a depth
// texture, so the OpenGL backend would fail to build this pipeline (issue #91).
@group(0) @binding(4) var depth_tex: texture_2d<f32>;
@group(0) @binding(5) var ao_tex: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn view_pos(px: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) * p.size.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let v = p.inv_proj * vec4<f32>(ndc, depth, 1.0);
    return v.xyz / v.w;
}

fn load_depth(px: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    return textureLoad(depth_tex, clamp(px, vec2<i32>(0), dims - vec2<i32>(1)), 0).r;
}

fn gradient_noise(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Alchemy-style SSAO from depth alone: normals come from the nearest depth neighbours.
@fragment
fn fs_ao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let d = load_depth(px);
    if (d >= 1.0) {
        return vec4<f32>(1.0);
    }
    let pos = view_pos(px, d);
    let pl = view_pos(px + vec2<i32>(-1, 0), load_depth(px + vec2<i32>(-1, 0)));
    let pr = view_pos(px + vec2<i32>(1, 0), load_depth(px + vec2<i32>(1, 0)));
    let pu = view_pos(px + vec2<i32>(0, -1), load_depth(px + vec2<i32>(0, -1)));
    let pd = view_pos(px + vec2<i32>(0, 1), load_depth(px + vec2<i32>(0, 1)));
    var dx = pr - pos;
    if (abs(pl.z - pos.z) < abs(dx.z)) { dx = pos - pl; }
    var dy = pd - pos;
    if (abs(pu.z - pos.z) < abs(dy.z)) { dy = pos - pu; }
    var n = normalize(cross(dx, dy));
    // Face the camera (view space looks down -Z).
    if (dot(n, -pos) < 0.0) { n = -n; }

    let radius = p.params.x;
    // Screen radius of the world radius: perspective shrinks with distance, orthographic does not.
    let focal = 0.5 * p.size.y * p.misc.w;
    var r_px = radius * focal;
    if (p.params.w < 0.5) { r_px = r_px / max(-pos.z, 1e-4); }
    r_px = clamp(r_px, 2.0, 96.0);
    let rot = gradient_noise(frag.xy) * 6.2831853;
    var occ = 0.0;
    let count = 16;
    for (var i = 0; i < count; i = i + 1) {
        let t = (f32(i) + 0.5) / f32(count);
        let a = rot + f32(i) * 2.3999632;
        let off = vec2<f32>(cos(a), sin(a)) * (r_px * (0.15 + 0.85 * t));
        let spx = px + vec2<i32>(round(off));
        let sd = load_depth(spx);
        if (sd >= 1.0) { continue; }
        let sp = view_pos(spx, sd);
        let v = sp - pos;
        let dist2 = dot(v, v);
        // Range check keeps far-away surfaces from darkening the point.
        let range = smoothstep(0.0, 1.0, radius / max(sqrt(dist2), 1e-4));
        occ = occ + max(0.0, dot(n, v) - 0.02 * radius) / (dist2 + 0.01 * radius * radius) * range;
    }
    let ao = clamp(1.0 - occ * radius * 2.0 / f32(count), 0.0, 1.0);
    return vec4<f32>(ao);
}

// Bright pass at half resolution: soft knee around the threshold, in linear light.
@fragment
fn fs_bright(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let c = textureSample(src, samp, uv).rgb;
    let lum = max(max(c.r, c.g), c.b);
    let knee = 0.15;
    let soft = clamp((lum - p.misc.z + knee) / (2.0 * knee), 0.0, 1.0);
    let w = max(soft * soft, select(0.0, 1.0, lum > p.misc.z + knee)) ;
    return vec4<f32>(c * w, 1.0);
}

// 9-tap Gaussian along p.misc.xy texels.
@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let step = p.misc.xy * p.size.zw;
    var w = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var acc = textureSample(src, samp, uv).rgb * w[0];
    for (var i = 1; i < 5; i = i + 1) {
        let o = step * f32(i);
        acc = acc + textureSample(src, samp, uv + o).rgb * w[i] + textureSample(src, samp, uv - o).rgb * w[i];
    }
    return vec4<f32>(acc, 1.0);
}

// Scene (already tone mapped) times the AO factor plus the glow.
@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    var color = textureSample(src, samp, uv).rgb;
    if (p.params.y > 0.0) {
        let ao = textureSample(ao_tex, samp, uv).r;
        color = color * mix(1.0, ao, clamp(p.params.y, 0.0, 1.0));
    }
    if (p.params.z > 0.0) {
        color = color + textureSample(aux, samp, uv).rgb * p.params.z;
    }
    return vec4<f32>(color, 1.0);
}

// Plain bilinear copy (used to bring a full-resolution image down to half resolution).
@fragment
fn fs_copy(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    return vec4<f32>(textureSample(src, samp, uv).rgb, 1.0);
}

// src scaled by p.params.z, to be added into the effects buffer.
@fragment
fn fs_scale(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    return vec4<f32>(textureSample(src, samp, uv).rgb * p.params.z, 1.0);
}

// One stage of a glare streak: [1 2 1] / 4 along p.misc.xy texels. Stages at strides 1, 2, 4, 8...
// multiply into a box and then a smooth tent, so a long streak needs only a few cheap passes.
@fragment
fn fs_tent(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let o = p.misc.xy * p.size.zw;
    let c = textureSample(src, samp, uv).rgb * 0.5;
    let a = textureSample(src, samp, uv + o).rgb * 0.25;
    let b = textureSample(src, samp, uv - o).rgb * 0.25;
    // Spectral streaks (p.params.x > 0): red reaches further and blue less far than green.
    let s = p.params.x;
    if (s > 0.0) {
        let far = o * (1.0 + s);
        let near = o * (1.0 - s);
        let r = textureSample(src, samp, uv + far).r * 0.25 + textureSample(src, samp, uv - far).r * 0.25;
        let bl = textureSample(src, samp, uv + near).b * 0.25 + textureSample(src, samp, uv - near).b * 0.25;
        return vec4<f32>(c.r + r, c.g + a.g + b.g, c.b + bl, 1.0);
    }
    return vec4<f32>(c + a + b, 1.0);
}

// Glare source: emission above the threshold (p.params.x), its excess brightness raised to the
// remap exponent (p.params.y), hue kept.
@fragment
fn fs_glare_src(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let c = textureSample(src, samp, uv).rgb;
    let lum = max(max(c.r, c.g), c.b);
    let kept = pow(max(lum - p.params.x, 0.0), p.params.y);
    return vec4<f32>(c * (kept / max(lum, 1e-5)), 1.0);
}

// Ghost images: src mirrored through the view centre at p.params.x evenly spaced scales (spacing
// p.params.y), each fading toward the edge and tinted a little differently, times p.params.z.
@fragment
fn fs_ghost(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * p.size.zw;
    let count = i32(clamp(p.params.x, 1.0, 8.0));
    let dir = vec2<f32>(0.5) - uv;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        // Ghost i of a bright point at offset d from the centre lands at -d * k: the spacing scales
        // how far out the ghosts sit, so a pixel reads the point at its own offset divided by k.
        let k = p.params.y * (f32(i) + 1.0) * 2.0 / f32(count);
        let at = vec2<f32>(0.5) + dir / k;
        let inside = step(0.0, at.x) * step(at.x, 1.0) * step(0.0, at.y) * step(at.y, 1.0);
        let edge = 1.0 - smoothstep(0.25, 0.5, length(at - vec2<f32>(0.5)));
        let phase = f32(i) * 2.1;
        let tint = vec3<f32>(0.75) + 0.25 * vec3<f32>(cos(phase), cos(phase + 2.094), cos(phase + 4.189));
        acc = acc + textureSampleLevel(src, samp, at, 0.0).rgb * tint * (inside * edge);
    }
    return vec4<f32>(acc * (p.params.z / f32(count)), 1.0);
}

// ---- lens and color stage ---------------------------------------------------------------------
// Runs last, over the finished picture (display-linear 0..1). The CPU reference, tested against
// this pass pixel for pixel, is `grade::Grade::apply`.

// The picture at `uv` with lateral aberration and the vignette.
fn grade_fetch(uv: vec2<f32>) -> vec3<f32> {
    let amount = p.grade[0].x;
    var c = textureSampleLevel(src, samp, uv, 0.0).rgb;
    if (amount > 0.0) {
        let d = uv - p.grade[1].xy;
        let e = vec2<f32>(d.x / p.grade[0].w, d.y) * 1.41421356;
        let radius = p.grade[0].y;
        let soft = p.grade[0].z;
        let shift = d * (amount * 0.04 * smoothstep(radius - soft, radius + soft, length(e)));
        let plus = textureSampleLevel(src, samp, uv + shift, 0.0).rgb;
        let minus = textureSampleLevel(src, samp, uv - shift, 0.0).rgb;
        if (p.grade[1].z > 0.5) {
            c = (plus + c + minus) / 3.0;
        } else {
            c = vec3<f32>(plus.r, c.g, minus.b);
        }
    }
    let r = length(uv - vec2<f32>(0.5)) * 1.41421356;
    return c * (1.0 - p.grade[1].w * smoothstep(0.4, 1.0, r));
}

fn grain_hash(cell: vec2<f32>, salt: f32) -> f32 {
    return fract(sin(dot(cell, vec2<f32>(12.9898, 78.233)) + salt) * 43758.5453);
}

@fragment
// Lens distortion (CPU reference: `grade::Grade::lens_uv`): where view point `uv` reads the
// picture (xy) and how much of it shows (z, 0 outside the rounded edge).
fn lens_uv(uv: vec2<f32>) -> vec3<f32> {
    let aspect = p.size.x * p.size.w;
    let d = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
    let r2 = dot(d, d) / ((aspect * aspect + 1.0) * 0.25);
    let f = (1.0 + p.lens.x * r2) * p.lens.y;
    let at = vec2<f32>(0.5) + (uv - vec2<f32>(0.5)) * f;
    let q = abs(at - vec2<f32>(0.5)) * 2.0;
    let n = 16.0 + (2.0 - 16.0) * clamp(p.lens.z, 0.0, 1.0);
    let s = pow(pow(q.x, n) + pow(q.y, n), 1.0 / n);
    let soft = clamp(p.lens.w, 0.0, 1.0) * 0.5;
    return vec3<f32>(at, 1.0 - smoothstep(1.0 - soft - 1e-4, 1.0, s));
}

fn fs_grade(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var uv = frag.xy * p.size.zw;
    var mask = 1.0;
    if (p.lens.x != 0.0) {
        let l = lens_uv(uv);
        uv = l.xy;
        mask = l.z;
    }
    var c = grade_fetch(uv);
    let sharpen = p.grade[2].x;
    if (sharpen > 0.0) {
        let o = 0.5 * p.grade[2].y * p.size.zw;
        let blur = 0.25 * (grade_fetch(uv + vec2<f32>(-o.x, -o.y)) + grade_fetch(uv + vec2<f32>(o.x, -o.y))
            + grade_fetch(uv + vec2<f32>(-o.x, o.y)) + grade_fetch(uv + vec2<f32>(o.x, o.y)));
        c = max(c + sharpen * (c - blur), vec3<f32>(0.0));
    }
    c = c * mask;
    let grain = p.grade[2].z;
    if (grain > 0.0) {
        let cell = floor(frag.xy / p.grade[2].w);
        var n = vec3<f32>(grain_hash(cell, 0.0));
        if (p.grade[3].x > 0.5) {
            n = vec3<f32>(n.x, grain_hash(cell, 17.0), grain_hash(cell, 43.0));
        }
        c = max(c + (n - vec3<f32>(0.5)) * grain, vec3<f32>(0.0));
    }
    if (p.grade[3].w > 0.5) {
        c = ((c * p.misc.xyz - vec3<f32>(0.5)) * p.params.y + vec3<f32>(0.5)) * p.params.z + vec3<f32>(p.params.w);
        let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * p.params.x;
        let sepia = vec3<f32>(
            dot(c, vec3<f32>(0.393, 0.769, 0.189)),
            dot(c, vec3<f32>(0.349, 0.686, 0.168)),
            dot(c, vec3<f32>(0.272, 0.534, 0.131)));
        c = max(mix(c, sepia, p.grade[3].z), vec3<f32>(0.0));
    }
    c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(p.grade[3].y));
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// ---- depth of field ---------------------------------------------------------------------------
// params: x focus distance, y in-focus depth band, z maximum blur radius (px), w 1 = orthographic.

fn view_dist(px: vec2<i32>) -> f32 {
    let d = load_depth(px);
    // Only the z and w rows of the inverse projection matter for a point on the view axis.
    let vz = p.inv_proj[2][2] * d + p.inv_proj[3][2];
    let vw = p.inv_proj[2][3] * d + p.inv_proj[3][3];
    return max(-(vz / vw), 1e-4);
}

// Circle of confusion in pixels for a view distance: zero inside the band around the focus
// distance, growing with the relative distance from it (so far away saturates, near saturates fast).
fn coc_of(dist: f32) -> f32 {
    let off = max(abs(dist - p.params.x) - 0.5 * p.params.y, 0.0);
    return p.params.z * clamp(2.0 * off / max(dist, 1e-4), 0.0, 1.0);
}

// Single-pass gather with a fixed 20-tap golden-angle disk. A tap only counts when its own circle of
// confusion reaches the centre pixel, and taps behind the centre are limited to the centre's own
// circle: sharp foreground edges stay sharp (no background halo) while blurry foreground still
// spreads over what is behind it.
@fragment
fn fs_dof(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let uv = frag.xy * p.size.zw;
    let center = textureSampleLevel(src, samp, uv, 0.0).rgb;
    let radius = p.params.z;
    let cd = view_dist(px);
    let c_coc = coc_of(cd);
    var acc = center;
    var wsum = 1.0;
    let rot = gradient_noise(frag.xy) * 6.2831853;
    let count = 20;
    for (var i = 0; i < count; i = i + 1) {
        let t = (f32(i) + 0.5) / f32(count);
        let r = radius * sqrt(t);
        let a = rot + f32(i) * 2.3999632;
        let off = vec2<f32>(cos(a), sin(a)) * r;
        let tpx = px + vec2<i32>(round(off));
        let td = view_dist(tpx);
        var reach = coc_of(td);
        if (td > cd) { reach = min(reach, c_coc); }
        // One pixel of soft edge on the disk; a tap with no blur of its own never spreads.
        let w = clamp(reach - r + 1.0, 0.0, 1.0) * select(0.0, 1.0, reach > 0.5);
        acc = acc + textureSampleLevel(src, samp, uv + off * p.size.zw, 0.0).rgb * w;
        wsum = wsum + w;
    }
    return vec4<f32>(acc / wsum, 1.0);
}
