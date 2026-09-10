// Shared WGSL prelude. `shader_template::specialize` concatenates it
// ahead of every shader in this backend, so everything here has to
// compile in front of all five pipelines — nothing may declare a
// binding, which is the one thing they disagree about.

// The whole immediate region. Every pipeline declares the same size
// (`IMMEDIATES_BYTES`), which is what keeps the bytes one pipeline
// writes valid after a switch to another:
//   offset 0: viewport size, written once per pass by the backend.
//   offset 8: atlas sizes (color, mask), written per text batch by
//   `TextBackend::render_batch` and read by the raster-atlas shader alone.
//
// **Flat members, no nested structs.** HLSL constant-buffer rules start a
// *struct* member on the next 16-byte register, so a nested
// `struct Immediates { viewport: Viewport, params: Params }` put `params`
// at offset 16 on Dx12 — past the four root constants the layout declares.
// It read back as zero, `uv_texel / 0` sent every glyph's UV to infinity,
// and text vanished on Dx12 while every other pipeline was fine. Vectors
// pack tightly inside one register.
struct Immediates {
    viewport_size: vec2<f32>,
    atlas_px: vec2<u32>,
};
var<immediate> imm: Immediates;

// Rec. 709 luma. Both readers weigh colours that have already been
// decoded to linear, which is the space these coefficients are defined in.
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// Unit-quad corners in triangle-strip order, indexed by `vertex_index`.
const CORNERS = array<vec2<f32>, 4>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
);

// Physical pixels to a clip-space position.
//
// The y lane flips because the two spaces disagree about direction: the
// pixel grid runs down from the top-left, clip space up from the centre.
// The viewport is read here rather than passed in, so two shaders cannot
// answer this against different ones.
fn clip_from_px(px: vec2<f32>) -> vec4<f32> {
    let ndc = px * (vec2<f32>(2.0, -2.0) / imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

// Straight alpha in, premultiplied out — the contract every fragment
// entry point in this backend writes under, because the blend state is
// `PREMULTIPLIED_ALPHA_BLENDING`. See AGENTS.md "Colour pipeline".
fn premultiply(rgb: vec3<f32>, alpha: f32) -> vec4<f32> {
    return vec4<f32>(rgb * alpha, alpha);
}
