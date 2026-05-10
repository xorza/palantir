# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage. The only cross-frame cache in the render
path — encode and compose were both removed after benches showed they
contributed < 1% (see `docs/encode-cache-investigation.md`,
`docs/compose-cache-under-scroll.md`).

Code lives in `cache/` (this directory's sibling).

## Mechanism

- **Subtree-hash rollup.** `Tree.subtree_hashes: Vec<NodeHash>` is
  populated alongside `hashes` in `compute_hashes` via a reverse
  pre-order walk. Pinned by `forest::tree::tests::subtree_hash_*`.
- **Subtree-skip lookup.** `MeasureCache::try_lookup` fires at every
  non-collapsed node in `LayoutEngine::measure`. A hit blits the whole
  subtree's `desired` + `text_shapes` from the cache and skips
  recursion. `available_q` (integer-px-quantized) gates `Hug` / `Fill`
  variance.
- **Single-arena storage.** Three flat node-indexed arenas
  (`desired`, `text_spans`, `scroll_content`) shared across all
  snapshots, plus a per-`WidgetId` map of 40-byte
  `ArenaSnapshot { subtree_hash, available_q, nodes: Span, hugs: Span,
  text_shapes: Span }`. The dimensional cache key (`available_q`) is
  inline on the snapshot — the validity check on `try_lookup` doesn't
  hit a parallel arena. Per-grid hug arrays for `LayoutMode::Grid`
  descendants live in a separate `hugs` arena; flat shaped-text runs
  live in `text_shapes_arena`. Liveness bookkeeping rides on the
  shared [`LiveArena`] primitive (`src/common/cache_arena.rs`); the
  three node-indexed arenas share `nodes.live`; `hugs` and
  `text_shapes_arena` track their own. In-place rewrite on same-len
  writes; append + mark-garbage on size changes; lazy compaction when
  `arena.len() > live × COMPACT_RATIO` (= 2) and `live > COMPACT_FLOOR`
  (= 64).
- **Lifecycle hooks.** Eviction via `SeenIds.removed` →
  `MeasureCache::sweep_removed`, called from `Ui::end_frame`.
  `MeasureCache::clear` exposed via `internals::clear_measure_cache`
  (gated to `cfg(test)` + `internals` feature).

## Tests

`src/layout/cache/tests.rs` and `src/layout/cache/integration_tests.rs`:
hit/miss paths, eviction, subtree-snapshot coverage, in-place rewrite
preserves arena position, compaction invariant, post-compaction hit
validity, plus the rect-stability contract via
`subtree_skip_preserves_descendant_rects`.

## Bench

`benches/measure_cache.rs` covers `flat` (1000 leaves, depth 1) and
`nested` (3200 nodes, depth 4) with `cached` vs `forced_miss`
variants.

| workload | cached | forced_miss |
| --- | --- | --- |
| flat   | 85.0 µs  | 112.7 µs |
| nested | 375.5 µs | 496.2 µs |

Steady-state cache hits dominate by ~25 % on the nested workload.
Per-snapshot memory footprint on that workload is ~77 KB across the
arenas and the `FxHashMap` index.

Future-work items (intrinsic-query cache, real-workload validation,
cold-cache mitigations, coarser quantization) live in
`docs/roadmap/caches.md`.
