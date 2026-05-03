# Cross-frame measure cache

WPF-style short-circuit: skip a node's measure when neither its
authoring inputs nor its incoming `available` size changed since last
frame. Composes with damage (subtree-hash equality is the same key the
encode cache wants).

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

   | workload | cached | forced_miss | save | % |
   | --- | --- | --- | --- | --- |
   | flat | 93.5 µs | 104.0 µs | 10.5 µs | 10.1% |
   | nested | 414.1 µs | 455.5 µs | 41.4 µs | 9.1% |

   Phase 1's percentage win is roughly constant across depths because
   it scales with leaf count and the stack overhead grows in lockstep.
   The absolute win is 4× larger on nested because the leaf count is.
   The ~900 non-leaf stack measure calls in nested are *not* skipped
   by Phase 1 — that's the headroom Phase 2 unlocks.

4. **Phase 2: full subtree skip.** Add a NodeId-indexed
   `prev_desired: Vec<Size>` mirror, built up-front from the WidgetId
   map. On a subtree-hash hit, `copy_from_slice(desired[i..subtree_end[i]])`
   and skip recursion. Same for `result.text_shapes`. Defer until
   Phase 1 + bench prove the dispatch overhead is worth optimizing
   further.

## Open questions

- Quantization granularity for `available` (1 logical px is the
  starting position; finer if Fill children show jitter-driven misses).
- Whether to extend the cache to `intrinsic()` queries cross-frame
  (keyed on `subtree_hash + axis + req`). Small extension once Phase 1
  ships.
