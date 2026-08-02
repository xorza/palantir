# Open issues

- `CpuGradientAtlas::register_stops` scans the whole row table twice
  (probe sweep + `lru_victim`) on every miss once the table is full, and
  `grow()` ratchets capacity up to `max_texture_dimension_2d` (16384)
  without ever shrinking, so one gradient-heavy frame permanently
  multiplies every later miss.
- `CpuGradientAtlas` has no load-factor bound; linear-probe clustering
  degrades the hit path from ~1.1 probes to ~11.6 probes as the table
  approaches full.
- `GlyphAtlas::evict_one` scans the entire glyph cache per eviction and
  is called from a loop in `allocate`; `allocate` also prefers eviction
  over growth, so the mask atlas parks at 1024² and pays the scan for
  every new raster under churn.
- `CosmicMeasure`'s probation tier does not hold the population it was
  written for: `TextEncoder::encode_run` → `extract_glyphs` →
  `ensure_buffer` → `cache_hit` promotes a buffer to the 120-frame
  protected window on the same frame it was inserted, so resize/zoom
  drags retain ~`120 × visible runs` shaped buffers.
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
