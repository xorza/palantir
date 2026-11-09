// A gradient fill at a point of a shape's own 0..1 frame: the kind's parameter, spread, and the
// LUT's color times the fill. The includer declares `PALANTIR_FILL_TAG_MASK` and
// `PALANTIR_ZERO_EPS`, and includes `gradient_lut.wgsl`.

// The gradient kinds' tags (`FillKind`), conic the one left when neither matches:
//   linear (sample LUT via `fill_axis = (dir.xy, t0, t1)`)
//   radial (sample LUT via `fill_axis = (cx, cy, rx, ry)`)
//   conic  (sample LUT via `fill_axis = (cx, cy, start_angle, _)`)
const PALANTIR_FILL_LINEAR:       u32 = /*{FILL_LINEAR}*/;
const PALANTIR_FILL_RADIAL:       u32 = /*{FILL_RADIAL}*/;

// The spread mode's field of `fill_kind`, `PALANTIR_SPREAD_SHIFT` up.
const PALANTIR_SPREAD_SHIFT: u32 = /*{SPREAD_SHIFT}*/;
const PALANTIR_SPREAD_MASK: u32 = /*{SPREAD_MASK}*/;

// Spread mode, only meaningful for gradients.
// `Pad` is the fallback below rather than a constant of its own.
const PALANTIR_SPREAD_REPEAT:  u32 = /*{SPREAD_REPEAT}*/;
const PALANTIR_SPREAD_REFLECT: u32 = /*{SPREAD_REFLECT}*/;
const PALANTIR_TAU: f32 = 6.2831853;

// Apply the user-selected spread mode to a parametric `t`. The `Pad`
// clamp is kept here rather than left to the sampler's clamp
// addressing, so the contract holds whatever the sampler is set to.
fn palantir_apply_spread(t: f32, mode: u32) -> f32 {
    if (mode == PALANTIR_SPREAD_REPEAT) {
        return fract(t);
    }
    if (mode == PALANTIR_SPREAD_REFLECT) {
        return abs(fract(t * 0.5) - 0.5) * 2.0;
    }
    // Pad, the remaining mode.
    return clamp(t, 0.0, 1.0);
}

// The gradient of `fill_kind` at `local01`, a point of the quad's own 0..1 frame, times `fill`.
fn palantir_gradient_fill(local01: vec2<f32>, fill_kind: u32, fill_axis: vec4<f32>, lut_row: u32, fill: vec4<f32>) -> vec4<f32> {
    let kind = fill_kind & PALANTIR_FILL_TAG_MASK;
    let spread  = (fill_kind >> PALANTIR_SPREAD_SHIFT) & PALANTIR_SPREAD_MASK;
    var t01: f32 = 0.0;
    if (kind == PALANTIR_FILL_LINEAR) {
        let axis = fill_axis.xy;
        let t0   = fill_axis.z;
        let t1   = fill_axis.w;
        let raw  = dot(local01, axis);
        let span = t1 - t0;
        let span_safe = select(1.0, span, abs(span) > PALANTIR_ZERO_EPS);
        t01 = (raw - t0) / span_safe;
    } else if (kind == PALANTIR_FILL_RADIAL) {
        // `t = 1` on the ellipse the radius vector spans.
        let center = fill_axis.xy;
        let radius = fill_axis.zw;
        let rx = select(1.0, radius.x, abs(radius.x) > PALANTIR_ZERO_EPS);
        let ry = select(1.0, radius.y, abs(radius.y) > PALANTIR_ZERO_EPS);
        let d  = (local01 - center) / vec2<f32>(rx, ry);
        t01 = length(d);
    } else {
        // Conic, the remaining kind (`QuadForm::of`): clockwise on screen, as y points down.
        // `+ 1.0` lifts atan2's negative half before `fract`.
        let center      = fill_axis.xy;
        let start_angle = fill_axis.z;
        let p           = local01 - center;
        let theta       = atan2(p.y, p.x);
        t01 = fract((theta - start_angle) / PALANTIR_TAU + 1.0);
    }
    let t = palantir_apply_spread(t01, spread);
    // `fill` multiplies the straight sample, channel by channel. It is
    // white for every gradient, with its alpha scaled by a paint
    // animation's fade. See `BrushSource::gpu_fill`.
    return palantir_gradient_lut(t, lut_row) * fill;
}
