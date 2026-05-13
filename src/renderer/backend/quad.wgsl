struct Viewport {
    size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;
// Gradient LUT atlas: rows of baked 256-texel gradients, sampled at
// fragment time for `Brush::Linear`. Format is sRGB so the sampler
// returns linear-RGB on read; matches the rest of the pipeline.
@group(0) @binding(1) var gradient_tex:     texture_2d<f32>;
@group(0) @binding(2) var gradient_sampler: sampler;

const ATLAS_ROWS_F: f32 = 256.0;

// Brush kind low byte:
//   0 = solid  (use `fill` directly)
//   1 = linear (sample LUT via `fill_axis = (dir.xy, t0, t1)`)
//   2 = radial (sample LUT via `fill_axis = (cx, cy, rx, ry)`)
//   3 = conic  (sample LUT via `fill_axis = (cx, cy, start_angle, _)`)
const BRUSH_KIND_SOLID:        u32 = 0u;
const BRUSH_KIND_LINEAR:       u32 = 1u;
const BRUSH_KIND_RADIAL:       u32 = 2u;
const BRUSH_KIND_CONIC:        u32 = 3u;
// Drop/inset shadow: closed-form Gaussian-blurred rounded rect.
// `fill_axis = (offset.x, offset.y, sigma, _)` in physical px.
// `fill` is the shadow colour, `radius` is the source rect's corner
// radii, `size` is the paint bbox (source.inflated(|offset| + 3σ +
// spread)). Spread is baked into the paint bbox at encode time, so
// the shader doesn't need it explicitly.
const BRUSH_KIND_SHADOW_DROP:  u32 = 4u;
const BRUSH_KIND_SHADOW_INSET: u32 = 5u;
// Spread mode (bits 8..16 of fill_kind), only meaningful for gradients.
const SPREAD_PAD:     u32 = 0u;
const SPREAD_REPEAT:  u32 = 1u;
const SPREAD_REFLECT: u32 = 2u;
const TAU: f32 = 6.2831853;

struct VertexOut {
    @builtin(position) clip:         vec4<f32>,
    @location(0)       local:        vec2<f32>,
    @location(1)       size:         vec2<f32>,
    @location(2)       fill:         vec4<f32>,
    @location(3)       radius:       vec4<f32>,
    @location(4)       stroke_color: vec4<f32>,
    @location(5)       stroke_width: f32,
    // `@interpolate(flat)` — brush metadata is per-instance, the
    // same value at all four vertices; interpolating wastes vertex
    // output bandwidth without affecting the fragment-stage value.
    @location(6) @interpolate(flat) fill_kind:    u32,
    @location(7) @interpolate(flat) fill_lut_row: u32,
    @location(8) @interpolate(flat) fill_axis:    vec4<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) pos:          vec2<f32>,
    @location(1) size:         vec2<f32>,
    @location(2) fill:         vec4<f32>,
    @location(3) radius:       vec4<f32>,
    @location(4) stroke_color: vec4<f32>,
    @location(5) stroke_width: f32,
    @location(6) fill_kind:    u32,
    @location(7) fill_lut_row: u32,
    @location(8) fill_axis:    vec4<f32>,
) -> VertexOut {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let pixel = pos + c * size;
    let clip = vec2<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
    );

    var out: VertexOut;
    out.clip         = vec4<f32>(clip, 0.0, 1.0);
    out.local        = c * size;
    out.size         = size;
    out.fill         = fill;
    out.radius       = radius;
    out.stroke_color = stroke_color;
    out.stroke_width = stroke_width;
    out.fill_kind    = fill_kind;
    out.fill_lut_row = fill_lut_row;
    out.fill_axis    = fill_axis;
    return out;
}

// Per-corner SDF rounded rect. radius = (tl, tr, br, bl).
fn sdf_rounded_rect(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    let right  = step(half.x, p.x);
    let bottom = step(half.y, p.y);
    let r = mix(mix(radius.x, radius.y, right),
                mix(radius.w, radius.z, right),
                bottom);
    let q = abs(p - half) - (half - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

// Apply the user-selected spread mode to a parametric `t`. `Pad` clamps
// to 0..1 (sampler clamp-addressing would also do this, but doing it
// here keeps the contract explicit). `Repeat` wraps. `Reflect` mirrors.
fn apply_spread(t: f32, mode: u32) -> f32 {
    switch mode {
        case 1u: { return fract(t); }                       // Repeat
        case 2u: { return abs(fract(t * 0.5) - 0.5) * 2.0; } // Reflect
        default: { return clamp(t, 0.0, 1.0); }              // Pad
    }
}

// Resolve the fill colour at a given fragment. Solid path returns
// `in.fill` verbatim — byte-identical to the pre-brush behaviour.
// Linear path projects `in.local` onto `fill_axis.xy` (object-local
// 0..1 axis), maps to 0..1 via `(t0, t1)`, applies spread, samples
// the LUT row at `fill_lut_row`.
fn eval_fill(in: VertexOut) -> vec4<f32> {
    let kind = in.fill_kind & 0xFFu;
    if (kind == BRUSH_KIND_SOLID) {
        return in.fill;
    }
    let spread  = (in.fill_kind >> 8u) & 0xFFu;
    let local01 = in.local / in.size;
    var t01: f32;
    if (kind == BRUSH_KIND_LINEAR) {
        // Linear: project local01 onto the gradient direction, remap
        // (raw - t0) / (t1 - t0) → 0..1.
        let axis = in.fill_axis.xy;
        let t0   = in.fill_axis.z;
        let t1   = in.fill_axis.w;
        let raw  = dot(local01, axis);
        let span = t1 - t0;
        t01 = select(0.0, (raw - t0) / span, abs(span) > 1e-6);
    } else if (kind == BRUSH_KIND_RADIAL) {
        // Radial: distance from `center` measured in `radius` units.
        // `t = 1.0` at the elliptical edge of the radius vector.
        let center = in.fill_axis.xy;
        let radius = in.fill_axis.zw;
        let rx = select(1.0, radius.x, abs(radius.x) > 1e-6);
        let ry = select(1.0, radius.y, abs(radius.y) > 1e-6);
        let d  = (local01 - center) / vec2<f32>(rx, ry);
        t01 = length(d);
    } else {
        // Conic: sweep around `center`, starting at `start_angle`
        // (radians, CCW). atan2 returns -π..π; the +1.0 then fract
        // wraps to 0..1 in a single step regardless of sign.
        let center      = in.fill_axis.xy;
        let start_angle = in.fill_axis.z;
        let p           = local01 - center;
        let theta       = atan2(p.y, p.x);
        t01 = fract((theta - start_angle) / TAU + 1.0);
    }
    let t = apply_spread(t01, spread);
    let v = (f32(in.fill_lut_row) + 0.5) / ATLAS_ROWS_F;
    return textureSample(gradient_tex, gradient_sampler, vec2<f32>(t, v));
}

// Rounded-box SDF centered at the origin: source half-extents `b`,
// per-corner radius `r = (tl, tr, br, bl)`. Same quadrant selection
// scheme as `sdf_rounded_rect` but operates in centered coords —
// matches the source-shape frame the shadow integrates over.
fn sdf_rounded_box_centered(p: vec2<f32>, b: vec2<f32>, radius: vec4<f32>) -> f32 {
    let right  = step(0.0, p.x);
    let bottom = step(0.0, p.y);
    let r = mix(mix(radius.x, radius.y, right),
                mix(radius.w, radius.z, right),
                bottom);
    let q = abs(p) - (b - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

// `erf` approximation (Abramowitz & Stegun 7.1.26 form, max error
// ~1.5e-7). WGSL doesn't ship `erf` as a builtin.
fn erf_approx(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * a);
    let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * exp(-a * a);
    return s * y;
}

// Closed-form coverage of a Gaussian-blurred rounded box. For σ → 0
// the result is `clamp(0.5 - d, 0, 1)` — same shape as the existing
// non-blurred SDF coverage, so the path collapses cleanly to a sharp
// shadow. For σ > 0 we use the SDF distance as the input to an erf,
// which is exact for an axis-aligned half-plane and a smooth
// approximation for a rounded rect (the same trick Evan Wallace's
// shader uses; see `references/vello.md` §3).
fn blurred_rect_coverage(d: f32, sigma: f32) -> f32 {
    if (sigma <= 1.0e-4) {
        return clamp(0.5 - d, 0.0, 1.0);
    }
    // d < 0 inside the shape → coverage ≈ 1; d > 0 outside → 0.
    // `erf(-d / (√2 σ))` smoothly transitions, mapped to 0..1.
    let inv = 1.0 / (1.41421356 * sigma);
    return 0.5 - 0.5 * erf_approx(d * inv);
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let kind = in.fill_kind & 0xFFu;
    if (kind == BRUSH_KIND_SHADOW_DROP) {
        // Paint bbox covers (source + offset).inflated(3σ + spread).
        // Source center (in paint-local) = paint_size/2 - offset.
        // Source half = paint_size/2 - |offset| - 3σ (spread baked in).
        let offset = in.fill_axis.xy;
        let sigma  = in.fill_axis.z;
        let half   = in.size * 0.5;
        let src_half = max(half - abs(offset) - vec2<f32>(3.0 * sigma), vec2<f32>(0.0));
        let p = in.local - half - offset;
        let d = sdf_rounded_box_centered(p, src_half, in.radius);
        let cov = blurred_rect_coverage(d, sigma);
        let a = in.fill.a * cov;
        return vec4<f32>(in.fill.rgb * a, a);
    }
    if (kind == BRUSH_KIND_SHADOW_INSET) {
        // Inset: paint bbox = source. Shadow lives inside the source
        // rect, clipped to inside. Coverage is 1 - blurred(source - hole),
        // where the hole is the source shifted by offset (deflated by
        // spread, already baked in source half). Source is paint bbox
        // itself; hole rect = source shifted by offset, evaluated as
        // the SDF of the source minus a translated copy.
        let offset = in.fill_axis.xy;
        let sigma  = in.fill_axis.z;
        let half   = in.size * 0.5;
        // Outer source SDF (paint = source).
        let p_src = in.local - half;
        let d_src = sdf_rounded_box_centered(p_src, half, in.radius);
        if (d_src > 0.0) {
            // Outside the source — inset shadows never paint outside.
            return vec4<f32>(0.0);
        }
        // Inner "hole" — the source shifted by offset and 3σ-inflated.
        // Inset coverage = (1 - hole coverage) inside source.
        let p_hole = in.local - half - offset;
        let d_hole = sdf_rounded_box_centered(p_hole, half, in.radius);
        let cov_hole = blurred_rect_coverage(d_hole, sigma);
        let cov = clamp(1.0 - cov_hole, 0.0, 1.0);
        let a = in.fill.a * cov;
        return vec4<f32>(in.fill.rgb * a, a);
    }

    let d = sdf_rounded_rect(in.local, in.size, in.radius);
    let outer_aa = clamp(0.5 - d, 0.0, 1.0);

    let fill_rgba = eval_fill(in);

    if (in.stroke_width > 0.0) {
        // Stroke sits on the inner edge: fill region is everything inside the rect
        // shifted inward by stroke_width.
        let inner_d  = d + in.stroke_width;
        let inner_aa = clamp(0.5 - inner_d, 0.0, 1.0);
        let stroke_a = (outer_aa - inner_aa) * in.stroke_color.a;
        let fill_a   = inner_aa * fill_rgba.a;
        let rgb = fill_rgba.rgb * fill_a + in.stroke_color.rgb * stroke_a;
        let a   = fill_a + stroke_a - fill_a * stroke_a;
        return vec4<f32>(rgb, a);
    }
    let a = fill_rgba.a * outer_aa;
    return vec4<f32>(fill_rgba.rgb * a, a);
}

// Stencil mask-write: `discard` outside the rounded shape so those
// pixels skip the post-fragment stencil op (Replace) entirely, leaving
// stencil at 0 outside the rounded region. The color write_mask is
// empty in the mask pipeline, so the returned vec4 is dropped — only
// the stencil side effect matters. Hard threshold at SDF = 0 (no AA on
// the mask edge): the panel's painted rounded background already AA's
// the visible boundary; the stencil mask just controls which children
// pixels survive, and a 1-pixel hard inner edge sits behind the AA rim
// where it's invisible.
@fragment
fn fs_mask(in: VertexOut) -> @location(0) vec4<f32> {
    let d = sdf_rounded_rect(in.local, in.size, in.radius);
    if (d > 0.0) {
        discard;
    }
    return vec4<f32>(0.0);
}
