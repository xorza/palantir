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
};

// A box's template (`ImageForm::Box`).
@vertex
fn vs_box(@builtin(vertex_index) vi: u32, in: VsIn) -> ShapedOut {
    return shaped_vertex(vi, in, 4u);
}

// A triangle's template (`ImageForm::Triangle`), three of the instance's points.
@vertex
fn vs_triangle(@builtin(vertex_index) vi: u32, in: VsIn) -> ShapedOut {
    return shaped_vertex(vi, in, 3u);
}

// Vertex `vi` of the template of the instance's shape of `n` corners, a constant of each entry.
fn shaped_vertex(vi: u32, in: VsIn, n: u32) -> ShapedOut {
    var s: PalantirShape;
    s.n = n;
    s.p0 = in.points01.xy;
    s.p1 = in.points01.zw;
    s.p2 = in.points23.xy;
    s.p3 = in.points23.zw;
    s.radius = in.corners;
    s.inradius = in.inradius;
    // No border: the image covers the whole shape.
    let v = palantir_template_vertex(s, vi, 0.0, PALANTIR_AA_HALF_WIDTH, false);
    var out: ShapedOut;
    out.region = v.region;
    out.geo = v.geo;
    out.ring_r = v.ring_r;
    out.uv = in.uv_min + v.pos / max(in.rect_size, vec2<f32>(1e-6)) * in.uv_size;
    out.uv_rect = vec4<f32>(in.uv_min, in.uv_min + in.uv_size);
    out.tint = in.tint;
    out.flags = in.flags;
    out.local = v.pos;
    out.decal = in.decal;
    out.clip = clip_from_px(turned(in.rect_min + v.pos, in.rect_min + in.rect_size * 0.5, in.rotation));
    return out;
}

// The image inside; across a band the interpolated distance and across a ring the distance to
// its arc fade it, as a quad's template edge fades.
@fragment
fn fs_shaped(in: ShapedOut) -> @location(0) vec4<f32> {
    let e = max(in.decal.xy - in.local, in.local - in.decal.zw);
    let paint = image_paint(in.uv, in.uv_rect, in.flags, in.tint)
        * palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y);
    if (in.region == PALANTIR_REGION_INTERIOR) {
        return paint;
    }
    return paint * palantir_edge_coverage(palantir_template_distance(in.region, in.geo, in.ring_r));
}
