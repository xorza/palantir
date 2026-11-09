// The gradient LUT atlas, included by every shader that binds the gradient
// group: rows of baked 256-texel gradients, sampled at fragment time.
// `Rgba16Float` holds premultiplied linear RGB, so the filter between two
// texels blends premultiplied, and f16 keeps dark gradients band-free.
@group(0) @binding(0) var palantir_gradient_tex:     texture_2d<f32>;
@group(0) @binding(1) var palantir_gradient_sampler: sampler;
