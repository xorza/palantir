// Every pipeline's device has dual-source blending (`DeviceRequirements::FEATURES`), which a
// backdrop's effect stage writes through; an enable stands before every declaration.
enable dual_source_blending;

// Shared WGSL prelude. `ShaderBody::specialize` concatenates it
// ahead of every shader in this backend, so everything here has to
// compile in front of every one of them — nothing may declare a
// binding, which is the one thing they disagree about.

// The whole immediate region: the viewport size and the fragment origin,
// which the backend pushes after every pipeline bind. Every pipeline
// layout declares its size, `IMMEDIATES_BYTES`.
struct Immediates {
    viewport_size: vec2<f32>,
    // The surface pixel the target's pixel 0 is: zero, but on a damage
    // canvas, which holds a rect of the surface.
    frag_origin: vec2<f32>,
};
var<immediate> imm: Immediates;

// See `AA_HALF_WIDTH` in `primitives::paint::antialias`: the half-width of
// the box filter every edge is antialiased with.
const PALANTIR_AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

// Coverage, at a pixel centered `r` from a band's center line, of the band
// reaching `core_half` to either side: the edge ramp, capped at the share
// of the filter the band's whole width fills, since a band narrower than
// the filter can never cover more of a pixel than its own width.
fn band_coverage(core_half: f32, r: f32) -> f32 {
    let filter_width = 2.0 * PALANTIR_AA_HALF_WIDTH;
    let plateau = clamp(2.0 * core_half / filter_width, 0.0, 1.0);
    return clamp((core_half + PALANTIR_AA_HALF_WIDTH - r) / filter_width, 0.0, plateau);
}

// Rec. 709 luma. Both readers weigh colors that have already been
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
// pixel grid runs down from the top-left, clip space up from the center.
// The viewport is read here rather than passed in, so two shaders cannot
// answer this against different ones.
fn clip_from_px(px: vec2<f32>) -> vec4<f32> {
    let ndc = px * (vec2<f32>(2.0, -2.0) / imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

// A fragment's position in the surface's physical px, whatever target it
// paints into: `@builtin(position)` counts from the target's pixel 0.
fn frag_px(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy + imm.frag_origin;
}

// Straight alpha in, premultiplied out — the contract every fragment
// entry point in this backend writes under, because the blend state is
// `PREMULTIPLIED_ALPHA_BLENDING`.
fn premultiply(rgb: vec3<f32>, alpha: f32) -> vec4<f32> {
    return vec4<f32>(rgb * alpha, alpha);
}

// `p` turned `rotation` radians about `center`. Exact at no turn, so an
// unturned quad draws where it always has.
fn turned(p: vec2<f32>, center: vec2<f32>, rotation: f32) -> vec2<f32> {
    if (rotation == 0.0) {
        return p;
    }
    let cs = vec2<f32>(cos(rotation), sin(rotation));
    let v = p - center;
    return center + vec2<f32>(cs.x * v.x - cs.y * v.y, cs.y * v.x + cs.x * v.y);
}

