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
//   0 = solid (use `fill` directly)
//   1 = linear gradient (sample `gradient_tex` via `fill_axis` projection)
const BRUSH_KIND_SOLID:  u32 = 0u;
const BRUSH_KIND_LINEAR: u32 = 1u;
// Spread mode (bits 8..16 of fill_kind), only meaningful when kind == 1.
const SPREAD_PAD:     u32 = 0u;
const SPREAD_REPEAT:  u32 = 1u;
const SPREAD_REFLECT: u32 = 2u;

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
    // Linear gradient.
    let spread  = (in.fill_kind >> 8u) & 0xFFu;
    let local01 = in.local / in.size;
    let axis    = in.fill_axis.xy;
    let t0      = in.fill_axis.z;
    let t1      = in.fill_axis.w;
    let raw     = dot(local01, axis);
    let span    = t1 - t0;
    let t01     = select(0.0, (raw - t0) / span, abs(span) > 1e-6);
    let t       = apply_spread(t01, spread);
    let v       = (f32(in.fill_lut_row) + 0.5) / ATLAS_ROWS_F;
    return textureSample(gradient_tex, gradient_sampler, vec2<f32>(t, v));
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
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
