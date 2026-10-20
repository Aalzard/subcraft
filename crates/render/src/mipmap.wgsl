@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(p[i], 0., 1.);
}

// Area-average in linear space. Odd dimensions include the last source texel instead of
// discarding it. Encoded normal vectors deliberately retain their averaged length.
@fragment
fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let size = textureDimensions(source);
    let dst = max(size / 2u, vec2(1u));
    let lo = floor(p.xy) * vec2<f32>(size) / vec2<f32>(dst);
    let hi = (floor(p.xy) + 1.) * vec2<f32>(size) / vec2<f32>(dst);
    var sum = vec4(0.);
    for (var y = i32(floor(lo.y)); y < i32(ceil(hi.y)); y++) {
        for (var x = i32(floor(lo.x)); x < i32(ceil(hi.x)); x++) {
            let weight = min(hi, vec2<f32>(f32(x + 1), f32(y + 1))) - max(lo, vec2<f32>(f32(x), f32(y)));
            sum += textureLoad(source, vec2(x, y), 0) * weight.x * weight.y;
        }
    }
    return sum / ((hi.x - lo.x) * (hi.y - lo.y));
}
