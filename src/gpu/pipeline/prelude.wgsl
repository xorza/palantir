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

// RGBA, given the signed distance `d` (negative inside). `outer_aa =
// palantir_edge_coverage(d)` is the coverage. With a border, the border covers the annulus
// between the outer edge and the edge inset by `border_width`, and the fill
// covers the interior inside that inset. The two are *spatially disjoint*
// within any pixel (border = `outer_aa - inner_aa`, fill = `inner_aa`), so they
// sum additively in premultiplied space — the coverages partition and add back
// to `outer_aa`. Compositing border OVER fill instead (`a = border_a +
// fill_a*(1-border_a)`) dips total alpha to ~0.75 where the two AA bands cross
// at ~0.5 each, showing a 1px seam of background bleeding between border and
// fill at fractional zoom; summing keeps total coverage at `outer_aa`. Shared
// by the rounded-rect and triangle paths so they can't drift.
fn composite(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    if (border_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + border_width);
        let border_a = (outer_aa - inner_aa) * border_color.a;
        let fill_a   = inner_aa * fill.a;
        return premultiply(border_color.rgb, border_a) + premultiply(fill.rgb, fill_a);
    }
    return premultiply(fill.rgb, fill.a * outer_aa);
}

// Inverted-fill counterpart of `composite` for `FILL_FLAG_OUTSIDE`: the
// border covers the same inner-edge annulus, but the fill paints the
// complement of the rounded shape (`1 - outer_aa`) — the corner wedges
// out to the quad edge — and the interior is transparent. The
// three coverages (fill, border, interior) partition each pixel exactly,
// so fill + border sum additively in premultiplied space with no seam,
// same rationale as `composite`. The quad edge itself is a hard cut
// (no outward AA): the shape is a mask laid exactly over content of
// the same extent, so its outer boundary is never a visible edge.
fn composite_outside(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    let fill_a = (1.0 - outer_aa) * fill.a;
    if (border_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + border_width);
        let border_a = (outer_aa - inner_aa) * border_color.a;
        return premultiply(border_color.rgb, border_a) + premultiply(fill.rgb, fill_a);
    }
    return premultiply(fill.rgb, fill_a);
}
