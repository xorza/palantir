// Palantir text shader. Contract:
// - color comes in straight-alpha linear-u8 (no sRGB decode here).
// - output is premultiplied linear: vec4(rgb*a, a).
// - blend = PREMULTIPLIED_ALPHA_BLENDING; render target is sRGB
//   (GPU re-encodes on write).
// - mask atlas = R8Unorm linear; color atlas = Rgba8UnormSrgb
//   (auto-decodes to linear straight RGBA on sample).
// - content_type lives in the high bit of u (uv_and_kind & 0x8000).

struct VertexIn {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,           // (w | h<<16)
    @location(2) uv_and_kind: u32,   // (u | kind<<15 | v<<16)
    @location(3) color: u32,         // linear straight RGBA u8
}

struct VertexOut {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,          // linear straight
    @location(1) uv: vec2<f32>,             // normalized atlas uv
    @location(2) @interpolate(flat) kind: u32, // 0=mask, 1=color
}

// Group(1) = text-specific atlas textures + sampler. Both viewport
// and atlas-size params ride the shared immediate region:
//   offset 0 (8 bytes): `Viewport` — set per pass by the backend.
//   offset 8 (8 bytes): `Params` — set per text batch in
//   `render_batch` when atlas dimensions change.
// Same `Immediates` shape as the other shaders' subset; non-text
// shaders only declare the prefix they read.
struct Viewport {
    size: vec2<f32>,
};
struct Params {
    atlas_px: vec2<u32>, // [color, mask]
};
struct Immediates {
    viewport: Viewport,
    params: Params,
};
var<immediate> imm: Immediates;

@group(0) @binding(0) var mask_atlas: texture_2d<f32>;
@group(0) @binding(1) var color_atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let w = in.dim & 0xFFFFu;
    let h = (in.dim >> 16u) & 0xFFFFu;

    // u stored in low 15 bits, kind in bit 15, v in upper 16.
    let u = in.uv_and_kind & 0x7FFFu;
    let kind = (in.uv_and_kind >> 15u) & 0x1u;
    let v = (in.uv_and_kind >> 16u) & 0xFFFFu;

    let corner = vec2<u32>(in.idx & 1u, (in.idx >> 1u) & 1u);
    let dim = vec2<u32>(w, h);
    let pos = in.pos + vec2<i32>(dim * corner);
    let uv_texel = vec2<f32>(vec2<u32>(u, v) + dim * corner);

    let atlas_size_texels = select(imm.params.atlas_px.y, imm.params.atlas_px.x, kind == 1u);

    var out: VertexOut;
    let ndc = vec2<f32>(pos) * (vec2<f32>(2.0, -2.0) / imm.viewport.size)
        + vec2<f32>(-1.0, 1.0);
    out.position = vec4<f32>(ndc, 0.0, 1.0);

    // Straight-alpha linear color from the instance. Shader premuls
    // at output. No sRGB decode — caller hands us linear bytes.
    out.color = vec4<f32>(
        f32((in.color >>  0u) & 0xFFu) / 255.0,
        f32((in.color >>  8u) & 0xFFu) / 255.0,
        f32((in.color >> 16u) & 0xFFu) / 255.0,
        f32((in.color >> 24u) & 0xFFu) / 255.0,
    );
    out.uv = uv_texel / f32(atlas_size_texels);
    out.kind = kind;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if (in.kind == 0u) {
        // Mask: vertex color modulated by R-channel coverage.
        let cov = textureSampleLevel(mask_atlas, atlas_sampler, in.uv, 0.0).x;
        let a = in.color.a * cov;
        return vec4<f32>(in.color.rgb * a, a);
    }
    // Color emoji: sRGB texture decodes to linear straight RGBA on
    // sample. Premultiply at output.
    let s = textureSampleLevel(color_atlas, atlas_sampler, in.uv, 0.0);
    return vec4<f32>(s.rgb * s.a, s.a);
}
