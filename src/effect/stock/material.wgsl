// The look the stock frosted glass and glass share.

// Rec. 709 luminance weights, over linear light.
const MATERIAL_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// `c` saturated as CSS `saturate()` scales it, and grained, so frosted glass does not read as a
// flat color, each channel floored at 0, below which the glass's spectrum and a saturation past 1
// reach. The saturation is `c + (c − luma)·(s − 1)`, not `mix(luma, c, s)`, so `s = 1` returns a
// channel at or above 0 to the bit, in whatever form a driver evaluates `mix`: `Material::blur`
// shows the capture through it.
fn material_frost(c: vec3<f32>, input: EffectInput, saturation: f32, noise: f32) -> vec3<f32> {
    let luma = vec3<f32>(dot(c, MATERIAL_LUMA));
    var out = max(c + (c - luma) * (saturation - 1.0), vec3<f32>(0.0));
    if (noise > 0.0) {
        out = mix(out, vec3<f32>(material_grain(input.pos * input.scale)), noise);
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
