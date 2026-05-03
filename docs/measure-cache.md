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

   Phase 2 (subtree-skip at every non-collapsed node, single-arena
   storage — what's shipped):

   | workload | cached | forced_miss |
   | --- | --- | --- |
   | flat | **85.0 µs** | **112.7 µs** |
   | nested | **375.5 µs** | **496.2 µs** |

   The arena design (single `Vec<Size>` + single `Vec<Option<ShapedText>>`
   shared across all snapshots, in-place rewrite when subtree size is
   stable, append + periodic compaction otherwise) cuts the cold-cache
   write cost vs the earlier per-WidgetId-Vec design by ~27% on both
   workloads. Steady-state cached path is unchanged (within noise). It
   also drops the per-snapshot memory footprint from ~358 KB to ~77 KB
   on the nested workload.

4. **Phase 2: full subtree skip with single-arena storage — shipped.**
   Cache holds two flat arenas (`desired_arena: Vec<Size>`,
   `text_arena: Vec<Option<ShapedText>>`) plus a per-`WidgetId` map of
   24-byte `ArenaSnapshot { subtree_hash, available_q, start, len }`.
   On hit at any non-collapsed node, the cache returns a `Range<usize>`
   into the arenas and the caller `copy_from_slice`s into
   `LayoutEngine.desired[i..]` and `LayoutResult.text_shapes[i..]`.
   On miss, `write_subtree` rewrites the arena slot in place if the
   subtree size matches the previous snapshot (steady-state hot path);
   otherwise it appends to the arenas and marks the old slot as
   garbage. When `arena_len > live_entries × 2`, a compact pass walks
   every snapshot and rewrites pointers into a freshly-packed arena.

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
