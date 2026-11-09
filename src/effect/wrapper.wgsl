
// Palantir's stages around `effect`. Every name here carries the `palantir_` prefix, which an
// effect's own text leaves to palantir.

struct PalantirImmediates {
    viewport_size: vec2<f32>,
    frag_origin: vec2<f32>,
}
var<immediate> palantir_imm: PalantirImmediates;

// Every instance's parameters, as 32-bit words; an instance reads from its own offset.
@group(0) @binding(0) var<storage, read> palantir_words: array<u32>;

const PALANTIR_AA_HALF_WIDTH: f32 = /*{AA_HALF_WIDTH}*/;

const PALANTIR_CORNERS = array<vec2<f32>, 4>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
);

// `EffectInstance::flags`'s bits: a material's box, and one that covers outside its rounded rect.
const PALANTIR_MATERIAL: u32 = /*{MATERIAL}*/;
const PALANTIR_WINDOW: u32 = /*{WINDOW}*/;

// One shape: its rect (`min`, `size`) and corner radii in physical px, its clock (`time`,
// `opacity`, `scale`, and its turn about its center in radians), the word its parameters start
// at, a backdrop effect's pane, and its flags.
struct PalantirInstance {
    @location(0) rect: vec4<f32>,
    @location(1) corners: vec4<f32>,
    @location(2) clock: vec4<f32>,
    @location(3) params: u32,
    @location(4) pane: u32,
    @location(5) flags: u32,
}

struct PalantirVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) corners: vec4<f32>,
    @location(2) @interpolate(flat) clock: vec4<f32>,
    @location(3) @interpolate(flat) params: u32,
    @location(4) @interpolate(flat) pane: u32,
    @location(5) @interpolate(flat) flags: u32,
}

struct PalantirOut {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) under: vec4<f32>,
}

// Every pixel center within the antialiasing reach of the rect, as a quad shades it: the rect's
// pixels grown by it, or past each edge by it where a turn takes the edges off the pixel grid; a
// window's rect is its edge exactly, as it lies over content of its own extent.
@vertex
fn palantir_vs(@builtin(vertex_index) vi: u32, instance: PalantirInstance) -> PalantirVarying {
    let pos = instance.rect.xy;
    let size = instance.rect.zw;
    let rotation = instance.clock.w;
    var lo = pos;
    var hi = pos + size;
    if (rotation != 0.0) {
        lo -= vec2<f32>(PALANTIR_AA_HALF_WIDTH);
        hi += vec2<f32>(PALANTIR_AA_HALF_WIDTH);
    } else if ((instance.flags & PALANTIR_WINDOW) == 0u) {
        lo = floor(lo - vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
        hi = ceil(hi + vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
    }
    let corner = select(lo, hi, PALANTIR_CORNERS[vi] > vec2<f32>(0.5));
    let px = palantir_turned(corner, pos + size * 0.5, rotation);
    let ndc = px * (vec2<f32>(2.0, -2.0) / palantir_imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return PalantirVarying(
        vec4<f32>(ndc, 0.0, 1.0),
        instance.rect,
        instance.corners,
        instance.clock,
        instance.params,
        instance.pane,
        instance.flags,
    );
}

// The rounded rect's signed distance at `p` from its top-left, radii fitted on the CPU.
fn palantir_sdf(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    let c = p - half;
    let right = step(0.0, c.x);
    let bottom = step(0.0, c.y);
    let r = mix(mix(radius.x, radius.y, right), mix(radius.w, radius.z, right), bottom);
    let q = abs(c) - (half - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
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
// its rounded rect; or, for a material's box, palantir's material around what it shows.
@fragment
fn palantir_fs(v: PalantirVarying) -> PalantirOut {
    let local = palantir_local(v.clip, v.rect, v.clock.w);
    let scale = v.clock.z;
    let input = EffectInput(local / scale, v.rect.zw / scale, v.corners / scale, v.clock.x, scale);
    /*{BACKDROP_SET}*/let out = /*{CALL}*/;
    /*{MATERIAL_BRANCH}*/let d = palantir_sdf(local, v.rect.zw, v.corners);
    let coverage = clamp((PALANTIR_AA_HALF_WIDTH - d) / (2.0 * PALANTIR_AA_HALF_WIDTH), 0.0, 1.0)
        * v.clock.y;
    return PalantirOut(out.color * coverage, vec4<f32>(1.0) - coverage * (vec4<f32>(1.0) - out.under));
}
