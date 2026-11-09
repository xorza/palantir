// Color transfers every palantir shader shares: sRGB encoding and premultiplied alpha.

// Premultiplied in, straight out — for a color that was interpolated
// premultiplied (a gradient texel, a vertex color) and must meet a
// straight-alpha multiply. A fully transparent color has no hue; it
// comes back black.
fn palantir_unpremultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(select(vec3<f32>(0.0), c.rgb / c.a, c.a > 0.0), c.a);
}

// sRGB-encoded channels → linear light: the exact piecewise transfer
// function of IEC 61966-2-1, the inverse of what the GPU applies at every
// sRGB write, so a color authored as sRGB bytes reaches the screen as
// those bytes. The CPU decodes with the same function.
fn palantir_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// Linear light → sRGB-encoded channels: the inverse of `palantir_srgb_to_linear`, the transfer the GPU
// applies at an sRGB write.
fn palantir_linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}
