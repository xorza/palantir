// The gradient LUT atlas, included by every shader that binds the gradient
// group: rows of baked 256-texel gradients, sampled at fragment time.
// `Rgba16Float` holds premultiplied linear RGB, so the filter between two
// texels blends premultiplied, and f16 keeps dark gradients band-free.
@group(0) @binding(0) var gradient_tex:     texture_2d<f32>;
@group(0) @binding(1) var gradient_sampler: sampler;

// LUT row `row` at ramp parameter `t` in 0..1, straight. Row count is
// queried, not baked in as a const: the atlas grows when one frame registers
// more distinct gradients than it holds, and a query keeps the pipeline valid
// across that resize. The LUT has one mip, so the level is 0, taken
// explicitly: a caller's branch on a flat instance lane is not uniform
// control flow.
fn gradient_lut(t: f32, row: u32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(gradient_tex));
    let v = (f32(row) + 0.5) / dims.y;
    return unpremultiply(textureSampleLevel(gradient_tex, gradient_sampler, vec2<f32>(lut_u(t, dims.x), v), 0.0));
}

