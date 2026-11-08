
// The backdrop contract: what a backdrop effect reads besides a color effect's.

// One pane's result as `Backdrops` writes its slot (`BackdropSlot`): the result's texel coordinate
// of the lattice's first texel, the physical px of a texel or 0 for a pane that shows `clear`, and
// the physical rect the shape reads, its rect grown by its reach. The rest is the built-in looks'.
struct PalantirPane {
    offset: vec2<f32>,
    lattice: f32,
    highlight: f32,
    lens: vec4<f32>,
    clear: vec3<f32>,
    max_luminance: f32,
    min_luminance: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
    reads: vec4<f32>,
}
@group(2) @binding(0) var palantir_results: texture_2d<f32>;
@group(2) @binding(1) var palantir_results_sampler: sampler;
@group(2) @binding(2) var<storage, read> palantir_panes: array<PalantirPane>;

// The pane this fragment draws, its rect's top-left in physical px and the physical px per logical
// px, which palantir's fragment stage sets before it calls `effect`.
var<private> palantir_pane: u32;
var<private> palantir_origin: vec2<f32>;
var<private> palantir_scale: f32;

// What is painted under the shape at `pos`, logical px from its rect's top-left, blurred by the
// shape's σ: linear and opaque. A read past the shape's reach reads the edge of its reach.
fn effect_backdrop(pos: vec2<f32>) -> vec4<f32> {
    let pane = palantir_panes[palantir_pane];
    let p = clamp(palantir_origin + pos * palantir_scale, pane.reads.xy, pane.reads.zw);
    return vec4<f32>(palantir_result(p, pane), 1.0);
}

// The pane's result at screen position `p`: the cubic B-spline over the slot's texels through four
// bilinear fetches (Sigg and Hadwiger), whose weights add the same variance at every phase; the
// bilinear read at full resolution, where an effect reads between pixels; or the clear the slot
// holds. `quad_pipeline/shader.wgsl`'s `backdrop_result` reads the same.
fn palantir_result(p: vec2<f32>, pane: PalantirPane) -> vec3<f32> {
    if (pane.lattice == 0.0) {
        return pane.clear;
    }
    let size = vec2<f32>(textureDimensions(palantir_results));
    let x = p / pane.lattice - vec2<f32>(0.5) + pane.offset;
    if (pane.lattice == 1.0) {
        return textureSampleLevel(palantir_results, palantir_results_sampler, (x + vec2<f32>(0.5)) / size, 0.0).rgb;
    }
    let base = floor(x);
    let f = x - base;
    let g = vec2<f32>(1.0) - f;
    let w0 = g * g * g / 6.0;
    let w1 = (4.0 - 6.0 * f * f + 3.0 * f * f * f) / 6.0;
    let w3 = f * f * f / 6.0;
    let w2 = vec2<f32>(1.0) - w0 - w1 - w3;
    let s0 = w0 + w1;
    let s1 = w2 + w3;
    let lo = (base - vec2<f32>(0.5) + w1 / s0) / size;
    let hi = (base + vec2<f32>(1.5) + w3 / s1) / size;
    return s0.y * (s0.x * textureSampleLevel(palantir_results, palantir_results_sampler, vec2<f32>(lo.x, lo.y), 0.0).rgb
                 + s1.x * textureSampleLevel(palantir_results, palantir_results_sampler, vec2<f32>(hi.x, lo.y), 0.0).rgb)
         + s1.y * (s0.x * textureSampleLevel(palantir_results, palantir_results_sampler, vec2<f32>(lo.x, hi.y), 0.0).rgb
                 + s1.x * textureSampleLevel(palantir_results, palantir_results_sampler, vec2<f32>(hi.x, hi.y), 0.0).rgb);
}
