# Cascade subtree-skip cache (speculative)

Profiler-motivated proposal — **not yet justified by a bench**. The
encode-cache postmortem (`docs/cache-history/encode.md`) is a direct
warning that "memcpy-shaped O(N) walks" don't necessarily amortize
under a cache; read it before committing.

## Motivation

`scripts/bench-perf.sh BENCH=frame FEATURES=internals` (2026-05-20,
ASUS ROG, P-core 0) attributes **11.7% self-time** to
`CascadesEngine::run` across the four `frame/*` cases — the largest
single function in the report. Annotation showed no inner hot
instruction (max 1.38%); the cost is a dense per-node body, ~100 ns
on ~150 nodes, with IPC 3.1 and 0.26% branch miss rate. There is no
local micro-optimization available (a layer-hoist rewrite measured
+2.8% regression on `cached_cpu`).

For `frame/cached_cpu` (99 µs, fully-cached path) cascade is
~11.6 µs. If a subtree-skip cache turned that into a memcpy at
~100 B/node, the upper-bound win is ~10% of `cached_cpu`.

## Why it could work where the encode cache didn't

The encode cache failed because re-encoding was already memcpy-shaped
(`Vec<u32>` payload push) and the cache replay (`extend_from_slice` +
per-cmd start/rect rebase) did equal work in a different shape.

Cascade is more than memcpy per node:

- Per-node `compute_paint_rect` does `parent_transform.compose(self_transform)`
  + per-shape `apply_rect` + per-shape `Rect::union` rollup.
- `build_cascade_prefix` + `finish_cascade_input` per node (FxHash over
  32 B of state).
- `Rect::union` ripple through the parent stack on pop.
- Multiple `Vec::push` (rows, subtree_paint_rects, entries SoA,
  paint_arena rows, paint_arena spans) with bounds checks each.

A cache hit replaces all of that with `extend_from_slice` of stored
row ranges — strictly less work per node. **If** the hit rate is high
enough.

## Mechanism sketch

Mirror `MeasureCache`. Key per cache-eligible subtree on:

```text
(WidgetId, subtree_hash, parent_cascade_prefix_hash, root_layout_rect_q)
```

Inputs already exist:

- `subtree_hash` — `Tree.rollups.subtree[i]` (computed in `post_record`)
- `parent_cascade_prefix_hash` — finish the `Hasher` on the parent's
  `Frame.cascade_prefix` once per push and stash the u64; cheap because
  `build_cascade_prefix` already runs for non-leaves.
- `root_layout_rect_q` — integer-quantized `layout.rect[i]`; mirrors
  `available_q` in MeasureCache.

A hit blits five row ranges from per-WidgetId arenas into the live
cascade output:

1. `Cascade` rows → `cascades.rows[base..base+span]`
2. `Rect` rollups → `cascades.subtree_paint_rects[base..base+span]`
3. `EntryRow` SoA columns → global `cascades.entries` (variable-length;
   stored subtree-relative, copied at current `entries.len()`)
4. `Paint` rows → `cascades.paint_arena.rows` (stored subtree-relative
   indices; rebase on copy)
5. `Span` per-node → `cascades.paint_arena.node_spans[base..]` (stored
   subtree-relative; rebase by the current `paint_arena.rows.len()`
   before copy)

Plus `paint_arena.shape_to_paint[shape_idx]` — sparse per-shape,
indexed by tree-wide shape index, so stored subtree-relative-to-the-
root-shape-index and rebased on copy.

Storage layout: same `LiveArena` discipline as `MeasureCache` — flat
per-WidgetId arenas with mark-garbage compaction, eviction via
`SeenIds.removed` plumbed through `Ui::post_record`.

## Risks / open questions

1. **Encode-cache shape match.** The encode cache had the same
   "store subtree-relative, rebase on copy" pattern. If the rebase
   step (translating row indices by `entries.len()` /
   `paint_arena.rows.len()` / shape-index base) ends up as expensive
   as the per-node walk, the cache is a wash. Need a back-of-the-
   envelope cycle count *before* implementing.
2. **Authoring-stable but transform-shifting subtrees.** A scrolled
   panel re-records the same authoring (same subtree_hash) but its
   `parent_transform` shifts every frame. That invalidates every
   descendant cache entry — they all rebuild. Plausible bench: a
   long scrolled list. Need to know whether the workloads where
   cascade is hot are the workloads where this cache hits.
3. **Damage dependency.** Damage already does
   `subtree_hash + cascade_input` equality at the root of a subtree
   to skip its diff (`src/ui/damage/mod.rs:553`). The cascade cache
   would be doing the same check one pass earlier, then duplicating
   the output. Worth checking whether damage's skip can be widened
   to also skip the cascade walk for the subtree — i.e. cascade
   becomes a *consumer* of last frame's cascade output rather than
   gaining its own cache. That's a cheaper refactor with the same
   payoff.
4. **Hit-rate floor for the steady-state showcase.** The MeasureCache
   bench (`benches/measure_cache.rs`) sees ~25% wall-time improvement
   on the `nested` workload at high hit rate. Cascade's potential
   ceiling is ~11.6 µs / 99 µs = 12% on `cached_cpu`. If the actual
   hit rate is materially below 90% the win disappears under
   bookkeeping overhead.

## Before implementing

Land a bench first:

1. Pick a representative workload (showcase, frame_visual, or a
   synthetic high-fanout tree mirroring `benches/frame.rs`).
2. Add `benches/cascade_cache.rs` with `cached` (steady-state) and
   `forced_miss` variants — mirror the MeasureCache bench shape.
3. **Without writing the cache**, instrument `CascadesEngine::run` to
   count nodes whose `(subtree_hash, parent_prefix, root_rect)` would
   have hit had a cache existed. If that count is < 80% in steady
   state on realistic workloads, the cache is dead on arrival — its
   ceiling is below the bookkeeping floor.
4. Only if hit-rate ≥ 80% and projected savings ≥ 5% of frame time,
   implement the cache and run the same bench A/B.

## Bring it back if

- A workload bench shows cascade > 10% of frame time *and* the hit-
  rate instrumentation in step (3) above clears 80%.
- A future refactor makes cascade meaningfully more expensive per
  node (e.g. multi-pass paint-rect rollup, per-shape stencil
  pre-computation) — today the walk is dense but tight.

Otherwise: file alongside the encode/compose cache postmortems as
"considered, instrumented, declined".
