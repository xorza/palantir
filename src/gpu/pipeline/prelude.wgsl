// Shared WGSL prelude. `ShaderBody::specialize` concatenates it
// ahead of every shader in this backend, so everything here has to
// compile in front of every one of them — nothing may declare a
// binding, which is the one thing they disagree about.

// The whole immediate region: the viewport size, which the backend
// pushes after every pipeline bind. Every pipeline layout declares its
// size, `IMMEDIATES_BYTES`.
//
// **Flat members, no nested structs**, should it ever grow: HLSL
// constant-buffer rules start a *struct* member on the next 16-byte
// register, past the root constants the layout declares, where Dx12
// reads it back as zero. Vectors pack tightly inside one register.
struct Immediates {
    viewport_size: vec2<f32>,
};
var<immediate> imm: Immediates;

// See `AA_HALF_WIDTH` in `primitives::paint::antialias`: the half-width of
// the box filter every edge is antialiased with.
const AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

// Coverage, at a pixel centred `d` outside an edge (negative inside), of
// the side the edge bounds: zero from `AA_HALF_WIDTH` out, full from
// `AA_HALF_WIDTH` in.
fn edge_coverage(d: f32) -> f32 {
    return clamp((AA_HALF_WIDTH - d) / (2.0 * AA_HALF_WIDTH), 0.0, 1.0);
}

// Coverage, at a pixel centred `r` from a band's centre line, of the band
// reaching `core_half` to either side: the edge ramp, capped at the share
// of the filter the band's whole width fills, since a band narrower than
// the filter can never cover more of a pixel than its own width.
fn band_coverage(core_half: f32, r: f32) -> f32 {
    let filter_width = 2.0 * AA_HALF_WIDTH;
    let plateau = clamp(2.0 * core_half / filter_width, 0.0, 1.0);
    return clamp((core_half + AA_HALF_WIDTH - r) / filter_width, 0.0, plateau);
}

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

// A ramp parameter `t` in 0..1 as the texture coordinate of a LUT row
// `width` texels wide. The bake puts texel `i` at `t = i / (width - 1)`,
// so `t = 0` and `t = 1` must land on the first and last texel centres
// and everything between on the matching point between two centres.
fn lut_u(t: f32, width: f32) -> f32 {
    return (t * (width - 1.0) + 0.5) / width;
}

// Premultiplied in, straight out — for a colour that was interpolated
// premultiplied (a gradient texel, a vertex colour) and must meet a
// straight-alpha multiply. A fully transparent colour has no hue; it
// comes back black.
fn unpremultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(select(vec3<f32>(0.0), c.rgb / c.a, c.a > 0.0), c.a);
}

// sRGB-encoded channels → linear light: the exact piecewise transfer
// function of IEC 61966-2-1, the inverse of what the GPU applies at every
// sRGB write, so a colour authored as sRGB bytes reaches the screen as
// those bytes. The CPU decodes with the same function.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}
