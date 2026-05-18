// Native cubic-bezier stroke pipeline. One `draw(96, instance_count)`
// per scissor group; each instance describes a sub-range of one curve
// and the vertex shader expands it into 16 quads (192 indices via the
// shared per-vertex `vertex_index`). No index buffer, no per-instance
// CPU tessellation.
//
// Lockstep contract: `SEGMENTS_PER_INSTANCE` here matches the const of
// the same name in `frontend/composer/mod.rs` — the composer derives
// the adaptive sub-instance count assuming the shader subdivides each
// instance into exactly this many chords. Bump together.
//
// Caps. Encoded as `cap: u32` per instance (0 = Butt, 1 = Square,
// 2 = Round). The leading sub-instance (`t_range.x ≈ 0`) and trailing
// sub-instance (`t_range.y ≈ 1`) shift their outermost vertices by
// `half_w` along the tangent for non-Butt caps; interior sub-instances
// don't extend. `cap_t` (signed tangential distance past the endpoint)
// rides as a varying — fragment uses it for the round-cap SDF; butt
// throws away samples with `cap_t > 0` (none, since no extension);
// square keeps full coverage in the extension zone.
//
// Shader contract: straight-alpha linear in, premultiplied linear out
// — same as mesh.wgsl / quad.wgsl. The pipeline uses
// PREMULTIPLIED_ALPHA_BLENDING.

struct Viewport { size: vec2<f32> };
@group(0) @binding(0) var<uniform> vp: Viewport;

// `SEGMENTS_PER_INSTANCE` is substituted at shader-module construction
// from the Rust const of the same name (see `curve_pipeline.rs`). Don't
// change the placeholder syntax without updating the substitution.
const SEGMENTS_PER_INSTANCE: u32 = /*{SEGMENTS_PER_INSTANCE}*/16u;
const INV_N: f32 = 1.0 / f32(SEGMENTS_PER_INSTANCE);
const HALF_FRINGE: f32 = 0.5;

const CAP_BUTT: u32 = 0u;
const CAP_SQUARE: u32 = 1u;
const CAP_ROUND: u32 = 2u;

// Epsilon for the "is this the curve's leading / trailing endpoint?"
// test against `t_range`. Sub-instance boundaries are exactly
// `i / n`, so the only true matches come out as 0.0 and 1.0 exactly;
// keep a small slack against float drift through the composer's
// `1.0 / n` math.
const T_END_EPS: f32 = 1.0e-4;

struct VsIn {
    @location(0) p0: vec2<f32>,
    @location(1) p1: vec2<f32>,
    @location(2) p2: vec2<f32>,
    @location(3) p3: vec2<f32>,
    // `t_range.x = t0`, `t_range.y = t1` — the sub-range of the
    // parent curve this instance covers in [0, 1].
    @location(4) t_range: vec2<f32>,
    @location(5) width: f32,
    @location(6) color: vec4<f32>,
    @location(7) cap: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // Signed perpendicular offset across the strip in physical px.
    // Fragment uses |offset| for the AA fringe alpha. Per-vertex.
    @location(0) offset: f32,
    // Tangential distance past the nearest endpoint (>= 0 inside the
    // cap zone, 0 inside the body). Round/Square caps key on this;
    // Butt never sees a non-zero value because Butt doesn't extend.
    // Per-vertex (only the cap-zone corners carry a non-zero value).
    @location(1) cap_t: f32,
    // Per-instance: constant across all 96 verts of an instance, so
    // flat-interpolate to skip per-fragment varying evaluation.
    @location(2) @interpolate(flat) half_w: f32,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) cap_kind: u32,
};

struct PosTan { pos: vec2<f32>, tan: vec2<f32> };

// Fused cubic position + tangent at `t`. Shares `u*u`, `u*t`, `t*t`
// across both expressions — the standalone `cubic` / `cubic_tangent`
// pair recomputed them independently and relied on the compiler to
// CSE, which isn't guaranteed through the WGSL→SPIR-V→native chain.
fn cubic_pos_tan(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, t: f32) -> PosTan {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let ut = u * t;
    var out: PosTan;
    out.pos = (uu * u) * p0
            + (3.0 * uu * t) * p1
            + (3.0 * u * tt) * p2
            + (tt * t) * p3;
    out.tan = (3.0 * uu) * (p1 - p0)
            + (6.0 * ut) * (p2 - p1)
            + (3.0 * tt) * (p3 - p2);
    return out;
}

// (corner_t, side) lookup for the two-triangle quad. Replaces a 6-way
// switch — branchless on every backend.
const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(0.0, -1.0),
    vec2<f32>(1.0, -1.0),
    vec2<f32>(0.0,  1.0),
    vec2<f32>(0.0,  1.0),
    vec2<f32>(1.0, -1.0),
    vec2<f32>(1.0,  1.0),
);

@vertex
fn vs(in: VsIn, @builtin(vertex_index) vid: u32) -> VsOut {
    let seg = vid / 6u;
    let corner = vid % 6u;
    // Triangle layout per quad (two triangles, CCW after the Y-flip
    // in NDC). `t_off` is the segment-local parameter (0 at the start
    // edge, 1 at the end edge); `side` is the strip-half marker (-1/+1).
    let c = CORNERS[corner];
    let t_off = c.x;
    let side = c.y;
    let local_t = (f32(seg) + t_off) * INV_N;
    let t = mix(in.t_range.x, in.t_range.y, local_t);
    let pt = cubic_pos_tan(in.p0, in.p1, in.p2, in.p3, t);
    let pos = pt.pos;
    var tan = pt.tan;
    let len_sq = dot(tan, tan);
    if (len_sq < 1.0e-8) {
        // Degenerate tangent (coincident control points around `t`).
        // Fall back to the chord p0→p3 so the strip doesn't collapse;
        // if that's also zero, project along +x.
        tan = in.p3 - in.p0;
        let l2 = dot(tan, tan);
        if (l2 < 1.0e-8) {
            tan = vec2<f32>(1.0, 0.0);
        }
    }
    let tan_n = normalize(tan);
    // Right-hand perpendicular (rotate +90°). Sign matches the cap
    // convention used by stroke_tessellate.
    let normal = vec2<f32>(-tan_n.y, tan_n.x);
    let core_hw = max(in.width * 0.5, 0.0);
    let half_w = core_hw + HALF_FRINGE;

    // Cap extension. Only the leading edge of segment 0 of the first
    // sub-instance (and the trailing edge of the last segment of the
    // last sub-instance) shifts; everything else stays put.
    let is_leading_edge = (seg == 0u) && (t_off == 0.0)
        && (in.t_range.x < T_END_EPS);
    let is_trailing_edge = (seg == SEGMENTS_PER_INSTANCE - 1u) && (t_off == 1.0)
        && (in.t_range.y > 1.0 - T_END_EPS);
    var cap_shift: f32 = 0.0;
    if (in.cap != CAP_BUTT) {
        if (is_leading_edge) { cap_shift = -half_w; }
        if (is_trailing_edge) { cap_shift =  half_w; }
    }

    let offset = side * half_w;
    let phys = pos + normal * offset + tan_n * cap_shift;
    let inv_size_2 = 2.0 / vp.size;
    let ndc = vec2<f32>(
        phys.x * inv_size_2.x - 1.0,
        1.0 - phys.y * inv_size_2.y,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.offset = offset;
    out.half_w = core_hw;
    out.color = in.color;
    out.cap_t = abs(cap_shift);
    out.cap_kind = in.cap;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let dist = abs(in.offset);
    var coverage: f32;
    if (in.cap_t > 0.0 && in.cap_kind == CAP_ROUND) {
        // Round cap: distance to the endpoint, not to the centerline.
        // Endpoint sits at (cap_t = 0, offset = 0) in local strip
        // coords; cap_t > 0 is the projected cap zone.
        let r = sqrt(in.cap_t * in.cap_t + dist * dist);
        coverage = clamp(in.half_w - r + HALF_FRINGE, 0.0, 1.0);
    } else {
        // Body (cap_t == 0) and Square cap (cap_t > 0, no rounding):
        // standard cross-strip AA. Butt never sees cap_t > 0 because
        // the vertex shader doesn't extend for Butt.
        coverage = clamp(in.half_w - dist + HALF_FRINGE, 0.0, 1.0);
    }
    let a = in.color.a * coverage;
    return vec4<f32>(in.color.rgb * a, a);
}
