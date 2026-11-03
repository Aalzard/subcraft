// GPU layer compositing (ADR-0044): one thread per texel walks a channel's program (leaf layers,
// folder begin / end, content effects) with the same math as `subcraft_compositor::blend`.
// Stored paint / mask tiles are f16 in a pool buffer; CPU-evaluated content arrives as f32 blocks.

struct Params {
    ops_off: u32,
    nops: u32,
    refs_off: u32,
    out_off: u32,
    flags: u32, // bit0 signed, bit1 normal, bit2 color
    tiles: u32,
    eff_off: u32,
    pad1: u32,
    default_value: vec4<f32>,
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> prog: array<u32>;
@group(0) @binding(2) var<storage, read> refs: array<u32>;
@group(0) @binding(3) var<storage, read> consts: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> pool: array<u32>;
@group(0) @binding(5) var<storage, read> dyn_px: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> dyn_mask: array<f32>;
@group(0) @binding(7) var<storage, read_write> outbuf: array<u32>;

const TEXELS: u32 = 4096u;
const SLOT_WORDS: u32 = 8192u;
const SIGNED_LIMIT: f32 = 1.0e4;

// Blend modes (order of `BlendMode`).
const M_NORMAL: u32 = 0u;
const M_DISABLE: u32 = 2u;
const M_REPLACE: u32 = 3u;
const M_MULTIPLY: u32 = 4u;
const M_DIVIDE: u32 = 5u;
const M_INV_DIVIDE: u32 = 6u;
const M_DARKEN: u32 = 7u;
const M_LIGHTEN: u32 = 8u;
const M_LINEAR_DODGE: u32 = 9u;
const M_SUBTRACT: u32 = 10u;
const M_INV_SUBTRACT: u32 = 11u;
const M_DIFFERENCE: u32 = 12u;
const M_EXCLUSION: u32 = 13u;
const M_SIGNED_ADD: u32 = 14u;
const M_OVERLAY: u32 = 15u;
const M_SCREEN: u32 = 16u;
const M_LINEAR_BURN: u32 = 17u;
const M_COLOR_BURN: u32 = 18u;
const M_COLOR_DODGE: u32 = 19u;
const M_SOFT_LIGHT: u32 = 20u;
const M_HARD_LIGHT: u32 = 21u;
const M_VIVID_LIGHT: u32 = 22u;
const M_LINEAR_LIGHT: u32 = 23u;
const M_PIN_LIGHT: u32 = 24u;
const M_TINT: u32 = 25u;
const M_SATURATION: u32 = 26u;
const M_COLOR: u32 = 27u;
const M_VALUE: u32 = 28u;
const M_NM_COMBINE: u32 = 29u;
const M_NM_DETAIL: u32 = 30u;
const M_NM_INV_DETAIL: u32 = 31u;

fn is_signed() -> bool { return (P.flags & 1u) != 0u; }
fn is_normal() -> bool { return (P.flags & 2u) != 0u; }
fn is_color() -> bool { return (P.flags & 4u) != 0u; }

// ---- sources ------------------------------------------------------------------------------

fn f16_at(base: u32, e: u32) -> f32 {
    let v = unpack2x16float(pool[base + (e >> 1u)]);
    return select(v.x, v.y, (e & 1u) == 1u);
}

fn load_px(r: u32, texel: u32) -> vec4<f32> {
    let k = r >> 28u;
    let idx = r & 0xFFFFFFu;
    let comps = (r >> 24u) & 15u;
    if k == 1u {
        return consts[idx];
    }
    if k == 2u {
        let base = idx * SLOT_WORDS;
        if comps == 2u {
            let va = unpack2x16float(pool[base + texel]);
            return vec4<f32>(va.x, va.x, va.x, va.y);
        }
        let a = unpack2x16float(pool[base + texel * 2u]);
        let b = unpack2x16float(pool[base + texel * 2u + 1u]);
        return vec4<f32>(a.x, a.y, b.x, b.y);
    }
    if k == 3u {
        return dyn_px[idx * TEXELS + texel];
    }
    return vec4<f32>(0.0);
}

fn load_mask(r: u32, texel: u32) -> f32 {
    let k = r >> 28u;
    let idx = r & 0xFFFFFFu;
    let comps = (r >> 24u) & 15u;
    if k == 1u {
        return consts[idx].x;
    }
    if k == 2u {
        return clamp(f16_at(idx * SLOT_WORDS, texel * comps), 0.0, 1.0);
    }
    if k == 3u {
        return dyn_mask[idx * TEXELS + texel];
    }
    return 1.0;
}

// ---- blend math (mirrors crates/compositor/src/blend.rs) -------------------------------------

fn safe_div(a: f32, b: f32) -> f32 {
    var d = b;
    if abs(b) < 1e-6 {
        d = select(1e-6, -1e-6, b < 0.0);
    }
    return a / d;
}

fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb >= 1.0 { return 1.0; }
    if cs <= 0.0 { return 0.0; }
    return 1.0 - min((1.0 - cb) / cs, 1.0);
}

fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb <= 0.0 { return 0.0; }
    if cs >= 1.0 { return 1.0; }
    return min(cb / (1.0 - cs), 1.0);
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 { return cb * 2.0 * cs; }
    let s = 2.0 * cs - 1.0;
    return cb + s - cb * s;
}

fn soft_light(cb0: f32, cs: f32) -> f32 {
    let cb = max(cb0, 0.0);
    if cs <= 0.5 {
        return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    }
    var d: f32;
    if cb <= 0.25 {
        d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
    } else {
        d = sqrt(cb);
    }
    return cb + (2.0 * cs - 1.0) * (d - cb);
}

fn separable(mode: u32, cb: f32, cs: f32) -> f32 {
    switch mode {
        case 4u: { return cb * cs; }
        case 5u: { return safe_div(cb, cs); }
        case 6u: { return safe_div(cs, cb); }
        case 7u: { return min(cb, cs); }
        case 8u: { return max(cb, cs); }
        case 9u: { return cb + cs; }
        case 10u: { return cb - cs; }
        case 11u: { return cs - cb; }
        case 12u: { return abs(cb - cs); }
        case 13u: { return cb + cs - 2.0 * cb * cs; }
        case 14u: {
            if is_signed() { return cb + cs; }
            return cb + 2.0 * cs - 1.0;
        }
        case 15u: { return hard_light(cs, cb); }
        case 16u: { return cb + cs - cb * cs; }
        case 17u: { return cb + cs - 1.0; }
        case 18u: { return color_burn(cb, cs); }
        case 19u: { return color_dodge(cb, cs); }
        case 20u: { return soft_light(cb, cs); }
        case 21u: { return hard_light(cb, cs); }
        case 22u: {
            if cs <= 0.5 { return color_burn(cb, 2.0 * cs); }
            return color_dodge(cb, 2.0 * cs - 1.0);
        }
        case 23u: { return cb + 2.0 * cs - 1.0; }
        case 24u: {
            if cs <= 0.5 { return min(cb, 2.0 * cs); }
            return max(cb, 2.0 * cs - 1.0);
        }
        default: { return cs; }
    }
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.x, max(c.y, c.z));
    let mn = min(c.x, min(c.y, c.z));
    let d = mx - mn;
    var s = 0.0;
    if mx > 0.0 { s = d / mx; }
    var h = 0.0;
    if d > 1e-8 {
        if mx == c.x {
            let x = (c.y - c.z) / d;
            h = (x - 6.0 * floor(x / 6.0)) / 6.0;
        } else if mx == c.y {
            h = ((c.z - c.x) / d + 2.0) / 6.0;
        } else {
            h = ((c.x - c.y) / d + 4.0) / 6.0;
        }
    }
    return vec3<f32>(h, s, mx);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let h6 = (c.x - floor(c.x)) * 6.0;
    let i = floor(h6);
    let f = h6 - i;
    let v = c.z;
    let p = v * (1.0 - c.y);
    let q = v * (1.0 - c.y * f);
    let t = v * (1.0 - c.y * (1.0 - f));
    let n = i32(i);
    if n == 0 { return vec3<f32>(v, t, p); }
    if n == 1 { return vec3<f32>(q, v, p); }
    if n == 2 { return vec3<f32>(p, v, t); }
    if n == 3 { return vec3<f32>(p, q, v); }
    if n == 4 { return vec3<f32>(t, p, v); }
    return vec3<f32>(v, p, q);
}

fn decode_normal(v: vec3<f32>) -> vec3<f32> { return v * 2.0 - vec3<f32>(1.0); }

fn encode_normal(n: vec3<f32>) -> vec3<f32> {
    let l = length(n);
    var u = vec3<f32>(0.0, 0.0, 1.0);
    if l > 0.0 && l < 3.0e38 { u = n / l; }
    return u * 0.5 + vec3<f32>(0.5);
}

fn whiteout(base: vec3<f32>, detail: vec3<f32>) -> vec3<f32> {
    let a = decode_normal(base);
    let b = decode_normal(detail);
    return encode_normal(vec3<f32>(a.x + b.x, a.y + b.y, a.z * b.z));
}

fn reoriented(base: vec3<f32>, detail: vec3<f32>) -> vec3<f32> {
    let t = decode_normal(base) + vec3<f32>(0.0, 0.0, 1.0);
    let u = decode_normal(detail) * vec3<f32>(-1.0, -1.0, 1.0);
    if abs(t.z) < 1e-6 {
        return encode_normal(-u);
    }
    return encode_normal(t * dot(t, u) / t.z - u);
}

fn blend_fn(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    if mode == M_TINT {
        let b = rgb_to_hsv(cb); let s = rgb_to_hsv(cs);
        return hsv_to_rgb(vec3<f32>(s.x, b.y, b.z));
    }
    if mode == M_SATURATION {
        let b = rgb_to_hsv(cb); let s = rgb_to_hsv(cs);
        return hsv_to_rgb(vec3<f32>(b.x, s.y, b.z));
    }
    if mode == M_COLOR {
        let b = rgb_to_hsv(cb); let s = rgb_to_hsv(cs);
        return hsv_to_rgb(vec3<f32>(s.x, s.y, b.z));
    }
    if mode == M_VALUE {
        let b = rgb_to_hsv(cb); let s = rgb_to_hsv(cs);
        return hsv_to_rgb(vec3<f32>(b.x, b.y, s.z));
    }
    if mode >= M_NM_COMBINE {
        if !is_normal() { return cs; }
        if mode == M_NM_COMBINE { return whiteout(cb, cs); }
        if mode == M_NM_DETAIL { return reoriented(cb, cs); }
        return reoriented(cs, cb);
    }
    if mode == M_NORMAL || mode == M_REPLACE || mode == 1u || mode == M_DISABLE {
        return cs;
    }
    var o = vec3<f32>(separable(mode, cb.x, cs.x), separable(mode, cb.y, cs.y), separable(mode, cb.z, cs.z));
    if is_signed() {
        o = clamp(o, vec3<f32>(-SIGNED_LIMIT), vec3<f32>(SIGNED_LIMIT));
    } else {
        o = clamp(o, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return o;
}

fn composite(dst: vec4<f32>, cs: vec3<f32>, alpha: f32, mode: u32) -> vec4<f32> {
    if alpha <= 0.0 || mode == M_DISABLE {
        return dst;
    }
    let a = min(alpha, 1.0);
    let ab = dst.w;
    let cb = dst.xyz;
    var b = cs;
    if ab > 0.0 { b = blend_fn(mode, cb, cs); }
    let ao = a + ab * (1.0 - a);
    let mixed = (1.0 - ab) * cs + ab * b;
    let rgb = (a * mixed + (1.0 - a) * ab * cb) / ao;
    return vec4<f32>(rgb, ao);
}

// ---- content effects -------------------------------------------------------------------------

fn levels1(v: f32, in_low: f32, in_high: f32, gamma: f32, out_low: f32, out_high: f32) -> f32 {
    var t = clamp((v - in_low) / max(in_high - in_low, 1e-6), 0.0, 1.0);
    if t > 0.0 { t = pow(t, 1.0 / max(gamma, 1e-3)); }
    return out_low + t * (out_high - out_low);
}

fn hist_scan(v: f32, position: f32, contrast: f32, invert: bool) -> f32 {
    let width = 1.0 - contrast;
    var o: f32;
    if width <= 1e-6 {
        o = select(0.0, 1.0, v >= position);
    } else {
        o = clamp((v - (position - width * 0.5)) / width, 0.0, 1.0);
    }
    if invert { return 1.0 - o; }
    return o;
}

fn hist_range(v: f32, position: f32, range: f32, softness: f32, invert: bool) -> f32 {
    let lo = position - range * 0.5;
    let hi = position + range * 0.5;
    let ramp = softness * range * 0.5;
    var o: f32;
    if v < lo || v > hi {
        o = 0.0;
    } else if ramp <= 1e-6 {
        o = 1.0;
    } else {
        o = clamp(min((v - lo) / ramp, (hi - v) / ramp), 0.0, 1.0);
    }
    if invert { return 1.0 - o; }
    return o;
}

fn effect_value(kind: u32, px: vec3<f32>, a: u32) -> vec3<f32> {
    let p0 = bitcast<f32>(prog[a + 2u]);
    let p1 = bitcast<f32>(prog[a + 3u]);
    let p2 = bitcast<f32>(prog[a + 4u]);
    let p3 = bitcast<f32>(prog[a + 5u]);
    let p4 = bitcast<f32>(prog[a + 6u]);
    switch kind {
        case 1u: {
            return vec3<f32>(
                levels1(px.x, p0, p1, p2, p3, p4),
                levels1(px.y, p0, p1, p2, p3, p4),
                levels1(px.z, p0, p1, p2, p3, p4));
        }
        case 2u: {
            if p0 == 1.0 { return px; }
            var v = p1 + (px - vec3<f32>(p1)) * p0;
            if !is_signed() { v = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)); }
            return v;
        }
        case 3u: {
            var value = px.x;
            if is_color() { value = dot(px, vec3<f32>(0.2126, 0.7152, 0.0722)); }
            return vec3<f32>(select(0.0, 1.0, value >= p0));
        }
        case 4u: {
            return min(max(px, vec3<f32>(p0)), vec3<f32>(p1));
        }
        case 5u: {
            let inv = p2 != 0.0;
            return vec3<f32>(hist_scan(px.x, p0, p1, inv), hist_scan(px.y, p0, p1, inv), hist_scan(px.z, p0, p1, inv));
        }
        default: {
            let inv = p3 != 0.0;
            return vec3<f32>(hist_range(px.x, p0, p1, p2, inv), hist_range(px.y, p0, p1, p2, inv), hist_range(px.z, p0, p1, p2, inv));
        }
    }
}

fn run_effects(px_in: vec4<f32>, first: u32, count: u32) -> vec4<f32> {
    var px = px_in;
    for (var i = 0u; i < count; i++) {
        let a = P.eff_off + (first + i) * 8u;
        let w0 = prog[a];
        let kind = w0 & 255u;
        let mode = (w0 >> 8u) & 255u;
        let opacity = bitcast<f32>(prog[a + 1u]);
        let v = effect_value(kind, px.xyz, a);
        let d = composite(vec4<f32>(px.xyz, 1.0), v, opacity, mode);
        px = vec4<f32>(d.xyz, px.w);
    }
    return px;
}

@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let texel = wg.x * 64u + lid.x;
    let tile = wg.y;
    var acc = P.default_value;
    var stack: array<vec4<f32>, 4>;
    var sp = 0u;
    let rbase = P.refs_off + tile * P.nops * 2u;
    for (var i = 0u; i < P.nops; i++) {
        let o = P.ops_off + i * 4u;
        let w0 = prog[o];
        let kind = w0 & 255u;
        let mode = (w0 >> 8u) & 255u;
        let passthrough = ((w0 >> 16u) & 1u) == 1u;
        let opacity = bitcast<f32>(prog[o + 1u]);
        let ef = prog[o + 2u];
        let en = prog[o + 3u];
        if kind == 0u {
            var px = load_px(refs[rbase + i * 2u], texel);
            px = run_effects(px, ef, en);
            let m = load_mask(refs[rbase + i * 2u + 1u], texel);
            let a = select(px.w, 1.0, mode == M_REPLACE) * opacity * m;
            acc = composite(acc, px.xyz, a, mode);
        } else if kind == 1u {
            stack[sp] = acc;
            sp += 1u;
            if !passthrough { acc = vec4<f32>(0.0); }
        } else {
            sp -= 1u;
            let base = stack[sp];
            let inner = run_effects(acc, ef, en);
            let m = load_mask(refs[rbase + i * 2u + 1u], texel);
            if passthrough {
                let f = clamp(opacity * m, 0.0, 1.0);
                acc = base + (inner - base) * f;
            } else {
                let a = select(inner.w, 1.0, mode == M_REPLACE) * opacity * m;
                acc = composite(base, inner.xyz, a, mode);
            }
        }
    }
    let o = P.out_off + (tile * TEXELS + texel) * 2u;
    outbuf[o] = pack2x16float(acc.xy);
    outbuf[o + 1u] = pack2x16float(acc.zw);
}
