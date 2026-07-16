# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage. The only cross-frame cache in the render
path — encode and compose were both removed after benches showed they
contributed < 1%.

Code lives in `cache/` (this directory's sibling).

## Mechanism

- **Subtree-hash rollup.** `Tree.rollups.subtree: Vec<ContentHash>` is
  populated alongside `rollups.node` in `Tree::post_record` via a fused
  reverse-pre-order walk. Pinned by `forest::tree::tests::subtree_hash_*`.
- **Subtree-skip lookup.** `MeasureCache::try_lookup` fires at every
  non-collapsed node in `LayoutEngine::measure`. A hit blits the whole
  subtree's `desired` + `text_shapes` from the cache and skips
  recursion. `available_q` (integer-px-quantized) gates `Hug` / `Fill`
  variance.
- **Single-arena storage.** Two flat node-indexed arenas
  (`desired`, `text_spans`) shared across all
  snapshots, plus a per-`WidgetId` map of
  `ArenaSnapshot { subtree_hash, available_q, root_intrinsics, nodes: Span,
  hugs: Span, text_shapes: Span }`. The dimensional cache key
  (`available_q`) is inline on the snapshot — the validity check on
  `try_lookup` doesn't hit a parallel arena. `root_intrinsics`
  (`[f32; SLOT_COUNT]`, X/Y × Min/Max-content) is the subtree root's
  cached intrinsic; `MeasureCache::lookup_root_intrinsic` serves it to
  `LayoutEngine::intrinsic` keyed on `subtree_hash` **alone** (intrinsics
  are computed at `available = ∞`, so they're valid across `available_q`
  buckets). This stops an ancestor's `intrinsic_min` query — which runs
  *before* its children are measured — from cold-recursing through
  unchanged sibling subtrees on a localized change or a resize. Per-grid hug arrays for `LayoutMode::Grid`
  descendants live in a separate `hugs` arena; flat shaped-text runs
  live in `text_shapes_arena`. Liveness bookkeeping rides on the
  shared [`LiveArena`] primitive (`src/common/live_arena.rs`); the
  two node-indexed arenas share `nodes.live`; `hugs` and
  `text_shapes_arena` track their own. In-place rewrite on same-len
  writes; append + mark-garbage on size changes; lazy compaction when
  `arena.len() > live × COMPACT_RATIO` (= 2) and `live > COMPACT_FLOOR`
  (= 64).
- **Lifecycle hooks.** Eviction via `SeenIds.removed` →
  `MeasureCache::sweep_removed`, called from `Ui::post_record`.
  `MeasureCache::clear` exposed via `internals::clear_measure_cache`
  (gated to `cfg(test)` + `internals` feature).

## Tests

`src/layout/cache/tests.rs` and `src/layout/cache/integration_tests.rs`:
hit/miss paths, eviction, subtree-snapshot coverage, in-place rewrite
preserves arena position, compaction invariant, post-compaction hit
validity, plus the rect-stability contract via
`subtree_skip_preserves_descendant_rects`.

## Bench

`src/bench/layout/cache.rs` compares `cached` and `forced_miss` arms for both a
representative measure workload and a heavier clipped/text-shaped tree. It also
pins two adversarial shapes: a 194-node unary chain retaining 18,914 overlapping
node rows (O(N²)), and a 1,098-node balanced tree retaining 5,403 rows
(O(N log N)). Both add resize arms; the broad tree also toggles one paint-only
leaf so localized sibling-subtree reuse is measured separately.

A bounded selective-root prototype retained every subtree up to 32 nodes but
only roots and branch points above that. On a pinned CPU it reduced the deep
fixture to 721 retained rows and improved forced-miss time from 56.6 to 44.8 µs
and resize time from 52.3 to 48.2 µs, but cached time stayed flat (23.9 versus
24.1 µs). The broad fixture kept the same hit coverage and storage yet was
consistently 0.1–2.1% slower across cached, forced-miss, resize, and localized
arms. A flat-root lower bound discarded all 21 localized sibling hits and was
materially slower on that arm. Neither candidate cleared the requirement to
improve miss behavior and steady-state reuse together, so production keeps the
overlapping per-branch snapshots. Criterion output remains the source of truth
for current timings.

The cross-frame intrinsic-query cache landed as `root_intrinsics` on
the snapshot (above) — it reuses the subtree root's intrinsic, which
covers the dominant `children_max_intrinsic` re-walk; a full
per-descendant intrinsic snapshot is still open. Remaining future-work
items (real-workload validation, cold-cache mitigations, coarser
quantization) remain profile-driven.
