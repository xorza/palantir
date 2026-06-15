// User-image pipeline. Per-instance rect + tint; texture+sampler in
// group 1, switched per draw by the backend. Four-corner quad emitted
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

// Per-instance sampling mode — mirrors `ImageMode` in `render_buffer.rs`.
// Only `Tile` changes sampling here; `Direct` and `RenderTarget` both
// sample the encoder-/composer-resolved crop directly (a `GpuView`'s crop
// is `used / capacity`, written by the composer's GpuView size pass).
const MODE_TILE: u32 = 1u;

struct VsIn {
    // Per-instance.
    @location(0) rect_min:  vec2<f32>,
    @location(1) rect_size: vec2<f32>,
    @location(2) uv_min:    vec2<f32>,
    @location(3) uv_size:   vec2<f32>,
    @location(4) tint:      vec4<f32>,
    @location(5) mode:      u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)        uv:   vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) mode: u32,
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
    out.mode = in.mode;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // `ImageFit::Tile` ships UVs spanning [0, repeats]; wrap into the
    // [0,1) tile with `fract` (the ClampToEdge sampler would otherwise
    // clamp). Per-fragment, so each repeat samples the full tile. Other
    // modes keep UVs in [0,1] and sample directly — `fract(1.0)=0.0`
    // would wrap a Cover crop's far edge, so it must stay gated.
    var uv = in.uv;
    if (in.mode == MODE_TILE) {
        uv = fract(in.uv);
    }
    // sRGB-format texture decodes to linear on read; tint is linear
    // straight-alpha. Multiply, then premultiply for the blend.
    let s = textureSample(tex, tex_smp, uv) * in.tint;
    return vec4<f32>(s.rgb * s.a, s.a);
}
