
// Palantir's stages around `effect`. Every name here carries the `palantir_` prefix, which an
// effect's own text leaves to palantir.

struct PalantirImmediates {
    viewport_size: vec2<f32>,
    frag_origin: vec2<f32>,
}
var<immediate> palantir_imm: PalantirImmediates;

// Every instance's parameters, as 32-bit words; an instance reads from its own offset.
@group(/*{WORDS_GROUP}*/) @binding(0) var<storage, read> palantir_words: array<u32>;

const PALANTIR_AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

// `EffectInstance::flags`'s bits: a material's box, one that covers outside its rounded rect, and
// a triangle.
const PALANTIR_MATERIAL: u32 = /*{MATERIAL}*/;
const PALANTIR_OUTSIDE: u32 = /*{OUTSIDE}*/;
const PALANTIR_TRIANGLE: u32 = /*{TRIANGLE}*/;

// One instance past its shape (`PalantirShapeIn`): its clock (`time`, `opacity`, `scale`), the word
// its parameters start at, a backdrop effect's backdrop, its flags, and the border width a
// material's box wears.
struct PalantirInstance {
    @location(6) clock: vec3<f32>,
    @location(7) params: u32,
    @location(8) backdrop_idx: u32,
    @location(9) flags: u32,
    @location(10) border_width: f32,
}

struct PalantirVarying {
    @builtin(position) clip: vec4<f32>,
    // The shape's rect and radii, and its clock with its turn about the rect's center in `w`.
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) corners: vec4<f32>,
    @location(2) @interpolate(flat) clock: vec4<f32>,
    @location(3) @interpolate(flat) params: u32,
    @location(4) @interpolate(flat) backdrop_idx: u32,
    @location(5) @interpolate(flat) flags: u32,
    // The template region, distance and ring (`palantir_template_distance`).
    @location(6) @interpolate(flat) region: u32,
    @location(7) geo: vec2<f32>,
    @location(8) @interpolate(flat) ring_r: f32,
    @location(9) @interpolate(flat) chain: u32,
}

struct PalantirOut {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) under: vec4<f32>,
}

// The shape's template (`palantir_template_vertex`), as a quad's is drawn: a ring per corner, a
// band per edge and an interior, reaching the antialiasing reach past the shape; a rect covering
// its outside stops at its rect, as it lies over content of its own extent, and draws no interior.
@vertex
fn palantir_vs(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, instance: PalantirInstance) -> PalantirVarying {
    let pos = shape.rect.xy;
    let size = shape.rect.zw;
    let rotation = shape.inradius_rotation.y;
    let outside = (instance.flags & PALANTIR_OUTSIDE) != 0u;
    // A run draws one corner count's vertices (`EffectInstance::corner_count`).
    let s = palantir_shape_of(shape, select(4u, 3u, (instance.flags & PALANTIR_TRIANGLE) != 0u));
    let outset = select(PALANTIR_AA_HALF_WIDTH, 0.0, outside && rotation == 0.0);
    let t = palantir_template_vertex(s, vi, instance.border_width, outset, outside);
    let px = palantir_turned(pos + t.pos, pos + size * 0.5, rotation);
    let ndc = px * (vec2<f32>(2.0, -2.0) / palantir_imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return PalantirVarying(
        vec4<f32>(ndc, 0.0, 1.0),
        shape.rect,
        shape.radius,
        vec4<f32>(instance.clock, rotation),
        instance.params,
        instance.backdrop_idx,
        instance.flags,
        t.region,
        t.geo,
        t.ring_r,
        shape.chain,
    );
}

/*{PARAMS}*/
// The fragment's position in the shape's own frame, from its rect's top-left in physical px: the
// surface pixel it shades turned back about the rect's center, exact where the shape is not
// turned, so a read of `effect_backdrop` at its own position reads the pixel it covers.
fn palantir_local(clip: vec4<f32>, rect: vec4<f32>, rotation: f32) -> vec2<f32> {
    let frag = clip.xy + palantir_imm.frag_origin;
    return palantir_turned(frag, rect.xy + rect.zw * 0.5, -rotation) - rect.xy;
}

// One stage for every draw of the effect: its color and what it keeps of what is under it, over
// its shape, by the template's distance; or, for a material's box, palantir's material around what
// it shows.
@fragment
fn palantir_fs(v: PalantirVarying) -> PalantirOut {
    let local = palantir_local(v.clip, v.rect, v.clock.w);
    let scale = v.clock.z;
    // A triangle's corners are no rect's: its effect reads none.
    let corners = select(v.corners, vec4<f32>(0.0), (v.flags & PALANTIR_TRIANGLE) != 0u);
    let input = EffectInput(local / scale, v.rect.zw / scale, corners / scale, v.clock.x, scale);
    /*{BACKDROP_SET}*/let out = /*{CALL}*/;
    /*{MATERIAL_BRANCH}*/var coverage = v.clock.y
        * palantir_clip_coverage(v.clip.xy + palantir_imm.frag_origin, v.chain);
    if (v.region != PALANTIR_REGION_INTERIOR) {
        coverage *= palantir_edge_coverage(palantir_template_distance(v.region, v.geo, v.ring_r));
    }
    return PalantirOut(out.color * coverage, vec4<f32>(1.0) - coverage * (vec4<f32>(1.0) - out.under));
}
