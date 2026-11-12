// Tone-mapping functions shared by the raster viewport and the path-traced display
// (prepended to both shaders). The CPU reference is `grade::tone_map`.
// Modes: 0 ACES, 1 Filmic, 2 Reinhard, 3 Linear, 5 LinearSat, 6 Sensitometric, 7 ReinhardLum,
// 8 Log, 9 LogLum. `factor` is the Mapping Factor of the Reinhard and Log functions.

fn tm_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn tm_hable(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15; let b = 0.50; let c = 0.10; let d = 0.20; let e = 0.02; let f = 0.30;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn tm_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn tm_sat(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn tonemap(color: vec3<f32>, mode: i32, factor: f32) -> vec3<f32> {
    let f = max(factor, 1e-3);
    switch mode {
        case 1: {
            let white = tm_hable(vec3<f32>(11.2));
            return tm_sat(tm_hable(color * 2.0) / white);
        }
        case 2: { return tm_sat(color * f / (color * f + vec3<f32>(1.0))); }
        case 3, 5: { return tm_sat(color); }
        case 6: { return tm_sat(vec3<f32>(1.0) - exp(-max(color, vec3<f32>(0.0)))); }
        case 7: {
            let l = tm_luma(color);
            return tm_sat(color * ((l * f / (l * f + 1.0)) / max(l, 1e-6)));
        }
        case 8: { return tm_sat(log(vec3<f32>(1.0) + max(color, vec3<f32>(0.0)) * f) / log(1.0 + f)); }
        case 9: {
            let l = tm_luma(color);
            return tm_sat(color * ((log(1.0 + max(l, 0.0) * f) / log(1.0 + f)) / max(l, 1e-6)));
        }
        default: { return tm_aces(color); }
    }
}

