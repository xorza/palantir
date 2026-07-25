# Cross-frame measure cache

The measure cache skips a subtree when its authoring fingerprint and
integer-pixel-quantized incoming `available` size match the preceding layout
pass. It is the only cross-frame cache in the render path; encode and compose
caches were removed after benchmarks showed contributions below 1%.

It covers **both** halves of the layout pass despite the name: a hit skips the
subtree's measure recursion, and the same snapshot row then lets arrange replay
its rects instead of re-running the drivers (see *Arrange replay* below). The
type names predate that second half.

## Storage

`MeasureCache` owns read-only `previous` and writable `current`
`MeasureSnapshot`s. Each snapshot concatenates layer trees in paint order while
keeping every tree's pre-order rows contiguous:

- `NodeArenas` stores one desired size, arranged rect, text span,
  intrinsic-slot row, and available key per recorded node. `rect` is the only
  column produced by the second half of the pass, captured after `arrange` has
  written it.
- `text_shapes` stores each measure-owned shaped text run once. Direct
  container text remains paint-only and is shaped after capture. Stack
  solvers can shape siblings out of node order, so a reverse tree walk unions
  the per-node spans into contiguous subtree ranges without assuming text
  payload order.
- `hugs` stores each Grid track-hug payload once.
- Dense `ArenaSnapshot` descriptors hold subtree node, text, and hug ranges.
  `WidgetIdMap<u32>` maps each cacheable non-leaf identity to its dense
  descriptor index.

The descriptor `subtree_hash` and `available_q` form the desired-size cache key.
`lookup_root_intrinsic` uses the same descriptor but checks only
`subtree_hash`, because intrinsic measurements are independent of the parent's
available size. Cache hits restore descendant intrinsic and available metadata
as well as desired/text/Grid state, so a parent hit does not erase arbitrary
descendant lookup roots from the next snapshot.

The writable snapshot retains its descriptor map when the ordered cacheable
`WidgetId` fingerprint matches that buffer's preceding contents. Only dense
descriptor values change on paint/layout authoring updates with stable
structure. A reorder, insertion, removal, or cacheability change rebuilds the
map from the retained ordered identities. The first captured tree moves its
completed desired and availability vectors into the snapshot, exchanging them
for the warmed alternate buffers; additional layer trees append.

When every current root's `(WidgetId, subtree_hash, available_q)` matches the
previous root signature and the total node count is unchanged, the previous
snapshot is already an exact materialization of the current output. The engine
keeps it in place instead of rewriting identical rows.

## Lifecycle

`LayoutEngine::run` validates the root signature, measures and arranges each
tree, captures changed output, then swaps `current` and `previous`. Empty and
removed trees disappear as part of that full-frame materialization; the
`SeenIds.removed` sweep no longer owns cache arena reclamation.

`MeasureCache::clear` clears both buffers while retaining their allocations.
Tests and benches reach it through `Ui.layout_engine.cache`.

## Arrange replay

Measure stamps the snapshot arena base of every subtree it short-circuits into
`LayoutScratch::arrange_src`. `LayoutEngine::replay_arranged` reads that stamp
and reproduces the subtree's rects from `NodeArenas::rect` instead of
dispatching the drivers.

This is sound because **arrange's only output is `out.rect`** — every driver
writes rects and recurses, `scroll::arrange` delegates to stack/zstack, and
container text shapes later in `run`, off this path. So for a subtree whose
authoring and `desired` are both known identical to the snapshot, which is
exactly what a measure hit proves, arrange is a pure function of the slot.

Three slot outcomes, two of which replay:

- unchanged rendered rect → `copy_from_slice`;
- same size, moved origin → copy with one add per node, the case where a
  sibling above grew and everything below shifts;
- resized → bail to the normal path, since a different size redistributes
  `Fill` children and nothing below is reusable.

Replaying from the snapshot rather than from the retained live `rect` column is
deliberate. The live column would make the unchanged case free, but it is
indexed by pre-order position and a measure hit does *not* prove index
stability — the per-`WidgetId` descriptors let a subtree hit after moving. The
snapshot's destination range is computed from the *current* tree, so it is
index-safe by construction.

Two consequences worth knowing before touching neighbouring code:

- `restore_after_cache_hit`'s `grid.hugs` splat is dead work whenever the
  replay fires; only the resize-bail path still reads hugs. It stays eager
  until that is measured.
- `LayerLayout::rect`'s per-frame zero-fill is **no longer** redundant. It used
  to be safe to drop on the grounds that arrange overwrites every node; arrange
  now skips nodes.

## Validation

`src/layout/cache/tests.rs` pins linear retained rows, subtree-range contents,
root and localized hits, descriptor-index rebuilds after reorder/removal,
available-size misses, reappearance, solver-order text restoration, exact
desired/rect replay, and stable capacity across oscillating tree sizes.

`src/layout/cache/integration_tests.rs` cross-checks warm output against a cold
cache across every driver, Grid hug restoration, intrinsic reuse, text command
stability, width changes, and the translated arrange replay — the last asserts
the branch actually fired via `LayoutScratch::arrange_replays`, since a
warm-equals-cold test passes vacuously when no replay happens.

`src/bench/layout/cache.rs` covers representative and real-text workloads plus
a 194-node unary chain and a 1,098-node balanced tree. The adversarial fixtures
now retain exactly 194 and 1,098 node rows respectively, while preserving all
21 localized sibling hits in the balanced fixture.
