# Aperture data-structure review

Reviewed 2026-07-19.

## Executive summary

Aperture's central frame structures are already strongly data-oriented. The
tree's hot columns, sparse node extras, paint rows, command payload arena,
typed widget state, render batches, and retained scratch buffers are all good
fits for their access patterns. The remaining worthwhile changes are not broad
container substitutions; they are places where a wide or overlapping
representation defeats the otherwise columnar design.

The largest confirmed issue is the measure cache: it retains one complete copy
of every cached container subtree. The existing adversarial fixture measures
18,914 retained rows for a 194-node chain. A whole-tree previous/current
snapshot pair should preserve every subtree lookup while making storage linear.

The next largest size problems are public widget builders that embed complete
optional theme values, generic animation rows that hold four copies of their
value type, and the 88-byte `ShapeRecord` enum. At the start of this review,
`-Zprint-type-sizes` reported:

| Type | Review baseline |
|---|---:|
| `DragValue<'_>` | 1,504 B |
| `Checkbox<'_>` / `Switch<'_>` | 1,400 B |
| `Button<'_>` | 776 B |
| `AnimRow<AnimatedLook>` | 624 B |
| `AnimatedLook` | 152 B |
| `ShapeRecord` | 88 B |
| `ResponseState` | 136 B |
| `WidgetEntry` | 280 B |

All proposed replacements must preserve Aperture's steady-state
allocation-free contract. Changes below are ordered by expected value and
grouped into independently implementable batches.

## Batch 1 — Remove multiplicative and oversized storage

- [ ] **Replace overlapping measure-cache snapshots with a double-buffered
  whole-tree snapshot.** Every non-leaf cache miss recursively measures its
  children and then copies its complete `[start..subtree_end)` result into a
  separate snapshot at `src/layout/engine.rs:572-628`; `write_subtree` owns an
  independent node, hug, and text range for every such root at
  `src/layout/cache/mod.rs:301-372`. Storage is therefore proportional to the
  number of ancestor/descendant pairs: quadratic for a unary chain and
  `O(N log N)` for a balanced tree. The design note confirms 18,914 rows for
  194 chained nodes and 5,403 rows for 1,098 balanced nodes at
  `src/layout/measure-cache.md:56-63`. Keep a read-only previous-frame arena and
  a current-frame arena per tree; store every node's desired size/text span,
  every shaped text run, and every grid hug payload once in pre-order, while
  per-`WidgetId` descriptors point into the previous tree's contiguous
  subtree ranges. Cache hits still blit arbitrary subtrees; after layout, the
  fully materialized current output becomes the next previous snapshot.
  Preserve `root_intrinsics` on each descriptor. This is not the rejected
  selective-root prototype at `src/layout/measure-cache.md:65-75`: it retains
  all lookup roots and therefore keeps localized sibling hits. Validate exact
  output and hit counts first, then require linear retained-row counts and no
  regression in the existing cached, forced-miss, resize, localized, unary,
  and balanced cache benchmarks.

- [x] **Borrow widget style overrides instead of embedding entire theme trees
  in every builder.** `Button`, `Checkbox`, and `Switch` store
  `Option<ButtonTheme>` / `Option<ToggleTheme>` directly at
  `src/widgets/button.rs:11-18`, `src/widgets/checkbox/mod.rs:23-29`, and
  `src/widgets/switch.rs:28-34`; the same pattern appears on `ComboBox`,
  `DragValue`, `RadioButton`, and `TextEdit`. Those theme values contain four
  or eight complete `WidgetLook` values
  (`src/widgets/theme/widget_look.rs:94-109`,
  `src/widgets/theme/toggle.rs:22-43`), so the `None` case still fixes the
  public builder's layout to hundreds or thousands of bytes. Store
  `Option<&'a T>` and make `.style` accept a borrowed override; the existing
  resolution path already consumes `Option<&T>` at
  `src/widgets/theme/mod.rs:208-228`. This leaves the common inherited-style
  builder pointer-sized and makes sharing a custom style explicit. Add the
  public builders to the hot-size regression table, compile an external
  consumer to cover the cross-crate by-value ABI, and benchmark construction
  plus `show` for inherited and custom styles.

  Implemented on 2026-07-19. The live 64-bit layouts are now 136–184 B:
  `Button` 776 → 144 B, `Checkbox` / `Switch` 1,400 → 144 B,
  `ComboBox` 768 → 136 B, `DragValue` 1,504 → 184 B, and `TextEdit`
  848 → 168 B. `RadioButton<u8>` is 152 B. On the aggregate CPU frame bench,
  same-machine midpoint comparisons were cached 443.46 → 436.40 µs, partial
  390.17 → 382.63 µs, resizing 588.36 → 579.80 µs, and scrolling
  509.74 → 502.31 µs.

## Batch 2 — Use variant-specific storage for wide sums

- [ ] **Union the mutually exclusive duration and spring payloads in
  `AnimRow`.** A row currently stores `current`, `target`, `velocity`, and
  `segment_start` simultaneously at `src/animation/mod.rs:243-271`, although
  `velocity` is spring-only and `segment_start`/`elapsed` are duration-only.
  The motion kind is already an enum at `src/animation/mod.rs:77-88`. Replace
  the parallel fields with `MotionRow<T>::Duration { segment_start, elapsed }`
  or `MotionRow<T>::Spring { velocity }`; this removes one full `T` from every
  row and should reduce `AnimRow<AnimatedLook>` from 624 B to roughly 480 B
  without changing lookup shape. If retained settled rows still dominate,
  follow with a measured state-specific table where a settled row keeps only
  its last value and active duration/spring rows live in separate dense
  arenas. Preserve same-frame double-tick suppression and untouched-slot
  eviction at `src/animation/mod.rs:250-271,500-512`. Pin sizes and exact
  trajectories for initial appearance, retarget, motion-kind switch, settle,
  and multi-pass frames; compare animated-switch and broad frame benchmarks.

- [ ] **Turn the shape sequence into compact tagged handles backed by typed
  payload arenas.** All shapes occupy 88 bytes because `Vec<ShapeRecord>` at
  `src/forest/shapes/mod.rs:17-44` is sized by the `Curve` variant's four
  control points and metadata at `src/forest/shapes/record.rs:156-189`.
  Common text, rectangles, images, and especially `GpuView { epoch }`
  (`src/forest/shapes/record.rs:243-255`) pay the same width through rollup,
  intrinsic, cascade, and encode walks. Keep record order in a compact
  `ShapeRef { kind, index }` stream and place each variant's fixed payload in a
  retained typed arena; bulk mesh/polyline/text/gradient payloads remain in
  `RecordStore` as today. `TreeItems` already centralizes sequence decoding at
  `src/forest/tree/iter.rs:62-120`, and the canonical per-shape hash is already
  a parallel compact column at `src/forest/shapes/mod.rs:34-43`. This should
  reduce the walked sequence to about 8 B per shape while avoiding padding
  common variants up to the rare maximum. Validate every variant's exact hash,
  bbox, layout text ordinal, paint order, animation attachment, and emitted
  command. Measure total retained bytes and all four frame arms; reject the
  split if indirection costs more than the bandwidth it removes.

## Batch 3 — Keep sparse adjuncts sparse and stop copying response snapshots

- [ ] **Replace `PaintAnims::by_shape` with a sorted sparse index.** The
  registry calls `by_shape` sparse, but the first animation at shape `k`
  resizes a `Vec<Option<Index16>>` to `k + 1` at
  `src/forest/tree/paint_anims.rs:215-256`. A single caret or spinner recorded
  after a large static scene therefore initializes and retains two bytes for
  every preceding shape. Keep a parallel `shape_indices: Vec<u32>` beside the
  already sparse entries. The encoder visits shape indices monotonically
  through `TreeItems` and recursion at
  `src/renderer/frontend/encoder/mod.rs:511-525,644-654`, so one cursor can
  advance across skipped or culled ranges and sample a matching entry in
  amortized `O(1)` without a dense reverse map. Wake and damage already iterate
  `entries` directly. Test an animation on the first and last shape, multiple
  animations, and viewport/damage subtree culls; assert storage scales with
  animated-shape count, not the largest shape index.

- [ ] **Keep one `ResponseState` in `WidgetEntry` and borrow it through theme
  selection.** `enter_widget` copies a 136-byte `ResponseState` solely to OR
  one disabled bit, then returns both copies in a 280-byte `WidgetEntry` at
  `src/widgets/mod.rs:57-76`. Theme picking continues to take the full state by
  value even though it reads only interaction flags
  (`src/widgets/theme/widget_look.rs:111-126`), and the original copy is kept
  only for `Response::eager`. Retain one mutable state plus the original
  disabled bit, use `&ResponseState` throughout `resolve_look` and the theme
  `pick` chain, then restore the original bit before moving that same value
  into the eager response. This cuts the entry roughly in half and makes the
  internal API express read-only access. Validate freshly self-disabled
  visuals, ancestor-disabled state, interaction suppression, and the returned
  eager response's deliberately unmerged value; add `ResponseState` and
  `WidgetEntry` size pins and compare button-heavy frame profiles.

- [ ] **Store widget IDs only for interactive cascade rows.** Cascades already
  retain a `WidgetId -> Endpoint` snapshot for response lookup
  (`src/ui/cascade/mod.rs:289-321`), and `entries_base + node` recovers the full
  response row (`src/ui/cascade/mod.rs:260-267,324-330`). The separate
  `EntryRow.widget_id` column nevertheless stores another eight bytes for
  every node, while its only consumers are reverse hit-test scans over
  `hit_entries` at `src/ui/cascade/mod.rs:348-418`. Add a parallel
  `hit_widget_ids` vector pushed only when `hit_entries` is pushed at
  `src/ui/cascade/mod.rs:714-732`, and remove `widget_id` from `EntryRow`.
  Reverse-iterate the two compact hit vectors together. This saves eight bytes
  for every inert layout/container node without adding a lookup to the hit-test
  hot path. Pin cross-layer paint order, focusable-only rows, scroll/pinch
  routing, and duplicate-ID rejection; compare cascade size and pointer-move
  timing on container-heavy and fully interactive trees.

## Batch 4 — Compact transient command and gradient metadata

- [ ] **Pack command kind and payload offset into one `u32`.**
  `RenderCmdBuffer` currently keeps a one-byte `kinds` column and a four-byte
  `starts` column for every command at
  `src/renderer/frontend/cmd_buffer/mod.rs:58-69`; recording writes both and
  decoding loads both at `src/renderer/frontend/cmd_buffer/mod.rs:388-417`.
  There are 13 command kinds (`src/renderer/frontend/cmd_buffer/payload.rs:81-139`),
  so four tag bits and a 28-bit word offset fit in one descriptor. A 28-bit
  offset still permits a 1 GiB payload arena, beyond a viable frame. This
  reduces metadata from five to four bytes per command, removes one retained
  allocation, and turns iteration into one sequential descriptor load.
  Preserve the typed `u32` payload arena and unaligned `Pod` reads. Exhaustively
  round-trip every command kind, pin the representable offset boundary, and
  compare command-buffer bytes plus encode/compose timing.

- [ ] **Intern record-local gradients and resolve each unique ID once per
  encode.** Every gradient occurrence unconditionally appends a 56-byte
  `RecordedGradient` at `src/record_store.rs:28-41,93-100`, because all three
  lowering arms call `record_gradient` independently at
  `src/forest/shapes/lower.rs:64-121`. Encoding then probes the shared atlas
  again for every occurrence at
  `src/renderer/frontend/encoder/mod.rs:55-73`, even when a theme gradient is
  repeated across hundreds of widgets. Add a capacity-retained, record-local
  content interner keyed by the already computed canonical gradient hash with
  equality confirmation on collisions, and a retained encode scratch mapping
  each `GradientId` to one `ResolvedGradient`. Keep geometry in the interner
  identity—same stops with different axes or gradient kinds may share an atlas
  row but not a complete recorded gradient. Test identical gradients,
  same-stops/different-geometry gradients, all interpolation/spread modes, and
  forced hash collisions; benchmark both solid-only and gradient-heavy frames
  and require zero steady-state allocations.

## Structures to keep

- `Tree.records` as SoA plus the six-byte `ExtrasIdx` sparse indirection is
  canonical for the per-pass column access pattern.
- `StateMap`'s type-erased registry with dense per-type values and owner
  columns is a good compromise between open-ended widget state and compact
  iteration.
- The `u32` command payload arena, render-kind-specific output vectors,
  text grid, and retained composer/backend scratch should remain separate;
  their consumers and lifetimes differ.
- Keep public gradient stops inline for allocation-free authoring. The
  proposed gradient interning happens after lowering and does not replace
  `Brush` with an `Rc`/heap allocation.
- Do not weaken damage or cascade fingerprints to save storage. The duplicated
  values identified above can be removed without changing the cache keys that
  protect correctness.

## Validation baseline

For each implemented batch, run the targeted tests/benchmarks above, the
allocation-free CPU and GPU checks, and the standard crate verification. Track
live layouts with both the existing ignored size test and compiler layout
output:

```sh
cargo test --lib print_hot_struct_sizes -- --nocapture --ignored
RUSTC_BOOTSTRAP=1 cargo rustc --lib -- -Zprint-type-sizes
cargo test
cargo fmt --all
cargo check
cargo clippy --all-targets -- -D warnings
```
