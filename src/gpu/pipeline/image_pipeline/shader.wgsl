// User-image pipeline. Per-instance rect + tint; texture+sampler in
// the group past the frame group, switched per draw by the backend. Four-corner quad emitted
// implicitly via `vertex_index` (TriangleStrip).
//
// Color pipeline: texture is `Rgba8UnormSrgb`, so the sampler decodes
// sRGB → linear automatically. Texels were premultiplied at upload
// (`image_store::premultiply_into`), so the sample is premultiplied and
// the filter between texels blends premultiplied. `tint` (linear, straight
// alpha) is premultiplied in `image_paint` and the two multiply as premultiplied
// values, matching the rest of the premultiplied-blend pipeline.

const IMAGE_GROUP: u32 = /*{IMAGE_GROUP}*/;
@group(IMAGE_GROUP) @binding(0) var tex:     texture_2d<f32>;
@group(IMAGE_GROUP) @binding(1) var tex_smp: sampler;

// The instance attributes an image reads past its shape (`PalantirShapeIn`): the UV rect
// `(min, size)`, the tint, the flags, and the decal a shaped image fades past.
struct VsIn {
    @location(6) uv:    vec4<f32>,
    @location(7) tint:  vec4<f32>,
    @location(8) flags: u32,
    @location(9) decal: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)        uv:   vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    // The UV rect, `(min, max)`, which a `FLAG_CLAMP_TO_UV` sample stays inside.
    @location(3) @interpolate(flat) uv_rect: vec4<f32>,
    // A turned image's position in its own unturned frame, relative to its
    // center, and its half size: `fs` fades the edges by them. Unread
    // unturned, where the rect's edges are the drawn quad's.
    @location(4) local: vec2<f32>,
    @location(5) @interpolate(flat) half_size: vec2<f32>,
    @location(6) @interpolate(flat) turned: u32,
    @location(7) @interpolate(flat) chain: u32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, in: VsIn) -> VsOut {
    let rect_min = shape.rect.xy;
    let rect_size = shape.rect.zw;
    let rotation = shape.inradius_rotation.y;
    var c = CORNERS[vi];
    var out: VsOut;
    let half = rect_size * 0.5;
    var at = rect_min + c * rect_size;
    if (rotation != 0.0) {
        // A turned edge falls between pixels, so the quad reaches
        // `PALANTIR_AA_HALF_WIDTH` past it in the image's own frame, where a rotation
        // keeps distances, and the uv runs on with it.
        c = (c * rect_size + (c * 2.0 - 1.0) * PALANTIR_AA_HALF_WIDTH) / max(rect_size, vec2<f32>(1e-6));
        let l = c * rect_size - half;
        let cs = vec2<f32>(cos(rotation), sin(rotation));
        at = rect_min + half + vec2<f32>(cs.x * l.x - cs.y * l.y, cs.y * l.x + cs.x * l.y);
    }
    out.clip = clip_from_px(at);
    out.local = c * rect_size - half;
    out.half_size = half;
    out.turned = u32(rotation != 0.0);
    out.uv   = in.uv.xy + c * in.uv.zw;
    out.tint = in.tint;
    out.flags = in.flags;
    out.uv_rect = vec4<f32>(in.uv.xy, in.uv.xy + in.uv.zw);
    out.chain = shape.chain;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var out = image_paint(in.uv, in.uv_rect, in.flags, in.tint)
        * palantir_clip_coverage(frag_px(in.clip), in.chain);
    if (in.turned != 0u) {
        // Distance outside the nearer edge on each axis, a box's ramp.
        let d = abs(in.local) - in.half_size;
        out *= palantir_edge_coverage(d.x) * palantir_edge_coverage(d.y);
    }
    return out;
}
