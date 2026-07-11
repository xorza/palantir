// User-image pipeline. Per-instance rect + tint; texture+sampler in
// group 0, switched per draw by the backend. Four-corner quad emitted
// implicitly via `vertex_index` (TriangleStrip).
//
// Colour pipeline: texture is `Rgba8UnormSrgb`, so the sampler decodes
// sRGB → linear automatically. We multiply by `tint` (linear, straight
// alpha) and premultiply at write time to match the rest of the
// premultiplied-blend pipeline.

// Viewport via the shared immediate region (offset 0). See
// `quad.wgsl` for the cross-pipeline layout rationale.
struct Viewport { size: vec2<f32> };
struct Immediates { viewport: Viewport };
var<immediate> imm: Immediates;

@group(0) @binding(0) var tex:     texture_2d<f32>;
@group(0) @binding(1) var tex_smp: sampler;

// Bits of `flags` — must match `IMG_FLAG_*` in `render_buffer.rs`.
const FLAG_TILED:   u32 = 1u;
const FLAG_NEAREST: u32 = 2u;

struct VsIn {
    // Per-instance.
    @location(0) rect_min:  vec2<f32>,
    @location(1) rect_size: vec2<f32>,
    @location(2) uv_min:    vec2<f32>,
    @location(3) uv_size:   vec2<f32>,
    @location(4) tint:      vec4<f32>,
    @location(5) flags:     u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)        uv:   vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, in: VsIn) -> VsOut {
    // Four-corner triangle-strip: (0,0) (1,0) (0,1) (1,1).
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let phys = in.rect_min + c * in.rect_size;
    let ndc = vec2<f32>(
        phys.x / imm.viewport.size.x * 2.0 - 1.0,
        1.0 - phys.y / imm.viewport.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv   = in.uv_min + c * in.uv_size;
    out.tint = in.tint;
    out.flags = in.flags;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // `ImageFit::Tile` ships UVs spanning [0, repeats]; wrap into the
    // [0,1) tile with `fract` (the ClampToEdge sampler would otherwise
    // clamp). Per-fragment, so each repeat samples the full tile. Other
    // fits keep UVs in [0,1] and sample directly — `fract(1.0)=0.0`
    // would wrap a Cover crop's far edge, so it must stay gated.
    var uv = in.uv;
    if ((in.flags & FLAG_TILED) != 0u) {
        uv = fract(in.uv);
    }
    // `ImageFilter::Nearest`: snap the UV to the texel center, which
    // lands the bilinear weights exactly on one texel — nearest
    // sampling without a second sampler / bind group. Single mip, so
    // the snapped UV's garbage derivatives can't pick a wrong level.
    if ((in.flags & FLAG_NEAREST) != 0u) {
        let dims = vec2<f32>(textureDimensions(tex));
        uv = (floor(uv * dims) + vec2<f32>(0.5)) / dims;
    }
    // sRGB-format texture decodes to linear on read; tint is linear
    // straight-alpha. Multiply, then premultiply for the blend.
    let s = textureSample(tex, tex_smp, uv) * in.tint;
    return vec4<f32>(s.rgb * s.a, s.a);
}
