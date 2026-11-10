// What every quad pipeline reads: the instance, the forms and fill kinds it carries, and the
// vertex work the forms share (`quad_vertex`).

// The paths a quad's fragment takes (`quad_paint`, `fs_template`, the reference `fs`), pinned per
// pipeline (`QuadForm`): the driver drops the rest, so a solid rect's pipeline does not hold a
// gradient's or a triangle's registers.
override QUAD_FORM: u32;
const QUAD_FORM_BOX_SOLID: u32 = /*{QUAD_FORM_BOX_SOLID}*/;
const QUAD_FORM_BOX_GRADIENT: u32 = /*{QUAD_FORM_BOX_GRADIENT}*/;
const QUAD_FORM_TRIANGLE_SOLID: u32 = /*{QUAD_FORM_TRIANGLE_SOLID}*/;
const QUAD_FORM_BOX_OUTSIDE: u32 = /*{QUAD_FORM_BOX_OUTSIDE}*/;
const QUAD_FORM_TRIANGLE_GRADIENT: u32 = /*{QUAD_FORM_TRIANGLE_GRADIENT}*/;

// Whether the pipeline's form draws a triangle, three of the instance's points, of either paint.
fn form_is_triangle() -> bool {
    return QUAD_FORM == QUAD_FORM_TRIANGLE_SOLID || QUAD_FORM == QUAD_FORM_TRIANGLE_GRADIENT;
}

// A `core` no point is inside: the fragment always takes its full form.
const NO_SPAN: f32 = 1e30;

// `fill_kind`'s fields: the family tag in the `PALANTIR_FILL_TAG_MASK` bits and
// the spread mode `PALANTIR_SPREAD_SHIFT` up (`gradient_fill.wgsl`). Each flag bit is a constant below.

// Outside rect (`FillKind::OUTSIDE_BIT`) — inverted fill coverage. The fill
// paints *outside* the rounded boundary (the corner wedges, out to the
// quad edge), the border keeps its usual inner-edge annulus, and the
// interior stays transparent. Cheap stand-in for rounded-corner
// scissor clipping: draw content as plain rects, then paint this over
// it with the surrounding background as `fill`. Only meaningful for the
// rect path (solid and the gradients).
const FILL_FLAG_OUTSIDE: u32 = /*{FILL_FLAG_OUTSIDE}*/;

// Rounded triangle (`FillKind::TRIANGLE_BIT`): `points01`/`points23` hold its three points grown
// by its radius and `radius` that radius at each (`Quad::points`); the tag names its paint.
const FILL_FLAG_TRIANGLE: u32 = /*{FILL_FLAG_TRIANGLE}*/;

struct VertexOut {
    @builtin(position) clip:         vec4<f32>,
    @location(0)       local:        vec2<f32>,
    // Everything below is per-instance: identical at every vertex.
    // `flat` skips plane-equation setup + per-fragment interpolation and
    // avoids f32 drift across large quads.
    @location(1) @interpolate(flat) size:         vec2<f32>,
    @location(2) @interpolate(flat) fill:         vec4<f32>,
    @location(3) @interpolate(flat) radius:       vec4<f32>,
    @location(4) @interpolate(flat) border_color: vec4<f32>,
    @location(5) @interpolate(flat) border_width: f32,
    @location(6) @interpolate(flat) fill_kind:    u32,
    @location(7) @interpolate(flat) fill_lut_row: u32,
    @location(8) @interpolate(flat) fill_axis:    vec4<f32>,
    // Precomputed `1.0 / max(size, PALANTIR_ZERO_EPS)` so `quad_paint`'s gradient
    // path multiplies per-fragment instead of dividing (solid fills and the
    // shadow path don't read it).
    @location(9) @interpolate(flat) inv_size:     vec2<f32>,
    // `(lo.x, lo.y, hi.x, hi.y)` in `local`: the rounded rect's interior,
    // past every corner's arc and the border's inner edge by
    // `PALANTIR_AA_HALF_WIDTH`, where `composite` is the fill (`fs`). Empty where the
    // shape has no such interior.
    @location(10) @interpolate(flat) core: vec4<f32>,
    // Nonzero for a turned quad, whose quad edge is no pixel edge (`fs`).
    @location(11) @interpolate(flat) turned: u32,
};

// The instance attributes every quad pipeline reads.
struct QuadIn {
    @location(0) pos:          vec2<f32>,
    @location(1) size:         vec2<f32>,
    @location(2) fill_packed:  vec2<u32>,
    @location(3) radius_packed: vec2<u32>,
    @location(4) border_color_packed: vec2<u32>,
    @location(5) border_width: f32,
    @location(6) fill_kind:    u32,
    @location(7) fill_lut_row: u32,
    @location(8) fill_axis_packed: vec2<u32>,
    // A paint animation's turn about the quad's center, in radians; 0 for
    // none. The quad's own frame is unturned: `local`, the SDF and the
    // fill read it, and only the drawn corners turn.
    @location(10) rotation:    f32,
    // The closed shape's sharp points from `pos`, clockwise on screen, two to a lane
    // (`Quad::points`), and its inradius (`Quad::inradius`).
    @location(11) points01:    vec4<f32>,
    @location(12) points23:    vec4<f32>,
    @location(13) inradius:    f32,
};

// The pixel-center extent the quad shader shades for a quad: its rect
// grown to every pixel center within `PALANTIR_AA_HALF_WIDTH` of it, out to whole
// pixels, since its coverage reaches that far and the rasterizer only
// shades a pixel whose center is inside what it draws. A rect on pixel
// boundaries does not grow. A outside rect is a mask over content of its
// own extent and paints its fill outside the shape, so it stays at its
// rect. `Quad::shaded_rect` is the same extent on the CPU.
struct Bounds {
    lo: vec2<f32>,
    hi: vec2<f32>,
};

// Every pixel center within `PALANTIR_AA_HALF_WIDTH` of the rect; an outside rect's is a
// hard cut, drawn at its rect. A turned quad's frame is not the pixel grid,
// so it reaches `PALANTIR_AA_HALF_WIDTH` past each edge there instead, a distance the
// rotation keeps, and an outside rect's edge fades with it (`fs`).
fn shaded_bounds(pos: vec2<f32>, size: vec2<f32>, fill_kind: u32, rotation: f32) -> Bounds {
    var b = Bounds(pos, pos + size);
    if (rotation != 0.0) {
        b.lo -= vec2<f32>(PALANTIR_AA_HALF_WIDTH);
        b.hi += vec2<f32>(PALANTIR_AA_HALF_WIDTH);
    } else if ((fill_kind & FILL_FLAG_OUTSIDE) == 0u) {
        b.lo = floor(b.lo - vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
        b.hi = ceil(b.hi + vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
    }
    return b;
}

// Four f16 lanes packed two to a `u32`, in lane order.
fn unpack_f16x4(packed: vec2<u32>) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(packed.x), unpack2x16float(packed.y));
}

fn quad_vertex(vi: u32, quad: QuadIn) -> VertexOut {
    let size = quad.size;
    let fill_kind = quad.fill_kind;
    // `(tl, tr, br, bl)`, the `Corners` lane order, a radius at each of the shape's points; the
    // axis lanes as the top of this file lays them out per fill kind.
    let radius = unpack_f16x4(quad.radius_packed);
    let fill_axis = unpack_f16x4(quad.fill_axis_packed);
    let bounds = shaded_bounds(quad.pos, size, fill_kind, quad.rotation);
    let corner = select(bounds.lo, bounds.hi, CORNERS[vi] > vec2<f32>(0.5));

    var out: VertexOut;
    // Only the drawn corner turns: `local` stays in the quad's own frame.
    out.clip         = clip_from_px(turned(corner, quad.pos + size * 0.5, quad.rotation));
    out.local        = corner - quad.pos;
    out.size         = size;
    out.fill         = unpack_f16x4(quad.fill_packed);
    out.radius       = radius;
    out.border_color = unpack_f16x4(quad.border_color_packed);
    out.border_width = quad.border_width;
    out.fill_kind    = fill_kind;
    out.fill_lut_row = quad.fill_lut_row;
    out.fill_axis    = fill_axis;
    out.inv_size     = 1.0 / max(size, vec2<f32>(PALANTIR_ZERO_EPS));
    out.core         = vec4<f32>(NO_SPAN, NO_SPAN, -NO_SPAN, -NO_SPAN);
    out.turned       = u32(quad.rotation != 0.0);
    // Inside the quad by the largest radius, no corner's arc reaches; by the
    // border and `PALANTIR_AA_HALF_WIDTH`, its inner edge's coverage is 1. An outside
    // rect paints outside its shape, and a triangle's rect is not its shape.
    let outside = (fill_kind & FILL_FLAG_OUTSIDE) != 0u;
    if (!outside && (fill_kind & FILL_FLAG_TRIANGLE) == 0u) {
        let inset = max(max(max(radius.x, radius.y), max(radius.z, radius.w)), quad.border_width + PALANTIR_AA_HALF_WIDTH);
        out.core = vec4<f32>(vec2<f32>(inset), size - vec2<f32>(inset));
    }
    return out;
}
