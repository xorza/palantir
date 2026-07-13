// Native parametric stroke pipeline (cubic beziers + circular arcs).
// One `draw(96, instance_count)` per scissor group; each instance
// describes a sub-range of one stroke and the vertex shader expands it
// into 16 quads (192 indices via the shared per-vertex `vertex_index`).
// No index buffer, no per-instance CPU tessellation.
//
// Basis kinds. `kind` selects how the geometry lanes are read:
// KIND_CUBIC evaluates the `p0..p3` cubic; KIND_ARC evaluates
// `p0 + p1.x * (cos θ, sin θ)` with `θ = mix(p2.x, p2.y, t)` — an
// exact circle (no flattening, no cubic-approximation error) whose
// gradient `t` tracks the sweep linearly. The Rust-side tags are
// pinned in `curve_pipeline.rs`.
//
// Lockstep contract: `SEGMENTS_PER_INSTANCE` here matches the const of
// the same name in `renderer/render_buffer.rs` — the composer derives
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

// Viewport via the shared immediate region (offset 0). See
// `quad.wgsl` for the cross-pipeline layout rationale.
struct Viewport { size: vec2<f32> };
struct Immediates { viewport: Viewport };
var<immediate> imm: Immediates;
// Gradient LUT atlas, shared with the quad pipeline. Sampled per
// fragment when `fill_kind != 0`. Same `Rgba16Float` (linear) format
// + linear filter / clamp-to-edge sampler as quad.wgsl — the curve's
// `t` is already in [0, 1] by construction, so spread is a no-op.
@group(0) @binding(0) var gradient_tex:     texture_2d<f32>;
@group(0) @binding(1) var gradient_sampler: sampler;
const ATLAS_ROWS_F: f32 = 256.0;

// `SEGMENTS_PER_INSTANCE` is substituted at shader-module construction
// from the Rust const of the same name (see `curve_pipeline.rs`). Don't
// change the placeholder syntax without updating the substitution.
const SEGMENTS_PER_INSTANCE: u32 = /*{SEGMENTS_PER_INSTANCE}*/16u;
const INV_N: f32 = 1.0 / f32(SEGMENTS_PER_INSTANCE);
const HALF_FRINGE: f32 = 0.5;

const CAP_BUTT: u32 = 0u;
const CAP_SQUARE: u32 = 1u;
const CAP_ROUND: u32 = 2u;

const KIND_CUBIC: u32 = 0u;
const KIND_ARC: u32 = 1u;

// `VsOut.flags` bits — the per-instance predicates the fragment
// actually branches on, packed once in `vs` so they ride one flat
// lane instead of three.
const FLAG_ROUND_CAP: u32 = 1u;
const FLAG_LINEAR_FILL: u32 = 2u;

const BRUSH_KIND_SOLID:  u32 = 0u;
const BRUSH_KIND_LINEAR: u32 = 1u;

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
    @location(8) fill_kind: u32,
    @location(9) fill_lut_row: u32,
    // Basis tag: KIND_CUBIC or KIND_ARC. Constant per instance.
    @location(10) kind: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // Signed perpendicular offset across the strip in physical px.
    // Fragment uses |offset| for the AA fringe alpha. Per-vertex.
    @location(0) offset: f32,
    // Signed tangential distance past the nearest endpoint: positive
    // in the cap zone, negative into the cap segment's body (so the
    // lerp zeroes exactly at the endpoint cross-section), 0 elsewhere.
    // Round/Square caps key on `> 0`; Butt never sees a positive value
    // because Butt doesn't extend.
    @location(1) cap_t: f32,
    // Strip half-extent: core half-width + AA fringe, physical px.
    // Fringe is baked in once here so the fragment's coverage is just
    // `clamp(half_w - r, 0, 1)`. Flat: constant per instance, like
    // every lane below except `curve_t`.
    @location(2) @interpolate(flat) half_w: f32,
    @location(3) @interpolate(flat) color: vec4<f32>,
    // `FLAG_*` bits (round cap / linear fill).
    @location(4) @interpolate(flat) flags: u32,
    // Gradient LUT row pre-resolved to the atlas `v` coordinate;
    // ignored without `FLAG_LINEAR_FILL`.
    @location(5) @interpolate(flat) lut_v: f32,
    // Per-vertex curve parameter `t` ∈ [0, 1] for LUT sampling. The
    // hardware lerps it across the strip cross-section, which is
    // correct: each strip cross-section corresponds to a single `t`,
    // so the lerp is constant along the cross-section.
    @location(6) curve_t: f32,
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
    // Degenerate tangent (coincident control points around `t`): fall
    // back to the chord p0→p3 so the strip doesn't collapse; a fully
    // degenerate curve projects along +x. Arcs never need this — their
    // tangent is unit-length by construction.
    if (dot(out.tan, out.tan) < 1.0e-8) {
        let chord = p3 - p0;
        out.tan = select(vec2<f32>(1.0, 0.0), chord, dot(chord, chord) >= 1.0e-8);
    }
    return out;
}

// Circular-arc position + tangent at `t`. Lanes: p0 = center,
// p1.x = radius, p2 = (a0, a1). The tangent's magnitude is irrelevant
// (normalized by the caller); its *sign* must follow the sweep
// direction so cap extension points outward at both ends. `select`
// instead of `sign()` — a degenerate a0 == a1 must not zero the
// tangent (sign(0) == 0 would collapse the strip).
fn arc_pos_tan(center: vec2<f32>, radius: f32, a0: f32, a1: f32, t: f32) -> PosTan {
    let ang = mix(a0, a1, t);
    let cs = vec2<f32>(cos(ang), sin(ang));
    var out: PosTan;
    out.pos = center + radius * cs;
    let dir = select(-1.0, 1.0, a1 >= a0);
    out.tan = dir * vec2<f32>(-cs.y, cs.x);
    return out;
}

// Kind dispatch: evaluate the instance's parametric basis at `t`.
fn stroke_pos_tan(in: VsIn, t: f32) -> PosTan {
    if (in.kind == KIND_ARC) {
        return arc_pos_tan(in.p0, in.p1.x, in.p2.x, in.p2.y, t);
    }
    return cubic_pos_tan(in.p0, in.p1, in.p2, in.p3, t);
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
    let pt = stroke_pos_tan(in, t);
    let pos = pt.pos;
    let tan_n = normalize(pt.tan);
    // Right-hand perpendicular (rotate +90°). Sign matches the cap
    // convention used by stroke_tessellate.
    let normal = vec2<f32>(-tan_n.y, tan_n.x);
    let half_w = max(in.width * 0.5, 0.0) + HALF_FRINGE;

    // Cap extension. Only the leading edge of segment 0 of the first
    // sub-instance (and the trailing edge of the last segment of the
    // last sub-instance) shifts; everything else stays put.
    let is_first_cap_seg = (seg == 0u) && (in.t_range.x < T_END_EPS);
    let is_last_cap_seg = (seg == SEGMENTS_PER_INSTANCE - 1u)
        && (in.t_range.y > 1.0 - T_END_EPS);
    var cap_shift: f32 = 0.0;
    // `cap_t` must lerp to zero exactly at the endpoint cross-section,
    // so a cap segment's body edge carries -chord (not 0): the linear
    // function -s is then exact across the fused cap+body quad. With 0
    // at the body edge the zero landed at the segment's far edge and
    // the round-cap SDF over-estimated r through the whole segment,
    // visibly necking thin caps (~chord^2 / stroke width).
    var cap_t: f32 = 0.0;
    if (in.cap != CAP_BUTT) {
        if (is_first_cap_seg) {
            if (t_off == 0.0) {
                cap_shift = -half_w;
                cap_t = half_w;
            } else {
                let lead = stroke_pos_tan(in, in.t_range.x);
                cap_t = -distance(pos, lead.pos);
            }
        }
        if (is_last_cap_seg) {
            if (t_off == 1.0) {
                cap_shift = half_w;
                cap_t = half_w;
            } else {
                let trail = stroke_pos_tan(in, in.t_range.y);
                cap_t = -distance(pos, trail.pos);
            }
        }
    }

    let offset = side * half_w;
    let phys = pos + normal * offset + tan_n * cap_shift;
    let inv_size_2 = 2.0 / imm.viewport.size;
    let ndc = vec2<f32>(
        phys.x * inv_size_2.x - 1.0,
        1.0 - phys.y * inv_size_2.y,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.offset = offset;
    out.half_w = half_w;
    out.color = in.color;
    out.cap_t = cap_t;
    out.flags = u32(in.cap == CAP_ROUND)
        | (u32((in.fill_kind & 0xFFu) == BRUSH_KIND_LINEAR) << 1u);
    out.lut_v = (f32(in.fill_lut_row) + 0.5) / ATLAS_ROWS_F;
    out.curve_t = t;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let dist = abs(in.offset);
    // Body (cap_t == 0) and Square cap (cap_t > 0, no rounding) use
    // the cross-strip distance; a Round cap swaps in the distance to
    // the endpoint. The endpoint cross-section is exactly cap_t == 0
    // (the vertex shader emits -chord at the cap segment's body edge
    // so the lerp lands there); cap_t > 0 is the projected cap zone,
    // which Butt never sees because the vertex shader doesn't extend.
    var r = dist;
    if (in.cap_t > 0.0 && (in.flags & FLAG_ROUND_CAP) != 0u) {
        r = length(vec2<f32>(in.cap_t, dist));
    }
    let coverage = clamp(in.half_w - r, 0.0, 1.0);
    var rgba = in.color;
    if ((in.flags & FLAG_LINEAR_FILL) != 0u) {
        // `curve_t` is in [0, 1] by construction and the sampler is
        // clamp-to-edge, so no explicit clamp.
        rgba = textureSample(gradient_tex, gradient_sampler, vec2<f32>(in.curve_t, in.lut_v));
    }
    let a = rgba.a * coverage;
    return vec4<f32>(rgba.rgb * a, a);
}
