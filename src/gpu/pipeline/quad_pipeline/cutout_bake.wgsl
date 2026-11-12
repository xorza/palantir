// The bake of a shadow corner's cutout into a table on a mask page (`fs_cutout_bake`).

// Midpoint nodes of a corner cutout baked into a table (`fs_cutout_bake`):
// twice `CUTOUT_NODES`, so the bake errs about a quarter of the shaded
// form, 3e-4, beside the table's bilinear error of under 6e-4 and the
// mask page's 8-bit store, half a step.
const CUTOUT_BAKE_NODES: u32 = /*{CUTOUT_BAKE_NODES}*/;

// One cutout table for `fs_cutout_bake` (`CutoutPlan::BakeTable`): where
// it goes on its page (`cutout_lookup`'s packing) and its `(r, σ)`.
struct BakeIn {
    @location(0) table: u32,
    @location(1) r:     f32,
    @location(2) sigma: f32,
    // The texels of the table to bake, from `lo` up to `hi`.
    @location(3) lo:    vec2<u32>,
    @location(4) hi:    vec2<u32>,
    // The edge of the page the pass renders into, in texels.
    @location(5) page_edge: f32,
};

struct BakeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) table: u32,
    @location(1) @interpolate(flat) r:     f32,
    @location(2) @interpolate(flat) sigma: f32,
};

// The texels of the table the frame bakes, from `lo` up to `hi` of its
// square of its page, one fragment per texel.
@vertex
fn vs_cutout_bake(@builtin(vertex_index) vi: u32, bake: BakeIn) -> BakeOut {
    let texel = vec2<f32>(cutout_origin(bake.table) + select(bake.lo, bake.hi, CORNERS[vi] > vec2<f32>(0.5)));
    var out: BakeOut;
    out.clip = vec4<f32>(texel.x / bake.page_edge * 2.0 - 1.0, 1.0 - texel.y / bake.page_edge * 2.0, 0.0, 1.0);
    out.table = bake.table;
    out.r = bake.r;
    out.sigma = bake.sigma;
    return out;
}

// The cutout at the `q` this texel stands for (`cutout_lookup`), with
// `CUTOUT_BAKE_NODES`.
@fragment
fn fs_cutout_bake(in: BakeOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(vec2<u32>(in.clip.xy) - cutout_origin(in.table));
    let reach = SHADOW_REACH_SIGMAS * in.sigma + PALANTIR_AA_HALF_WIDTH;
    let q = texel * (in.sigma / CUTOUT_TEXELS_PER_SIGMA) - vec2<f32>(reach);
    return vec4<f32>(corner_cutout(q, in.r, in.sigma, reach, CUTOUT_BAKE_NODES), 0.0, 0.0, 0.0);
}
