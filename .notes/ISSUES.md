# Open issues

- `GlyphAtlas::evict_one` scans the entire glyph cache per eviction and
  is called from a loop in `allocate`, so a side that has reached
  `EAGER_GROWTH_BYTE_BUDGET` and filled up pays O(live glyphs) for every
  further insert. No bench in the tree reaches that state.
- `MeasureCache::refresh_snapshots` rebuilds the whole `WidgetId` map
  whenever the descriptor id sequence changes at all — every frame
  during virtualized-list scroll or any widget add/remove.
- `emit_inverted_overlaps` enumerates every pair of matched paint rows,
  so a parent with many children (graph canvas) pays O(children²) per
  frame whenever a child's paint order flips.
- `TextSystem::end_frame` runs `retain` every frame over a table whose
  capacity never shrinks, so a map that peaked large keeps paying for
  its peak.
- `EncodedCache::sweep`'s arena compaction is a single memcpy of every
  live glyph on the frame its threshold trips, so the frame that follows
  a zoom or resize drag pays for the whole population the drag left
  behind while every other frame pays nothing.
- Nothing pins the property `WidgetIdMap`'s identity `IdHasher` depends
  on: that `Hasher::new()`'s output has usable entropy in its low bits
  (true only because rustc-hash 2.x's `finish()` rotates for hashbrown).
  Documented on `IdHasher`; untestable without pinning rustc-hash's
  internal mix.
- `WidgetId` low bits cluster mildly under sequential derivation, which
  is the virtualized-list pattern: 4096 ids into 4096 buckets occupy
  1999 of them via `parent.with(i)` and 1192 via
  `WidgetId::from_hash(i).with("label")`, against ~2589 for a uniform
  hash.
- `EncodedKey`'s `scale_q` changes per `TEXT_SCALE_STEP` rung under zoom
  and its `max_w_q` per committed width under a resize drag, so either
  gesture mints a fresh encoded entry per run per frame that is asked
  for exactly once. The encoded cache has no probation tier and no
  supersede signal, so each lives the full `ENCODED_CACHE_KEEP_FRAMES`:
  eight visible runs settle at 968 resident rows and ~11.6k glyph
  templates, held for two seconds after the gesture ends
  (`a_gesture_frame_retains_a_full_keep_window_of_single_use_rows`).
  A promotion-on-lookup signal does not reach it — `RenderKind::Partial`
  culls the encoder walk to the damage region, so a static run is not
  consulted on frames that do not damage it. (`bins` is bounded at 16
  and cycles, so sub-pixel motion alone does not churn.)
- `TextReuseEntry::wrap` holds one bounded resolve, so a node measured at
  two widths in one frame — grid's grow-driven second pass — misses the
  row and supersedes the other width's key on every measure, leaving both
  buffers permanently in the probation tier.
- `CosmicMeasure::measure_truncated`'s `fits_whole` branch shapes and
  caches a second buffer identical to the unbounded probe under the
  truncated key.
- The `RECYCLE_POOL_CAP` recycle pool holds up to 128 buffers for the
  shaper's life with no shrink, and is LIFO, so its tail never rotates
  out after a workload that filled it.
- Above `EAGER_GROWTH_BYTE_BUDGET`, `GlyphAtlas::allocate` tries eviction
  before growth, and `evict_one` only fails when every resident glyph was
  touched this frame — so a side grows past the budget only when a single
  frame's working set does not fit, and `MAX_ATLAS_BYTE_BUDGET` is
  effectively unreachable for the mask side.
- A retained `InternedStr` keeps its whole frame's text arena alive, not
  just the span it addresses; and handles held from several frames at
  once leave both `TextStore`'s active and spare arenas externally owned,
  so `clear` allocates a fresh arena every frame.
- Every fixture in `tests/alloc` is a still frame, so the churn workloads
  the caches exist for — resize drag, zoom, virtualized scroll, widget
  add/remove — are outside the per-frame allocation guard.
