// Palantir raster-atlas shader — the draw program for both the glyph atlas
// and the icon atlas. Contract:
// - color comes in straight-alpha linear f16 (no sRGB decode here).
// - output is premultiplied linear: vec4(rgb*a, a).
// - blend = PREMULTIPLIED_ALPHA_BLENDING; render target is sRGB
//   (GPU re-encodes on write).
// - mask pages = R8Unorm linear; color pages = Rgba8Unorm holding sRGB bytes,
//   which `color_load` decodes to linear straight RGBA, so a move copies them
//   exactly on every backend.
// - each kind binds up to four pages; a quad's page rides in `uv_and_kind`.
// - `uv_and_kind` packs u, the page, two flags, and v; Rust owns the field widths and
//   substitutes them below. Both atlases cap well under the room u gets.

struct VertexIn {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: vec2<u32>,     // raster texels
    @location(2) size: vec2<u32>,    // drawn physical px
    @location(3) uv_and_kind: u32,   // (u | page and flags<<U_BITS | v<<V_SHIFT)
    // Linear straight RGBA — the `Float16x4` fetch widens in hardware,
    // no shader unpack.
    @location(4) color: vec4<f32>,
    // The rounded clips that cut the quad (`ClipChain`).
    @location(5) chain: u32,
}

struct VertexOut {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,          // linear straight
    @location(1) @interpolate(flat) flags: u32, // FLAG_* below
    // Atlas texel coordinate. A quad drawn at its raster's own size sits on
    // whole pixels, so every fragment center lands on a texel center and the
    // texel it reads is this one, truncated.
    @location(2) texel: vec2<f32>,
    // The raster's texel rect as inclusive min/max — read only under
    // FLAG_RESAMPLE.
    @location(3) @interpolate(flat) texel_rect: vec4<i32>,
    @location(4) @interpolate(flat) chain: u32,
}

// The `uv_and_kind` layout: `u` in the low `U_BITS`, the page and the carried
// flags in the `FLAG_MASK` bits above it, `v` from `V_SHIFT` up. Rust owns
// every number and substitutes it in — see `raster_quad` and `ShaderBody`.
const U_BITS: u32 = /*{U_BITS}*/;
const U_MASK: u32 = (1u << U_BITS) - 1u;
const V_SHIFT: u32 = /*{V_SHIFT}*/;
// The page and the carried flags, already shifted down past `u`.
const FLAG_MASK: u32 = /*{FLAG_MASK}*/;
const FLAG_PAGE: u32 = /*{FLAG_PAGE}*/;               // the page of the quad's kind
const FLAG_DESATURATE: u32 = /*{FLAG_DESATURATE}*/;  // color icons only; see `fs`
const FLAG_COLOR: u32 = /*{FLAG_COLOR}*/;            // sample color, not mask
// Set by `vs`, not carried in: the quad is drawn at a size other than its
// raster's, so `fs` filters instead of reading texel for texel. The bit
// just above the carried ones.
const FLAG_RESAMPLE: u32 = FLAG_MASK + 1u;

// The atlas pages, past the frame group: four of each kind, a closed page bound
// as a 1 × 1 stand-in. Every read is a `textureLoad` by texel, so there is no
// sampler, and a chain of branches on a flat page picks the texture, as Skia
// picks among its glyph pages (`GrAtlasedShaderHelpers`). The pipeline pins the
// open slots of each kind, `PageSlots`, so the compiler drops every branch it
// can decide (`palantir_reads_slot`): over one open page, a load and no branch,
// where V3D compiles every arm of a choice among four.
override MASK_SLOTS: u32 = 15u;
override COLOR_SLOTS: u32 = 15u;
const ATLAS_GROUP: u32 = /*{ATLAS_GROUP}*/;
@group(ATLAS_GROUP) @binding(0) var mask_page0: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(1) var mask_page1: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(2) var mask_page2: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(3) var mask_page3: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(4) var color_page0: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(5) var color_page1: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(6) var color_page2: texture_2d<f32>;
@group(ATLAS_GROUP) @binding(7) var color_page3: texture_2d<f32>;

// The coverage at `at` of mask page `page`, one of the `MASK_SLOTS`.
fn mask_load(page: u32, at: vec2<i32>) -> f32 {
    if (palantir_reads_slot(MASK_SLOTS, 0u, page)) { return textureLoad(mask_page0, at, 0).x; }
    if (palantir_reads_slot(MASK_SLOTS, 1u, page)) { return textureLoad(mask_page1, at, 0).x; }
    if (palantir_reads_slot(MASK_SLOTS, 2u, page)) { return textureLoad(mask_page2, at, 0).x; }
    return textureLoad(mask_page3, at, 0).x;
}

// The texel at `at` of color page `page`, one of the `COLOR_SLOTS`, as stored.
fn color_texel(page: u32, at: vec2<i32>) -> vec4<f32> {
    if (palantir_reads_slot(COLOR_SLOTS, 0u, page)) { return textureLoad(color_page0, at, 0); }
    if (palantir_reads_slot(COLOR_SLOTS, 1u, page)) { return textureLoad(color_page1, at, 0); }
    if (palantir_reads_slot(COLOR_SLOTS, 2u, page)) { return textureLoad(color_page2, at, 0); }
    return textureLoad(color_page3, at, 0);
}

// The straight linear RGBA at `at` of color page `page`, its sRGB bytes decoded.
fn color_load(page: u32, at: vec2<i32>) -> vec4<f32> {
    let s = color_texel(page, at);
    return vec4<f32>(palantir_srgb_to_linear(s.rgb), s.a);
}

@vertex
fn vs(in: VertexIn) -> VertexOut {
    let u = in.uv_and_kind & U_MASK;
    let flags = (in.uv_and_kind >> U_BITS) & FLAG_MASK;
    let v = in.uv_and_kind >> V_SHIFT;

    let corner = vec2<u32>(in.idx & 1u, (in.idx >> 1u) & 1u);
    let pos = in.pos + vec2<i32>(in.size * corner);
    let uv_texel = vec2<f32>(vec2<u32>(u, v) + in.dim * corner);

    var out: VertexOut;
    out.position = palantir_clip_from_px(vec2<f32>(pos));

    // Straight-alpha linear color. Shader premuls at output; no sRGB
    // decode — the instance lanes are linear.
    out.color = in.color;
    out.flags = flags | select(0u, FLAG_RESAMPLE, any(in.size != in.dim));
    out.texel = uv_texel;
    out.texel_rect = vec4<i32>(vec2<i32>(vec2<u32>(u, v)), vec2<i32>(vec2<u32>(u, v) + in.dim) - 1);
    out.chain = in.chain;
    return out;
}

// A turned quad: `VertexIn` plus its run's turn about the physical `pivot`,
// `rotor = (cos, sin)` (`SpunRasterQuad`).
struct SpunIn {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: vec2<u32>,
    @location(2) size: vec2<u32>,
    @location(3) uv_and_kind: u32,
    @location(4) color: vec4<f32>,
    @location(5) pivot: vec2<f32>,
    @location(6) rotor: vec2<f32>,
    @location(7) chain: u32,
}

// `VertexOut`'s fields, then the quad's own unturned frame: the position
// relative to its center and its half size, which `fs_spun` fades the edges
// by.
struct SpunOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(flat) flags: u32,
    @location(2) texel: vec2<f32>,
    @location(3) @interpolate(flat) texel_rect: vec4<i32>,
    @location(4) local: vec2<f32>,
    @location(5) @interpolate(flat) half_size: vec2<f32>,
    @location(6) @interpolate(flat) chain: u32,
}

// A turned quad. Its texels no longer sit on pixels, so it always takes the
// filtered path; it reaches `PALANTIR_AA_HALF_WIDTH` past each drawn edge in its own
// frame, where a rotation keeps distances, and the texel coordinate runs on
// with it, clamped to the raster's slot by `taps`.
@vertex
fn vs_spun(in: SpunIn) -> SpunOut {
    let u = in.uv_and_kind & U_MASK;
    let flags = (in.uv_and_kind >> U_BITS) & FLAG_MASK;
    let v = in.uv_and_kind >> V_SHIFT;

    let corner = vec2<f32>(f32(in.idx & 1u), f32((in.idx >> 1u) & 1u));
    let size = vec2<f32>(in.size);
    let drawn = corner * size + (corner * 2.0 - 1.0) * PALANTIR_AA_HALF_WIDTH;
    let p = vec2<f32>(in.pos) + drawn - in.pivot;
    let turned = in.pivot + vec2<f32>(in.rotor.x * p.x - in.rotor.y * p.y, in.rotor.y * p.x + in.rotor.x * p.y);

    var out: SpunOut;
    out.position = palantir_clip_from_px(turned);
    out.color = in.color;
    out.flags = flags | FLAG_RESAMPLE;
    out.texel = vec2<f32>(vec2<u32>(u, v)) + drawn * vec2<f32>(in.dim) / max(size, vec2<f32>(1.0));
    out.texel_rect = vec4<i32>(vec2<i32>(vec2<u32>(u, v)), vec2<i32>(vec2<u32>(u, v) + in.dim) - 1);
    out.local = drawn - size * 0.5;
    out.half_size = size * 0.5;
    out.chain = in.chain;
    return out;
}

// Bilinear weights and the four texel addresses around `texel`, clamped to
// the raster's own rect so a tap never reads a neighboring slot.
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
    return texel_color(in) * palantir_clip_coverage(palantir_frag_px(in.position), in.chain);
}

// What `fs` draws before its clips cut it.
fn texel_color(in: VertexOut) -> vec4<f32> {
    if ((in.flags & FLAG_RESAMPLE) != 0u) {
        return resampled(in);
    }
    if ((in.flags & FLAG_COLOR) == 0u) {
        // Mask: vertex color modulated by R-channel coverage.
        let cov = mask_load(in.flags & FLAG_PAGE, vec2<i32>(in.texel));
        return premultiply(in.color.rgb, in.color.a * cov);
    }
    // Color emoji or color icon: the load decodes the page's sRGB bytes to
    // linear straight RGBA. Premultiply at output; the run alpha modulates
    // the whole premultiplied result, so faded text fades its emoji too and a
    // faded icon fades whole.
    let s = color_load(in.flags & FLAG_PAGE, vec2<i32>(in.texel));
    // DESATURATE collapses the artwork to its luminance — the disabled look
    // for an icon whose own colors the tint cannot replace. Alpha is
    // untouched, so the shape is unchanged and only the hue goes.
    let gray = vec3<f32>(dot(s.rgb, PALANTIR_LUMA));
    let rgb = select(s.rgb, gray, (in.flags & FLAG_DESATURATE) != 0u);
    return premultiply(rgb, s.a) * in.color.a;
}

// A turned quad: filtered as a resampled one, faded over `PALANTIR_AA_HALF_WIDTH`
// past each of its turned edges.
@fragment
fn fs_spun(in: SpunOut) -> @location(0) vec4<f32> {
    let d = abs(in.local) - in.half_size;
    let fade = palantir_edge_coverage(d.x) * palantir_edge_coverage(d.y)
        * palantir_clip_coverage(palantir_frag_px(in.position), in.chain);
    return resampled(VertexOut(in.position, in.color, in.flags, in.texel, in.texel_rect, in.chain)) * fade;
}

// An icon drawn at a size other than its raster's: the four nearest texels,
// filtered by hand, clamped to the raster's own slot. Color taps are
// premultiplied before they blend, like every color interpolation.
fn resampled(in: VertexOut) -> vec4<f32> {
    let t = taps(in.texel, in.texel_rect);
    let page = in.flags & FLAG_PAGE;
    if ((in.flags & FLAG_COLOR) == 0u) {
        let a = mask_load(page, vec2<i32>(t.lo.x, t.lo.y));
        let b = mask_load(page, vec2<i32>(t.hi.x, t.lo.y));
        let c = mask_load(page, vec2<i32>(t.lo.x, t.hi.y));
        let d = mask_load(page, vec2<i32>(t.hi.x, t.hi.y));
        let cov = mix(mix(a, b, t.f.x), mix(c, d, t.f.x), t.f.y);
        return premultiply(in.color.rgb, in.color.a * cov);
    }
    let a = premultiplied_texel(page, vec2<i32>(t.lo.x, t.lo.y));
    let b = premultiplied_texel(page, vec2<i32>(t.hi.x, t.lo.y));
    let c = premultiplied_texel(page, vec2<i32>(t.lo.x, t.hi.y));
    let d = premultiplied_texel(page, vec2<i32>(t.hi.x, t.hi.y));
    let s = mix(mix(a, b, t.f.x), mix(c, d, t.f.x), t.f.y);
    // Luminance is linear, so the premultiplied gray is the gray of the
    // straight color times its alpha — the same look as `fs`'s.
    let gray = vec3<f32>(dot(s.rgb, PALANTIR_LUMA));
    let rgb = select(s.rgb, gray, (in.flags & FLAG_DESATURATE) != 0u);
    return vec4<f32>(rgb, s.a) * in.color.a;
}

fn premultiplied_texel(page: u32, at: vec2<i32>) -> vec4<f32> {
    let s = color_load(page, at);
    return premultiply(s.rgb, s.a);
}
