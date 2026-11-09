// A read of the gradient LUT atlas. The includer binds `palantir_gradient_tex` and
// `palantir_gradient_sampler`, in whichever group its layout gives them.

// A ramp parameter `t` in 0..1 as the texture coordinate of a LUT row
// `width` texels wide. The bake puts texel `i` at `t = i / (width - 1)`,
// so `t = 0` and `t = 1` must land on the first and last texel centers
// and everything between on the matching point between two centers.
fn palantir_lut_u(t: f32, width: f32) -> f32 {
    return (t * (width - 1.0) + 0.5) / width;
}

// LUT row `row` at ramp parameter `t` in 0..1, straight. Row count is
// queried, not baked in as a const: the atlas grows when one frame registers
// more distinct gradients than it holds, and a query keeps the pipeline valid
// across that resize. The LUT has one mip, so the level is 0, taken
// explicitly: a caller's branch on a flat instance lane is not uniform
// control flow.
fn palantir_gradient_lut(t: f32, row: u32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(palantir_gradient_tex));
    let v = (f32(row) + 0.5) / dims.y;
    return palantir_unpremultiply(textureSampleLevel(palantir_gradient_tex, palantir_gradient_sampler, vec2<f32>(palantir_lut_u(t, dims.x), v), 0.0));
}
