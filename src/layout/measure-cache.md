# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage. Same `(subtree_hash, available_q)` key is
reused by the [encode cache](../renderer/frontend/encoder/encode-cache.md),
which mirrors the SoA arena + snapshot shape and is eviction-locked to
this cache via the shared `removed` sweep.

Code lives in `cache/` (this directory's sibling).

## Mechanism

- **Subtree-hash rollup.** `Tree.subtree_hashes: Vec<NodeHash>` is
  populated alongside `hashes` in `compute_hashes` via a reverse
  pre-order walk. Pinned by `tree::tests::subtree_hash_*`.
- **Subtree-skip lookup.** `MeasureCache::try_lookup` fires at every
  non-collapsed node in `LayoutEngine::measure`. A hit blits the whole
  subtree's `desired` + `text_shapes` from the cache and skips
  recursion. `available_q` (integer-px-quantized) gates `Hug` / `Fill`
  variance.
- **Single-arena storage.** Three flat node-indexed arenas
  (`desired_arena`, `text_arena`, `available_arena`) shared across all
  snapshots, plus a per-`WidgetId` map of 24-byte
  `ArenaSnapshot { subtree_hash, nodes: Span, hugs: Span }`. Per-grid
  hug arrays for `LayoutMode::Grid` descendants live in a separate
  `hugs_arena`. In-place rewrite on same-len writes; append +
  mark-garbage on size changes; lazy compaction when
  `arena_len > live_entries × COMPACT_RATIO` (= 2) and `live_entries
  > COMPACT_FLOOR` (= 64).
- **Lifecycle hooks.** Eviction via `SeenIds.removed` →
  `MeasureCache::sweep_removed`, called from `Ui::end_frame`.
  `MeasureCache::clear` exposed via `bench_support::clear_measure_cache`
  (gated to `cfg(test)` + `bench-support` feature).

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

Future-work items (intrinsic-query cache, allocation audit,
real-workload validation, cold-cache mitigations, coarser
quantization) live in `docs/todo.md`.
