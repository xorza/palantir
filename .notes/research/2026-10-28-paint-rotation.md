# Research for turning every shape kind (2026-10-28)

## Practice elsewhere

- WebRender: a primitive under a transform that is not axis-aligned takes the
  antialiased path; its quad "is extruded by 2 local-space pixels in each
  direction, which gives enough space around the edge to apply distance
  anti-aliasing", computed in the primitive's local space.
  <https://kdab.searchfox.org/firefox-main/source/gfx/wr/webrender/res/transform.glsl>,
  <https://kdab.searchfox.org/firefox-main/source/gfx/wr/webrender/res/brush.glsl>,
  <https://bugzilla.mozilla.org/show_bug.cgi?id=1759526>
- egui: `TextShape::angle` rotates text "by this many radians clockwise. The
  pivot is `pos`"; text is a mesh, so the CPU turns its vertices.
  <https://docs.rs/epaint/latest/epaint/struct.TextShape.html>

## In the code

- `PaintChannel::turn` is a turn about the owner box's center
  (`Rect::spin_pivot`); `Forest::add_shape_animated` panics through
  `turn_unimplemented` unless `ShapeRecord::rotates` (polyline, curve).
- The encoder samples a `PaintMod { alpha, rotation }` per shape
  (`encoder/plane_ctx.rs`); only the stroke arms read `rotation`, through
  `StrokeBounds::new`, whose `Spun` arm the composer applies by turning
  points on the CPU (`composer/session.rs` `curve`, `polyline`).
- Cascade damage and composer culling bound a turning stroke by
  `Rect::spun_cover`, the square it sweeps about the pivot, with no angle
  (`cascade/paint_rect.rs`, `PaintAnimations::rotates`).
- Instances: `Quad` 60 B (axis-aligned rect, SDF in the quad shader, plus the
  shadow grid and cutout tables); `ImageInstance` (rect, uv, tint, flags; no
  edge AA); `RasterQuad` 28 B with an `i32` position, shared by text and
  icons, and an encoded-run cache of origin-relative templates;
  `MeshInstance` (`translate`, `scale`, `tint`, applied in the vertex shader).
- A curve of no length lowers to a `Quad` rect, so its dot does not turn
  (`shape/curve.rs`).
- The raster shader reads by `textureLoad`, with a hand bilinear path
  (`FLAG_RESAMPLE`) for quads drawn at a size other than their raster's.
