# Palantir review

Delete an item once it is addressed. Delete a heading once its items are gone.
Paths are relative to `palantir/src`. Line numbers are from the reviewed
revision and drift as files change.

## One convention, two spellings

- [ ] Inline `glam::` paths in expressions instead of a `use`: `renderer/frontend/encoder/layer_ctx.rs:339`, `renderer/frontend/encoder/geometry.rs:30,43,52-58`, `renderer/render_buffer/image.rs:103,107`, `renderer/frontend/payload/draw_curve_payload.rs:26`, `draw_mesh_payload.rs:20`, `draw_polyline_payload.rs:27`, `draw_quad_payload.rs:33-36`, `frame_fixture/mod.rs:73,96,121`.
- [ ] `text/shaped_ref.rs` — import block broken by blank lines. `widgets/theme/mod.rs:64-65` — no blank line between the last `use` and the `Theme` doc comment.

## Paths and visibility that bypass the module tree

- [ ] Production `use` of `lib.rs` re-export paths: `host/core.rs:17` (`crate::Display`), `host/offscreen.rs:40` (`crate::FrameReport`), `host/window_driver/mod.rs:44` (`crate::{Display, FrameReport}`), `scene/tree/mod.rs:26` (`crate::ClipMode`). Every other production file names the defining module, so these four give each item a second canonical path.
- [ ] `renderer/backend/raster_atlas/raster_quad.rs:10` (and its tests) — imports `ContentType` through the parent module's private `use` (`raster_atlas::ContentType`) instead of `raster_atlas::content_type::ContentType`.
- [ ] `renderer/backend/raster_atlas/mod.rs:56-88` — `RasterAtlasConfig` is `pub(super)` while every field is `pub(crate)`.
- [ ] `renderer/frontend/encoder/collision_overlay.rs:40` — `emit` takes `&mut dyn PaintSink` while the `PaintSink` doc says the encoder is generic over the sink so the gates can inline. Debug-only, but it is the one sink call in the crate that goes through a vtable.

## Small redundancies

- [ ] `scene/cascade/engine.rs:170` — `can_update` checks `lc.arena_hashes.len() != n` beside per-row hash compares that already fail on a length mismatch.
- [ ] `renderer/backend/gpu_timings.rs:67-73` — `pipeline_stats_flags()` is a function that returns a constant bitflag union; a `const` says the same and the doc on `STATS_FIELD_COUNT` already points at it as a declaration.
