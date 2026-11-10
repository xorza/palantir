// The one-quad form of a box and a triangle, each fragment taking the shape's SDF: the reference
// the template is compared against, built only by the crate's internals.

@vertex
fn vs(@builtin(vertex_index) vi: u32, shape: PalantirShapeIn, quad: QuadIn) -> VertexOut {
    var out = quad_vertex(vi, shape, quad);
    if (form_is_triangle()) {
        // The triangle's points are its corners' arc centers, which the instance's grown points
        // and radius give (`palantir_shape_corner`); `fs` takes them in `radius` and `core`, which
        // a triangle does not read, the radius in `core.z`.
        let s = palantir_shape_of(shape, 3u);
        let a = palantir_shape_corner(s, 0u);
        out.radius = vec4<f32>(a.c, palantir_shape_corner(s, 1u).c);
        out.core = vec4<f32>(palantir_shape_corner(s, 2u).c, a.r, 0.0);
    }
    return out;
}

// Signed distance to the triangle (a, b, c) — negative inside, positive
// outside (Inigo Quilez's `sdTriangle`). `s` folds in the winding sign so
// the result is correctly signed for either orientation; subtracting a
// radius from the caller rounds all three corners uniformly.
//
// A degenerate triangle — collinear corners, or coincident ones — has no
// inside. Its winding sign is 0, so `d.y` is 0 everywhere; a sign taken
// from `d.y` would make the distance 0 everywhere, and a radius would
// then fill the whole quad. So `d.y == 0` reads as outside, which on a
// proper triangle happens only on an edge, where the distance is 0
// either way. A zero-length edge divides 0 by 0 in its projection; the
// floor keeps that finite, and the edge's zero vector then makes the
// projection's value irrelevant.
fn sdf_triangle(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a; let e1 = c - b; let e2 = a - c;
    let v0 = p - a; let v1 = p - b; let v2 = p - c;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / max(dot(e0, e0), 1e-30), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / max(dot(e1, e1), 1e-30), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / max(dot(e2, e2), 1e-30), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(min(
        vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
        vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
        vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)));
    let dist = sqrt(d.x);
    return select(dist, -dist, d.y > 0.0);
}

// The one-quad form's fragment: what `reference_color` draws, cut by the quad's rounded clips.
@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    return reference_color(in) * palantir_clip_coverage(frag_px(in.clip), in.chain);
}

// The shape's SDF composites its fill and inner-edge border into premultiplied linear.
fn reference_color(in: VertexOut) -> vec4<f32> {
    let fill = quad_paint(in.fill, in.local * in.inv_size, in.fill_kind, in.fill_axis, in.fill_lut_row);
    if (form_is_triangle()) {
        // The three corner centers, in `local` 0..size coords, and the corner radius, as `vs`
        // put them. `sdf_triangle - radius` gives the rounded shape, whose edges lie on the
        // sharp triangle's; `composite` applies the coverage and the inner border a rect takes.
        let td = sdf_triangle(in.local, in.radius.xy, in.radius.zw, in.core.xy) - in.core.z;
        return composite(td, fill, in.border_color, in.border_width);
    }
    return shade_rect(in, fill);
}

// A rounded rect of `fill` under the instance's border. A point past
// `border_width + PALANTIR_AA_HALF_WIDTH` inside the shape is the fill, without the
// distance: both coverages are 1 and the border's share 0.
fn shade_rect(in: VertexOut, fill: vec4<f32>) -> vec4<f32> {
    if (all(in.local > in.core.xy) && all(in.local < in.core.zw)) {
        return premultiply(fill.rgb, fill.a);
    }
    let d = palantir_sdf_rounded_rect(in.local, in.size, in.radius);
    if (form_is_outside()) {
        var cover = composite_outside(d, fill, in.border_color, in.border_width);
        if (in.turned != 0u) {
            // Turned, the quad edge no longer lies on content of its own
            // extent, so it is a visible edge and fades as any edge does.
            let e = abs(in.local - in.size * 0.5) - in.size * 0.5;
            cover *= palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y);
        }
        return cover;
    }
    return composite(d, fill, in.border_color, in.border_width);
}
