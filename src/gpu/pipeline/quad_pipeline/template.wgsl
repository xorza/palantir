// A box, a circle and a triangle drawn as the shape template (`shape_template.wgsl`) the vertex
// stage builds from the quad instance (`vs_template`), and its fragment (`fs_template`).

// What a template fragment reads (`vs_template`, `fs_template`), and nothing else: on an Apple
// GPU a fragment's cost follows the varyings its stage reads, so each form reads only its own.
struct TemplateOut {
    @builtin(position) clip: vec4<f32>,
    // The region the fragment's triangle lies in: `PALANTIR_REGION_INTERIOR`,
    // `PALANTIR_REGION_BAND` or `PALANTIR_REGION_RING`.
    @location(0) @interpolate(flat) region: u32,
    // A band's signed distance to its edge in `x`; a ring's position from its arc's center.
    @location(1) geo: vec2<f32>,
    // A ring's arc radius.
    @location(2) @interpolate(flat) ring_r: f32,
    // The fill and the border's color, as the instance packs them: four f16 lanes two to a
    // `u32`, half the varyings of a `vec4<f32>`.
    @location(3) @interpolate(flat) fill_packed: vec2<u32>,
    @location(4) @interpolate(flat) border_color_packed: vec2<u32>,
    @location(5) @interpolate(flat) border_width: f32,
    // The quad's own frame, which a gradient and a turned outside rect's edge read.
    @location(6) local: vec2<f32>,
    @location(7) @interpolate(flat) fill_kind: u32,
    @location(8) @interpolate(flat) fill_lut_row: u32,
    @location(9) @interpolate(flat) fill_axis: vec4<f32>,
    @location(10) @interpolate(flat) inv_size: vec2<f32>,
    // A turned outside rect's size, whose edge fades as any edge does; zero otherwise.
    @location(11) @interpolate(flat) turned_size: vec2<f32>,
    @location(12) @interpolate(flat) chain: u32,
};

// `vs` for a box, a circle or a triangle drawn as its template: the regions where the shape's
// distance has a closed form, so the fragment takes the fill alone inside, interpolates the
// distance across an edge's band, and measures one length across a corner's ring.
@vertex
fn vs_template(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, quad: QuadIn) -> TemplateOut {
    let q = quad_vertex(0u, shape, quad);
    let pos = shape.rect.xy;
    let size = shape.rect.zw;
    let rotation = shape.inradius_rotation.y;
    let outside = form_is_outside();
    let s = palantir_shape_of(shape, select(4u, 3u, form_is_triangle()));
    // An outside rect over content of its own extent stops at its rect; turned, its edge is visible
    // and fades as any edge does, so it reaches past it too.
    let outset = select(PALANTIR_AA_HALF_WIDTH, 0.0, outside && rotation == 0.0);
    let v = palantir_template_vertex(s, vi, quad.border_width, outset, outside);
    var out: TemplateOut;
    out.region = v.region;
    out.geo = v.geo;
    out.ring_r = v.ring_r;
    out.fill_packed = quad.fill_packed;
    out.border_color_packed = quad.border_color_packed;
    out.border_width = q.border_width;
    out.local = v.pos;
    out.fill_kind = q.fill_kind;
    out.fill_lut_row = q.fill_lut_row;
    out.fill_axis = q.fill_axis;
    out.inv_size = q.inv_size;
    out.turned_size = select(vec2<f32>(0.0), size, outside && rotation != 0.0);
    out.chain = shape.chain;
    out.clip = clip_from_px(turned(pos + v.pos, pos + size * 0.5, rotation));
    return out;
}

// A template fragment: what `template_color` draws, cut by the quad's rounded clips.
@fragment
fn fs_template(in: TemplateOut) -> @location(0) vec4<f32> {
    return template_color(in) * palantir_clip_coverage(frag_px(in.clip), in.chain);
}

// The fill alone inside; across a band the interpolated distance; across a ring the distance to
// its arc. Then the fill and the border composite as the reference's do.
fn template_color(in: TemplateOut) -> vec4<f32> {
    let fill = quad_paint(
        unpack_f16x4(in.fill_packed),
        in.local * in.inv_size,
        in.fill_kind,
        in.fill_axis,
        in.fill_lut_row,
    );
    if (in.region == PALANTIR_REGION_INTERIOR) {
        return premultiply(fill.rgb, fill.a);
    }
    let border_color = unpack_f16x4(in.border_color_packed);
    let d = palantir_template_distance(in.region, in.geo, in.ring_r);
    if (form_is_outside()) {
        var cover = composite_outside(d, fill, border_color, in.border_width);
        if (in.turned_size.x > 0.0) {
            let e = abs(in.local - in.turned_size * 0.5) - in.turned_size * 0.5;
            cover *= palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y);
        }
        return cover;
    }
    return composite(d, fill, border_color, in.border_width);
}
