// The frame group, which every color pipeline and every effect binds at `PALANTIR_FRAME_GROUP`:
// the gradient LUT atlas and the frame's rounded clips, and the clip's coverage. The includer
// declares `PALANTIR_AA_HALF_WIDTH` and includes the SDF and the antialiasing.

const PALANTIR_FRAME_GROUP: u32 = /*{FRAME_GROUP}*/;

// Rows of baked 256-texel gradients, sampled at fragment time. `Rgba16Float` holds premultiplied
// linear RGB, so the filter between two texels blends premultiplied, and f16 keeps dark gradients
// band-free.
@group(PALANTIR_FRAME_GROUP) @binding(0) var palantir_gradient_tex: texture_2d<f32>;
@group(PALANTIR_FRAME_GROUP) @binding(1) var palantir_gradient_sampler: sampler;

// One rounded clip as `RoundedClip` lays it out: its rect in the surface's physical px and its
// corner radii, four f16 lanes two to a `u32`, `(tl, tr, br, bl)`.
struct PalantirClip {
    min: vec2<f32>,
    size: vec2<f32>,
    corners: vec2<u32>,
};

// The frame's rounded clips, each chain a run of them, outer to inner (`ClipChain`).
@group(PALANTIR_FRAME_GROUP) @binding(2) var<storage, read> palantir_clips: array<PalantirClip>;

// Whether the pipeline reads the rounded clips: off for a frame whose draws name none, where the
// coverage below folds to 1 (`ClipVariant`).
override PALANTIR_CLIPS: bool = true;

// The bits of a `ClipChain` that hold its depth; its start in `palantir_clips` lies above them.
const PALANTIR_CLIP_DEPTH_BITS: u32 = /*{CLIP_DEPTH_BITS}*/;

// The share of the pixel centered at `px`, the surface's physical px, that the clips of `chain`
// leave: the product of each clip's coverage, as the edge of a shape of that rounded rect takes
// it. A chain of none leaves all of it.
fn palantir_clip_coverage(px: vec2<f32>, chain: u32) -> f32 {
    if (!PALANTIR_CLIPS || chain == 0u) {
        return 1.0;
    }
    let depth = chain & ((1u << PALANTIR_CLIP_DEPTH_BITS) - 1u);
    let start = chain >> PALANTIR_CLIP_DEPTH_BITS;
    var coverage = 1.0;
    for (var i = 0u; i < depth; i++) {
        let clip = palantir_clips[start + i];
        let radius = vec4<f32>(unpack2x16float(clip.corners.x), unpack2x16float(clip.corners.y));
        let d = palantir_sdf_rounded_rect(px - clip.min, clip.size, radius);
        coverage *= palantir_edge_coverage(d);
    }
    return coverage;
}
