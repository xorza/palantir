# Per-frame Allocation Audit Suite

Catches the regression where someone introduces a per-frame `Vec::new()`
/ `HashMap` rebuild / `format!` and silently violates CLAUDE.md's
"alloc-free steady state" rule.

## Goals

- Pin alloc-count behavior of representative scenes after warmup.
- Fail the build when a hot path starts allocating.
- Stay deterministic — alloc counts are a pure function of code paths.

## Non-goals

- Replacing `dhat` for ad-hoc profiling (this is a regression gate).
- Measuring wgpu / OS / cosmic-text allocs (not ours; we'd never reach
  zero). The audit covers the CPU pipeline through `Ui::end_frame` only;
  GPU submit lives in `WgpuBackend` and is excluded.
- Bytes-as-budget. We assert on alloc *count*; bytes are reported for
  diagnosis but a single capacity-doubling event would produce false
  failures.

## Layout

```
tests/alloc/
├── main.rs              entry: #[global_allocator] + mod decls
├── allocator.rs         CountingAllocator + snapshot/delta
├── harness.rs           run_audit(name, warmup, audit, budget, scene)
├── fixtures.rs          mod decls
├── fixtures/
│   └── widgets.rs       per-widget minimal scenes
└── alloc-testing.md     this file
```

Single test binary (`cargo test --test alloc`); Cargo auto-discovers
`tests/alloc/main.rs` per the standard project layout.

## How it works

`#[global_allocator]` installs `CountingAllocator`, which delegates to
`System` and increments `ALLOCS` / `BYTES` (relaxed atomics) on every
`alloc` / `realloc` / `alloc_zeroed`. `dealloc` is delegated unchanged
— we count heap *operations*, not residency.

`run_audit(name, warmup, audit, budget, scene)`:

1. Construct `Ui::new()` with a fixed 800×600 logical display.
2. Run `warmup` frames (default 8) — lets measure cache, encode cache,
   scratch `Vec`s reach steady-state capacity.
3. Acquire `AUDIT_LOCK` (process-global `Mutex`) to serialize the
   measured region against parallel cargo test threads.
4. Snapshot counters → run `audit` frames → snapshot delta.
5. Print per-frame averages. Assert delta ≤ `budget × audit`.

The lock only guards the measured region; warmup and assertion still
parallelize across fixtures.

## Status

### Infrastructure ✅
- `allocator.rs` — counting wrapper around `System`.
- `harness.rs` — `run_audit` with `AllocBudget`, parallel-test mutex.

### Fixtures
- `empty_frame` ✅ — `Ui` with no widgets, budget 0. Sanity baseline.
- `button_only` 🟡 `#[ignore]` — single `Button::label("hello")` allocates
  exactly 2× per frame in steady state. Budget pinned at 2 to capture
  the regression; ignored so the green suite stays green. Hunt + fix
  → flip budget to 0 → drop `#[ignore]`.

### Planned
- `nested_vstack_64` — past scratch-Vec growth; budget 0.
- `grid_8x8` — grid driver scratch + track-list; budget 0.
- `static_text_label` — pin honest baseline (cosmic-text isn't ours).
- `damage_animated_rect` — rect-mutating widget, exercises damage diff.

### CI ⏳
Local-only. Same posture as `tests/visual` — wire one pinned-runner job
once a flake or a second platform appears.

## When a fixture starts failing

Don't raise the budget. Find the alloc:

1. Add a `dbg!(snapshot())` around suspect spans inside the frame loop
   to bisect which pass introduced it.
2. Run with `RUST_BACKTRACE=1` and a temporary `eprintln!` in
   `CountingAllocator::alloc` gated on a thread-local "in audited
   region" flag — gives one stack trace per offending alloc.
3. The fix is almost always: lift a `Vec::new()` to a retained scratch
   field, `.clear()` instead of replacing, or `with_capacity` with a
   sane initial size at construction time.

Raising a budget is the right call only when the alloc comes from a
dependency we don't control (cosmic-text, glyphon atlas growth) — and
even then, document why in the fixture.
