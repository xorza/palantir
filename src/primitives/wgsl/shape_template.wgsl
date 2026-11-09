// The template a closed shape's vertex stage builds, the same for every pipeline that draws one:
// per corner a ring, per edge a band, and an interior fan (`palantir_template_vertex`). The
// includer describes the `PalantirShape` from its own instance and reads the regions in its
// fragment.

// The regions of a shape's template (`palantir_template_vertex`).
const PALANTIR_REGION_INTERIOR: u32 = 0u;
const PALANTIR_REGION_BAND: u32 = 1u;
const PALANTIR_REGION_RING: u32 = 2u;

// Segments of each corner's arc in the template (`wgsl::ARC_SEGMENTS`).
const PALANTIR_ARC_SEGMENTS: u32 = /*{ARC_SEGMENTS}*/;

// A template point's kinds (`palantir_template_point`).
const PALANTIR_POINT_OUTER: u32 = 0u;
const PALANTIR_POINT_INNER: u32 = 1u;
const PALANTIR_POINT_DEEP: u32 = 2u;

// A shape as its template reads it: a convex polygon of `n` corners, 3 or 4, in the quad's own
// frame, as the CPU describes it (`Quad::points`): sharp points clockwise on screen, a radius at
// each, and how deep its interior reaches. Corner `i` is an arc of radius `r` about `c`, swept from
// `na`, the outward normal of the edge before it, to `nb`, the normal of the edge from corner `i` to
// corner `i + 1`; a sharp corner has radius 0. Every kind of shape takes the one path: each corner
// is worked out on its own from the instance, with no array a vertex indexes, which the Apple
// compiler spilled to memory.
struct PalantirShape {
    n: u32,
    p0: vec2<f32>,
    p1: vec2<f32>,
    p2: vec2<f32>,
    p3: vec2<f32>,
    // The radius at each point, `(tl, tr, br, bl)` for a box.
    radius: vec4<f32>,
    // The depth past every edge where the medial axis stops the interior.
    deep: f32,
};

struct PalantirCorner {
    c: vec2<f32>,
    r: f32,
    na: vec2<f32>,
    nb: vec2<f32>,
};

// Point `i` of `s`.
fn palantir_shape_point(s: PalantirShape, i: u32) -> vec2<f32> {
    return select(select(s.p3, s.p2, i == 2u), select(s.p1, s.p0, i == 0u), i < 2u);
}

// The outward normal of edge `e`, from point `e` to point `e + 1`: clockwise on screen, its
// direction turned a quarter to the left of the pixel grid's y-down frame.
fn palantir_edge_normal(s: PalantirShape, e: u32) -> vec2<f32> {
    let d = palantir_shape_point(s, (e + 1u) % s.n) - palantir_shape_point(s, e);
    return vec2<f32>(d.y, -d.x) / length(d);
}

// Corner `i` of `s`: its arc's center lies its radius inside both edges' lines, along the
// bisector of their normals.
fn palantir_shape_corner(s: PalantirShape, i: u32) -> PalantirCorner {
    var k: PalantirCorner;
    k.na = palantir_edge_normal(s, (i + s.n - 1u) % s.n);
    k.nb = palantir_edge_normal(s, i);
    k.r = select(select(s.radius.w, s.radius.z, i == 2u), select(s.radius.y, s.radius.x, i == 0u), i < 2u);
    k.c = palantir_shape_point(s, i) - k.r * (k.na + k.nb) / (1.0 + dot(k.na, k.nb));
    return k;
}

// The unit direction a fraction `f` of the way from `na` to `nb` along the arc between them,
// exactly `na` and `nb` at the ends, so a point shared with a band lands where the band's does.
fn palantir_arc_dir(na: vec2<f32>, nb: vec2<f32>, f: f32) -> vec2<f32> {
    if (f <= 0.0) {
        return na;
    }
    if (f >= 1.0) {
        return nb;
    }
    let sweep = acos(clamp(dot(na, nb), -1.0, 1.0));
    return (sin((1.0 - f) * sweep) * na + sin(f * sweep) * nb) / sin(sweep);
}

// A point of the template, in the quad's own frame. Every vertex goes through this one call, so
// two triangles that share a point compute it from the same arithmetic and the same inputs, and
// the rasterizer shades each pixel along their shared edge once.
//   - `PALANTIR_POINT_OUTER`, `j` in `0..=PALANTIR_ARC_SEGMENTS + 1`: the outer chain of corner
//     `k`, `outset` past its arc. The ends lie on the two normals; between them the vertices of
//     the polygon that circumscribes the arc, or the sharp corner of an outside rect, which paints
//     out to it.
//   - `PALANTIR_POINT_INNER`, `j` in `0..=PALANTIR_ARC_SEGMENTS`: the inner chain, inscribed in
//     the arc `depth` inside the shape, or the arc's center where the corner is not that deep.
//   - `PALANTIR_POINT_DEEP`: where the two edges' lines `depth` inside the shape meet, or the
//     medial axis where the shape is not that deep.
fn palantir_template_point(k: PalantirCorner, deep: f32, kind: u32, j: u32, outset: f32, depth: f32, outside: bool) -> vec2<f32> {
    let segments = f32(PALANTIR_ARC_SEGMENTS);
    // The sharp corner's direction from the arc's center, scaled so the corner sits a unit
    // depth past both edges' lines.
    let m = (k.na + k.nb) / (1.0 + dot(k.na, k.nb));
    if (kind == PALANTIR_POINT_DEEP) {
        return k.c + max(k.r - depth, k.r - deep) * m;
    }
    if (kind == PALANTIR_POINT_INNER) {
        return k.c + max(k.r - depth, 0.0) * palantir_arc_dir(k.na, k.nb, f32(j) / segments);
    }
    let reach = k.r + outset;
    if (j == 0u) {
        return k.c + reach * k.na;
    }
    if (j == PALANTIR_ARC_SEGMENTS + 1u) {
        return k.c + reach * k.nb;
    }
    if (outside) {
        return k.c + reach * m;
    }
    let half_step = 0.5 * acos(clamp(dot(k.na, k.nb), -1.0, 1.0)) / segments;
    return k.c + (reach / cos(half_step)) * palantir_arc_dir(k.na, k.nb, (f32(j) - 0.5) / segments);
}

// The triangles of a template of `n` corners, in vertex order: each corner's ring, a strip of
// `2 * PALANTIR_ARC_SEGMENTS + 1` triangles between its outer and inner chains; each edge's band, 4
// triangles over the hexagon from its two outer ends in to its two inner ends and the two deep
// points; then the interior, a fan over every corner's inner chain, or its deep point where the
// corner is not deep enough. `shape_template::vertices` counts them.
const PALANTIR_RING_TRIANGLES: u32 = 2u * PALANTIR_ARC_SEGMENTS + 1u;
const PALANTIR_BAND_TRIANGLES: u32 = 4u;

// A band's hexagon `[O_e, O_next, I_next, D_next, D_e, I_e]`, its 4 triangles as 3 nibbles each:
// `O` an outer end, `I` an inner end and `D` a deep point, which is the inner end where its
// corner is deep enough.
const PALANTIR_BAND_HEXAGON = vec4<u32>(0x210u, 0x520u, 0x325u, 0x435u);

// One vertex of the template: its region, its point, and for a ring the arc it reads, for a band
// the edge's distance at the point.
struct PalantirTemplateVertex {
    region: u32,
    pos: vec2<f32>,
    geo: vec2<f32>,
    ring_r: f32,
};

// Vertex `vi` of the template of `s`, whose border is `border_width` wide. An outside rect paints out
// to its rect, `outset` 0, and draws no interior, which it leaves transparent.
fn palantir_template_vertex(s: PalantirShape, vi: u32, border_width: f32, outset: f32, outside: bool) -> PalantirTemplateVertex {
    let depth = border_width + PALANTIR_AA_HALF_WIDTH;
    let tri = vi / 3u;
    let at = vi % 3u;
    let rings = s.n * PALANTIR_RING_TRIANGLES;
    let bands = s.n * PALANTIR_BAND_TRIANGLES;
    var out: PalantirTemplateVertex;
    var kind = PALANTIR_POINT_OUTER;
    var i = 0u;
    var j = 0u;
    // The edge a band vertex measures its distance to.
    var edge = 0u;
    if (tri < rings) {
        out.region = PALANTIR_REGION_RING;
        i = tri / PALANTIR_RING_TRIANGLES;
        let t = tri % PALANTIR_RING_TRIANGLES;
        let step = t / 2u;
        // `(o_s, o_s+1, q_s)` on even steps, `(q_s, o_s+1, q_s+1)` on odd ones.
        let odd = t % 2u == 1u;
        let inner = (odd && at != 1u) || (!odd && at == 2u);
        kind = select(PALANTIR_POINT_OUTER, PALANTIR_POINT_INNER, inner);
        j = step + u32(at == 1u || (odd && at == 2u));
    } else if (tri < rings + bands) {
        out.region = PALANTIR_REGION_BAND;
        edge = (tri - rings) / PALANTIR_BAND_TRIANGLES;
        let h = (PALANTIR_BAND_HEXAGON[(tri - rings) % PALANTIR_BAND_TRIANGLES] >> (4u * at)) & 0xfu;
        let next = (edge + 1u) % s.n;
        let first = h == 1u || h == 2u || h == 3u;
        i = select(edge, next, first);
        if (h <= 1u) {
            kind = PALANTIR_POINT_OUTER;
            j = select(PALANTIR_ARC_SEGMENTS + 1u, 0u, first);
        } else if (h == 2u || h == 5u || palantir_shape_corner(s, i).r >= depth) {
            kind = PALANTIR_POINT_INNER;
            j = select(PALANTIR_ARC_SEGMENTS, 0u, first);
        } else {
            kind = PALANTIR_POINT_DEEP;
        }
    } else {
        out.region = PALANTIR_REGION_INTERIOR;
        let fan = select(0u, tri - rings - bands + at, at > 0u);
        i = fan / (PALANTIR_ARC_SEGMENTS + 1u);
        j = fan % (PALANTIR_ARC_SEGMENTS + 1u);
        kind = select(PALANTIR_POINT_DEEP, PALANTIR_POINT_INNER, palantir_shape_corner(s, i).r >= depth);
        if (outside) {
            // No interior: every vertex on one point.
            kind = PALANTIR_POINT_INNER;
            i = 0u;
            j = 0u;
        }
    }
    let k = palantir_shape_corner(s, i);
    out.pos = palantir_template_point(k, s.deep, kind, j, outset, depth, outside);
    out.ring_r = k.r;
    if (out.region == PALANTIR_REGION_RING) {
        out.geo = out.pos - k.c;
    } else if (out.region == PALANTIR_REGION_BAND) {
        let e = palantir_shape_corner(s, edge);
        out.geo = vec2<f32>(dot(out.pos - e.c, e.nb) - e.r, 0.0);
    } else {
        out.geo = vec2<f32>(0.0);
    }
    return out;
}

// The shape's signed distance at a fragment of a band or a ring, from what `palantir_template_vertex`
// handed it: a band's interpolated distance, a ring's length to its arc. An interior fragment is
// inside past every edge's reach, so its caller takes the fill there without a distance.
fn palantir_template_distance(region: u32, geo: vec2<f32>, ring_r: f32) -> f32 {
    if (region == PALANTIR_REGION_RING) {
        return length(geo) - ring_r;
    }
    return geo.x;
}
