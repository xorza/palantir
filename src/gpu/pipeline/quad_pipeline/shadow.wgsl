// Drop and inset shadows: the closed-form blur of a rounded box, drawn as the cells of a grid
// around a hole that paints nothing (`vs_shadow`, `fs_shadow`, `fs_shadow_tables`).

// σ below which `filter_cdf` takes its σ = 0 limit, the bare pixel box.
// The two differ by about σ there, far under an 8-bit step, and the
// limit keeps `1/σ` out of the arithmetic.
const BLUR_EPS: f32 = 1e-4;

// How far a shadow's Gaussian is followed, in σ (`ShadowGeometry::REACH_SIGMAS`):
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

// A cutout table's texels per σ, both axes (`CutoutPlan`). At 10 its
// bilinear error stays under 6e-4 of the exact cutout (measured at
// σ = 0.25–16 against radii 4–40), under the shaded form's 0.0011.
const CUTOUT_TEXELS_PER_SIGMA: f32 = /*{CUTOUT_TEXELS_PER_SIGMA}*/;

// Side of the cell a table's origin is a multiple of, from `CutoutPlan`.
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
// everywhere: the reference the two are tested against, built only by the
// crate's internals.
override SHADOW_GRID: bool = true;

// Which form of `filter_cdf` a tables entry carries for its edges
// (`tables_edge_cdf`): the series, for `ShadowEntry::TablesWide`, or the
// difference, for `ShadowEntry::Tables`. Pinned per pipeline rather than
// branched on, which would hold both forms' registers in one.
override TABLES_SERIES: bool = false;

// The σ from which `filter_cdf` takes its series form
// (`CutoutPlan::SERIES_MIN_SIGMA`).
const FILTER_SERIES_MIN_SIGMA: f32 = /*{FILTER_SERIES_MIN_SIGMA}*/;

// The σ from which `filter_pdf` takes its series: the box's mean of `φ`
// around `v = u / σ` is `(φ(v) + (h²/6)·φ″(v) + (h⁴/120)·φ⁗(v) + …) / σ`,
// with `h = PALANTIR_AA_HALF_WIDTH / σ`, `φ″(v) = (v² − 1)·φ(v)` and `|φ⁗| ≤ 1.2`.
// The difference of `Φ` at the box's ends errs about 3.5e-7 in f32 at every
// σ, the dropped term `1e-2 · h⁴ / σ`; the series errs less from σ = 5
// (2.0e-7 against 3.2e-7 there, 3e-9 against 3.8e-7 at σ = 16), and costs
// one `exp` where the difference takes two and two reciprocals. Later than
// `FILTER_SERIES_MIN_SIGMA`, whose difference loses more, as it grows with σ.
const FILTER_PDF_SERIES_MIN_SIGMA: f32 = 5.0;

// A cell's two triangles, as indices into `CORNERS`.
const CELL_TRIANGLES = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);

const SQRT_HALF: f32 = 0.70710678;
const INV_SQRT_TAU: f32 = 0.39894228;

// A drop shadow clips where its source's own coverage is within this of
// full: one 8-bit step, so what the clip takes from under an opaque fill
// never reaches a stored value, while a pixel center that sits exactly on
// the threshold clips the same way under the float noise that the
// interpolated `local` carries.
const SHADOW_CLIP_EPS: f32 = 1.0 / 255.0;

// Drop/inset shadow: closed-form Gaussian-blurred rounded rect.
// The shape's `radius` is the source rect's corner radii, its rect the
// paint bbox, and `geometry = (offset.x, offset.y, sigma, spread)` for both.
//   - Drop:  paint bbox = (source + offset).inflated(SHADOW_REACH_SIGMAS·σ + max(spread, 0)).
//   - Inset: paint bbox = source. Spread shrinks the "hole" rect
//            inside the shader.
const SHADOW_DROP:  u32 = /*{SHADOW_DROP}*/;
const SHADOW_INSET: u32 = /*{SHADOW_INSET}*/;

// A shadow's lanes past its shape (`ShadowQuad`, `SHADOW_INSTANCE_ATTRS`): its straight-alpha
// color, its kind, and its geometry `(offset.x, offset.y, sigma, spread)`, each four f16 two to a
// `u32`.
struct ShadowIn {
    @location(6) color_packed: vec2<u32>,
    @location(7) kind: u32,
    @location(8) geometry_packed: vec2<u32>,
};

// What a shadow's fragments read: `vs_shadow`'s geometry, worked out once
// per vertex. Every position is in pixels, and a fragment takes its own as
// the pixel center less one of them, which is exact and the same however
// the quad was cut into cells, where an interpolated position carries the
// rounding of whichever triangle covered the pixel.
struct ShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) fill: vec4<f32>,
    // `SHADOW_DROP` or `SHADOW_INSET`.
    @location(1) @interpolate(flat) kind: u32,
    @location(2) @interpolate(flat) sigma: f32,
    // The box whose blur the shadow paints (`ShadowBox`).
    @location(3) @interpolate(flat) box_center: vec2<f32>,
    @location(4) @interpolate(flat) box_half: vec2<f32>,
    @location(5) @interpolate(flat) box_radius: vec4<f32>,
    // The rounded rect that casts the shadow, which clips it.
    @location(6) @interpolate(flat) source_center: vec2<f32>,
    @location(7) @interpolate(flat) source_half: vec2<f32>,
    @location(8) @interpolate(flat) source_radius: vec4<f32>,
    // The baked cutout tables, `(tl, tr, br, bl)` like `box_radius`: each
    // the table's origin cell and side (`cutout_lookup`), or
    // `NO_CUTOUT_TABLE`.
    @location(9) @interpolate(flat) cutouts: vec4<u32>,
    // The box less `r + reach` on each side, `(lo.x, lo.y, hi.x, hi.y)`
    // relative to its center: on an axis strictly inside it, the pair of
    // edges across that axis is constant (`shadow_coverage`).
    @location(10) @interpolate(flat) core: vec4<f32>,
    // The quad's turn: its center and `(cos, sin)`, which `shadow_pixel`
    // undoes so every position above stays in the unturned frame.
    @location(11) @interpolate(flat) turn: vec4<f32>,
    // The rounded clips that cut the shadow (`ClipChain`).
    @location(12) @interpolate(flat) chain: u32,
};

// `vs` for the shadow pipeline, which also binds the per-instance cutout
// tables (`CutoutTables`). A shadow draws as the eight cells of a 3×3
// grid around a hole that paints nothing, six vertices each
// (`QuadPipeline::draw_shadows`).
//
// A drop shadow's hole is its source inset by its largest radius and
// `PALANTIR_AA_HALF_WIDTH`: at every pixel center there the source SDF is at most
// `−PALANTIR_AA_HALF_WIDTH`, so the clip in `shadow_pixel` zeroes it, and skipping it
// changes no pixel. Its four edge cells stop short of the hole too, a pixel
// past `PALANTIR_AA_HALF_WIDTH` inside the source: between the corners the source's
// edge is straight, so the same clip zeroes the band they leave. An inset
// shadow's hole is its core (`ShadowOut::core`), where the blurred box
// covers everything and the shadow is zero by its form. A hole that is
// empty, or a reference build (`SHADOW_GRID` off), puts all four inner
// lines on the quad's far corner, which leaves one cell: the whole quad.
//
// The inner lines are clamped into the quad. A clamp keeps the order of a
// hole that is not empty, so the cells tile the quad less the hole and the
// edge cells' bands. Every vertex is a pair of these lines, so two cells
// that share an edge share its line exactly and the rasterizer shades each
// pixel once. An edge cell's short side ends partway along a corner cell's,
// so a turned shadow, whose lines turn apart in float, keeps its edge cells
// whole.
@vertex
fn vs_shadow(
    @builtin(vertex_index) vi: u32,
    shape: PalantirShapeIn,
    shadow: ShadowIn,
    @location(9) cutouts: vec4<u32>,
) -> ShadowOut {
    let pos = shape.rect.xy;
    let size = shape.rect.zw;
    let rotation = shape.inradius_rotation.y;
    let radius = shape.radius;
    let geometry = unpack_f16x4(shadow.geometry_packed);
    let bounds = shaded_bounds(pos, size, rotation);
    let half = size * 0.5;
    let center = pos + half;
    let kind = shadow.kind;
    let sb = shadow_box(kind, half, radius, geometry);
    let sigma = geometry.z;
    let reach = SHADOW_REACH_SIGMAS * sigma + PALANTIR_AA_HALF_WIDTH;
    let r = sb.radius;
    let core_lo = -sb.half + vec2<f32>(max(r.x, r.w), max(r.x, r.y)) + vec2<f32>(reach);
    let core_hi = sb.half - vec2<f32>(max(r.y, r.z), max(r.w, r.z)) - vec2<f32>(reach);

    var hole_lo = center + sb.center + core_lo;
    var hole_hi = center + sb.center + core_hi;
    // An inset shadow's source is its quad. A drop shadow's quad is the
    // source moved by `offset` and grown by the halo, so the source sits
    // `offset` back from its center.
    var source_center = center;
    var source_half = half;
    if (kind == SHADOW_DROP) {
        source_half = shadow_source_half(half, geometry);
        source_center = center - geometry.xy;
        let inset = max(max(radius.x, radius.y), max(radius.z, radius.w)) + PALANTIR_AA_HALF_WIDTH;
        hole_lo = source_center - source_half + vec2<f32>(inset);
        hole_hi = source_center + source_half - vec2<f32>(inset);
    }
    if (!SHADOW_GRID || any(hole_lo >= hole_hi)) {
        hole_lo = bounds.hi;
        hole_hi = bounds.hi;
    }
    let inner_lo = clamp(hole_lo, bounds.lo, bounds.hi);
    let inner_hi = clamp(hole_hi, bounds.lo, bounds.hi);
    var xs = array<f32, 4>(bounds.lo.x, inner_lo.x, inner_hi.x, bounds.hi.x);
    var ys = array<f32, 4>(bounds.lo.y, inner_lo.y, inner_hi.y, bounds.hi.y);
    var edge_lo = inner_lo;
    var edge_hi = inner_hi;
    if (kind == SHADOW_DROP && rotation == 0.0) {
        let deep = vec2<f32>(PALANTIR_AA_HALF_WIDTH + 1.0);
        edge_lo = clamp(source_center - source_half + deep, bounds.lo, inner_lo);
        edge_hi = clamp(source_center + source_half - deep, inner_hi, bounds.hi);
    }

    // The grid's cells in row order, past the center one: the hole.
    let draw_cell = vi / 6u;
    let cell = draw_cell + u32(draw_cell >= 4u);
    let cx = cell % 3u;
    let cy = cell / 3u;
    let unit = CORNERS[CELL_TRIANGLES[vi % 6u]];
    let line = vec2<u32>(cx + u32(unit.x), cy + u32(unit.y));
    var vertex = vec2<f32>(xs[line.x], ys[line.y]);
    // An edge cell's inner side: the left and right cells sit in the middle row, the top and
    // bottom ones in the middle column.
    if (cy == 1u) {
        vertex.x = select(select(vertex.x, edge_hi.x, line.x == 2u), edge_lo.x, line.x == 1u);
    }
    if (cx == 1u) {
        vertex.y = select(select(vertex.y, edge_hi.y, line.y == 2u), edge_lo.y, line.y == 1u);
    }
    var out: ShadowOut;
    out.clip = clip_from_px(turned(vertex, center, rotation));
    out.turn = vec4<f32>(center, cos(rotation), sin(rotation));
    out.chain = shape.chain;
    out.fill = unpack_f16x4(shadow.color_packed);
    out.kind = kind;
    out.sigma = sigma;
    out.box_center = center + sb.center;
    out.box_half = sb.half;
    out.box_radius = sb.radius;
    out.source_center = source_center;
    out.source_half = source_half;
    out.source_radius = radius;
    out.cutouts = cutouts;
    out.core = select(
        vec4<f32>(NO_SPAN, NO_SPAN, -NO_SPAN, -NO_SPAN),
        vec4<f32>(core_lo, core_hi),
        SHADOW_GRID,
    );
    return out;
}

// `p` turned back by `turn` (`ShadowOut::turn`), the inverse of `turned`.
fn unturned(p: vec2<f32>, turn: vec4<f32>) -> vec2<f32> {
    if (turn.w == 0.0) {
        return p;
    }
    let v = p - turn.xy;
    return turn.xy + vec2<f32>(turn.z * v.x + turn.w * v.y, turn.z * v.y - turn.w * v.x);
}

// CSS `box-shadow` spread on each radius: `max(r + s, 0)` for `s < 0`;
// for `s ≥ 0`, `r + s` when `r ≥ s` and `r + s·(1 + (r/s − 1)³)` below, so
// a sharp corner stays sharp, as CSS Backgrounds 3 §7.1 has it. The cutout
// plan's copy (`ShadowBox`) sizes the corner tables from the same rule.
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

// `erf` approximation (Abramowitz & Stegun 7.1.26 form, max error
// ~1.5e-7). WGSL doesn't ship `erf` as a builtin.
fn erf_approx(x: f32) -> f32 {
    return erf_given(x, exp(-x * x));
}

// `erf_approx` given `gauss = exp(−x²)`, the unscaled `φ(x·√2)`, which a
// caller that also needs that density has already worked out.
fn erf_given(x: f32, gauss: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * gauss;
    return sign(x) * y;
}

fn normal_cdf(v: f32) -> f32 {
    return 0.5 + 0.5 * erf_approx(v * SQRT_HALF);
}

// `normal_cdf` given `gauss = exp(−v²/2)`, which is both `erf`'s Gaussian at
// `v / √2` and `φ(v)`'s, for a caller that also needs that density.
fn normal_cdf_given(v: f32, gauss: f32) -> f32 {
    return 0.5 + 0.5 * erf_given(v * SQRT_HALF, gauss);
}

// ∫ Φ from −∞ to `v`: `v·Φ(v) + φ(v)`.
fn normal_cdf_integral(v: f32) -> f32 {
    let gauss = exp(-0.5 * v * v);
    return v * normal_cdf_given(v, gauss) + INV_SQRT_TAU * gauss;
}

// The CDF at `u` of what one pixel sees of a blurred edge: a Gaussian of
// `sigma` convolved with the pixel box of half-width `PALANTIR_AA_HALF_WIDTH`. So
// `filter_cdf(e - x, σ)` is the coverage, at a pixel centered on `x`, of
// the blurred half-line below `e`. At σ = 0 it is the box alone, the same
// ramp the unblurred shapes are drawn with.
fn filter_cdf(u: f32, sigma: f32) -> f32 {
    if (sigma <= BLUR_EPS) {
        return palantir_edge_coverage(-u);
    }
    if (sigma >= FILTER_SERIES_MIN_SIGMA) {
        return filter_cdf_series(u, sigma);
    }
    return filter_cdf_difference(u, sigma);
}

// `filter_cdf` as the difference of `∫Φ` at the box's two ends,
// `(u ± PALANTIR_AA_HALF_WIDTH) / σ`. Exact, but the two ends are near `u / σ`
// apiece and their difference is scaled by `σ`, so in f32 it loses about
// `8e-7 · σ`: 3.5e-6 at σ = 4, 1.3e-5 at σ = 16.
fn filter_cdf_difference(u: f32, sigma: f32) -> f32 {
    let inv = 1.0 / sigma;
    return sigma / (2.0 * PALANTIR_AA_HALF_WIDTH)
        * (normal_cdf_integral((u + PALANTIR_AA_HALF_WIDTH) * inv) - normal_cdf_integral((u - PALANTIR_AA_HALF_WIDTH) * inv));
}

// `filter_cdf` as a series in `h = PALANTIR_AA_HALF_WIDTH / σ`: the box's mean of
// `Φ` around `v = u / σ` is `Φ(v) + (h²/6)·φ′(v) + (h⁴/120)·φ‴(v) + …`,
// with `φ′(v) = −v·φ(v)` and `|φ‴| ≤ 0.55`. The term it drops is under
// `4.6e-3 · h⁴`, 1.1e-6 at σ = 4 and falling as `σ⁻⁴`, and nothing in it
// cancels, so it errs less in f32 than the difference from σ = 4 up: 1.2e-6
// against 3.5e-6 there, 2.3e-7 against 1.3e-5 at σ = 16. One `exp` and
// one reciprocal where the difference takes two of each.
fn filter_cdf_series(u: f32, sigma: f32) -> f32 {
    let inv = 1.0 / sigma;
    let v = u * inv;
    let h = PALANTIR_AA_HALF_WIDTH * inv;
    let gauss = exp(-0.5 * v * v);
    return normal_cdf_given(v, gauss) - (h * h / 6.0) * v * INV_SQRT_TAU * gauss;
}

// `filter_cdf` in the one form a tables entry was built with
// (`TABLES_SERIES`), which the plan picks by σ as `filter_cdf` does. Above
// `BLUR_EPS` always: a table is baked from `CUTOUT_MIN_SIGMA` up.
fn tables_edge_cdf(u: f32, sigma: f32) -> f32 {
    if (TABLES_SERIES) {
        return filter_cdf_series(u, sigma);
    }
    return filter_cdf_difference(u, sigma);
}

// The density of `filter_cdf` at `u`: the pixel box's mean of the
// Gaussian. Reached only at σ ≥ `CUTOUT_MIN_SIGMA`, so it has no σ = 0
// limit to take. As `filter_cdf` does, it takes a series from
// `FILTER_PDF_SERIES_MIN_SIGMA` up.
fn filter_pdf(u: f32, sigma: f32) -> f32 {
    let inv = 1.0 / sigma;
    if (sigma >= FILTER_PDF_SERIES_MIN_SIGMA) {
        let v = u * inv;
        let h = PALANTIR_AA_HALF_WIDTH * inv;
        return INV_SQRT_TAU * exp(-0.5 * v * v) * (1.0 + (h * h / 6.0) * (v * v - 1.0)) * inv;
    }
    return (normal_cdf((u + PALANTIR_AA_HALF_WIDTH) * inv) - normal_cdf((u - PALANTIR_AA_HALF_WIDTH) * inv))
        / (2.0 * PALANTIR_AA_HALF_WIDTH);
}

// One corner's cutout: the filter's weight over the `r`×`r` square at the
// corner minus its quarter disk, at the pixel `q` relative to the arc's
// center, reflected so the corner points to +x and +y. Kernel and region
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

// `corner_cutout_at` for `fs_shadow_tables`: a corner with no table has
// no cutout, since it has no radius or no pixel the frame draws is in its
// region (`CutoutPlan`).
fn tabled_cutout(table: u32, q: vec2<f32>, sigma: f32, reach: f32) -> f32 {
    if (table == NO_CUTOUT_TABLE) {
        return 0.0;
    }
    return cutout_lookup(table, q, sigma, reach);
}

// `p` relative to each corner's arc center, reflected so the corner points
// to +x and +y as `corner_cutout` takes it, `(tl, tr, br, bl)`.
fn corner_points(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>) -> array<vec2<f32>, 4> {
    return array<vec2<f32>, 4>(
        (radius.xx - half) - p,
        (p - vec2<f32>(half.x - radius.y, radius.y - half.y)) * vec2<f32>(1.0, -1.0),
        p - (half - radius.zz),
        (p - vec2<f32>(radius.w - half.x, half.y - radius.w)) * vec2<f32>(-1.0, 1.0),
    );
}

// `blurred_box_coverage` from σ = `CUTOUT_MIN_SIGMA` up: the sharp box,
// separable and so closed form, less each corner's cutout. The same
// integral as the outline form, split so that the four corners share one
// symmetric term.
fn cutout_box_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, reach: f32, cutouts: vec4<u32>) -> f32 {
    let sharp = (filter_cdf(half.x - p.x, sigma) - filter_cdf(-half.x - p.x, sigma))
        * (filter_cdf(half.y - p.y, sigma) - filter_cdf(-half.y - p.y, sigma));
    let q = corner_points(p, half, radius);
    let cut = corner_cutout_at(cutouts.z, q[2], radius.z, sigma, reach)
        + corner_cutout_at(cutouts.w, q[3], radius.w, sigma, reach)
        + corner_cutout_at(cutouts.x, q[0], radius.x, sigma, reach)
        + corner_cutout_at(cutouts.y, q[1], radius.y, sigma, reach);
    return clamp(sharp - cut, 0.0, 1.0);
}

// The blurred span `[-half, half]` at `x`, `tables_edge_cdf(half - x) -
// tables_edge_cdf(-half - x)`, with an edge `reach` or more away taken as the
// step it is once the Gaussian is zero past `reach`, as the core of
// `shadow_coverage` takes it: at most 3.2e-5 of coverage.
fn reached_span(x: f32, half: f32, sigma: f32, reach: f32) -> f32 {
    let hi = half - x;
    let lo = -half - x;
    var upper = 1.0;
    if (hi < reach) {
        upper = tables_edge_cdf(hi, sigma);
    }
    var lower = 0.0;
    if (lo > -reach) {
        lower = tables_edge_cdf(lo, sigma);
    }
    return clamp(upper - lower, 0.0, 1.0);
}

// One half of a corner arc's share of `blurred_box_coverage`, integrated
// along one axis over `[e0, e1]` (either order): the sum of
// `filter_cdf` across the arc, at each slice's midpoint, times the
// slice's exact weight along. `p` and `center` are given `(along,
// across)`; the arc is `across = center.y + side·√(r² − (along −
// center.x)²)`. Only the part of the range the kernel reaches is sliced:
// outside it the weight is zero.
fn arc_half(p: vec2<f32>, center: vec2<f32>, side: f32, r: f32, e0: f32, e1: f32, sigma: f32, reach: f32) -> f32 {
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
        let along = at - 0.5 * step - center.x;
        let across = center.y + side * sqrt(max(r * r - along * along, 0.0));
        sum += filter_cdf(across - p.y, sigma) * (next - prev);
        prev = next;
    }
    return sum;
}

// One rounded corner's share of `blurred_box_coverage`. The arc is the
// quarter circle of radius `r` about `center`, bulging toward `side`.
//
// Green's theorem turns the coverage integral into one along the outline,
// in either of two forms: by rows, `∮ C(x)·dC(y)`, or by columns,
// `−∮ C(y)·dC(x)` (C being `filter_cdf` about the pixel). They differ by
// the exact differential of `C(x)·C(y)`, so the outline may switch forms
// at any point for the cost of that product at the switch. The steep half
// of the arc goes by rows and the shallow half by columns, so the
// variable sliced is always the one the arc moves along fastest.
fn blurred_corner(p: vec2<f32>, center: vec2<f32>, r: f32, side: vec2<f32>, sigma: f32, reach: f32) -> f32 {
    if (r <= 0.0) {
        return 0.0;
    }
    let far = center + side * r;
    let lo = min(center, far);
    let hi = max(center, far);
    let gap = abs(p - clamp(p, lo, hi));
    // Rows and columns cancel exactly when the kernel misses the corner
    // vertically; horizontally, it sees the corner as a straight side.
    if (gap.y > reach) {
        return 0.0;
    }
    if (gap.x > reach) {
        return side.x * filter_cdf(center.x - p.x, sigma)
            * (filter_cdf(hi.y - p.y, sigma) - filter_cdf(lo.y - p.y, sigma));
    }
    let diagonal = center + side * (r * SQRT_HALF);
    let rows = arc_half(p.yx, center.yx, side.x, r, center.y, diagonal.y, sigma, reach);
    let columns = arc_half(p, center, side.y, r, center.x, diagonal.x, sigma, reach);
    let switch_terms = filter_cdf(center.x - p.x, sigma) * filter_cdf(far.y - p.y, sigma)
        - filter_cdf(diagonal.x - p.x, sigma) * filter_cdf(diagonal.y - p.y, sigma);
    return side.x * rows + side.y * columns + side.x * side.y * switch_terms;
}

// Coverage, at the pixel centered on `p`, of the rounded box of half-extents
// `half` centered at the origin, blurred by a Gaussian of `sigma`: the
// integral of the pixel's filter (`filter_cdf`) over the box. The kernel
// is separable, so the straight sides are closed form; only the corner
// arcs are sliced (`blurred_corner`). A sharp box is the exact product of
// its two edge pairs, an empty box covers nothing, and σ = 0 is the box's
// exact pixel coverage. `radius` is `(tl, tr, br, bl)`, fitted to the box.
// From σ = `CUTOUT_MIN_SIGMA` up, the corners are cut out instead
// (`cutout_box_coverage`).
fn blurred_box_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, cutouts: vec4<u32>) -> f32 {
    let reach = SHADOW_REACH_SIGMAS * sigma + PALANTIR_AA_HALF_WIDTH;
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

// The box whose blur a shadow paints, relative to its quad's center, with
// the radii fitted to it. A drop shadow's box is its source grown by the
// spread, centered on the quad, which is the source moved by `offset` and
// grown by the halo. An inset shadow's is the hole: the source shrunk by
// the spread, moved by `offset`. `vs_shadow` lays the cells out from it and
// hands it to the fragments, so the two cannot disagree.
struct ShadowBox {
    center: vec2<f32>,
    half: vec2<f32>,
    radius: vec4<f32>,
};

fn shadow_box(kind: u32, half: vec2<f32>, radius: vec4<f32>, geometry: vec4<f32>) -> ShadowBox {
    let spread = geometry.w;
    if (kind == SHADOW_DROP) {
        // CSS's radius rule for a spread, fitted to the shadow's own box,
        // as the inset hole's are.
        let shadow_half = max(shadow_source_half(half, geometry) + vec2<f32>(spread), vec2<f32>(0.0));
        return ShadowBox(vec2<f32>(0.0), shadow_half, fit_radii(spread_radius(radius, spread), shadow_half));
    }
    // The hole is the source shrunk by `spread`, so its radii follow the
    // CSS spread rule with `-spread` — less by `spread`, floored at 0, or
    // grown the way a drop shadow's are when the spread is negative — then
    // fit the hole's own box.
    let hole_half = max(half - vec2<f32>(spread), vec2<f32>(0.0));
    return ShadowBox(geometry.xy, hole_half, fit_radii(spread_radius(radius, -spread), hole_half));
}

// A drop shadow's source S, as half-extents: its quad less the halo.
fn shadow_source_half(half: vec2<f32>, geometry: vec4<f32>) -> vec2<f32> {
    return half - vec2<f32>(SHADOW_REACH_SIGMAS * geometry.z + max(geometry.w, 0.0));
}

// `blurred_box_coverage` at `p`, relative to the shadow's box, taking the
// cheaper form where it is exact. Strictly inside `core` on one axis (the
// box less `r + reach` there, `ShadowOut::core`), the point is past
// `reach` from both edges across that axis and from every corner's
// cutout, so only the pair along the other axis varies; inside it on both,
// the box covers the point fully. The Gaussian is taken as zero past
// `reach` there, as the cutouts and the quad's own bounds take it.
fn shadow_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, cutouts: vec4<u32>, core: vec4<f32>) -> f32 {
    let inside = (p > core.xy) & (p < core.zw);
    if (all(inside)) {
        return 1.0;
    }
    if (inside.x) {
        return clamp(filter_cdf(half.y - p.y, sigma) - filter_cdf(-half.y - p.y, sigma), 0.0, 1.0);
    }
    if (inside.y) {
        return clamp(filter_cdf(half.x - p.x, sigma) - filter_cdf(-half.x - p.x, sigma), 0.0, 1.0);
    }
    return blurred_box_coverage(p, half, radius, sigma, cutouts);
}

// `shadow_coverage` for a shadow blurred from `CUTOUT_MIN_SIGMA` up whose
// every shown corner reads a table: the sharp box less its cutouts, with
// an edge past `reach` taken whole (`reached_span`). Inside the core on one
// axis, that axis's span is whole and no corner reaches the point. Each
// span is written once, so the entry carries one copy of `filter_cdf` per
// edge rather than one per region.
fn tabled_coverage(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>, sigma: f32, cutouts: vec4<u32>, core: vec4<f32>) -> f32 {
    let inside = (p > core.xy) & (p < core.zw);
    if (all(inside)) {
        return 1.0;
    }
    let reach = SHADOW_REACH_SIGMAS * sigma + PALANTIR_AA_HALF_WIDTH;
    var span = vec2<f32>(1.0);
    if (!inside.x) {
        span.x = reached_span(p.x, half.x, sigma, reach);
    }
    if (!inside.y) {
        span.y = reached_span(p.y, half.y, sigma, reach);
    }
    var cut = 0.0;
    if (!any(inside)) {
        let q = corner_points(p, half, radius);
        cut = tabled_cutout(cutouts.z, q[2], sigma, reach)
            + tabled_cutout(cutouts.w, q[3], sigma, reach)
            + tabled_cutout(cutouts.x, q[0], sigma, reach)
            + tabled_cutout(cutouts.y, q[1], sigma, reach);
    }
    return clamp(span.x * span.y - cut, 0.0, 1.0);
}

// Where a shadow's pixel sits and what its source lets through. Both
// shadow entries start here and differ only in the coverage they take
// (`shadow_paint`).
struct ShadowPixel {
    // The pixel relative to the center of the shadow's box.
    p: vec2<f32>,
    // What the source lets through: 0 where it clips the pixel, otherwise
    // 1 for a drop shadow and the source's own coverage for an inset one.
    source: f32,
};

fn shadow_pixel(in: ShadowOut) -> ShadowPixel {
    let frag = unturned(frag_px(in.clip), in.turn);
    var px = ShadowPixel(frag - in.box_center, 0.0);
    let s = frag - in.source_center;
    if (in.kind == SHADOW_DROP) {
        // CSS clips an outer shadow inside the box that casts it
        // (Backgrounds 3 §7.1.1). Only where the source's own coverage is
        // full: the fill drawn over its edge pixels then blends with the
        // shadow under them as CSS's geometric clip would, where a clip at
        // the edge itself would open a seam. Outside the source's rect its
        // distance is positive, which the clip passes, so the distance is
        // taken only inside it.
        if (any(abs(s) >= in.source_half)
            || palantir_edge_coverage(palantir_sdf_rounded_box_centered(s, in.source_half, in.source_radius)) < 1.0 - SHADOW_CLIP_EPS) {
            px.source = 1.0;
        }
    }
    if (in.kind == SHADOW_INSET) {
        // Inset shadow: the source is the paint bbox, and the box is the
        // hole, the source deflated by `spread` per side and shifted by
        // `offset` (the light source moves in that direction → shadow
        // grows opposite). Coverage at a pixel inside the source is
        // `1 - blurred_cov(p relative to hole)`: outside the hole but
        // still inside the source is where the shadow paints; deep inside
        // the hole is lit (cov→0).
        //
        // Inset never paints outside the source, and across the source's
        // edge it takes the same coverage ramp the source's fill does, so
        // the two meet without a staircase on a rounded corner.
        px.source = palantir_edge_coverage(palantir_sdf_rounded_box_centered(s, in.source_half, in.source_radius));
    }
    return px;
}

// A shadow pixel's color at `cov`, the coverage of its box: a drop shadow
// paints it, an inset one its complement, both as far as the source and the
// rounded clips let.
fn shadow_paint(in: ShadowOut, px: ShadowPixel, cov: f32) -> vec4<f32> {
    let inset = in.kind == SHADOW_INSET;
    let clipped = palantir_clip_coverage(frag_px(in.clip), in.chain);
    return premultiply(in.fill.rgb, in.fill.a * px.source * clipped * select(cov, 1.0 - cov, inset));
}

// Drop and inset shadows. Its own entry, and so its own pipeline, because
// a pipeline gets the registers and code of everything its entry reaches:
// inside `fs`, the blurred-corner integral set every quad's. The schedule
// routes only the two shadow kinds here.
@fragment
fn fs_shadow(in: ShadowOut) -> @location(0) vec4<f32> {
    let px = shadow_pixel(in);
    if (px.source <= 0.0) {
        return vec4<f32>(0.0);
    }
    let cov = shadow_coverage(px.p, in.box_half, in.box_radius, in.sigma, in.cutouts, in.core);
    return shadow_paint(in, px, cov);
}

// `fs_shadow` for a shadow whose shown corners all read tables
// (`ShadowEntry::Tables` and `ShadowEntry::TablesWide`).
// Its own entry for the reason `fs_shadow` is one: it reaches neither the
// shaded cutout nor the outline integral, so a tiler runs it on more
// threads, with no spills.
@fragment
fn fs_shadow_tables(in: ShadowOut) -> @location(0) vec4<f32> {
    let px = shadow_pixel(in);
    if (px.source <= 0.0) {
        return vec4<f32>(0.0);
    }
    let cov = tabled_coverage(px.p, in.box_half, in.box_radius, in.sigma, in.cutouts, in.core);
    return shadow_paint(in, px, cov);
}
