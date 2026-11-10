// What every quad pipeline reads: the instance, the forms and fill kinds it carries, and the
// vertex work the forms share (`quad_vertex`).

// The fill form a pipeline draws (`FillForm`), pinned as two constants: the driver drops the
// paths the others take, so a solid box's pipeline does not hold a gradient's or a triangle's
// registers.
override SHAPE_FORM: u32;
override FILL_BRUSH: u32;
const SHAPE_FORM_TRIANGLE: u32 = /*{SHAPE_FORM_TRIANGLE}*/;
const FILL_BRUSH_GRADIENT: u32 = /*{FILL_BRUSH_GRADIENT}*/;

// Whether the pipeline's form draws a triangle, three of the instance's points.
fn form_is_triangle() -> bool {
    return SHAPE_FORM == SHAPE_FORM_TRIANGLE;
}

// A `core` no point is inside: the fragment always takes its full form.
const NO_SPAN: f32 = 1e30;

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
    // `PALANTIR_AA_HALF_WIDTH`, where `palantir_composite` is the fill (`fs`). Empty where the
    // shape has no such interior.
    @location(10) @interpolate(flat) core: vec4<f32>,
    // The rounded clips that cut the quad (`ClipChain`).
    @location(11) @interpolate(flat) chain: u32,
};

// The instance attributes every quad pipeline reads past its shape (`PalantirShapeIn`).
struct QuadIn {
    @location(6) fill_packed:  vec2<u32>,
    @location(7) border_color_packed: vec2<u32>,
    @location(8) border_width: f32,
    @location(9) fill_kind:    u32,
    @location(10) fill_lut_row: u32,
    @location(11) fill_axis_packed: vec2<u32>,
};

// The pixel-center extent the quad shader shades for a quad: its rect
// grown to every pixel center within `PALANTIR_AA_HALF_WIDTH` of it, out to whole
// pixels, since its coverage reaches that far and the rasterizer only
// shades a pixel whose center is inside what it draws. A rect on pixel
// boundaries does not grow. `ShapeHeader::shaded_rect` is the same extent on the CPU.
struct Bounds {
    lo: vec2<f32>,
    hi: vec2<f32>,
};

// Every pixel center within `PALANTIR_AA_HALF_WIDTH` of the rect. A turned quad's frame is not the
// pixel grid, so it reaches `PALANTIR_AA_HALF_WIDTH` past each edge there instead, a distance the
// rotation keeps.
fn shaded_bounds(pos: vec2<f32>, size: vec2<f32>, rotation: f32) -> Bounds {
    var b = Bounds(pos, pos + size);
    if (rotation != 0.0) {
        b.lo -= vec2<f32>(PALANTIR_AA_HALF_WIDTH);
        b.hi += vec2<f32>(PALANTIR_AA_HALF_WIDTH);
    } else {
        b.lo = floor(b.lo - vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
        b.hi = ceil(b.hi + vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
    }
    return b;
}

// Four f16 lanes packed two to a `u32`, in lane order.
fn unpack_f16x4(packed: vec2<u32>) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(packed.x), unpack2x16float(packed.y));
}

// The vertex work every quad form shares. The shape's turn about its center leaves `local`, the
// SDF and the fill in its own frame: only the drawn corners turn.
fn quad_vertex(vi: u32, shape: PalantirShapeIn, quad: QuadIn) -> VertexOut {
    let pos = shape.rect.xy;
    let size = shape.rect.zw;
    let rotation = shape.inradius_rotation.y;
    let fill_kind = quad.fill_kind;
    // `(tl, tr, br, bl)`, the `Corners` lane order, a radius at each of the shape's points; the
    // axis lanes as the top of this file lays them out per fill kind.
    let radius = shape.radius;
    let fill_axis = unpack_f16x4(quad.fill_axis_packed);
    let bounds = shaded_bounds(pos, size, rotation);
    let corner = select(bounds.lo, bounds.hi, CORNERS[vi] > vec2<f32>(0.5));

    var out: VertexOut;
    // Only the drawn corner turns: `local` stays in the quad's own frame.
    out.clip         = clip_from_px(turned(corner, pos + size * 0.5, rotation));
    out.local        = corner - pos;
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
    out.chain        = shape.chain;
    // Inside the quad by the largest radius, no corner's arc reaches; by the
    // border and `PALANTIR_AA_HALF_WIDTH`, its inner edge's coverage is 1. A triangle's rect is
    // not its shape.
    if (!form_is_triangle()) {
        let inset = max(max(max(radius.x, radius.y), max(radius.z, radius.w)), quad.border_width + PALANTIR_AA_HALF_WIDTH);
        out.core = vec4<f32>(vec2<f32>(inset), size - vec2<f32>(inset));
    }
    return out;
}
