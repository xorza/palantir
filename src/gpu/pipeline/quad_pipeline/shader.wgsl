// Gradient LUT atlas: rows of baked 256-texel gradients, sampled at
// fragment time for the gradient brushes. Format is `Rgba16Float` storing
// premultiplied linear RGB, so the filter between two texels blends
// premultiplied and `eval_fill` unpremultiplies the sample (f16 precision
// keeps dark gradients band-free).
@group(0) @binding(0) var gradient_tex:     texture_2d<f32>;
@group(0) @binding(1) var gradient_sampler: sampler;

// Divide-by-zero guard on object-local axes (quad size, gradient
// span, radial radius). Anything smaller than this rounds to "no
// meaningful direction" — the gradient collapses to a fallback.
const ZERO_EPS: f32 = 1e-6;

// σ below which `filter_cdf` takes its σ = 0 limit, the bare pixel box.
// The two differ by about σ there, far under an 8-bit step, and the
// limit keeps `1/σ` out of the arithmetic.
const BLUR_EPS: f32 = 1e-4;

// How far a shadow's Gaussian is followed, in σ (`ShadowGeom::REACH_SIGMAS`):
// a drop shadow's quad reaches this far past its source, beside the
// positive spread, and a blurred corner is sliced only this far past the
// pixel box.
const SHADOW_REACH_SIGMAS: f32 = /*{SHADOW_REACH_SIGMAS}*/;

// Slices per half of a blurred corner arc. Midpoint slices with exact
// weights err as 1/N²: 12 keep a blur below `CUTOUT_MIN_SIGMA`, the only
// one sliced this way, within 0.0015 of the exact integral, under half an
// 8-bit step.
const BLUR_ARC_SLICES: u32 = 12u;

// σ from which a blurred box's corners are cut out of its sharp box
// (`cutout_box_coverage`) instead of integrated along their outline
// (`blurred_corner`). Measured against the exact integral, the cutout's
// `CUTOUT_NODES` err at most 0.0011 from here up, where the outline's
// slices reach 0.0023, at half the kernel evaluations. Below it the
// cutout's density loses to the pixel box's step (0.0026 at σ = 0, against
// the outline's 0.0015), so small blurs keep the outline.
const CUTOUT_MIN_SIGMA: f32 = /*{CUTOUT_MIN_SIGMA}*/;

// Midpoint nodes of a corner cutout's angle integral, where it is shaded
// (`CutoutPlan::SHADED_NODES`, which prices shading against a table).
const CUTOUT_NODES: u32 = /*{CUTOUT_NODES}*/;

// Midpoint nodes of a corner cutout baked into a table (`fs_cutout_bake`):
// twice `CUTOUT_NODES`, so the bake errs about a quarter of the shaded
// form, 3e-4, beside the table's bilinear error of under 6e-4.
const CUTOUT_BAKE_NODES: u32 = /*{CUTOUT_BAKE_NODES}*/;

// A cutout table's texels per σ, both axes (`CutoutPlan`). At 10 its
// bilinear error stays under 6e-4 of the exact cutout (measured at
// σ = 0.25–16 against radii 4–40), under the shaded form's 0.0011.
const CUTOUT_TEXELS_PER_SIGMA: f32 = /*{CUTOUT_TEXELS_PER_SIGMA}*/;

// Side of the square cutout atlas in texels, and of the cell a table's
// origin is a multiple of, both from `CutoutPlan`.
const CUTOUT_ATLAS_SIZE: f32 = /*{CUTOUT_ATLAS_SIZE}*/;
const CUTOUT_CELL: u32 = /*{CUTOUT_CELL}*/;

// A corner with no table: its cutout is shaded (`corner_cutout`).
const NO_CUTOUT_TABLE: u32 = /*{NO_CUTOUT_TABLE}*/;

// Baked corner cutouts, one square table per `(r, σ)`, read by
// `textureLoad` and filtered by hand: `R32Float` is not filterable on
// every backend. Bound to the shadow pipeline only.
@group(1) @binding(0) var cutout_atlas: texture_2d<f32>;

// Whether a shadow skips the part of its quad that paints nothing and
// takes the cheaper forms where the cutoff at `reach` makes them exact
// (`vs_shadow`, `shadow_coverage`), or draws one quad of the full form
// everywhere: the
// reference the two are tested against, built only by the crate's
// internals.
override SHADOW_GRID: bool = true;

// Where `shadow_coverage` takes no cheaper form: a bound no point reaches.
const SHADOW_NO_SPAN: f32 = 1e30;

// A cell's two triangles, as indices into `CORNERS`.
const CELL_TRIANGLES = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);

const SQRT_HALF: f32 = 0.70710678;
const INV_SQRT_TAU: f32 = 0.39894228;

// A drop shadow clips where its source's own coverage is within this of
// full: one 8-bit step, so what the clip takes from under an opaque fill
// never reaches a stored value, while a pixel centre that sits exactly on
// the threshold clips the same way under the float noise that the
// interpolated `local` carries.
const SHADOW_CLIP_EPS: f32 = 1.0 / 255.0;

// `fill_kind`'s fields: the family tag in the `FILL_TAG_MASK` bits and
// the spread mode `SPREAD_SHIFT` up. Each flag bit is a constant below.
const FILL_TAG_MASK: u32 = /*{FILL_TAG_MASK}*/;
const SPREAD_SHIFT: u32 = /*{SPREAD_SHIFT}*/;
const SPREAD_MASK: u32 = /*{SPREAD_MASK}*/;

// Brush kind tag:
//   solid  (use `fill` directly)
//   linear (sample LUT via `fill_axis = (dir.xy, t0, t1)`)
//   radial (sample LUT via `fill_axis = (cx, cy, rx, ry)`)
//   conic  (sample LUT via `fill_axis = (cx, cy, start_angle, _)`)
const BRUSH_KIND_SOLID:        u32 = /*{BRUSH_KIND_SOLID}*/;
const BRUSH_KIND_LINEAR:       u32 = /*{BRUSH_KIND_LINEAR}*/;
const BRUSH_KIND_RADIAL:       u32 = /*{BRUSH_KIND_RADIAL}*/;
const BRUSH_KIND_CONIC:        u32 = /*{BRUSH_KIND_CONIC}*/;
// Fragment fast path (`FillKind::FAST_BIT`). The composer sets it on a
// solid, sharp, stroke-less quad whose rect is pixel-aligned — every
// rasterized fragment is interior (SDF coverage exactly 1.0), so `fs`
// returns the premultiplied fill directly.
const FILL_FLAG_FAST: u32 = /*{FILL_FLAG_FAST}*/;
// Windowed rect (`FillKind::WINDOW_BIT`) — inverted fill coverage. The fill
// paints *outside* the rounded boundary (the corner wedges, out to the
// quad edge), the stroke keeps its usual inner-edge annulus, and the
// window interior stays transparent. Cheap stand-in for rounded-corner
// scissor clipping: draw content as plain rects, then paint this over
// it with the surrounding background as `fill`. Only meaningful for the
// rect path (solid and the gradients).
const FILL_FLAG_WINDOW: u32 = /*{FILL_FLAG_WINDOW}*/;
// Drop/inset shadow: closed-form Gaussian-blurred rounded rect.
// `fill` is the shadow colour, `radius` is the source rect's corner
// radii, `size` is the paint bbox, and
// `fill_axis = (offset.x, offset.y, sigma, spread)` for both.
//   - Drop:  paint bbox = (source + offset).inflated(SHADOW_REACH_SIGMAS·σ + max(spread, 0)).
//   - Inset: paint bbox = source. Spread shrinks the "hole" rect
//            inside the shader.
const BRUSH_KIND_SHADOW_DROP:  u32 = /*{BRUSH_KIND_SHADOW_DROP}*/;
const BRUSH_KIND_SHADOW_INSET: u32 = /*{BRUSH_KIND_SHADOW_INSET}*/;
// Rounded-triangle SDF. `fill` is the solid fill; the three corner points
// ride the reused instance lanes — `radius.xy = a`, `radius.zw = b`,
// `fill_axis.xy = c` — stored as unorm16 shares of the quad's size, which
// `vs` turns into `local` (0..size) coords; `fill_axis.z` is the corner
// radius, f16 like every other lane. Stroke uses the usual
// `stroke_color`/`stroke_width`.
const BRUSH_KIND_TRIANGLE:     u32 = /*{BRUSH_KIND_TRIANGLE}*/;
// Spread mode, only meaningful for gradients.
// `Pad` is the fallback below rather than a constant of its own.
const SPREAD_REPEAT:  u32 = /*{SPREAD_REPEAT}*/;
const SPREAD_REFLECT: u32 = /*{SPREAD_REFLECT}*/;
const TAU: f32 = 6.2831853;

struct VertexOut {
    @builtin(position) clip:         vec4<f32>,
    @location(0)       local:        vec2<f32>,
    // Everything below is per-instance: identical at every vertex.
    // `flat` skips plane-equation setup + per-fragment interpolation and
    // avoids f32 drift across large quads.
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
    // A shadow's baked cutout tables, `(tl, tr, br, bl)` like `radius`: each
    // the table's origin cell and side (`cutout_lookup`), or
    // `NO_CUTOUT_TABLE`. Set by `vs_shadow` only.
    @location(10) @interpolate(flat) cutouts:     vec4<u32>,
    // A shadow's box less `r + reach` on each side, `(lo.x, lo.y, hi.x,
    // hi.y)` relative to the box's centre: on an axis strictly inside it,
    // the pair of edges across that axis is constant (`shadow_coverage`).
    // Set by `vs_shadow` only.
    @location(11) @interpolate(flat) shadow_core: vec4<f32>,
    // The quad's top-left in pixels. `fs_shadow` takes its `local` as the
    // pixel centre less this, which is exact and the same however the
    // quad was cut into cells, where an interpolated `local` carries the
    // rounding of whichever triangle covered the pixel.
    @location(12) @interpolate(flat) origin:      vec2<f32>,
};

// The instance attributes every quad pipeline reads.
struct QuadIn {
    @location(0) pos:          vec2<f32>,
    @location(1) size:         vec2<f32>,
    @location(2) fill_packed:  vec2<u32>,
    @location(3) radius_packed: vec2<u32>,
    @location(4) stroke_color_packed: vec2<u32>,
    @location(5) stroke_width: f32,
    @location(6) fill_kind:    u32,
    @location(7) fill_lut_row: u32,
    @location(8) fill_axis_packed: vec2<u32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, quad: QuadIn) -> VertexOut {
    return quad_vertex(vi, quad);
}

// `vs` for the shadow pipeline, which also binds the per-instance cutout
// tables (`CutoutTables`). A shadow draws as the eight cells of a 3×3
// grid around a hole that paints nothing, six vertices each
// (`QuadPipeline::draw_shadows`).
//
// A drop shadow's hole is its source inset by its largest radius and
// `AA_HALF_WIDTH`: at every pixel centre there the source SDF is at most
// `−AA_HALF_WIDTH`, so `fs_shadow`'s clip returns zero, and skipping it
// changes no pixel. An inset shadow's hole is its core (`shadow_core`),
// where the blurred box covers everything and the shadow is zero by its
// form. A hole that is empty, or a reference build (`SHADOW_GRID` off),
// puts all four inner lines on the quad's far corner, which leaves one
// cell: the whole quad.
//
// The inner lines are clamped into the quad, and to their midpoint when
// they cross, so the lines are always in order and the cells tile the quad
// less the hole. Every vertex is a pair of these lines, so two cells that
// share an edge share its vertices exactly and the rasterizer shades each
// pixel once.
@vertex
fn vs_shadow(
    @builtin(vertex_index) vi: u32,
    quad: QuadIn,
    @location(9) cutouts: vec4<u32>,
) -> VertexOut {
    var out = quad_vertex(0u, quad);
    let bounds = shaded_bounds(quad.pos, quad.size, quad.fill_kind);
    let half = quad.size * 0.5;
    let centre = quad.pos + half;
    let kind = quad.fill_kind & FILL_TAG_MASK;
    let sb = shadow_box(kind, half, out.radius, out.fill_axis);
    let reach = SHADOW_REACH_SIGMAS * out.fill_axis.z + AA_HALF_WIDTH;
    let r = sb.radius;
    let core_lo = -sb.half + vec2<f32>(max(r.x, r.w), max(r.x, r.y)) + vec2<f32>(reach);
    let core_hi = sb.half - vec2<f32>(max(r.y, r.z), max(r.w, r.z)) - vec2<f32>(reach);

    var hole_lo = centre + sb.centre + core_lo;
    var hole_hi = centre + sb.centre + core_hi;
    if (kind == BRUSH_KIND_SHADOW_DROP) {
        let source_half = shadow_source_half(half, out.fill_axis);
        let inset = max(max(out.radius.x, out.radius.y), max(out.radius.z, out.radius.w)) + AA_HALF_WIDTH;
        let source_centre = centre - out.fill_axis.xy;
        hole_lo = source_centre - source_half + vec2<f32>(inset);
        hole_hi = source_centre + source_half - vec2<f32>(inset);
    }
    if (!SHADOW_GRID || any(hole_lo >= hole_hi)) {
        hole_lo = bounds.hi;
        hole_hi = bounds.hi;
    }
    var inner_lo = clamp(hole_lo, bounds.lo, bounds.hi);
    var inner_hi = clamp(hole_hi, bounds.lo, bounds.hi);
    let crossed = inner_lo > inner_hi;
    let mid = (inner_lo + inner_hi) * 0.5;
    inner_lo = select(inner_lo, mid, crossed);
    inner_hi = select(inner_hi, mid, crossed);
    var xs = array<f32, 4>(bounds.lo.x, inner_lo.x, inner_hi.x, bounds.hi.x);
    var ys = array<f32, 4>(bounds.lo.y, inner_lo.y, inner_hi.y, bounds.hi.y);

    let cell = vi / 6u;
    let cx = cell % 3u;
    let cy = cell / 3u;
    let unit = CORNERS[CELL_TRIANGLES[vi % 6u]];
    var at = vec2<f32>(xs[cx + u32(unit.x)], ys[cy + u32(unit.y)]);
    if (cx == 1u && cy == 1u) {
        // The hole: a cell of zero area.
        at = inner_lo;
    }
    out.clip = clip_from_px(at);
    out.local = at - quad.pos;
    out.cutouts = cutouts;
    out.shadow_core = select(
        vec4<f32>(SHADOW_NO_SPAN, SHADOW_NO_SPAN, -SHADOW_NO_SPAN, -SHADOW_NO_SPAN),
        vec4<f32>(core_lo, core_hi),
        SHADOW_GRID,
    );
    return out;
}

// The pixel-centre extent the quad shader shades for a quad: its rect
// grown to every pixel centre within `AA_HALF_WIDTH` of it, out to whole
// pixels, since its coverage reaches that far and the rasterizer only
// shades a pixel whose centre is inside what it draws. A rect on pixel
// boundaries does not grow. A windowed rect is a mask over content of its
// own extent and paints its fill outside the shape, so it stays at its
// rect. `Quad::shaded_rect` is the same extent on the CPU.
struct Bounds {
    lo: vec2<f32>,
    hi: vec2<f32>,
};

fn shaded_bounds(pos: vec2<f32>, size: vec2<f32>, fill_kind: u32) -> Bounds {
    var b = Bounds(pos, pos + size);
    if ((fill_kind & FILL_FLAG_WINDOW) == 0u) {
        b.lo = floor(b.lo - vec2<f32>(AA_HALF_WIDTH - 0.5));
        b.hi = ceil(b.hi + vec2<f32>(AA_HALF_WIDTH - 0.5));
    }
    return b;
}

fn quad_vertex(vi: u32, quad: QuadIn) -> VertexOut {
    let pos = quad.pos;
    let size = quad.size;
    let fill_packed = quad.fill_packed;
    let radius_packed = quad.radius_packed;
    let stroke_color_packed = quad.stroke_color_packed;
    let stroke_width = quad.stroke_width;
    let fill_kind = quad.fill_kind;
    let fill_lut_row = quad.fill_lut_row;
    let fill_axis_packed = quad.fill_axis_packed;
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
    let bounds = shaded_bounds(pos, size, fill_kind);
    let corner = select(bounds.lo, bounds.hi, CORNERS[vi] > vec2<f32>(0.5));
    let local = corner - pos;

    var corner_lanes = radius;
    var axis_lanes = fill_axis;
    if ((fill_kind & FILL_TAG_MASK) == BRUSH_KIND_TRIANGLE) {
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
    out.clip         = clip_from_px(corner);
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
    out.cutouts      = vec4<u32>(NO_CUTOUT_TABLE);
    out.shadow_core  = vec4<f32>(SHADOW_NO_SPAN, SHADOW_NO_SPAN, -SHADOW_NO_SPAN, -SHADOW_NO_SPAN);
    out.origin       = pos;
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
// a sharp corner stays sharp, as CSS Backgrounds 3 §7.1 has it. Only
// the shader applies it: the shadow boxes are sized here.
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
// inside. Its winding sign is 0, so `d.y` is 0 everywhere; a sign taken
// from `d.y` would make the distance 0 everywhere, and a radius would
// then fill the whole quad. So `d.y == 0` reads as outside, which on a
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
// `in.fill` verbatim.
// Linear path projects `in.local` onto `fill_axis.xy` (object-local
// 0..1 axis), maps to 0..1 via `(t0, t1)`, applies spread, samples
// the LUT row at `fill_lut_row`.
fn eval_fill(in: VertexOut) -> vec4<f32> {
    let kind = in.fill_kind & FILL_TAG_MASK;
    if (kind == BRUSH_KIND_SOLID) {
        return in.fill;
    }
    let spread  = (in.fill_kind >> SPREAD_SHIFT) & SPREAD_MASK;
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
        // (radians, clockwise on screen: y points down). atan2 returns
        // -π..π; the +1.0 then fract
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

fn normal_cdf(v: f32) -> f32 {
    return 0.5 + 0.5 * erf_approx(v * SQRT_HALF);
}

// ∫ Φ from −∞ to `v`: `v·Φ(v) + φ(v)`.
fn normal_cdf_integral(v: f32) -> f32 {
    return v * normal_cdf(v) + INV_SQRT_TAU * exp(-0.5 * v * v);
}

// The CDF at `u` of what one pixel sees of a blurred edge: a Gaussian of
// `sigma` convolved with the pixel box of half-width `AA_HALF_WIDTH`. So
// `filter_cdf(e - x, σ)` is the coverage, at a pixel centred on `x`, of
// the blurred half-line below `e`. At σ = 0 it is the box alone, the same
// ramp the unblurred shapes are drawn with.
fn filter_cdf(u: f32, sigma: f32) -> f32 {
    if (sigma <= BLUR_EPS) {
        return edge_coverage(-u);
    }
    let inv = 1.0 / sigma;
    return sigma / (2.0 * AA_HALF_WIDTH)
        * (normal_cdf_integral((u + AA_HALF_WIDTH) * inv) - normal_cdf_integral((u - AA_HALF_WIDTH) * inv));
}

// The density of `filter_cdf` at `u`: the pixel box's mean of the
// Gaussian. Reached only at σ ≥ `CUTOUT_MIN_SIGMA`, so it has no σ = 0
// limit to take.
fn filter_pdf(u: f32, sigma: f32) -> f32 {
    let inv = 1.0 / sigma;
    return (normal_cdf((u + AA_HALF_WIDTH) * inv) - normal_cdf((u - AA_HALF_WIDTH) * inv))
        / (2.0 * AA_HALF_WIDTH);
}

// One corner's cutout: the filter's weight over the `r`×`r` square at the
// corner minus its quarter disk, at the pixel `q` relative to the arc's
// centre, reflected so the corner points to +x and +y. Kernel and region
// are both symmetric, so the one function serves all four corners.
//
// Integrated over the arc's angle, `x = r·sin t`, which keeps the
// integrand smooth where the arc runs steep, and only over the angles
// whose `x` the kernel reaches: outside them the weight is zero, as it is
// everywhere the kernel misses the square.
fn corner_cutout(q: vec2<f32>, r: f32, sigma: f32, reach: f32, nodes: u32) -> f32 {
    if (r <= 0.0 || any(q < vec2<f32>(-reach)) || any(q > vec2<f32>(r + reach))) {
        return 0.0;
    }
    let t0 = asin(clamp((q.x - reach) / r, 0.0, 1.0));
    let t1 = asin(clamp((q.x + reach) / r, 0.0, 1.0));
    let step = (t1 - t0) / f32(nodes);
    let top = filter_cdf(r - q.y, sigma);
    var sum = 0.0;
    for (var i = 0u; i < nodes; i++) {
        let t = t0 + (f32(i) + 0.5) * step;
        let x = r * sin(t);
        let y = r * cos(t);
        // `dx = r·cos t·dt = y·dt`.
        sum += filter_pdf(x - q.x, sigma) * (top - filter_cdf(y - q.y, sigma)) * y;
    }
    return sum * step;
}

// A table code's first texel in the atlas: its origin cell times
// `CUTOUT_CELL` (`CutoutPlan::code`).
fn cutout_origin(table: u32) -> vec2<u32> {
    return vec2<u32>(table & 63u, (table >> 6u) & 63u) * CUTOUT_CELL;
}

// A table code's side in texels (`CutoutPlan::code`).
fn cutout_side(table: u32) -> u32 {
    return table >> 12u;
}

// The cutout at `q` from a baked table, filtered bilinearly: texel `i` of
// each axis holds the cutout at `q = i·h − reach`, `h = σ /
// CUTOUT_TEXELS_PER_SIGMA`. Outside the table the cutout is zero, as it is
// past `reach` from its square.
fn cutout_lookup(table: u32, q: vec2<f32>, sigma: f32, reach: f32) -> f32 {
    let t = (q + vec2<f32>(reach)) * (CUTOUT_TEXELS_PER_SIGMA / sigma);
    if (any(t < vec2<f32>(0.0)) || any(t >= vec2<f32>(f32(cutout_side(table) - 1u)))) {
        return 0.0;
    }
    let i = vec2<u32>(t);
    let f = t - vec2<f32>(i);
    let at = cutout_origin(table) + i;
    let a = textureLoad(cutout_atlas, at, 0).x;
    let b = textureLoad(cutout_atlas, at + vec2<u32>(1u, 0u), 0).x;
    let c = textureLoad(cutout_atlas, at + vec2<u32>(0u, 1u), 0).x;
    let d = textureLoad(cutout_atlas, at + vec2<u32>(1u, 1u), 0).x;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// One corner's cutout, from its table when it has one.
fn corner_cutout_at(table: u32, q: vec2<f32>, r: f32, sigma: f32, reach: f32) -> f32 {
    if (table == NO_CUTOUT_TABLE) {
        return corner_cutout(q, r, sigma, reach, CUTOUT_NODES);
    }
    return cutout_lookup(table, q, sigma, reach);
}

// `blurred_box_coverage` from σ = `CUTOUT_MIN_SIGMA` up: the sharp box,
// separable and so closed form, less each corner's cutout. The same
// integral as the outline form, split so that the four corners share one
// symmetric term.
fn cutout_box_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, reach: f32, cutouts: vec4<u32>) -> f32 {
    let sharp = (filter_cdf(half.x - p.x, sigma) - filter_cdf(-half.x - p.x, sigma))
        * (filter_cdf(half.y - p.y, sigma) - filter_cdf(-half.y - p.y, sigma));
    let cut = corner_cutout_at(cutouts.z, p - (half - radius.zz), radius.z, sigma, reach)
        + corner_cutout_at(cutouts.w, (p - vec2<f32>(radius.w - half.x, half.y - radius.w)) * vec2<f32>(-1.0, 1.0), radius.w, sigma, reach)
        + corner_cutout_at(cutouts.x, (radius.xx - half) - p, radius.x, sigma, reach)
        + corner_cutout_at(cutouts.y, (p - vec2<f32>(half.x - radius.y, radius.y - half.y)) * vec2<f32>(1.0, -1.0), radius.y, sigma, reach);
    return clamp(sharp - cut, 0.0, 1.0);
}

// One half of a corner arc's share of `blurred_box_coverage`, integrated
// along one axis over `[e0, e1]` (either order): the sum of
// `filter_cdf` across the arc, at each slice's midpoint, times the
// slice's exact weight along. `p` and `centre` are given `(along,
// across)`; the arc is `across = centre.y + side·√(r² − (along −
// centre.x)²)`. Only the part of the range the kernel reaches is sliced:
// outside it the weight is zero.
fn arc_half(p: vec2<f32>, centre: vec2<f32>, side: f32, r: f32, e0: f32, e1: f32, sigma: f32, reach: f32) -> f32 {
    let lo = max(min(e0, e1), p.x - reach);
    let hi = min(max(e0, e1), p.x + reach);
    if (lo >= hi) {
        return 0.0;
    }
    let step = (hi - lo) / f32(BLUR_ARC_SLICES);
    var prev = filter_cdf(lo - p.x, sigma);
    var sum = 0.0;
    for (var i = 1u; i <= BLUR_ARC_SLICES; i++) {
        let at = lo + f32(i) * step;
        let next = filter_cdf(at - p.x, sigma);
        let along = at - 0.5 * step - centre.x;
        let across = centre.y + side * sqrt(max(r * r - along * along, 0.0));
        sum += filter_cdf(across - p.y, sigma) * (next - prev);
        prev = next;
    }
    return sum;
}

// One rounded corner's share of `blurred_box_coverage`. The arc is the
// quarter circle of radius `r` about `centre`, bulging toward `side`.
//
// Green's theorem turns the coverage integral into one along the outline,
// in either of two forms: by rows, `∮ C(x)·dC(y)`, or by columns,
// `−∮ C(y)·dC(x)` (C being `filter_cdf` about the pixel). They differ by
// the exact differential of `C(x)·C(y)`, so the outline may switch forms
// at any point for the cost of that product at the switch. The steep half
// of the arc goes by rows and the shallow half by columns, so the
// variable sliced is always the one the arc moves along fastest.
fn blurred_corner(p: vec2<f32>, centre: vec2<f32>, r: f32, side: vec2<f32>, sigma: f32, reach: f32) -> f32 {
    if (r <= 0.0) {
        return 0.0;
    }
    let far = centre + side * r;
    let lo = min(centre, far);
    let hi = max(centre, far);
    let gap = abs(p - clamp(p, lo, hi));
    // Rows and columns cancel exactly when the kernel misses the corner
    // vertically; horizontally, it sees the corner as a straight side.
    if (gap.y > reach) {
        return 0.0;
    }
    if (gap.x > reach) {
        return side.x * filter_cdf(centre.x - p.x, sigma)
            * (filter_cdf(hi.y - p.y, sigma) - filter_cdf(lo.y - p.y, sigma));
    }
    let diagonal = centre + side * (r * SQRT_HALF);
    let rows = arc_half(p.yx, centre.yx, side.x, r, centre.y, diagonal.y, sigma, reach);
    let columns = arc_half(p, centre, side.y, r, centre.x, diagonal.x, sigma, reach);
    let switch_terms = filter_cdf(centre.x - p.x, sigma) * filter_cdf(far.y - p.y, sigma)
        - filter_cdf(diagonal.x - p.x, sigma) * filter_cdf(diagonal.y - p.y, sigma);
    return side.x * rows + side.y * columns + side.x * side.y * switch_terms;
}

// Coverage, at the pixel centred on `p`, of the rounded box of half-extents
// `half` centred at the origin, blurred by a Gaussian of `sigma`: the
// integral of the pixel's filter (`filter_cdf`) over the box. The kernel
// is separable, so the straight sides are closed form; only the corner
// arcs are sliced (`blurred_corner`). A sharp box is the exact product of
// its two edge pairs, an empty box covers nothing, and σ = 0 is the box's
// exact pixel coverage. `radius` is `(tl, tr, br, bl)`, fitted to the box.
// From σ = `CUTOUT_MIN_SIGMA` up, the corners are cut out instead
// (`cutout_box_coverage`).
fn blurred_box_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, cutouts: vec4<u32>) -> f32 {
    let reach = SHADOW_REACH_SIGMAS * sigma + AA_HALF_WIDTH;
    if (sigma >= CUTOUT_MIN_SIGMA) {
        return cutout_box_coverage(p, half, radius, sigma, reach, cutouts);
    }
    let right = filter_cdf(half.x - p.x, sigma)
        * (filter_cdf(half.y - radius.z - p.y, sigma) - filter_cdf(radius.y - half.y - p.y, sigma));
    let left = filter_cdf(-half.x - p.x, sigma)
        * (filter_cdf(radius.x - half.y - p.y, sigma) - filter_cdf(half.y - radius.w - p.y, sigma));
    let corners = blurred_corner(p, half - radius.zz, radius.z, vec2<f32>(1.0, 1.0), sigma, reach)
        + blurred_corner(p, vec2<f32>(radius.w - half.x, half.y - radius.w), radius.w, vec2<f32>(-1.0, 1.0), sigma, reach)
        + blurred_corner(p, radius.xx - half, radius.x, vec2<f32>(-1.0, -1.0), sigma, reach)
        + blurred_corner(p, vec2<f32>(half.x - radius.y, radius.y - half.y), radius.y, vec2<f32>(1.0, -1.0), sigma, reach);
    return clamp(right + left + corners, 0.0, 1.0);
}

// Composite an SDF shape's fill + inner-edge stroke into premultiplied linear
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

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    // Uniform per instance (`fill_kind` is flat), so whole wavefronts
    // inside one quad take a single side of this branch.
    if ((in.fill_kind & FILL_FLAG_FAST) != 0u) {
        return premultiply(in.fill.rgb, in.fill.a);
    }
    let kind = in.fill_kind & FILL_TAG_MASK;
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

// The box whose blur a shadow paints, relative to its quad's centre, with
// the radii fitted to it. A drop shadow's box is its source grown by the
// spread, centred on the quad, which is the source moved by `offset` and
// grown by the halo. An inset shadow's is the hole: the source shrunk by
// the spread, moved by `offset`. Shared by `vs_shadow`, which lays the
// cells out from it, and `fs_shadow`, so the two cannot disagree.
struct ShadowBox {
    centre: vec2<f32>,
    half: vec2<f32>,
    radius: vec4<f32>,
};

fn shadow_box(kind: u32, half: vec2<f32>, radius: vec4<f32>, fill_axis: vec4<f32>) -> ShadowBox {
    let spread = fill_axis.w;
    if (kind == BRUSH_KIND_SHADOW_DROP) {
        // CSS's radius rule for a spread, fitted to the shadow's own box,
        // as the inset hole's are.
        let shadow_half = max(shadow_source_half(half, fill_axis) + vec2<f32>(spread), vec2<f32>(0.0));
        return ShadowBox(vec2<f32>(0.0), shadow_half, fit_radii(spread_radius(radius, spread), shadow_half));
    }
    // The hole is the source shrunk by `spread`, so its radii follow the
    // CSS spread rule with `-spread` — less by `spread`, floored at 0, or
    // grown the way a drop shadow's are when the spread is negative — then
    // fit the hole's own box.
    let hole_half = max(half - vec2<f32>(spread), vec2<f32>(0.0));
    return ShadowBox(fill_axis.xy, hole_half, fit_radii(spread_radius(radius, -spread), hole_half));
}

// A drop shadow's source S, as half-extents: its quad less the halo.
fn shadow_source_half(half: vec2<f32>, fill_axis: vec4<f32>) -> vec2<f32> {
    return half - vec2<f32>(SHADOW_REACH_SIGMAS * fill_axis.z + max(fill_axis.w, 0.0));
}

// `blurred_box_coverage` at `p`, relative to the shadow's box, taking the
// cheaper form where it is exact. Strictly inside `core` on one axis (the
// box less `r + reach` there, `VertexOut::shadow_core`), the point is past
// `reach` from both edges across that axis and from every corner's
// cutout, so only the pair along the other axis varies; inside it on both,
// the box covers the point fully. The Gaussian is taken as zero past
// `reach` there, as the cutouts and the quad's own bounds take it.
fn shadow_coverage(p: vec2<f32>, sb: ShadowBox, sigma: f32, cutouts: vec4<u32>, core: vec4<f32>) -> f32 {
    let inside = (p > core.xy) & (p < core.zw);
    if (all(inside)) {
        return 1.0;
    }
    if (inside.x) {
        return clamp(filter_cdf(sb.half.y - p.y, sigma) - filter_cdf(-sb.half.y - p.y, sigma), 0.0, 1.0);
    }
    if (inside.y) {
        return clamp(filter_cdf(sb.half.x - p.x, sigma) - filter_cdf(-sb.half.x - p.x, sigma), 0.0, 1.0);
    }
    return blurred_box_coverage(p, sb.half, sb.radius, sigma, cutouts);
}

// Drop and inset shadows. Its own entry, and so its own pipeline, because
// a pipeline gets the registers and code of everything its entry reaches:
// inside `fs`, the blurred-corner integral set every quad's. The schedule
// routes only the two shadow kinds here.
@fragment
fn fs_shadow(in: VertexOut) -> @location(0) vec4<f32> {
    let kind = in.fill_kind & FILL_TAG_MASK;
    let local = in.clip.xy - in.origin;
    let half = in.size * 0.5;
    let sigma = in.fill_axis.z;
    let sb = shadow_box(kind, half, in.radius, in.fill_axis);
    if (kind == BRUSH_KIND_SHADOW_DROP) {
        // Drop shadow: the quad is the source S moved by `offset` and
        // grown by the halo, so S sits `offset` back from its centre.
        // CSS clips an outer shadow inside the box that casts it
        // (Backgrounds 3 §7.1.1). Only where S's own coverage is full:
        // the fill drawn over S's edge pixels then blends with the shadow
        // under them as CSS's geometric clip would, where a clip at the
        // edge itself would open a seam.
        let d_src = sdf_rounded_box_centered(
            local - half + in.fill_axis.xy,
            shadow_source_half(half, in.fill_axis),
            in.radius,
        );
        if (edge_coverage(d_src) >= 1.0 - SHADOW_CLIP_EPS) {
            return vec4<f32>(0.0);
        }
        let cov = shadow_coverage(local - half - sb.centre, sb, sigma, in.cutouts, in.shadow_core);
        return premultiply(in.fill.rgb, in.fill.a * cov);
    }
    if (kind == BRUSH_KIND_SHADOW_INSET) {
        // Inset shadow: source rect S equals the paint bbox. The
        // "hole" is S deflated by `spread` per side, then shifted by
        // `offset` (the light source moves in that direction → shadow
        // grows opposite). Coverage at fragment p inside S =
        // `1 - blurred_cov(p relative to hole)` — outside the hole
        // but still inside S is where the shadow paints; deep inside
        // the hole is lit (cov→0).
        //
        // Inset never paints outside the source, and across the source's
        // edge it takes the same coverage ramp the source's fill does, so
        // the two meet without a staircase on a rounded corner.
        let d_src = sdf_rounded_box_centered(local - half, half, in.radius);
        let source_cov = edge_coverage(d_src);
        if (source_cov <= 0.0) {
            return vec4<f32>(0.0);
        }
        let hole_cov = shadow_coverage(local - half - sb.centre, sb, sigma, in.cutouts, in.shadow_core);
        return premultiply(in.fill.rgb, in.fill.a * source_cov * (1.0 - hole_cov));
    }
    return vec4<f32>(0.0);
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

// One cutout table for `fs_cutout_bake` (`CutoutPlan::BakeTable`): where
// it goes in the atlas (`cutout_lookup`'s packing) and its `(r, σ)`.
struct BakeIn {
    @location(0) table: u32,
    @location(1) r:     f32,
    @location(2) sigma: f32,
};

struct BakeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) table: u32,
    @location(1) @interpolate(flat) r:     f32,
    @location(2) @interpolate(flat) sigma: f32,
};

// The table's square of the atlas, one fragment per texel.
@vertex
fn vs_cutout_bake(@builtin(vertex_index) vi: u32, bake: BakeIn) -> BakeOut {
    let texel = vec2<f32>(cutout_origin(bake.table)) + CORNERS[vi] * f32(cutout_side(bake.table));
    var out: BakeOut;
    out.clip = vec4<f32>(texel.x / CUTOUT_ATLAS_SIZE * 2.0 - 1.0, 1.0 - texel.y / CUTOUT_ATLAS_SIZE * 2.0, 0.0, 1.0);
    out.table = bake.table;
    out.r = bake.r;
    out.sigma = bake.sigma;
    return out;
}

// The cutout at the `q` this texel stands for (`cutout_lookup`), with
// `CUTOUT_BAKE_NODES`.
@fragment
fn fs_cutout_bake(in: BakeOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(vec2<u32>(in.clip.xy) - cutout_origin(in.table));
    let reach = SHADOW_REACH_SIGMAS * in.sigma + AA_HALF_WIDTH;
    let q = texel * (in.sigma / CUTOUT_TEXELS_PER_SIGMA) - vec2<f32>(reach);
    return vec4<f32>(corner_cutout(q, in.r, in.sigma, reach, CUTOUT_BAKE_NODES), 0.0, 0.0, 0.0);
}
