# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage (subtree-hash equality is the same key the
encode cache wants).

Phase 2 (full subtree skip) is what's currently shipped. The cache
checks at every non-collapsed node and, on hit, blits the whole
subtree's `desired` and `text_shapes` arrays from the snapshot —
recursion is skipped entirely. Trade-off: cold-cache frames pay an
N × avg_depth snapshot-write cost; see results below.

## Plan

1. **Subtree hash rollup.** Add `Tree.subtree_hashes: Vec<NodeHash>`,
   populated in `compute_hashes` after the per-node pass. Reverse
   pre-order walk: `subtree_hashes[i] = fold(node_hash[i], subtree_hashes[c]
   for each direct child c)`. Test: changing a leaf must invalidate
   every ancestor's subtree hash; reordering siblings must change the
   parent's subtree hash; identical trees must hash identically.

2. **`MeasureCache` (leaf-only skip).**
   `prev: FxHashMap<WidgetId, MeasureSnapshot { subtree_hash, available_q, desired }>`.
   At measure entry for `LayoutMode::Leaf`, look up by `widget_ids[i]`
   and write `desired[i] = snapshot.desired` on hit. Quantize
   `available` to integer logical pixels. Eviction piggy-backs on
   `SeenIds.removed()` (same lifecycle as `Damage.prev` and the text
   cache).

3. **Bench.** `benches/measure_cache.rs` runs two workloads:

   - `flat`: 1 000 leaves under one VStack root, depth 1.
   - `nested`: 3 200 nodes — 100 groups × (header + 10 rows × 3 leaves
     + footer), depth 4.

   Phase 1 (leaf-only short-circuit, prior to Phase 2):

   | workload | cached | forced_miss | save | % |
   | --- | --- | --- | --- | --- |
   | flat | 93.5 µs | 104.0 µs | 10.5 µs | 10.1% |
   | nested | 414.1 µs | 455.5 µs | 41.4 µs | 9.1% |

   Phase 2 (subtree-skip at every non-collapsed node, what's shipped):

   | workload | cached | forced_miss | cached Δ vs P1 |
   | --- | --- | --- | --- |
   | flat | **84.3 µs** | 153.2 µs | -10% (-9.2 µs) |
   | nested | **369.8 µs** | 677.1 µs | -11% (-44 µs) |

   Steady-state hit path is ~10% faster than Phase 1, on top of
   Phase 1's ~10% over no cache. But forced_miss got ~50% slower
   because every node now writes a subtree-sized snapshot on miss
   (`extend_from_slice` of `desired` + `text_shapes`, lengths summing
   to O(N · depth) per cold frame). Real frames hit a mix; the
   regression only matters when the entire visible tree invalidates
   in one frame (resize, theme switch, first frame after navigation).

4. **Phase 2: full subtree skip — shipped.** Each `WidgetId` now
   owns a `SubtreeSnapshot { subtree_hash, available_q,
   desired: Vec<Size>, text_shapes: Vec<Option<ShapedText>> }`. On
   hit at any non-collapsed node, the cache blits both arrays into
   `LayoutEngine.desired[i..subtree_end[i]]` and
   `LayoutResult.text_shapes[i..subtree_end[i]]` and returns without
   recursing. On miss, the body runs as before, then
   `MeasureCache::write_subtree` overwrites the snapshot from the
   freshly-populated slices (capacity retained via
   `clear() + extend_from_slice`).

## Open questions

- Quantization granularity for `available` (1 logical px is the
  starting position; finer if Fill children show jitter-driven misses).
- Whether to extend the cache to `intrinsic()` queries cross-frame
  (keyed on `subtree_hash + axis + req`). Small extension on top of
  Phase 2.
- Whether the cold-cache regression matters in practice. The
  forced_miss bench is the upper bound; real frames have partial
  invalidation. If a real workload shows resize-frame jank, options:
  skip snapshot writes for collapsed subtrees, gate writes by
  subtree size threshold, or share storage via an arena
  (`Vec<Size>` + per-snapshot ranges).
