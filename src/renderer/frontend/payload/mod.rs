//! Lowered paint payloads: the values the encoder hands a [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink), one per paint operation.
//!
//! Plain value types, consumed inline and never serialized, so layout is the compiler's and fields are ordinary enums, not `u8` newtypes with `#[repr(C)]` and padding a `bytemuck::Pod` arena would need.

pub(crate) mod brush_source;
pub(crate) mod draw_curve_payload;
pub(crate) mod draw_icon_payload;
pub(crate) mod draw_image_payload;
pub(crate) mod draw_mesh_payload;
pub(crate) mod draw_polyline_payload;
pub(crate) mod draw_quad_payload;
pub(crate) mod draw_text_payload;
pub(crate) mod gpu_fill;
pub(crate) mod push_clip_payload;
pub(crate) mod resolved_gradient;
pub(crate) mod stroke_bounds;
