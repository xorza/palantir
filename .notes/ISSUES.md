# Issues
Same defect as the file-organisation audit, in files the audit did not list:

- `src/animation/mod.rs` holds `AnimSlot`, `AnimRow`, `AnimMapTyped`,
  `TickResult`, `MotionRow` and `AnimMap` — six independent types, none
  named for the module.
- `src/layout/grid/mod.rs` holds `GridScratch`, `GridContext`,
  `GridDepthStack`, `GridTrackStore`, `GridTrackSlot` and
  `RasterAtlasConfig`-style config alongside the `measure` / `arrange` /
  `intrinsic` driver entry points.
- `src/scene/tree/mod.rs` holds `Tree` plus `ChromeInput`, and its
  sibling `extras.rs` / `record.rs` name topics rather than their types.
- `src/renderer/backend/raster_atlas/mod.rs` holds `RasterAtlas`,
  `RasterAtlasConfig`, `AtlasLabels`, `BoundSides` and `Side` bindings.
- `src/text/cosmic/truncate.rs` holds `EllipsisMemo`, `ClusterGlyph` and
  three free functions after the `CosmicMeasure` impl moved out.
