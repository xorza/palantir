//! The rasterized-quad atlas and its two tenants: glyphs through the text
//! backend and icons through the icon backend.

pub(crate) mod icon_backend;
pub(crate) mod raster_atlas;
mod raster_pass;
pub(crate) mod raster_program;
pub(crate) mod text_backend;
