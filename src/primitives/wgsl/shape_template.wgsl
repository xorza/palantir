// The template a closed shape's vertex stage builds, the same for every pipeline that draws one:
// per corner a ring, per edge a band, and an interior fan (`palantir_template_vertex`), drawn
// indexed (`shape_template::TemplateIndices`). Every includer's instance opens with the shape
// (`ShapeHeader`), which `palantir_shape_of` reads, and its fragment stage takes the shape's
// coverage or a box's fill from the regions; it reads `ANTIALIAS` and `COMPOSITE`.

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
// frame, as the CPU describes it (`ShapeHeader::points`): sharp points clockwise on screen, a radius at
// each, and its inradius, the deepest any point lies inside every edge. Corner `i` is an arc of radius `r` about `c`, swept from
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
    // The radius of the largest circle inside the sharp polygon: no point lies deeper inside
    // every edge.
    inradius: f32,
};

// The shape every template instance opens with (`ShapeHeader`, `shape_template::HEADER_ATTRS`): its
// rect `(min, size)`, its sharp points from `min` two to a lane, a radius at each point, its
// inradius beside its turn about the rect's center in radians, and the rounded clips that cut it
// (`ClipChain`).
struct PalantirShapeIn {
    @location(0) rect: vec4<f32>,
    @location(1) points01: vec4<f32>,
    @location(2) points23: vec4<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) inradius_rotation: vec2<f32>,
    @location(5) chain: u32,
};

// The shape of `n` corners `shape` describes.
fn palantir_shape_of(shape: PalantirShapeIn, n: u32) -> PalantirShape {
    var s: PalantirShape;
    s.n = n;
    s.p0 = shape.points01.xy;
    s.p1 = shape.points01.zw;
    s.p2 = shape.points23.xy;
    s.p3 = shape.points23.zw;
    s.radius = shape.radius;
    s.inradius = shape.inradius_rotation.x;
    return s;
}

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

// A point of the template, in the quad's own frame. Every point is one vertex of the indexed
// template, so the triangles that share it share its arithmetic, and the rasterizer shades each
// pixel along their shared edge once.
//   - `PALANTIR_POINT_OUTER`, `j` in `0..=PALANTIR_ARC_SEGMENTS + 1`: the outer chain of corner
//     `outer`, `reach` past its arc. The ends lie on the two normals; between them the vertices of
//     the polygon that circumscribes the arc.
//   - `PALANTIR_POINT_INNER`, `j` in `0..=PALANTIR_ARC_SEGMENTS`: the inner chain, inscribed in
//     the arc of corner `inner` `depth` inside it, or the arc's center where the corner is not
//     that deep.
//   - `PALANTIR_POINT_DEEP`: where `inner`'s two edges' lines `depth` inside it meet, or the
//     medial axis where the shape is not that deep.
// The arc's directions are sums of unit vectors, normalized: `h` halves the sweep from `na` to
// `nb`, and `na + h` and `h + nb` halve each half, `2 cos(sweep / 4)` long, so the circumscribing
// vertex `reach / cos(sweep / 4)` out along one is that sum times `2 reach / |sum|²`. No
// trigonometry, which V3D runs as long polynomials in each vertex; `wgsl::ARC_SEGMENTS` is 2 for
// these forms.
fn palantir_template_point(
    inner: PalantirCorner,
    outer: PalantirCorner,
    inradius: f32,
    kind: u32,
    j: u32,
    depth: f32,
    reach: f32,
) -> vec2<f32> {
    // The sharp corner's direction from the arc's center, scaled so the corner sits a unit
    // depth past both edges' lines.
    let m = (inner.na + inner.nb) / (1.0 + dot(inner.na, inner.nb));
    if (kind == PALANTIR_POINT_DEEP) {
        return inner.c + max(inner.r - depth, inner.r - inradius) * m;
    }
    let h = normalize(inner.na + inner.nb);
    if (kind == PALANTIR_POINT_INNER) {
        let dir = select(select(inner.nb, h, j == 1u), inner.na, j == 0u);
        return inner.c + max(inner.r - depth, 0.0) * dir;
    }
    let out_r = outer.r + reach;
    if (j == 0u) {
        return outer.c + out_r * outer.na;
    }
    if (j == PALANTIR_ARC_SEGMENTS + 1u) {
        return outer.c + out_r * outer.nb;
    }
    let h_out = normalize(outer.na + outer.nb);
    let sum = select(h_out + outer.nb, outer.na + h_out, j == 1u);
    return outer.c + (2.0 * out_r / dot(sum, sum)) * sum;
}

// The two outlines a template lies between: its inner chain `depth` inside `inner`, its outer
// chain `reach` outside `outer`. `inner`'s corner count is both's, and the two share their
// corners' normals, so the strip between the chains tiles the region. A region's distance is
// measured from `inner`'s corners, the shape's own where the two share their corners' centers, as
// a fill's and a halo's do; outlines that do not, a shadow's, leave it unread. A shape's fill is
// `palantir_fill_outlines`.
struct PalantirTemplateOutlines {
    inner: PalantirShape,
    depth: f32,
    outer: PalantirShape,
    reach: f32,
};

// The outlines of `s`'s fill under a border `border_width` wide: the inner chain past the border
// and the antialiasing reach, where the interior returns the fill alone, the outer chain the
// antialiasing reach past the edge.
fn palantir_fill_outlines(s: PalantirShape, border_width: f32) -> PalantirTemplateOutlines {
    return PalantirTemplateOutlines(s, border_width + PALANTIR_AA_HALF_WIDTH, s, PALANTIR_AA_HALF_WIDTH);
}

// The template's triangles, each corner's ring, each edge's band and the interior fan, draw
// indexed over its points, each point of a region one vertex (`shape_template::indices` holds the
// triangles):
//   - per corner, its ring's outer chain, `PALANTIR_ARC_SEGMENTS + 2` points, then its inner
//     chain, `PALANTIR_ARC_SEGMENTS + 1`;
//   - per edge, its band's hexagon `[O_e, O_next, I_next, D_next, D_e, I_e]`: `O` an outer end,
//     `I` an inner end and `D` a deep point, which is the inner end where its corner is deep
//     enough;
//   - the interior fan's points, every corner's inner chain, or its deep point where the corner
//     is not deep enough.
const PALANTIR_RING_POINTS: u32 = 2u * PALANTIR_ARC_SEGMENTS + 3u;
const PALANTIR_BAND_POINTS: u32 = 6u;

// One vertex of the template: its region, its point, and for a ring the arc it reads, for a band
// the edge's distance at the point.
struct PalantirTemplateVertex {
    region: u32,
    pos: vec2<f32>,
    geo: vec2<f32>,
    ring_r: f32,
};

// Point `u` of the template between `o`'s outlines.
fn palantir_template_vertex(o: PalantirTemplateOutlines, u: u32) -> PalantirTemplateVertex {
    let s = o.inner;
    let depth = o.depth;
    let rings = s.n * PALANTIR_RING_POINTS;
    let bands = s.n * PALANTIR_BAND_POINTS;
    var out: PalantirTemplateVertex;
    var kind = PALANTIR_POINT_OUTER;
    var i = 0u;
    var j = 0u;
    // The edge a band vertex measures its distance to.
    var edge = 0u;
    if (u < rings) {
        out.region = PALANTIR_REGION_RING;
        i = u / PALANTIR_RING_POINTS;
        let p = u % PALANTIR_RING_POINTS;
        let inner = p >= PALANTIR_ARC_SEGMENTS + 2u;
        kind = select(PALANTIR_POINT_OUTER, PALANTIR_POINT_INNER, inner);
        j = select(p, p - (PALANTIR_ARC_SEGMENTS + 2u), inner);
    } else if (u < rings + bands) {
        out.region = PALANTIR_REGION_BAND;
        edge = (u - rings) / PALANTIR_BAND_POINTS;
        let h = (u - rings) % PALANTIR_BAND_POINTS;
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
        let fan = u - rings - bands;
        i = fan / (PALANTIR_ARC_SEGMENTS + 1u);
        j = fan % (PALANTIR_ARC_SEGMENTS + 1u);
        kind = select(PALANTIR_POINT_DEEP, PALANTIR_POINT_INNER, palantir_shape_corner(s, i).r >= depth);
    }
    let k = palantir_shape_corner(s, i);
    out.pos = palantir_template_point(
        k,
        palantir_shape_corner(o.outer, i),
        s.inradius,
        kind,
        j,
        depth,
        o.reach,
    );
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
// handed it: a band's interpolated distance, a ring's length to its arc.
fn palantir_template_distance(region: u32, geo: vec2<f32>, ring_r: f32) -> f32 {
    if (region == PALANTIR_REGION_RING) {
        return length(geo) - ring_r;
    }
    return geo.x;
}

// The shape's coverage at a fragment of its template: full in the interior, which lies inside past
// every edge's reach, and the edge's ramp at the shape's distance elsewhere.
fn palantir_template_coverage(region: u32, geo: vec2<f32>, ring_r: f32) -> f32 {
    if (region == PALANTIR_REGION_INTERIOR) {
        return 1.0;
    }
    return palantir_edge_coverage(palantir_template_distance(region, geo, ring_r));
}

// The coverage of what lies outside the shape at a fragment of its template's rings and bands,
// the complement of `palantir_template_coverage` there: a halo's, which draws no interior.
fn palantir_template_outside_coverage(region: u32, geo: vec2<f32>, ring_r: f32) -> f32 {
    return palantir_edge_coverage(-palantir_template_distance(region, geo, ring_r));
}

// A box's straight `fill` under its border at a fragment of its template, premultiplied: the fill
// alone in the interior, which lies inside past the border and every edge's reach, and
// `palantir_composite` at the shape's distance elsewhere.
fn palantir_template_fill(
    region: u32,
    geo: vec2<f32>,
    ring_r: f32,
    fill: vec4<f32>,
    border_color: vec4<f32>,
    border_width: f32,
) -> vec4<f32> {
    if (region == PALANTIR_REGION_INTERIOR) {
        return vec4<f32>(fill.rgb * fill.a, fill.a);
    }
    let d = palantir_template_distance(region, geo, ring_r);
    return palantir_composite(d, fill, border_color, border_width);
}
