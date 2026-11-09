// A quad's paint: what covers its shape at a fragment, apart from where the shape lies. The
// geometry, the template or the one-quad reference form, hands it the fragment's point of the
// quad's own 0..1 frame, and composites the coverage and the border around what it returns.

// The paint the pipeline's form applies at `local01`: the fill as it is, or its gradient there.
// `QUAD_FORM` is a pipeline constant, so a solid form compiles no gradient's registers.
fn quad_paint(
    fill: vec4<f32>,
    local01: vec2<f32>,
    fill_kind: u32,
    fill_axis: vec4<f32>,
    fill_lut_row: u32,
) -> vec4<f32> {
    let gradient = QUAD_FORM == QUAD_FORM_BOX_GRADIENT
        || QUAD_FORM == QUAD_FORM_TRIANGLE_GRADIENT
        || (QUAD_FORM == QUAD_FORM_BOX_OUTSIDE && (fill_kind & PALANTIR_FILL_TAG_MASK) != PALANTIR_FILL_SOLID);
    if (gradient) {
        return palantir_gradient_fill(local01, fill_kind, fill_axis, fill_lut_row, fill);
    }
    return fill;
}
