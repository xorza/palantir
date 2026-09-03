# Review: text system, shaped-buffer cache, glyph atlas

Scope: `src/text/`, `src/renderer/backend/text/`, `src/renderer/backend/raster_atlas/`,
`src/renderer/backend/raster_pass.rs`, `src/renderer/backend/icon/mod.rs`,
`src/icons/icon_rasterizer/`, and the callers in `layout`, `ui/frame_cycle.rs` and the composer.

When you address an item, delete it from this file. Delete a heading when its
items are gone. Test structure was ignored. Rewrite tests to fit the better
production shape.

## 6. Truncation reads the probe through a closure and re-probes per retry

Tried twice and reverted both times, on a measurement I do not trust —
re-run it on a quiet machine before deciding.

- [ ] `fitting_prefix` takes `glyph: impl Fn(usize) -> ClusterGlyph`, and
  `shape_truncated` calls `CacheEntry::probe(&self.cache, probe_key)` once per
  back-off round, both to keep the cache borrow disjoint from `logical_order`.
  Snapshot the probe's first layout run into a retained `Vec<ClusterGlyph>` once
  per miss; `fitting_prefix` then takes `&[ClusterGlyph]`, `CacheEntry::probe`
  goes, and a retry costs no hash. The copy is 24 bytes per glyph of one line.

  Two shapes were written: the snapshot plus the existing `order` index
  vector, and the snapshot sorted in place so `order` goes too. Against a
  `--save-baseline` pair they measured +1.0% and +2.2% on
  `text_shape/resize_drag_frame` — but a third run of the *reverted* code
  against the same baseline read −4.9%, so the machine had drifted about
  5% over the session and none of the three is a measurement of the
  change. What the attempt did establish is that the predicted win is not
  where the item says: the back-off runs a second round only when a
  reshaped prefix overruns, so the per-retry hash is almost never paid,
  and the copy is spent to save it. Measure the pair back to back, on a
  machine doing nothing else.
