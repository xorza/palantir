# Cross-frame measure cache

The measure cache skips a subtree when its authoring fingerprint and
integer-pixel-quantized incoming `available` size match the preceding layout
pass. It is the only cross-frame cache in the render path; encode and compose
caches were removed after benchmarks showed contributions below 1%.

## Storage

`MeasureCache` owns read-only `previous` and writable `current`
`MeasureSnapshot`s. Each snapshot concatenates layer trees in paint order while
keeping every tree's pre-order rows contiguous:

- `NodeArenas` stores one desired size, text span, intrinsic-slot row, and
  available key per recorded node.
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

`MeasureCache::clear`, exposed through the test/internals
`Ui::clear_measure_cache`, clears both buffers while retaining their
allocations.

## Validation

`src/layout/cache/tests.rs` pins linear retained rows, subtree-range contents,
root and localized hits, descriptor-index rebuilds after reorder/removal,
available-size misses, reappearance, solver-order text restoration, exact
desired/rect replay, and stable capacity across oscillating tree sizes.

`src/layout/cache/integration_tests.rs` cross-checks warm output against a cold
cache across every driver, Grid hug restoration, intrinsic reuse, text command
stability, and width changes.

`src/bench/layout/cache.rs` covers representative and real-text workloads plus
a 194-node unary chain and a 1,098-node balanced tree. The adversarial fixtures
now retain exactly 194 and 1,098 node rows respectively, while preserving all
21 localized sibling hits in the balanced fixture.
