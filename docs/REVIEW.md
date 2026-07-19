# Aperture prioritized review

Reviewed 2026-07-17 through 2026-07-19; merged and pruned 2026-07-19.

## Scope and ranking

This is the single backlog from the full-module, text, and data-structure
reviews. Every item below was checked against the current source when the
reports were merged. Completed work, superseded proposals, historical review
narrative, and citations to removed profiling documents have been dropped.

Priority is assigned first by correctness and invariant risk, then by measured
or structural cost and breadth. Within a priority, higher-impact items come
first. Performance changes remain benchmark-gated because Aperture requires
steady-state allocation-free frames and several plausible caches have regressed
the real frame workload.

## Priority 1 — Broad hot-path improvements

### 1. Query Grid Hug intrinsic ranges in one recursion

- [ ] Every span-1 Hug-column cell requests `MinContent` and `MaxContent`
  back-to-back at `src/layout/grid/mod.rs:416-417`. On a cold subtree these are
  separate recursive walks; a text leaf shapes the same unbounded input and
  selects two metrics from it.

  Add a targeted `intrinsic_range` query that fills both per-node cache slots
  in one recursion while retaining the single-slot API for Stack's min-only
  case. Validate exact equivalence for every layout driver, inspect intrinsic
  compute counts, and compare forced-miss and resize benchmarks before keeping
  the larger API.

### 2. Store widget IDs only for interactive cascade rows

- [ ] `EntryRow.widget_id` stores eight bytes for every node at
  `src/ui/cascade/mod.rs:137-165`, although its consumers are reverse hit-test
  scans over the sparse `hit_entries` list. Cascades already retain a
  `WidgetId -> Endpoint` snapshot for response lookup.

  Add a parallel `hit_widget_ids` vector pushed with `hit_entries` at
  `src/ui/cascade/mod.rs:722`, remove `widget_id` from `EntryRow`, and
  reverse-iterate the compact hit vectors together. This saves eight bytes for
  every inert layout/container node without adding a lookup to the hit-test
  hot path.

  Pin cross-layer paint order, focusable-only rows, scroll/pinch routing, and
  duplicate-ID rejection. Compare cascade storage and pointer-move timing on
  container-heavy and fully interactive trees.

### 3. Keep one response snapshot in `WidgetEntry`

- [ ] `enter_widget` copies a 136-byte `ResponseState` solely to OR one
  disabled bit, then returns both copies in a 280-byte `WidgetEntry` at
  `src/widgets/mod.rs:65-76`. Theme selection continues to take the full state
  by value even though it only reads interaction flags
  (`src/widgets/theme/mod.rs:179-228` and
  `src/widgets/theme/widget_look.rs:116-126`).

  Retain one mutable state plus the original disabled bit, borrow
  `ResponseState` throughout `resolve_look` and the theme `pick` chain, then
  restore the original bit before moving the state into `Response::eager`.

  Validate freshly self-disabled visuals, ancestor-disabled state,
  interaction suppression, and the eager response's deliberately unmerged
  value. Pin `ResponseState` and `WidgetEntry` sizes and compare button-heavy
  frame profiles.

## Priority 2 — Focused and workload-dependent compaction

### 4. Make the paint-animation reverse index truly sparse

- [ ] `PaintAnims::by_shape` is a `Vec<Option<Index16>>`; the first animation
  at shape `k` resizes it to `k + 1` at
  `src/forest/tree/paint_anims.rs:232-255`. A caret or spinner recorded after a
  large static scene therefore retains two bytes for every preceding shape.

  Keep sorted `shape_indices: Vec<u32>` beside the existing sparse entries.
  The encoder visits shape indices monotonically, so one cursor can advance
  across skipped or culled ranges and sample matches in amortized `O(1)`.
  Wake and damage already iterate the sparse entries directly.

  Test animation on the first and last shape, multiple animations, and
  viewport/damage subtree culls. Assert that storage scales with animated
  shape count, not the largest shape index.

### 5. Intern record-local gradients and resolve each unique ID once per encode

- [ ] Every gradient occurrence appends a 56-byte `RecordedGradient` through
  `RecordPayloads::record_gradient` at `src/record_store.rs:36-41,94-100`.
  All gradient-lowering arms reach that append through
  `src/forest/shapes/lower.rs:64-121`, and encoding probes the shared atlas
  again for every occurrence.

  Add a capacity-retained record-local content interner keyed by the existing
  canonical gradient hash, with equality confirmation on collisions. Add
  retained encode scratch mapping each `GradientId` to one
  `ResolvedGradient`. Include geometry in the interner identity: identical
  stops with different axes or gradient kinds may share an atlas row but not a
  complete recorded gradient.

  Test identical gradients, same-stops/different-geometry gradients, every
  interpolation/spread mode, and forced hash collisions. Benchmark solid-only
  and gradient-heavy frames and require zero steady-state allocations.

### 6. Pack command kind and payload offset into one `u32`

- [ ] `RenderCmdBuffer` keeps one-byte `kinds` and four-byte `starts` columns
  at `src/renderer/frontend/cmd_buffer/mod.rs:60-63`; recording and decoding
  touch both at `src/renderer/frontend/cmd_buffer/mod.rs:389-417`.

  There are 13 command kinds, so four tag bits and a 28-bit word offset fit in
  one descriptor. The offset still permits a 1 GiB payload arena. This reduces
  metadata from five to four bytes per command, removes one retained
  allocation, and turns iteration into one sequential descriptor load.

  Preserve the typed `u32` payload arena and unaligned `Pod` reads.
  Exhaustively round-trip every command kind, pin the representable offset
  boundary, and compare command-buffer bytes plus encode/compose timing.

## Current guardrails

- Keep `Tree.records` as SoA and the six-byte `ExtrasIdx` sparse indirection.
- Keep `StateMap`'s type-erased registry with dense per-type values and owner
  columns.
- Keep the `u32` command payload arena, render-kind output vectors, text grid,
  and retained composer/backend scratch separate; their consumers and
  lifetimes differ.
- Keep the direct `Vec<ShapeRecord>` sequence. Compact tagged handles backed by
  typed payload arenas were tried and did not improve frame performance.
- Keep public gradient stops inline for allocation-free authoring. Gradient
  interning belongs after lowering.
- Do not weaken damage or cascade fingerprints to save storage.
- Do not merge Stack and Grid Fill solvers without first choosing shared
  semantics; their freeze cadence intentionally differs.
- Do not consolidate composer geometry-to-scissor conversion with backend
  damage-to-scissor conversion; snapping, outward rounding, and antialias
  padding differ.
- Keep `InternedStr` and `RecordedText` separate. Recorded spans must remain
  owner-free so `RecordStore` can recycle the active text arena.

## Validation baseline

Each implemented item should run its targeted tests and benchmarks, the
allocation-free CPU and GPU checks, and standard crate verification. Track
live layouts with the ignored size test and compiler layout output:

```sh
cargo test --lib print_hot_struct_sizes -- --nocapture --ignored
RUSTC_BOOTSTRAP=1 cargo rustc --lib -- -Zprint-type-sizes
cargo test
cargo fmt --all
cargo check
cargo clippy --all-targets -- -D warnings
```
