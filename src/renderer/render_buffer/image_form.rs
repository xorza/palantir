//! Which geometry an image draws through.

/// The geometry of an image draw, named by its shape as [`QuadForm`] names a quad's: its rect's
/// one quad, or the shape template of the box or the triangle that clips it, so a plain image
/// keeps its four vertices and its pixels. Each form is a pipeline of its own, so the vertex stage
/// takes a shape's corner count as a constant.
///
/// [`QuadForm`]: crate::renderer::render_buffer::quad_form::QuadForm
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageForm {
    Plain = 0,
    Box = 1,
    Triangle = 2,
}

impl ImageForm {
    /// Every form, in discriminant order.
    pub(crate) const ALL: [Self; 3] = [Self::Plain, Self::Box, Self::Triangle];
}
