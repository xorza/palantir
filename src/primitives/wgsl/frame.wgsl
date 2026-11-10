// The frame group, which every color pipeline and every effect binds at `PALANTIR_FRAME_GROUP`:
// the gradient LUT atlas.

const PALANTIR_FRAME_GROUP: u32 = /*{FRAME_GROUP}*/;

// Rows of baked 256-texel gradients, sampled at fragment time. `Rgba16Float` holds premultiplied
// linear RGB, so the filter between two texels blends premultiplied, and f16 keeps dark gradients
// band-free.
@group(PALANTIR_FRAME_GROUP) @binding(0) var palantir_gradient_tex: texture_2d<f32>;
@group(PALANTIR_FRAME_GROUP) @binding(1) var palantir_gradient_sampler: sampler;
