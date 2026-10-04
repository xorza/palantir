// Palantir raster-atlas shader — the draw program for both the glyph atlas
// and the icon atlas. Contract:
// - color comes in straight-alpha linear f16 (no sRGB decode here).
// - output is premultiplied linear: vec4(rgb*a, a).
// - blend = PREMULTIPLIED_ALPHA_BLENDING; render target is sRGB
//   (GPU re-encodes on write).
// - mask atlas = R8Unorm linear; color atlas = Rgba8UnormSrgb
//   (a load decodes it to linear straight RGBA).
// - `uv_and_kind` packs u, two flags, and v; Rust owns the field widths and
//   substitutes them below. Both atlases cap well under the room u gets.

struct VertexIn {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,           // raster texels (w | h<<16)
    @location(2) size: u32,          // drawn physical px (w | h<<16)
    @location(3) uv_and_kind: u32,   // (u | flags<<U_BITS | v<<16)
    // Linear straight RGBA — the `Float16x4` fetch widens in hardware,
    // no shader unpack.
    @location(4) color: vec4<f32>,
}

struct VertexOut {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,          // linear straight
    @location(1) @interpolate(flat) flags: u32, // FLAG_* below
    // Atlas texel coordinate. A quad drawn at its raster's own size sits on
    // whole pixels, so every fragment centre lands on a texel centre and the
    // texel it reads is this one, truncated.
    @location(2) texel: vec2<f32>,
    // The raster's texel rect as inclusive min/max — read only under
    // FLAG_RESAMPLE.
    @location(3) @interpolate(flat) texel_rect: vec4<i32>,
}

// The `uv_and_kind` layout. Rust owns every number here and substitutes it in
// — see `raster_atlas::quad`, which panics if a marker goes unreplaced.
const U_BITS: u32 = /*{U_BITS}*/;
const U_MASK: u32 = (1u << U_BITS) - 1u;
// The two flags, already shifted down past `u`.
const FLAG_DESATURATE: u32 = /*{FLAG_DESATURATE}*/;  // colour icons only; see `fs`
const FLAG_COLOR: u32 = /*{FLAG_COLOR}*/;            // sample colour, not mask
// Set by `vs`, not carried in: the quad is drawn at a size other than its
// raster's, so `fs` filters instead of reading texel for texel.
const FLAG_RESAMPLE: u32 = 4u;

// Group(0) = the atlas textures. Every read is a `textureLoad` by texel,
// so there is no sampler.
@group(0) @binding(0) var mask_atlas: texture_2d<f32>;
@group(0) @binding(1) var color_atlas: texture_2d<f32>;

@vertex
fn vs(in: VertexIn) -> VertexOut {
    let w = in.dim & 0xFFFFu;
    let h = (in.dim >> 16u) & 0xFFFFu;

    // u in the low U_BITS, the two flags above it, v in the upper 16.
    let u = in.uv_and_kind & U_MASK;
    let flags = (in.uv_and_kind >> U_BITS) & 0x3u;
    let v = (in.uv_and_kind >> 16u) & 0xFFFFu;

    let corner = vec2<u32>(in.idx & 1u, (in.idx >> 1u) & 1u);
    let dim = vec2<u32>(w, h);
    let size = vec2<u32>(in.size & 0xFFFFu, (in.size >> 16u) & 0xFFFFu);
    let pos = in.pos + vec2<i32>(size * corner);
    let uv_texel = vec2<f32>(vec2<u32>(u, v) + dim * corner);

    var out: VertexOut;
    out.position = clip_from_px(vec2<f32>(pos));

    // Straight-alpha linear color. Shader premuls at output; no sRGB
    // decode — the instance lanes are linear.
    out.color = in.color;
    out.flags = flags | select(0u, FLAG_RESAMPLE, any(size != dim));
    out.texel = uv_texel;
    out.texel_rect = vec4<i32>(vec2<i32>(vec2<u32>(u, v)), vec2<i32>(vec2<u32>(u, v) + dim) - 1);
    return out;
}

// Bilinear weights and the four texel addresses around `texel`, clamped to
// the raster's own rect so a tap never reads a neighbouring slot.
struct Taps {
    lo: vec2<i32>,
    hi: vec2<i32>,
    f: vec2<f32>,
}

fn taps(texel: vec2<f32>, rect: vec4<i32>) -> Taps {
    let p = texel - vec2<f32>(0.5);
    let base = floor(p);
    let i = vec2<i32>(base);
    return Taps(
        clamp(i, rect.xy, rect.zw),
        clamp(i + vec2<i32>(1), rect.xy, rect.zw),
        p - base,
    );
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    if ((in.flags & FLAG_RESAMPLE) != 0u) {
        return resampled(in);
    }
    if ((in.flags & FLAG_COLOR) == 0u) {
        // Mask: vertex color modulated by R-channel coverage.
        let cov = textureLoad(mask_atlas, vec2<i32>(in.texel), 0).x;
        return premultiply(in.color.rgb, in.color.a * cov);
    }
    // Colour emoji or colour icon: the sRGB texture decodes to linear
    // straight RGBA on sample. Premultiply at output; the run alpha modulates
    // the whole premultiplied result, so faded text fades its emoji too and a
    // faded icon fades whole.
    let s = textureLoad(color_atlas, vec2<i32>(in.texel), 0);
    // DESATURATE collapses the artwork to its luminance — the disabled look
    // for an icon whose own colours the tint cannot replace. Alpha is
    // untouched, so the shape is unchanged and only the hue goes.
    let grey = vec3<f32>(dot(s.rgb, LUMA));
    let rgb = select(s.rgb, grey, (in.flags & FLAG_DESATURATE) != 0u);
    return premultiply(rgb, s.a) * in.color.a;
}

// An icon drawn at a size other than its raster's: the four nearest texels,
// filtered by hand, clamped to the raster's own slot. Colour taps are
// premultiplied before they blend, like every colour interpolation.
fn resampled(in: VertexOut) -> vec4<f32> {
    let t = taps(in.texel, in.texel_rect);
    if ((in.flags & FLAG_COLOR) == 0u) {
        let a = textureLoad(mask_atlas, vec2<i32>(t.lo.x, t.lo.y), 0).x;
        let b = textureLoad(mask_atlas, vec2<i32>(t.hi.x, t.lo.y), 0).x;
        let c = textureLoad(mask_atlas, vec2<i32>(t.lo.x, t.hi.y), 0).x;
        let d = textureLoad(mask_atlas, vec2<i32>(t.hi.x, t.hi.y), 0).x;
        let cov = mix(mix(a, b, t.f.x), mix(c, d, t.f.x), t.f.y);
        return premultiply(in.color.rgb, in.color.a * cov);
    }
    let a = premultiplied_texel(vec2<i32>(t.lo.x, t.lo.y));
    let b = premultiplied_texel(vec2<i32>(t.hi.x, t.lo.y));
    let c = premultiplied_texel(vec2<i32>(t.lo.x, t.hi.y));
    let d = premultiplied_texel(vec2<i32>(t.hi.x, t.hi.y));
    let s = mix(mix(a, b, t.f.x), mix(c, d, t.f.x), t.f.y);
    // Luminance is linear, so the premultiplied grey is the grey of the
    // straight colour times its alpha — the same look as `fs`'s.
    let grey = vec3<f32>(dot(s.rgb, LUMA));
    let rgb = select(s.rgb, grey, (in.flags & FLAG_DESATURATE) != 0u);
    return vec4<f32>(rgb, s.a) * in.color.a;
}

fn premultiplied_texel(at: vec2<i32>) -> vec4<f32> {
    let s = textureLoad(color_atlas, at, 0);
    return premultiply(s.rgb, s.a);
}
