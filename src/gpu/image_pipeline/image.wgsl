// User-image pipeline. Per-instance rect + tint; texture+sampler in
// group 0, switched per draw by the backend. Four-corner quad emitted
// implicitly via `vertex_index` (TriangleStrip).
//
// Colour pipeline: texture is `Rgba8UnormSrgb`, so the sampler decodes
// sRGB → linear automatically. We multiply by `tint` (linear, straight
// alpha) and premultiply at write time to match the rest of the
// premultiplied-blend pipeline.

@group(0) @binding(0) var tex:     texture_2d<f32>;
@group(0) @binding(1) var tex_smp: sampler;

// Bits of `flags`, substituted from the `IMG_FLAG_*` constants in
// `render_buffer::image` — see `ImagePipeline::new`. Rust owns the values, so
// there is no second definition here to drift out of step with them.
const FLAG_TILED:       u32 = /*{IMG_FLAG_TILED}*/;
const FLAG_MIN_NEAREST: u32 = /*{IMG_FLAG_MIN_NEAREST}*/;
const FLAG_MAG_NEAREST: u32 = /*{IMG_FLAG_MAG_NEAREST}*/;
const FLAG_TAPS_MEAN:   u32 = /*{IMG_FLAG_TAPS_MEAN}*/;
const FLAG_TAPS_PEAK:   u32 = /*{IMG_FLAG_TAPS_PEAK}*/;

// Taps per axis at the widest footprint. Each tap is itself a bilinear 2×2,
// so this many of them read about `2 * MAX_TAPS_PER_AXIS` texels per axis:
// coverage is complete out to an 8-texel footprint (8× minification) and an
// evenly spread sample of it past that. The cap is what bounds the cost —
// covering *every* footprint would mean reading the whole texture per frame,
// which is a mip build done the expensive way.
//
// Cheaper than the fetch count suggests: at 9 taps (5× minification) the image
// batch runs 1.8× the same draw with taps off, not 9×, because neighbouring
// taps overlap in the texture cache (`image_pipeline` bench, `minified_*`,
// RTX 4090 Laptop / Vulkan). Both reductions measure the same, which is what
// the branchless accumulate below is for.
const MAX_TAPS_PER_AXIS: i32 = 4;

// Squared footprint below which the tap grid collapses to `n = 1` — a single
// tap at the fragment's own UV, which is exactly what the plain path does. Two
// texels squared, since each tap already spans two. Gating here keeps a barely
// minified image off the loop.
const MIN_TAPPED_FOOTPRINT_SQUARED: f32 = 4.0;

struct VsIn {
    // Per-instance.
    @location(0) rect_min:  vec2<f32>,
    @location(1) rect_size: vec2<f32>,
    @location(2) uv_min:    vec2<f32>,
    @location(3) uv_size:   vec2<f32>,
    @location(4) tint:      vec4<f32>,
    @location(5) flags:     u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)        uv:   vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, in: VsIn) -> VsOut {
    let c = CORNERS[vi];
    var out: VsOut;
    out.clip = clip_from_px(in.rect_min + c * in.rect_size);
    out.uv   = in.uv_min + c * in.uv_size;
    out.tint = in.tint;
    out.flags = in.flags;
    return out;
}

// One bilinear tap. Every sampling path goes through here rather than
// `textureSample`: the texture has exactly one mip level, so the LOD
// `textureSample` derives is always 0 anyway, and taking it explicitly frees
// the tap loop below from WGSL's uniform-control-flow rule — `flags` is
// flat-interpolated per instance, so *nothing* downstream of it is uniform.
//
// A tiled draw filters itself, because `ClampToEdge` cannot express a
// repeat and the sampler is shared with the gradient LUT. Wrapping the
// coordinate is not enough: the filter reads the *neighbour* too, and at
// a seam that neighbour clamps to the edge texel instead of continuing
// into the next repeat — so the last half-texel of every tile blends
// with itself. The wrap has to reach each fetched texel, which only a
// hand-written filter can do. Four point fetches instead of one bilinear,
// on the tiled path alone.
fn tap(uv: vec2<f32>, tiled: bool) -> vec4<f32> {
    if (!tiled) {
        return textureSampleLevel(tex, tex_smp, uv, 0.0);
    }
    let dims = vec2<i32>(textureDimensions(tex));
    // Texel space as the sampler sees it: uv `(i + 0.5) / n` is the centre
    // of texel `i`, so the half-texel comes off before the floor.
    let t = fract(uv) * vec2<f32>(dims) - 0.5;
    let base = floor(t);
    let f = t - base;
    let i0 = vec2<i32>(base);
    let lo = vec2<i32>(wrap_texel(i0.x, dims.x), wrap_texel(i0.y, dims.y));
    let hi = vec2<i32>(wrap_texel(i0.x + 1, dims.x), wrap_texel(i0.y + 1, dims.y));
    let c00 = textureLoad(tex, lo, 0);
    let c10 = textureLoad(tex, vec2<i32>(hi.x, lo.y), 0);
    let c01 = textureLoad(tex, vec2<i32>(lo.x, hi.y), 0);
    let c11 = textureLoad(tex, hi, 0);
    // The weights are `f32` here rather than the fixed-point the sampler
    // interpolates with, which is the one thing this path gains for its
    // extra fetches.
    return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

// `i` brought into `[0, n)`. WGSL's `%` keeps the sign of the dividend,
// so a texel one to the left of the tile needs the extra turn.
fn wrap_texel(i: i32, n: i32) -> i32 {
    return ((i % n) + n) % n;
}

// A grid of taps tiling the fragment's source footprint — the parallelogram
// spanned by the UV derivatives — combined by average or by brightest.
//
// This is what stops fine detail scintillating under a pan. One bilinear tap
// reads 2×2 texels wherever the fractional UV lands, so a minified image shows
// a shifting sample of each pixel's footprint and its contents blink in and
// out; enough taps to cover the footprint have nothing left to shift between.
fn footprint_taps(
    uv: vec2<f32>,
    uv_dx: vec2<f32>,
    uv_dy: vec2<f32>,
    footprint: f32,
    flags: u32,
) -> vec4<f32> {
    let peak = (flags & FLAG_TAPS_PEAK) != 0u;
    // The taps step *off* the base UV by up to half the footprint, so a
    // tiled draw's leave [0,1) on their own and land in a neighbouring
    // repeat. `tap` wraps each fetch, so that is where they read — and
    // the flag has to reach it, since a Cover crop must not have its far
    // edge taken round to its near one.
    let tiled = (flags & FLAG_TILED) != 0u;
    // Each tap spans 2 texels, so a footprint that wide needs half as many.
    let n = clamp(i32(ceil(footprint * 0.5)), 1, MAX_TAPS_PER_AXIS);
    let step = 1.0 / f32(n);
    var sum = vec4<f32>(0.0);
    var best = vec4<f32>(0.0);
    var best_luma = -1.0;
    for (var j = 0; j < n; j++) {
        for (var i = 0; i < n; i++) {
            // (k + 0.5)/n - 0.5 walks tap centres across -0.5..0.5 of the
            // derivative span, so the grid tiles the footprint symmetrically
            // about the fragment's own UV.
            let t = (vec2<f32>(f32(i), f32(j)) + 0.5) * step - 0.5;
            let p = uv + uv_dx * t.x + uv_dy * t.y;
            // Premultiplied, as every tap is, which is what makes both
            // modes correct over alpha: averaging straight colour lets a
            // transparent texel drag an opaque one's rgb toward black,
            // and ranking by straight luma lets a near-invisible bright
            // texel outrank a solid one. Each tap already carries its own
            // coverage as its weight.
            let s = tap(p, tiled);
            sum += s;
            // Branchless so both modes cost the same walk; the mode picks a
            // result at the end rather than a path here. The whole tap wins
            // rather than a per-channel `max`, so a coloured point source
            // keeps its hue instead of being pushed toward white.
            let luma = dot(s.rgb, LUMA);
            let brighter = luma > best_luma;
            best = select(best, s, brighter);
            best_luma = select(best_luma, luma, brighter);
        }
    }
    // Premultiplied out, as it came in — what `tap` returns, so the
    // caller cannot tell the two paths apart.
    return select(sum / f32(n * n), best, peak);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // `flags` is flat-interpolated per instance, so it is non-uniform
    // across a draw: the derivatives must be taken here, in uniform
    // control flow, even though only the filtered paths consume them.
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);

    // `ImageFit::Tile` ships UVs spanning [0, repeats]; `tap` wraps into
    // the [0,1) tile per fetched texel, so each repeat samples the full
    // tile and a seam blends across it. Other fits keep UVs in [0,1] and
    // sample directly — a wrap would take a Cover crop's far edge round
    // to its near one, so it must stay gated.
    //
    // Wrapped here as well, though `tap` would: the nearest snap below
    // works in texel space, and a UV twenty repeats along has that many
    // fewer bits left for the half-texel it adds.
    let tiled = (in.flags & FLAG_TILED) != 0u;
    var uv = in.uv;
    if (tiled) {
        uv = fract(in.uv);
    }

    // Deliberately outside the branch below: gating the descriptor read
    // as well costs the nearest path more than it saves the bilinear one
    // (`image_pipeline` bench, 24-layer overdraw).
    let dims = vec2<f32>(textureDimensions(tex));
    // Zero filter bits is the common case (every plain image and GpuView);
    // the footprint measurement feeds the texel-center snap and the tap
    // count, so keep *that* out of the bilinear path entirely.
    var s: vec4<f32>;
    let filtered = in.flags
        & (FLAG_MIN_NEAREST | FLAG_MAG_NEAREST | FLAG_TAPS_MEAN | FLAG_TAPS_PEAK);
    if (filtered == 0u) {
        s = tap(uv, tiled);
    } else {
        let texel_dx = uv_dx * dims;
        let texel_dy = uv_dy * dims;
        let footprint_squared = max(dot(texel_dx, texel_dx), dot(texel_dy, texel_dy));
        let minifying = footprint_squared > 1.0;

        // Snap the UV to the texel center for the active scale direction.
        // Single mip, so the snapped UV's derivatives cannot select a
        // different level. No outer `nearest != 0u` guard: with neither bit
        // set the mask below is already zero, and the branch cost more than
        // the one `select` it skipped.
        let nearest = filtered & (FLAG_MIN_NEAREST | FLAG_MAG_NEAREST);
        let filter_flag = select(FLAG_MAG_NEAREST, FLAG_MIN_NEAREST, minifying);
        if ((nearest & filter_flag) != 0u) {
            uv = (floor(uv * dims) + vec2<f32>(0.5)) / dims;
        }

        // Only a footprint worth covering earns the loop: magnified and 1:1
        // draws have none, and under two texels the grid is one tap at this
        // same UV — which is the `else` arm, reached without the machinery.
        let taps = filtered & (FLAG_TAPS_MEAN | FLAG_TAPS_PEAK);
        if (taps != 0u && footprint_squared > MIN_TAPPED_FOOTPRINT_SQUARED) {
            s = footprint_taps(uv, uv_dx, uv_dy, sqrt(footprint_squared), in.flags);
        } else {
            s = tap(uv, tiled);
        }
    }

    // `s` is premultiplied already; the tint is not, so it is premultiplied
    // here and the two multiply as the premultiplied values they both are.
    return s * premultiply(in.tint.rgb, in.tint.a);
}
