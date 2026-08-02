# Open issues

- `GlyphAtlas::evict_one` scans the entire glyph cache per eviction and
  is called from a loop in `allocate`; `allocate` also prefers eviction
  over growth, so the mask atlas parks at 1024² and pays the scan for
  every new raster under churn.
- `MeasureCache::refresh_snapshots` rebuilds the whole `WidgetId` map
  whenever the descriptor id sequence changes at all — every frame
  during virtualized-list scroll or any widget add/remove.
- `emit_inverted_overlaps` enumerates every pair of matched paint rows,
  so a parent with many children (graph canvas) pays O(children²) per
  frame whenever a child's paint order flips.
- `TextSystem::end_frame` and `EncodedCache::sweep` run `retain` every
  frame over a table whose capacity never shrinks, so a map that peaked
  large keeps paying for its peak.
- Nothing pins the property `WidgetIdMap`'s identity `IdHasher` depends
  on: that `Hasher::new()`'s output has usable entropy in its low bits
  (true only because rustc-hash 2.x's `finish()` rotates for hashbrown).
