
// The backdrop contract: what a backdrop effect reads besides a color effect's.

// One backdrop's result as `Backdrops` writes its slot (`BackdropSlot`): the result's texel
// coordinate of the lattice's first texel, the physical px of a texel or 0 for a backdrop that
// shows `clear`, the physical rect the shape reads, `(min, max)`, its rect grown by its reach; and
// a material's luminance bounds, fill and border, which palantir's material path reads and an app's
// effect does not. The fill is a straight color, or the multiplier on the gradient at
// `fill_lut_row`, whose `fill_kind` and `fill_axis` read as a quad's.
struct PalantirBackdrop {
    offset: vec2<f32>,
    lattice: f32,
    max_luminance: f32,
    clear: vec3<f32>,
    min_luminance: f32,
    reads: vec4<f32>,
    fill: vec4<f32>,
    fill_axis: vec4<f32>,
    border_color: vec4<f32>,
    border_width: f32,
    fill_kind: u32,
    fill_lut_row: u32,
}
@group(2) @binding(0) var palantir_results: texture_2d<f32>;
@group(2) @binding(1) var palantir_results_sampler: sampler;
@group(2) @binding(2) var<storage, read> palantir_backdrops: array<PalantirBackdrop>;

// The spectrum a dispersive lens spreads, cell by cell (`spectrum_tables::SpectrumTables`), which
// the stock glass sums back to color.
struct PalantirSpectrumCell {
    weight: vec3<f32>,
    cumulative: vec3<f32>,
    moment: vec3<f32>,
}
@group(2) @binding(3) var<storage, read> palantir_spectrum: array<PalantirSpectrumCell>;

// The backdrop this fragment draws, its rect `(min, size)` in physical px, its turn about the
// rect's center and the physical px per logical px, which palantir's fragment stage sets before it
// calls `effect`.
var<private> palantir_backdrop_idx: u32;
var<private> palantir_rect: vec4<f32>;
var<private> palantir_rotation: f32;
var<private> palantir_scale: f32;

// What is painted under the shape at `pos`, logical px from its rect's top-left in its own frame,
// blurred by the shape's σ: linear and opaque. A read past the shape's reach reads the edge of its
// reach.
fn effect_backdrop(pos: vec2<f32>) -> vec4<f32> {
    let backdrop = palantir_backdrops[palantir_backdrop_idx];
    let center = palantir_rect.xy + palantir_rect.zw * 0.5;
    let at = palantir_turned(palantir_rect.xy + pos * palantir_scale, center, palantir_rotation);
    let p = clamp(at, backdrop.reads.xy, backdrop.reads.zw);
    return vec4<f32>(palantir_result(p, backdrop), 1.0);
}

// The backdrop's result at screen position `p`: the cubic B-spline over the slot's texels through
// four bilinear fetches (Sigg and Hadwiger), whose weights add the same variance at every phase; at
// full resolution the texel itself, or the bilinear read where an effect reads between pixels; or
// the clear the slot holds.
fn palantir_result(p: vec2<f32>, backdrop: PalantirBackdrop) -> vec3<f32> {
    if (backdrop.lattice == 0.0) {
        return backdrop.clear;
    }
    let size = vec2<f32>(textureDimensions(palantir_results));
    let x = p / backdrop.lattice - vec2<f32>(0.5) + backdrop.offset;
    if (backdrop.lattice == 1.0) {
        // A read at a pixel center lands on a texel's center, which a load returns whole, with no
        // filter weights to round; a read between pixels, as a lens bends it, filters.
        if (all(fract(x) == vec2<f32>(0.0))) {
            return textureLoad(palantir_results, vec2<i32>(x), 0).rgb;
        }
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
