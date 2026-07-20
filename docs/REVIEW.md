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

## Priority 1 — Focused and workload-dependent compaction

### 1. Make the paint-animation reverse index truly sparse

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

### 2. Intern record-local gradients and resolve each unique ID once per encode

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
