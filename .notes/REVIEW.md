# Palantir review

Delete an item once it is addressed. Delete a heading once its items are gone.
Paths are relative to `palantir/src`. Line numbers are from the reviewed
revision and drift as files change.

## The same computation spelled twice

- [ ] `scene/record_store/mod.rs:42` — `RecordStore` is a pure pass-through newtype over `RecordPayloads`; every method delegates. Two types for one thing.
- [ ] `scene/shapes/record/mod.rs:68,130` — `ShapeRecord::Polyline.content_hash` and `ShapeRecord::Mesh.content_hash` are written by `shapes/lower.rs` and read only by `compute_record_hash` in `shapes/hash.rs:102,143`. The parallel `Shapes::hashes` column already carries the folded result, so each record stores 8 bytes that nothing reads after the hash is taken. Check whether the record hash is taken once at `Shapes::add`; if so the field can go.
- [ ] `renderer/backend/quad.wgsl:330,361` — both shadow arms write `vec4<f32>(in.fill.rgb * a, a)` by hand; the prelude's `premultiply` exists so that no fragment entry spells it.
- [ ] `widgets/scroll/bars.rs:100-106` — `BarAxis::record` has two arms that both call `ui.widget(track).record(..)`, differing only in `Some(&chrome)` vs `None`. An `Option<Background>` and one call says the same.
- [ ] `widgets/overlay_scope.rs:131` — `OverlayScope::record` takes a body returning `()`, so `widgets/popup/mod.rs:211-223` and `widgets/modal/mod.rs:111-121` both smuggle the body result out through `let mut inner = None; … inner.expect("the body records unconditionally")`. A body returning `R` removes both `Option`s and both `expect`s.
- [ ] `renderer/backend/debug_marker.rs` — four functions with two `cfg` bodies each (eight items) for a feature gate whose real content is two lines.
- [ ] `renderer/gradient_atlas/mod.rs:240-253` — `CpuGradientAtlas::new` sizes `slots`, `baked`, and `mru` itself although `resize_rows` (line 377) is documented as "the one place any of them is resized".
- [ ] `host/winit/gpu/mod.rs:72-144` — `GpuInit::new` and `SurfaceManager::make_surface` both do create-surface, `inner_size`, and `build_window_surface`. The probe-surface reuse explains the first, but the second half of `new` is `make_surface` without the `self`.

## One convention, two spellings

- [ ] Theme handle clone: `ui.theme().clone()` in `widgets/modal/mod.rs:82`, `context_menu/menu_separator.rs:49`, `context_menu/mod.rs:147`, `tooltip/mod.rs:137`, `drag_value/mod.rs:301`, `combo_box/mod.rs:115`; `Rc::clone(ui.theme())` in `widgets/grid.rs:86`, `popup/mod.rs:206`, `panel/mod.rs:40`.
- [ ] `widgets/theme/context_menu/menu_item.rs:108-114` — `ThemeSlot::defaults` rebuilds `SlotDefaults` field by field; `ButtonTheme`, `TextEditTheme`, and `ToggleTheme` return `self.defaults`.
- [ ] `renderer/backend/mod.rs:420-444,543-544` — `submit` carries `dim_undamaged` into `upload_frame` through `UploadPlan`, while `upload_frame` re-derives `is_partial` from the submission. One fact travels, the other is recomputed.
- [ ] `renderer/backend/text/mod.rs:148-170` — `TextBackend::frame()` and `TextBackend::end_frame()` both read `shaper.frame()`; `WgpuBackend::submit` (`backend/mod.rs:512-514`) calls `frame()` only to hand the value to `icon.end_frame`. Returning the frame from `end_frame` removes the accessor.
- [ ] `ui/state.rs:36-48` — `try_get` and `try_get_mut` repeat the map→index lookup on `StateMap`, while `get_or_insert_with` lives on `Store`. One of the two owns the lookup.
- [ ] `layout/types/sizing.rs:145,292,305` — `From<T: Num>` on both `Sizes` and `Sizing`, plus `From<Size> for Sizes` and `From<(W, H)>`: four conversions into `Sizes` where a call site needs one.
- [ ] `scene/tree/mod.rs:698-707` — submodule declarations at the bottom of the file; every other `mod.rs` in the crate declares them at the top.
- [ ] `renderer/render_buffer/mod.rs:27-38`, `renderer/gradient_atlas/mod.rs:40-55`, `renderer/frontend/composer/mod.rs:7-32` — `use` lines split around the `mod` declarations, some separated by blank lines. The rest of the crate keeps one import block above the mods.
- [ ] `renderer/frontend/composer/session.rs:11-16` — nested-brace imports (`crate::primitives::{num::{F32Ext, Vec2Ext}, rect::Rect, …}`) where the rest of the crate writes one path per line.
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
- [ ] `widgets/overlay_scope.rs:51` — `OverlayEdges` is not `#[must_use]`. `Tooltip::show` discards it on purpose (`Backdrop::None`), but a popup that dropped it would compile silently too.
