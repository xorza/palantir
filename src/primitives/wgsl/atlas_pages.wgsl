// The atlas group, which every draw that reads the pool's pages binds at `PALANTIR_ATLAS_GROUP`:
// the mask list's four pages at bindings 0 to 3, the color list's at 4 to 7, a closed page bound
// as a 1 × 1 stand-in. Every read is a `textureLoad` by texel, so there is no sampler, and a chain
// of branches on a flat page picks the texture, as Skia picks among its glyph pages
// (`GrAtlasedShaderHelpers`). The pipeline pins the open slots of each list,
// `PALANTIR_MASK_SLOTS` and `PALANTIR_COLOR_SLOTS` (`PageSlots`), so the compiler drops every
// branch it can decide (`palantir_reads_slot`): over one open page, a load and no branch, where
// V3D compiles every arm of a choice among four. A read of four taps picks once.

const PALANTIR_ATLAS_GROUP: u32 = /*{ATLAS_GROUP}*/;
override PALANTIR_MASK_SLOTS: u32 = 15u;
override PALANTIR_COLOR_SLOTS: u32 = 15u;
@group(PALANTIR_ATLAS_GROUP) @binding(0) var palantir_mask_page0: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(1) var palantir_mask_page1: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(2) var palantir_mask_page2: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(3) var palantir_mask_page3: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(4) var palantir_color_page0: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(5) var palantir_color_page1: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(6) var palantir_color_page2: texture_2d<f32>;
@group(PALANTIR_ATLAS_GROUP) @binding(7) var palantir_color_page3: texture_2d<f32>;

// Whether a read of atlas page `page`, among the open page slots `slots` (slot `i` at bit `i`),
// takes slot `slot`: an open slot, and the page's own or the last open one, which needs no test.
// Called with `slot` 0, 1 and 2 in turn before a read of slot 3, under a `slots` the pipeline pins,
// the compiler drops every branch it can decide: over one open page the read loads it and never
// branches, wherever the page sits (`PageSlots`).
fn palantir_reads_slot(slots: u32, slot: u32, page: u32) -> bool {
    return (slots & (1u << slot)) != 0u && (page == slot || (slots >> (slot + 1u)) == 0u);
}

// The coverage at `at` of mask page `page`.
fn palantir_mask_texel(page: u32, at: vec2<i32>) -> f32 {
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 0u, page)) { return textureLoad(palantir_mask_page0, at, 0).x; }
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 1u, page)) { return textureLoad(palantir_mask_page1, at, 0).x; }
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 2u, page)) { return textureLoad(palantir_mask_page2, at, 0).x; }
    return textureLoad(palantir_mask_page3, at, 0).x;
}

// The coverage of mask page `page` at the four taps of `lo` and `hi`, `(lo, (hi.x, lo.y),
// (lo.x, hi.y), hi)`, the page picked once.
fn palantir_mask_taps(page: u32, lo: vec2<i32>, hi: vec2<i32>) -> vec4<f32> {
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 0u, page)) { return palantir_mask_quad(palantir_mask_page0, lo, hi); }
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 1u, page)) { return palantir_mask_quad(palantir_mask_page1, lo, hi); }
    if (palantir_reads_slot(PALANTIR_MASK_SLOTS, 2u, page)) { return palantir_mask_quad(palantir_mask_page2, lo, hi); }
    return palantir_mask_quad(palantir_mask_page3, lo, hi);
}

fn palantir_mask_quad(page: texture_2d<f32>, lo: vec2<i32>, hi: vec2<i32>) -> vec4<f32> {
    return vec4<f32>(
        textureLoad(page, lo, 0).x,
        textureLoad(page, vec2<i32>(hi.x, lo.y), 0).x,
        textureLoad(page, vec2<i32>(lo.x, hi.y), 0).x,
        textureLoad(page, hi, 0).x,
    );
}

// The texel at `at` of color page `page`, as stored.
fn palantir_color_texel(page: u32, at: vec2<i32>) -> vec4<f32> {
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 0u, page)) { return textureLoad(palantir_color_page0, at, 0); }
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 1u, page)) { return textureLoad(palantir_color_page1, at, 0); }
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 2u, page)) { return textureLoad(palantir_color_page2, at, 0); }
    return textureLoad(palantir_color_page3, at, 0);
}

// The texels of color page `page` at the four taps of `lo` and `hi`, as stored, one a column in
// `palantir_mask_taps`' order, the page picked once.
fn palantir_color_taps(page: u32, lo: vec2<i32>, hi: vec2<i32>) -> mat4x4<f32> {
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 0u, page)) { return palantir_color_quad(palantir_color_page0, lo, hi); }
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 1u, page)) { return palantir_color_quad(palantir_color_page1, lo, hi); }
    if (palantir_reads_slot(PALANTIR_COLOR_SLOTS, 2u, page)) { return palantir_color_quad(palantir_color_page2, lo, hi); }
    return palantir_color_quad(palantir_color_page3, lo, hi);
}

fn palantir_color_quad(page: texture_2d<f32>, lo: vec2<i32>, hi: vec2<i32>) -> mat4x4<f32> {
    return mat4x4<f32>(
        textureLoad(page, lo, 0),
        textureLoad(page, vec2<i32>(hi.x, lo.y), 0),
        textureLoad(page, vec2<i32>(lo.x, hi.y), 0),
        textureLoad(page, hi, 0),
    );
}
