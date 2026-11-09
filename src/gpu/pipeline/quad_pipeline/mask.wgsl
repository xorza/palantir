// The rounded clip's stencil: the clip's shape drawn as its template, with every fragment of an
// edge's band or a corner's ring past the shape discarded.

// What a mask fragment reads: the template region, distance and ring.
struct MaskOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) region: u32,
    @location(1) geo: vec2<f32>,
    @location(2) @interpolate(flat) ring_r: f32,
};

// The mask's template, as a box's is built with no border, its outer chain on the shape's edge:
// the stencil takes no antialiasing reach.
@vertex
fn vs_mask(@builtin(vertex_index) vi: u32, quad: QuadIn) -> MaskOut {
    let q = quad_vertex(0u, quad);
    let s = instance_shape(quad, 4u, q.radius);
    let v = palantir_template_vertex(s, vi, 0.0, 0.0, false);
    var out: MaskOut;
    out.region = v.region;
    out.geo = v.geo;
    out.ring_r = v.ring_r;
    out.clip = clip_from_px(turned(quad.pos + v.pos, quad.pos + quad.size * 0.5, quad.rotation));
    return out;
}

// Stencil mask-write: `discard` outside the rounded shape so those pixels skip the
// post-fragment stencil op (Replace) entirely, leaving stencil at 0 outside the rounded region.
// The color write_mask is empty in the mask pipeline, so the returned vec4 is dropped; only the
// stencil side effect matters. An interior fragment is inside with no distance. Hard threshold at
// distance 0 (no AA on the mask edge): the panel's painted rounded background already AA's the
// visible boundary; the stencil mask just controls which children pixels survive, and a 1-pixel
// hard inner edge sits behind the AA rim where it's invisible.
@fragment
fn fs_mask(in: MaskOut) -> @location(0) vec4<f32> {
    if (in.region != PALANTIR_REGION_INTERIOR
        && palantir_template_distance(in.region, in.geo, in.ring_r) > 0.0) {
        discard;
    }
    return vec4<f32>(0.0);
}
