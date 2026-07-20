# Aperture prioritized review

Reviewed 2026-07-17 through 2026-07-20; merged and pruned 2026-07-20.

## Scope and ranking

This was the single backlog from the full-module, text, and data-structure
reviews. Every merged item was checked against the current source. Completed
work, superseded proposals, historical review
narrative, and citations to removed profiling documents have been dropped.

Priority is assigned first by correctness and invariant risk, then by measured
or structural cost and breadth. Within a priority, higher-impact items come
first. Performance changes remain benchmark-gated because Aperture requires
steady-state allocation-free frames and several plausible caches have regressed
the real frame workload.

No outstanding implementation items remain from this review.

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
