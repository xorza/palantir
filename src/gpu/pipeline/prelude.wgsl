// Every pipeline's device has dual-source blending (`DeviceRequirements::FEATURES`), which a pane's
// effect stage writes through; an enable stands before every declaration.
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
const AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

// Coverage, at a pixel centered `d` outside an edge (negative inside), of
// the side the edge bounds: zero from `AA_HALF_WIDTH` out, full from
// `AA_HALF_WIDTH` in.
fn edge_coverage(d: f32) -> f32 {
    return clamp((AA_HALF_WIDTH - d) / (2.0 * AA_HALF_WIDTH), 0.0, 1.0);
}

// Coverage, at a pixel centered `r` from a band's center line, of the band
// reaching `core_half` to either side: the edge ramp, capped at the share
// of the filter the band's whole width fills, since a band narrower than
// the filter can never cover more of a pixel than its own width.
fn band_coverage(core_half: f32, r: f32) -> f32 {
    let filter_width = 2.0 * AA_HALF_WIDTH;
    let plateau = clamp(2.0 * core_half / filter_width, 0.0, 1.0);
    return clamp((core_half + AA_HALF_WIDTH - r) / filter_width, 0.0, plateau);
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

// A ramp parameter `t` in 0..1 as the texture coordinate of a LUT row
// `width` texels wide. The bake puts texel `i` at `t = i / (width - 1)`,
// so `t = 0` and `t = 1` must land on the first and last texel centers
// and everything between on the matching point between two centers.
fn lut_u(t: f32, width: f32) -> f32 {
    return (t * (width - 1.0) + 0.5) / width;
}

// Premultiplied in, straight out — for a color that was interpolated
// premultiplied (a gradient texel, a vertex color) and must meet a
// straight-alpha multiply. A fully transparent color has no hue; it
// comes back black.
fn unpremultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(select(vec3<f32>(0.0), c.rgb / c.a, c.a > 0.0), c.a);
}

// sRGB-encoded channels → linear light: the exact piecewise transfer
// function of IEC 61966-2-1, the inverse of what the GPU applies at every
// sRGB write, so a color authored as sRGB bytes reaches the screen as
// those bytes. The CPU decodes with the same function.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// Linear light → sRGB-encoded channels: the inverse of `srgb_to_linear`, the transfer the GPU
// applies at an sRGB write.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
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


// Rounded-rect SDF centered at the origin: half-extents `b`,
// per-corner radius `r = (tl, tr, br, bl)`. Quadrant-select picks the
// corner radius by sign of `p` so each corner can differ.
fn sdf_rounded_box_centered(p: vec2<f32>, b: vec2<f32>, radius: vec4<f32>) -> f32 {
    let right  = step(0.0, p.x);
    let bottom = step(0.0, p.y);
    let r = mix(mix(radius.x, radius.y, right),
                mix(radius.w, radius.z, right),
                bottom);
    let q = abs(p) - (b - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}


// Corner-origin convenience: `p` measured from the top-left of a rect
// of `size`. Forwards to the centered form.
fn sdf_rounded_rect(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    return sdf_rounded_box_centered(p - half, half, radius);
}


// RGBA, given the signed distance `d` (negative inside). `outer_aa =
// edge_coverage(d)` is the coverage. With a stroke, the stroke covers the annulus
// between the outer edge and the edge inset by `stroke_width`, and the fill
// covers the interior inside that inset. The two are *spatially disjoint*
// within any pixel (stroke = `outer_aa - inner_aa`, fill = `inner_aa`), so they
// sum additively in premultiplied space — the coverages partition and add back
// to `outer_aa`. Compositing stroke OVER fill instead (`a = stroke_a +
// fill_a*(1-stroke_a)`) dips total alpha to ~0.75 where the two AA bands cross
// at ~0.5 each, showing a 1px seam of background bleeding between stroke and
// fill at fractional zoom; summing keeps total coverage at `outer_aa`. Shared
// by the rounded-rect and triangle paths so they can't drift.
fn composite(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = edge_coverage(d);
    if (stroke_width > 0.0) {
        let inner_aa = edge_coverage(d + stroke_width);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        let fill_a   = inner_aa * fill.a;
        return premultiply(stroke_color.rgb, stroke_a) + premultiply(fill.rgb, fill_a);
    }
    return premultiply(fill.rgb, fill.a * outer_aa);
}

// Inverted-fill counterpart of `composite` for `FILL_FLAG_WINDOW`: the
// stroke covers the same inner-edge annulus, but the fill paints the
// complement of the rounded shape (`1 - outer_aa`) — the corner wedges
// out to the quad edge — and the window interior is transparent. The
// three coverages (fill, stroke, window) partition each pixel exactly,
// so fill + stroke sum additively in premultiplied space with no seam,
// same rationale as `composite`. The quad edge itself is a hard cut
// (no outward AA): the shape is a mask laid exactly over content of
// the same extent, so its outer boundary is never a visible edge.
fn composite_window(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = edge_coverage(d);
    let fill_a = (1.0 - outer_aa) * fill.a;
    if (stroke_width > 0.0) {
        let inner_aa = edge_coverage(d + stroke_width);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        return premultiply(stroke_color.rgb, stroke_a) + premultiply(fill.rgb, fill_a);
    }
    return premultiply(fill.rgb, fill_a);
}
