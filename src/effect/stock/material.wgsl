// What the stock materials read, palantir's `MaterialInput`, and the look they share.

struct Params {
    saturation: f32,
    noise: f32,
    bezel: f32,
    thickness: f32,
    refractive_index: f32,
    dispersion: f32,
    highlight: f32,
}

// Rec. 709 luminance weights, over linear light.
const MATERIAL_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// `c` saturated as CSS `saturate()` scales it, and grained, so frosted glass does not read as a
// flat color.
fn material_frost(c: vec3<f32>, input: EffectInput, params: Params) -> vec3<f32> {
    var out = max(mix(vec3<f32>(dot(c, MATERIAL_LUMA)), c, params.saturation), vec3<f32>(0.0));
    if (params.noise > 0.0) {
        out = mix(out, vec3<f32>(material_grain(input.pos * input.scale)), params.noise);
    }
    return out;
}

// A grain value in `0..1` for the whole physical pixel of the material's own frame at `local`,
// so the grain moves and turns with it.
fn material_grain(local: vec2<f32>) -> f32 {
    let cell = vec2<u32>(vec2<i32>(floor(local)) + vec2<i32>(32768));
    var h = cell.x * 0x9E3779B1u ^ (cell.y * 0x85EBCA77u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return f32(h >> 8u) / 16777216.0;
}
