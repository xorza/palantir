# Palantir review

Delete an item once it is addressed. Delete a heading once its items are gone.
Paths are relative to `palantir/src`. Line numbers are from the reviewed
revision and drift as files change.

## Paths and visibility that bypass the module tree

- [ ] `renderer/backend/raster_atlas/mod.rs:56-88` — `RasterAtlasConfig` is `pub(super)` while every field is `pub(crate)`.
- [ ] `renderer/frontend/encoder/collision_overlay.rs:40` — `emit` takes `&mut dyn PaintSink` while the `PaintSink` doc says the encoder is generic over the sink so the gates can inline. Debug-only, but it is the one sink call in the crate that goes through a vtable.

## Small redundancies

- [ ] `scene/cascade/engine.rs:170` — `can_update` checks `lc.arena_hashes.len() != n` beside per-row hash compares that already fail on a length mismatch.
- [ ] `renderer/backend/gpu_timings.rs:67-73` — `pipeline_stats_flags()` is a function that returns a constant bitflag union; a `const` says the same and the doc on `STATS_FIELD_COUNT` already points at it as a declaration.
