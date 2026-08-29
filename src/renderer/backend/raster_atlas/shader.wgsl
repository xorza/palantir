// Palantir raster-atlas shader — the draw program for both the glyph atlas
// and the icon atlas. Contract:
// - color comes in straight-alpha linear-u8 (no sRGB decode here).
// - output is premultiplied linear: vec4(rgb*a, a).
// - blend = PREMULTIPLIED_ALPHA_BLENDING; render target is sRGB
//   (GPU re-encodes on write).
// - mask atlas = R8Unorm linear; color atlas = Rgba8UnormSrgb
//   (auto-decodes to linear straight RGBA on sample).
// - `uv_and_kind` packs u, two flags, and v; Rust owns the field widths and
//   substitutes them below. Both atlases cap well under the room u gets.

struct VertexIn {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,           // (w | h<<16)
    @location(2) uv_and_kind: u32,   // (u | flags<<U_BITS | v<<16)
    // Linear straight RGBA — `Unorm8x4` fetch normalizes the u8 bytes
    // to 0..1 in hardware, no shader unpack.
    @location(3) color: vec4<f32>,
}

struct VertexOut {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,          // linear straight
    @location(1) uv: vec2<f32>,             // normalized atlas uv
    @location(2) @interpolate(flat) flags: u32, // FLAG_* below
}

// The `uv_and_kind` layout. Rust owns every number here and substitutes it in
// — see `raster_atlas::quad`, which panics if a marker goes unreplaced.
const U_BITS: u32 = /*{U_BITS}*/;
const U_MASK: u32 = (1u << U_BITS) - 1u;
// The two flags, already shifted down past `u`.
const FLAG_DESATURATE: u32 = /*{FLAG_DESATURATE}*/;  // colour icons only; see `fs`
const FLAG_COLOR: u32 = /*{FLAG_COLOR}*/;            // sample colour, not mask

// Group(0) = the atlas textures and their sampler. This is the only
// shader that reads `imm.atlas_px`, which the text backend rewrites per
// batch when either atlas is resized.
@group(0) @binding(0) var mask_atlas: texture_2d<f32>;
@group(0) @binding(1) var color_atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

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
    let pos = in.pos + vec2<i32>(dim * corner);
    let uv_texel = vec2<f32>(vec2<u32>(u, v) + dim * corner);

    let atlas_size_texels =
        select(imm.atlas_px.y, imm.atlas_px.x, (flags & FLAG_COLOR) != 0u);

    var out: VertexOut;
    out.position = clip_from_px(vec2<f32>(pos));

    // Straight-alpha linear color, already normalized by the Unorm8x4
    // vertex fetch. Shader premuls at output; no sRGB decode — the
    // instance bytes are linear.
    out.color = in.color;
    out.uv = uv_texel / f32(atlas_size_texels);
    out.flags = flags;
    return out;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    if ((in.flags & FLAG_COLOR) == 0u) {
        // Mask: vertex color modulated by R-channel coverage.
        let cov = textureSampleLevel(mask_atlas, atlas_sampler, in.uv, 0.0).x;
        return premultiply(in.color.rgb, in.color.a * cov);
    }
    // Colour emoji or colour icon: the sRGB texture decodes to linear
    // straight RGBA on sample. Premultiply at output; the run alpha modulates
    // the whole premultiplied result, so faded text fades its emoji too and a
    // faded icon fades whole.
    let s = textureSampleLevel(color_atlas, atlas_sampler, in.uv, 0.0);
    // DESATURATE collapses the artwork to its luminance — the disabled look
    // for an icon whose own colours the tint cannot replace. Alpha is
    // untouched, so the shape is unchanged and only the hue goes.
    let grey = vec3<f32>(dot(s.rgb, LUMA));
    let rgb = select(s.rgb, grey, (in.flags & FLAG_DESATURATE) != 0u);
    return premultiply(rgb, s.a) * in.color.a;
}
