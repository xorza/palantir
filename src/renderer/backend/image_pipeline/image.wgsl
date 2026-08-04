// User-image pipeline. Per-instance rect + tint; texture+sampler in
// group 0, switched per draw by the backend. Four-corner quad emitted
// implicitly via `vertex_index` (TriangleStrip).
//
// Colour pipeline: texture is `Rgba8UnormSrgb`, so the sampler decodes
// sRGB → linear automatically. We multiply by `tint` (linear, straight
// alpha) and premultiply at write time to match the rest of the
// premultiplied-blend pipeline.

// Viewport via the shared immediate region (offset 0). See
// `quad.wgsl` for the cross-pipeline layout rationale.
struct Viewport { size: vec2<f32> };
struct Immediates { viewport: Viewport };
var<immediate> imm: Immediates;

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

// Rec. 709 luma, for picking the brightest tap under `FLAG_TAPS_PEAK`. The
// whole tap wins rather than a per-channel `max`, so a coloured point source
// keeps its hue instead of being pushed toward white.
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

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
    // Four-corner triangle-strip: (0,0) (1,0) (0,1) (1,1).
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let phys = in.rect_min + c * in.rect_size;
    let ndc = vec2<f32>(
        phys.x / imm.viewport.size.x * 2.0 - 1.0,
        1.0 - phys.y / imm.viewport.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
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
fn tap(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tex, tex_smp, uv, 0.0);
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
    peak: bool,
) -> vec4<f32> {
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
            let s = tap(uv + uv_dx * t.x + uv_dy * t.y);
            // Combined premultiplied, which is what makes both modes correct
            // over alpha: averaging straight colour lets a transparent texel
            // drag an opaque one's rgb toward black, and ranking by straight
            // luma lets a near-invisible bright texel outrank a solid one.
            // Premultiplying weights each tap by its own coverage for free.
            let pm = vec4<f32>(s.rgb * s.a, s.a);
            sum += pm;
            // Branchless so both modes cost the same walk; the mode picks a
            // result at the end rather than a path here.
            let luma = dot(pm.rgb, LUMA);
            let brighter = luma > best_luma;
            best = select(best, pm, brighter);
            best_luma = select(best_luma, luma, brighter);
        }
    }
    let combined = select(sum / f32(n * n), best, peak);
    // Back to straight alpha, so this returns what `tap` does and the caller's
    // tint-then-premultiply path needs no branch of its own. Fully transparent
    // stays transparent rather than dividing by zero.
    return vec4<f32>(
        select(vec3<f32>(0.0), combined.rgb / combined.a, combined.a > 0.0),
        combined.a,
    );
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // `flags` is flat-interpolated per instance, so it is non-uniform
    // across a draw: the derivatives must be taken here, in uniform
    // control flow, even though only the filtered paths consume them.
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);

    // `ImageFit::Tile` ships UVs spanning [0, repeats]; wrap into the
    // [0,1) tile with `fract` (the ClampToEdge sampler would otherwise
    // clamp). Per-fragment, so each repeat samples the full tile. Other
    // fits keep UVs in [0,1] and sample directly — `fract(1.0)=0.0`
    // would wrap a Cover crop's far edge, so it must stay gated.
    var uv = in.uv;
    if ((in.flags & FLAG_TILED) != 0u) {
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
        s = tap(uv);
    } else {
        let texel_dx = uv_dx * dims;
        let texel_dy = uv_dy * dims;
        let footprint_squared = max(dot(texel_dx, texel_dx), dot(texel_dy, texel_dy));
        let minifying = footprint_squared > 1.0;

        let nearest = filtered & (FLAG_MIN_NEAREST | FLAG_MAG_NEAREST);
        if (nearest != 0u) {
            let filter_flag = select(FLAG_MAG_NEAREST, FLAG_MIN_NEAREST, minifying);
            // Snap the UV to the texel center for the active scale
            // direction. Single mip, so the snapped UV's derivatives cannot
            // select a different level.
            if ((nearest & filter_flag) != 0u) {
                uv = (floor(uv * dims) + vec2<f32>(0.5)) / dims;
            }
        }

        // Only minification has a footprint to cover: magnified and 1:1 draws
        // fall through to the single tap whatever the mode asked for.
        let taps = filtered & (FLAG_TAPS_MEAN | FLAG_TAPS_PEAK);
        if (taps != 0u && minifying) {
            s = footprint_taps(
                uv,
                uv_dx,
                uv_dy,
                sqrt(footprint_squared),
                (taps & FLAG_TAPS_PEAK) != 0u,
            );
        } else {
            s = tap(uv);
        }
    }

    // sRGB-format texture decodes to linear on read; tint is linear
    // straight-alpha. Multiply, then premultiply for the blend.
    let c = s * in.tint;
    return vec4<f32>(c.rgb * c.a, c.a);
}
