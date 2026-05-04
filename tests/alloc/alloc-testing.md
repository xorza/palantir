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
`System` and — only when the calling thread has `IN_AUDIT` set —
increments thread-local `ALLOCS`/`BYTES` and pushes a `Backtrace`.
`dealloc` is delegated unchanged; we count heap *operations*, not
residency.

Per-thread (not global) counters are deliberate: cargo runs tests in
parallel on the same process, and a global counter would let other
tests' setup allocations on other threads leak into our window.
Gating on the per-thread `IN_AUDIT` flag means only the auditing
thread's audit-window allocs ever count — no cross-test interference,
no global mutex.

`run_audit(name, warmup, audit, budget, scene)`:

1. Construct `Ui::new()` with a fixed 800×600 logical display.
2. Run `warmup` frames untracked — lets measure cache, encode cache,
   scratch `Vec`s reach steady-state capacity.
3. Set `IN_AUDIT`, snapshot the per-thread counter.
4. Run `audit` frames — every alloc on this thread bumps the counter
   and (with `RUST_BACKTRACE=1`) records a backtrace.
5. Clear `IN_AUDIT`, take delta + drained traces.
6. Print per-frame averages. On budget violation, dump captured
   backtraces (or hint at `RUST_BACKTRACE=1` if disabled), then panic.

Capture is free by default — `Backtrace::capture` is a no-op unless
`RUST_BACKTRACE` is set, so traces only cost when you ask for them.

## Status

### Infrastructure ✅
- `allocator.rs` — counting wrapper around `System`.
- `harness.rs` — `run_audit` with `AllocBudget`, parallel-test mutex.

### Fixtures
- `empty_frame` ✅ — `Ui` with no widgets, budget 0. Sanity baseline.
- `button_only` ✅ — single `Button::label("hello")`, budget 0. Pins the
  static-string label round-trip at zero allocs; uses 16 warmup / 64
  audit frames (the extra warmup absorbs scratch-Vec capacity growth
  that takes longer than the empty scene to settle).

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

1. Re-run with `RUST_BACKTRACE=1` — the harness captures one
   backtrace per audit-window alloc and dumps them all on failure,
   so you usually see the offending call site directly.
2. If the trace is ambiguous (deep inside a generic), add
   `dbg!(snapshot())` around suspect spans inside the frame loop
   to bisect which pass introduced it.
3. The fix is almost always: lift a `Vec::new()` to a retained scratch
   field, `.clear()` instead of replacing, or `with_capacity` with a
   sane initial size at construction time.

Raising a budget is the right call only when the alloc comes from a
dependency we don't control (cosmic-text, glyphon atlas growth) — and
even then, document why in the fixture.
