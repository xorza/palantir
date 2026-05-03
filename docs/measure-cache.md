# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage (subtree-hash equality is the same key the
encode cache wants).

**Status:** shipped. Subtree-skip at every non-collapsed node, backed
by a single SoA arena per attribute. See `src/layout/cache/mod.rs`.

## What's done

- [x] **Subtree-hash rollup.** `Tree.subtree_hashes: Vec<NodeHash>` is
  populated alongside `hashes` in `compute_hashes` via a reverse
  pre-order walk. Pinned by `tree::tests::subtree_hash_*`.
- [x] **Subtree-skip cache.** `MeasureCache::try_lookup` fires at
  every non-collapsed node in `LayoutEngine::measure`. Hit blits the
  whole subtree's `desired` + `text_shapes` from the cache and skips
  recursion. `available_q` (integer-px-quantized) gates `Hug`/`Fill`
  variance.
- [x] **Single-arena storage.** Two flat arenas (`desired_arena`,
  `text_arena`) shared across all snapshots, plus a per-`WidgetId`
  map of 24-byte `ArenaSnapshot { subtree_hash, available_q, start,
  len }`. In-place rewrite on same-len writes; append + mark-garbage
  on size changes; lazy compaction when `arena_len > live_entries × 2`.
- [x] **Lifecycle hooks.** Eviction via `SeenIds.removed` →
  `MeasureCache::sweep_removed`. `__clear_cache` for benches.
- [x] **Tests.** 13 in `cache/tests.rs`: hit/miss paths, eviction,
  subtree-snapshot coverage, in-place rewrite preserves arena
  position, compaction invariant, post-compaction hit validity, plus
  the rect-stability contract via `subtree_skip_preserves_descendant_rects`.
- [x] **Bench harness.** `benches/measure_cache.rs` covers `flat`
  (1000 leaves, depth 1) and `nested` (3200 nodes, depth 4) with
  `cached` vs `forced_miss` variants.

## Numbers

| workload | cached | forced_miss |
| --- | --- | --- |
| flat   | **85.0 µs** | **112.7 µs** |
| nested | **375.5 µs** | **496.2 µs** |

Steady-state cache hits dominate by ~25% on the nested workload;
cold-cache `forced_miss` improved ~27% over the previous per-Vec
design thanks to in-place arena writes (no per-snapshot allocation).
Per-snapshot memory footprint dropped from ~358 KB to ~77 KB on the
nested workload (24-byte `ArenaSnapshot` vs ~80 bytes inline + two
Vec headers + scattered heap data).

## Not done — deferred

- [ ] **Cross-frame intrinsic-query cache.** `LayoutEngine::intrinsic`
  is intra-frame only. A second column keyed on `subtree_hash + axis +
  req` would compose cleanly. Skip until a workload proves it matters.
- [ ] **Per-frame allocation audit.** CLAUDE.md flags this as a
  project-wide goal. The cache is alloc-amortized after warmup but
  there's no harness asserting it. Cross-cutting; not cache-local.
- [ ] **Real-workload validation.** Bench numbers are synthetic. The
  showcase doesn't push against the 400 µs ceiling, so the cache's
  user-visible win is unverified.
- [ ] **Cold-cache mitigations.** If a workload ever shows resize-
  frame jank, candidates: skip snapshot writes for collapsed
  subtrees, gate writes by subtree-size threshold, amortize compact
  across frames. Speculative.
- [ ] **Coarser `available` quantization.** Currently 1 logical px.
  If jittery `Fill` children show cache misses on sub-pixel parent
  drift, bump granularity. Wait for evidence.
