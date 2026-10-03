// Gradient LUT atlas: rows of baked 256-texel gradients, sampled at
// fragment time for `Brush::Linear`. Format is `Rgba16Float` storing
// straight-alpha linear-RGB, so the sampler returns linear directly on
// read (f16 precision keeps dark gradients band-free); matches the rest
// of the pipeline.
@group(0) @binding(0) var gradient_tex:     texture_2d<f32>;
@group(0) @binding(1) var gradient_sampler: sampler;

// Divide-by-zero guard on object-local axes (quad size, gradient
// span, radial radius). Anything smaller than this rounds to "no
// meaningful direction" — the gradient collapses to a fallback.
const ZERO_EPS: f32 = 1e-6;

// Cutoff below which `blurred_rect_coverage` short-circuits to the
// sharp SDF coverage. Below this, the erf-based Gaussian is
// numerically indistinguishable from `clamp(AA_RADIUS - d, 0, 1)` but
// risks divide-by-near-zero in `1/(√2 σ)`.
const BLUR_EPS: f32 = 1e-4;

// Half-width of the SDF antialiasing transition in physical pixels.
const AA_RADIUS: f32 = /*{AA_RADIUS}*/;

// Brush kind low byte:
//   0 = solid  (use `fill` directly)
//   1 = linear (sample LUT via `fill_axis = (dir.xy, t0, t1)`)
//   2 = radial (sample LUT via `fill_axis = (cx, cy, rx, ry)`)
//   3 = conic  (sample LUT via `fill_axis = (cx, cy, start_angle, _)`)
const BRUSH_KIND_SOLID:        u32 = /*{BRUSH_KIND_SOLID}*/;
const BRUSH_KIND_LINEAR:       u32 = /*{BRUSH_KIND_LINEAR}*/;
const BRUSH_KIND_RADIAL:       u32 = /*{BRUSH_KIND_RADIAL}*/;
const BRUSH_KIND_CONIC:        u32 = /*{BRUSH_KIND_CONIC}*/;
// Bit 16 of fill_kind: fragment fast path. The composer sets it on a
// solid, sharp, stroke-less quad whose rect is pixel-aligned — every
// rasterized fragment is interior (SDF coverage exactly 1.0), so `fs`
// returns the premultiplied fill directly. Kept in lockstep with
// `FillKind::FAST_BIT` on the CPU side.
const FILL_FLAG_FAST: u32 = /*{FILL_FLAG_FAST}*/;
// Bit 17 of fill_kind: windowed rect — inverted fill coverage. The fill
// paints *outside* the rounded boundary (the corner wedges, out to the
// quad edge), the stroke keeps its usual inner-edge annulus, and the
// window interior stays transparent. Cheap stand-in for rounded-corner
// scissor clipping: draw content as plain rects, then paint this over
// it with the surrounding background as `fill`. Only meaningful for the
// rect path (kinds 0..3 — solid + gradients). Kept in lockstep with
// `FillKind::WINDOW_BIT` on the CPU side.
const FILL_FLAG_WINDOW: u32 = /*{FILL_FLAG_WINDOW}*/;
// Drop/inset shadow: closed-form Gaussian-blurred rounded rect.
// `fill` is the shadow colour, `radius` is the source rect's corner
// radii, `size` is the paint bbox.
//   - Drop:  paint bbox = (source + offset).inflated(3σ + max(spread, 0)).
//            Offset and positive spread are baked into the paint bbox;
//            `fill_axis = (0, 0, sigma, spread)` preserves signed spread.
//   - Inset: paint bbox = source. Spread shrinks the "hole" rect
//            inside the shader via
//            `fill_axis = (offset.x, offset.y, sigma, spread)`.
const BRUSH_KIND_SHADOW_DROP:  u32 = /*{BRUSH_KIND_SHADOW_DROP}*/;
const BRUSH_KIND_SHADOW_INSET: u32 = /*{BRUSH_KIND_SHADOW_INSET}*/;
// Rounded-triangle SDF. `fill` is the solid fill; the three corner points
// ride the reused instance lanes — `radius.xy = a`, `radius.zw = b`,
// `fill_axis.xy = c` — stored as unorm16 shares of the quad's size, which
// `vs` turns into `local` (0..size) coords; `fill_axis.z` is the corner
// radius, f16 like every other lane. Stroke uses the usual
// `stroke_color`/`stroke_width`.
const BRUSH_KIND_TRIANGLE:     u32 = /*{BRUSH_KIND_TRIANGLE}*/;
// Spread mode (bits 8..16 of fill_kind), only meaningful for gradients.
// `Pad` is the fallback below rather than a constant of its own.
const SPREAD_REPEAT:  u32 = /*{SPREAD_REPEAT}*/;
const SPREAD_REFLECT: u32 = /*{SPREAD_REFLECT}*/;
const TAU: f32 = 6.2831853;

struct VertexOut {
    @builtin(position) clip:         vec4<f32>,
    @location(0)       local:        vec2<f32>,
    // Everything below is per-instance: identical at all four
    // vertices. `flat` skips plane-equation setup + per-fragment
    // interpolation and avoids f32 drift across large quads.
    @location(1) @interpolate(flat) size:         vec2<f32>,
    @location(2) @interpolate(flat) fill:         vec4<f32>,
    @location(3) @interpolate(flat) radius:       vec4<f32>,
    @location(4) @interpolate(flat) stroke_color: vec4<f32>,
    @location(5) @interpolate(flat) stroke_width: f32,
    @location(6) @interpolate(flat) fill_kind:    u32,
    @location(7) @interpolate(flat) fill_lut_row: u32,
    @location(8) @interpolate(flat) fill_axis:    vec4<f32>,
    // Precomputed `1.0 / max(size, ZERO_EPS)` so `eval_fill`'s gradient
    // path multiplies per-fragment instead of dividing (solid fills and the
    // shadow / triangle paths don't read it).
    @location(9) @interpolate(flat) inv_size:     vec2<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) pos:          vec2<f32>,
    @location(1) size:         vec2<f32>,
    @location(2) fill_packed:  vec2<u32>,
    @location(3) radius_packed: vec2<u32>,
    @location(4) stroke_color_packed: vec2<u32>,
    @location(5) stroke_width: f32,
    @location(6) fill_kind:    u32,
    @location(7) fill_lut_row: u32,
    @location(8) fill_axis_packed: vec2<u32>,
) -> VertexOut {
    // Unpack 4x f16 (tl, tr, br, bl) — matches `Corners` lane order.
    let r_lo = unpack2x16float(radius_packed.x);
    let r_hi = unpack2x16float(radius_packed.y);
    let radius = vec4<f32>(r_lo.x, r_lo.y, r_hi.x, r_hi.y);
    // Same pattern for fill_axis — variant-dependent lane layout
    // documented at the top of this file.
    let fa_lo = unpack2x16float(fill_axis_packed.x);
    let fa_hi = unpack2x16float(fill_axis_packed.y);
    let fill_axis = vec4<f32>(fa_lo.x, fa_lo.y, fa_hi.x, fa_hi.y);
    // Same pattern again for the two fill colours (linear-RGB).
    let f_lo = unpack2x16float(fill_packed.x);
    let f_hi = unpack2x16float(fill_packed.y);
    let fill = vec4<f32>(f_lo.x, f_lo.y, f_hi.x, f_hi.y);
    let s_lo = unpack2x16float(stroke_color_packed.x);
    let s_hi = unpack2x16float(stroke_color_packed.y);
    let stroke_color = vec4<f32>(s_lo.x, s_lo.y, s_hi.x, s_hi.y);
    let local = CORNERS[vi] * size;

    var corner_lanes = radius;
    var axis_lanes = fill_axis;
    if ((fill_kind & 0xFFu) == BRUSH_KIND_TRIANGLE) {
        // Corner points as unorm16 shares of the quad: an f16 lane steps
        // a whole pixel above 1024 px, a share of the quad steps
        // `size / 65535` at any size.
        corner_lanes = vec4<f32>(
            unpack2x16unorm(radius_packed.x) * size,
            unpack2x16unorm(radius_packed.y) * size,
        );
        axis_lanes = vec4<f32>(unpack2x16unorm(fill_axis_packed.x) * size, fa_hi);
    }

    var out: VertexOut;
    out.clip         = clip_from_px(pos + local);
    out.local        = local;
    out.size         = size;
    out.fill         = fill;
    out.radius       = corner_lanes;
    out.stroke_color = stroke_color;
    out.stroke_width = stroke_width;
    out.fill_kind    = fill_kind;
    out.fill_lut_row = fill_lut_row;
    out.fill_axis    = axis_lanes;
    out.inv_size     = 1.0 / max(size, vec2<f32>(ZERO_EPS));
    return out;
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

// CSS `box-shadow` spread on each radius: `max(r + s, 0)` for `s < 0`;
// for `s ≥ 0`, `r + s` when `r ≥ s` and `r + s·(1 + (r/s − 1)³)` below, so
// a sharp corner stays sharp. The shader's copy of the rule in
// `Corners::fit_to`, for the inset hole, whose box only the shader sizes.
// At `s = 0` the cubic arm divides by the floor and goes non-finite, and
// the `r ≥ s` arm is the one selected.
fn spread_radius(r: vec4<f32>, s: f32) -> vec4<f32> {
    if (s < 0.0) {
        return max(r + vec4<f32>(s), vec4<f32>(0.0));
    }
    let t = r / max(s, 1e-30) - vec4<f32>(1.0);
    return select(r + s * (vec4<f32>(1.0) + t * t * t), r + vec4<f32>(s), r >= vec4<f32>(s));
}

// CSS "overlapping curves": scale every radius `(tl, tr, br, bl)` by the
// least of each side's length over the sum of its two radii, when that is
// below 1, so no radius exceeds what the box of half-extents `half` holds.
// The shader's copy of `Corners::fit_to`'s fit.
fn fit_radii(r: vec4<f32>, half: vec2<f32>) -> vec4<f32> {
    let size = 2.0 * half;
    let sums = vec4<f32>(r.x + r.y, r.w + r.z, r.x + r.w, r.y + r.z);
    let sides = vec4<f32>(size.x, size.x, size.y, size.y);
    let ratios = select(vec4<f32>(1.0), sides / max(sums, vec4<f32>(1e-30)), sums > vec4<f32>(0.0));
    let f = min(1.0, min(min(ratios.x, ratios.y), min(ratios.z, ratios.w)));
    return r * f;
}

// Corner-origin convenience: `p` measured from the top-left of a rect
// of `size`. Forwards to the centered form.
fn sdf_rounded_rect(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    return sdf_rounded_box_centered(p - half, half, radius);
}

// Signed distance to the triangle (a, b, c) — negative inside, positive
// outside (Inigo Quilez's `sdTriangle`). `s` folds in the winding sign so
// the result is correctly signed for either orientation; subtracting a
// radius from the caller rounds all three corners uniformly.
//
// A degenerate triangle — collinear corners, or coincident ones — has no
// inside. Its winding sign is 0, so `d.y` is 0 everywhere, and the
// original `sign(d.y)` made the distance 0 everywhere: a radius then
// filled the whole quad. Here `d.y == 0` reads as outside, which on a
// proper triangle happens only on an edge, where the distance is 0
// either way. A zero-length edge divides 0 by 0 in its projection; the
// floor keeps that finite, and the edge's zero vector then makes the
// projection's value irrelevant.
fn sdf_triangle(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a; let e1 = c - b; let e2 = a - c;
    let v0 = p - a; let v1 = p - b; let v2 = p - c;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / max(dot(e0, e0), 1e-30), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / max(dot(e1, e1), 1e-30), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / max(dot(e2, e2), 1e-30), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(min(
        vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
        vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
        vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)));
    let dist = sqrt(d.x);
    return select(dist, -dist, d.y > 0.0);
}

// Apply the user-selected spread mode to a parametric `t`. The `Pad`
// clamp is kept here rather than left to the sampler's clamp
// addressing, so the contract holds whatever the sampler is set to.
fn apply_spread(t: f32, mode: u32) -> f32 {
    if (mode == SPREAD_REPEAT) {
        return fract(t);
    }
    if (mode == SPREAD_REFLECT) {
        return abs(fract(t * 0.5) - 0.5) * 2.0;
    }
    // Pad, and the safe answer for a mode this shader was not told
    // about: clamping keeps `t` inside the LUT row either way.
    return clamp(t, 0.0, 1.0);
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
    let local01 = in.local * in.inv_size;
    var t01: f32 = 0.0;
    if (kind == BRUSH_KIND_LINEAR) {
        // Linear: project local01 onto the gradient direction, remap
        // (raw - t0) / (t1 - t0) → 0..1.
        let axis = in.fill_axis.xy;
        let t0   = in.fill_axis.z;
        let t1   = in.fill_axis.w;
        let raw  = dot(local01, axis);
        let span = t1 - t0;
        let span_safe = select(1.0, span, abs(span) > ZERO_EPS);
        t01 = (raw - t0) / span_safe;
    } else if (kind == BRUSH_KIND_RADIAL) {
        // Radial: distance from `center` measured in `radius` units.
        // `t = 1.0` at the elliptical edge of the radius vector.
        let center = in.fill_axis.xy;
        let radius = in.fill_axis.zw;
        let rx = select(1.0, radius.x, abs(radius.x) > ZERO_EPS);
        let ry = select(1.0, radius.y, abs(radius.y) > ZERO_EPS);
        let d  = (local01 - center) / vec2<f32>(rx, ry);
        t01 = length(d);
    } else if (kind == BRUSH_KIND_CONIC) {
        // Conic: sweep around `center`, starting at `start_angle`
        // (radians, CCW). atan2 returns -π..π; the +1.0 then fract
        // wraps to 0..1 in a single step regardless of sign.
        let center      = in.fill_axis.xy;
        let start_angle = in.fill_axis.z;
        let p           = local01 - center;
        let theta       = atan2(p.y, p.x);
        t01 = fract((theta - start_angle) / TAU + 1.0);
    } else {
        // Unknown brush kind: fall back to solid fill rather than
        // silently sampling the LUT with garbage `t`.
        return in.fill;
    }
    let t = apply_spread(t01, spread);
    // Row count is queried, not baked in as a const: the atlas texture
    // grows when one frame registers more distinct gradients than it
    // holds, and a query keeps this pipeline valid across that resize.
    let dims = vec2<f32>(textureDimensions(gradient_tex));
    let v = (f32(in.fill_lut_row) + 0.5) / dims.y;
    let c = textureSample(gradient_tex, gradient_sampler, vec2<f32>(lut_u(t, dims.x), v));
    // Texels are premultiplied, so the filter between two of them is too.
    // `in.fill` multiplies the straight sample, channel by channel. It is
    // white for every gradient, with its alpha scaled by a paint
    // animation's fade. See `BrushSource::gpu_fill`.
    return unpremultiply(c) * in.fill;
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
// the result is `clamp(AA_RADIUS - d, 0, 1)` — same shape as the existing
// non-blurred SDF coverage, so the path collapses cleanly to a sharp
// shadow. For σ > 0 we use the SDF distance as the input to an erf,
// which is exact for an axis-aligned half-plane and a smooth
// approximation for a rounded rect (the same trick Evan Wallace's
// shader uses).
fn blurred_rect_coverage(d: f32, sigma: f32) -> f32 {
    if (sigma <= BLUR_EPS) {
        return clamp(AA_RADIUS - d, 0.0, 1.0);
    }
    // d < 0 inside the shape → coverage ≈ 1; d > 0 outside → 0.
    // `erf(-d / (√2 σ))` smoothly transitions, mapped to 0..1.
    let inv = 1.0 / (1.41421356 * sigma);
    return 0.5 - 0.5 * erf_approx(d * inv);
}

// Composite an SDF shape's fill + inner-edge stroke into premultiplied linear
// RGBA, given the signed distance `d` (negative inside). `outer_aa =
// clamp(AA_RADIUS - d)` is the coverage. With a stroke, the stroke covers the annulus
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
    let outer_aa = clamp(AA_RADIUS - d, 0.0, 1.0);
    if (stroke_width > 0.0) {
        let inner_aa = clamp(AA_RADIUS - (d + stroke_width), 0.0, 1.0);
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
    let outer_aa = clamp(AA_RADIUS - d, 0.0, 1.0);
    let fill_a = (1.0 - outer_aa) * fill.a;
    if (stroke_width > 0.0) {
        let inner_aa = clamp(AA_RADIUS - (d + stroke_width), 0.0, 1.0);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        return premultiply(stroke_color.rgb, stroke_a) + premultiply(fill.rgb, fill_a);
    }
    return premultiply(fill.rgb, fill_a);
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    // Uniform per instance (`fill_kind` is flat), so whole wavefronts
    // inside one quad take a single side of this branch.
    if ((in.fill_kind & FILL_FLAG_FAST) != 0u) {
        return premultiply(in.fill.rgb, in.fill.a);
    }
    let kind = in.fill_kind & 0xFFu;
    if (kind == BRUSH_KIND_SHADOW_DROP) {
        let sigma  = in.fill_axis.z;
        let spread = in.fill_axis.w;
        let half   = in.size * 0.5;
        let source_half = half - vec2<f32>(3.0 * sigma + max(spread, 0.0));
        let shadow_half = max(source_half + vec2<f32>(spread), vec2<f32>(0.0));
        let p = in.local - half;
        let d = sdf_rounded_box_centered(p, shadow_half, in.radius);
        let cov = blurred_rect_coverage(d, sigma);
        let a = in.fill.a * cov;
        return premultiply(in.fill.rgb, a);
    }
    if (kind == BRUSH_KIND_SHADOW_INSET) {
        // Inset shadow: source rect S equals the paint bbox. The
        // "hole" is S deflated by `spread` per side, then shifted by
        // `offset` (the light source moves in that direction → shadow
        // grows opposite). Coverage at fragment p inside S =
        // `1 - blurred_cov(p relative to hole)` — outside the hole
        // but still inside S is where the shadow paints; deep inside
        // the hole is lit (cov→0).
        let offset = in.fill_axis.xy;
        let sigma  = in.fill_axis.z;
        let spread = in.fill_axis.w;
        let half   = in.size * 0.5;
        // Clip to inside the source — inset never paints outside.
        let p_src = in.local - half;
        let d_src = sdf_rounded_box_centered(p_src, half, in.radius);
        if (d_src > 0.0) {
            return vec4<f32>(0.0);
        }
        let hole_half = max(half - vec2<f32>(spread), vec2<f32>(0.0));
        // The hole is the source shrunk by `spread`, so its radii follow
        // the CSS spread rule with `-spread` — less by `spread`, floored
        // at 0, or grown the way a drop shadow's are when the spread is
        // negative — then fit the hole's own box.
        let hole_radius = fit_radii(spread_radius(in.radius, -spread), hole_half);
        let p_hole = in.local - half - offset;
        let d_hole = sdf_rounded_box_centered(p_hole, hole_half, hole_radius);
        let cov_hole = blurred_rect_coverage(d_hole, sigma);
        let cov = clamp(1.0 - cov_hole, 0.0, 1.0);
        let a = in.fill.a * cov;
        return premultiply(in.fill.rgb, a);
    }

    if (kind == BRUSH_KIND_TRIANGLE) {
        // Three corner points (in `local` 0..size coords) + corner radius ride
        // the reused instance lanes. `sdf_triangle - radius` gives the rounded
        // shape; `composite` applies the same coverage AA + inner stroke as the
        // rounded-rect path, so a triangle gets crisp AA + rounded corners with
        // no MSAA and no tessellation. Solid fill only (no gradient lanes).
        let ta = in.radius.xy;
        let tb = in.radius.zw;
        let tc = in.fill_axis.xy;
        let corner_r = in.fill_axis.z;
        let td = sdf_triangle(in.local, ta, tb, tc) - corner_r;
        return composite(td, in.fill, in.stroke_color, in.stroke_width);
    }

    let d = sdf_rounded_rect(in.local, in.size, in.radius);
    if ((in.fill_kind & FILL_FLAG_WINDOW) != 0u) {
        return composite_window(d, eval_fill(in), in.stroke_color, in.stroke_width);
    }
    return composite(d, eval_fill(in), in.stroke_color, in.stroke_width);
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
