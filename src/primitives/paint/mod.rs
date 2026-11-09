//! What a shape is painted with: color, brushes, strokes, shadows, chrome and images.

pub(crate) mod antialias;
pub(crate) mod border;
pub(crate) mod brush;
pub(crate) mod chrome;
pub(crate) mod color;
pub(crate) mod content_type;
pub(crate) mod image;
pub(crate) mod line_style;
pub(crate) mod lut_row;
pub(crate) mod material;
#[expect(
    clippy::module_inception,
    reason = "a file is named after its one major struct, `Paint` here, as every file of the crate is"
)]
pub(crate) mod paint;
pub(crate) mod raster_image;
pub(crate) mod shadow;
pub(crate) mod stroke;
