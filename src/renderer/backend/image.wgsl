// User-image pipeline. Per-instance rect + tint; texture+sampler in
// group 1, switched per draw by the backend. Four-corner quad emitted
// implicitly via `vertex_index` (TriangleStrip).
//
// Colour pipeline: texture is `Rgba8UnormSrgb`, so the sampler decodes
// sRGB → linear automatically. We multiply by `tint` (linear, straight
// alpha) and premultiply at write time to match the rest of the
// premultiplied-blend pipeline.

struct Viewport { size: vec2<f32> };
@group(0) @binding(0) var<uniform> vp: Viewport;

@group(1) @binding(0) var tex:     texture_2d<f32>;
@group(1) @binding(1) var tex_smp: sampler;

struct VsIn {
    // Per-instance.
    @location(0) rect_min:  vec2<f32>,
    @location(1) rect_size: vec2<f32>,
    @location(2) tint:      vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)        uv:   vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
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
        phys.x / vp.size.x * 2.0 - 1.0,
        1.0 - phys.y / vp.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv   = c;
    out.tint = in.tint;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // sRGB-format texture decodes to linear on read; tint is linear
    // straight-alpha. Multiply, then premultiply for the blend.
    let s = textureSample(tex, tex_smp, in.uv) * in.tint;
    return vec4<f32>(s.rgb * s.a, s.a);
}
