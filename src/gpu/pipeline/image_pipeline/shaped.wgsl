// An image clipped to a shape: the shape template (`shape_template.wgsl`) the vertex stage builds
// from the instance, and the image paint (`image_paint`) its fragment covers it with, faded past
// the image's own rect where the shape reaches beyond it.

// What a shaped image's fragment reads (`shaped_vertex`, `fs_shaped`).
struct ShapedOut {
    @builtin(position) clip: vec4<f32>,
    // The template region, distance and ring, as `TemplateOut` carries them.
    @location(0) @interpolate(flat) region: u32,
    @location(1) geo: vec2<f32>,
    @location(2) @interpolate(flat) ring_r: f32,
    @location(3) uv: vec2<f32>,
    @location(4) @interpolate(flat) uv_rect: vec4<f32>,
    @location(5) @interpolate(flat) tint: vec4<f32>,
    @location(6) @interpolate(flat) flags: u32,
    // The point in the shape's own frame, which the decal fade reads.
    @location(7) local: vec2<f32>,
    @location(8) @interpolate(flat) decal: vec4<f32>,
    @location(9) @interpolate(flat) chain: u32,
};

// A box's template (`ShapeForm::Box`).
@vertex
fn vs_box(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, in: VsIn) -> ShapedOut {
    return shaped_vertex(vi, shape, in, 4u);
}

// A triangle's template (`ShapeForm::Triangle`), three of the instance's points.
@vertex
fn vs_triangle(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, in: VsIn) -> ShapedOut {
    return shaped_vertex(vi, shape, in, 3u);
}

// Vertex `vi` of the template of the instance's shape of `n` corners, a constant of each entry.
fn shaped_vertex(vi: u32, shape: PalantirShapeIn, in: VsIn, n: u32) -> ShapedOut {
    let rect_min = shape.rect.xy;
    let rect_size = shape.rect.zw;
    // No border: the image covers the whole shape.
    let v = palantir_template_vertex(palantir_shape_of(shape, n), vi, 0.0);
    var out: ShapedOut;
    out.region = v.region;
    out.geo = v.geo;
    out.ring_r = v.ring_r;
    out.uv = in.uv.xy + v.pos / max(rect_size, vec2<f32>(1e-6)) * in.uv.zw;
    out.uv_rect = vec4<f32>(in.uv.xy, in.uv.xy + in.uv.zw);
    out.tint = in.tint;
    out.flags = in.flags;
    out.local = v.pos;
    out.decal = in.decal;
    out.chain = shape.chain;
    out.clip = palantir_clip_from_px(palantir_turned(rect_min + v.pos, rect_min + rect_size * 0.5, shape.inradius_rotation.y));
    return out;
}

// The image within its decal, faded at its template's edge as a quad's is, and cut by its rounded
// clips.
@fragment
fn fs_shaped(in: ShapedOut) -> @location(0) vec4<f32> {
    let e = max(in.decal.xy - in.local, in.local - in.decal.zw);
    return image_paint(in.uv, in.uv_rect, in.flags, in.tint)
        * palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y)
        * palantir_template_coverage(in.region, in.geo, in.ring_r)
        * palantir_clip_coverage(palantir_frag_px(in.clip), in.chain);
}
