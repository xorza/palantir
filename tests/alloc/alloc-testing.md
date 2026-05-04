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
├── allocator.rs         CountingAllocator + with_audit
├── harness/
│   ├── mod.rs           run_audit + audit_until_stable
│   └── format.rs        user_frames backtrace filter
├── harness_tests.rs     unit tests for the harness itself
├── fixtures.rs          mod decls
├── fixtures/
│   └── widgets.rs       per-widget minimal scenes
└── alloc-testing.md     this file
```

Single test binary (`cargo test --test alloc`); Cargo auto-discovers
`tests/alloc/main.rs` per the standard project layout.

## How it works

`#[global_allocator]` installs `CountingAllocator`, which delegates to
`System` and — only when the calling thread is inside `with_audit` —
increments thread-local counters and pushes a `backtrace::Backtrace`
(captured unresolved; resolution is lazy on the failure path).
`dealloc` is delegated unchanged; we count heap *operations*, not
residency.

Per-thread (not global) counters are deliberate: cargo runs tests in
parallel on the same process, and a global counter would let other
tests' setup allocations on other threads leak into our window.
Gating on the per-thread `IN_AUDIT` flag means only the auditing
thread's audit-window allocs ever count — no cross-test interference,
no global mutex.

`with_audit(F)` is the load-bearing API in `allocator.rs`: it sets
`IN_AUDIT` via an RAII guard (so a panic inside `F` can't strand the
flag), drains stale traces, runs `F`, and returns the `(allocs,
bytes, traces)` delta.

Two test-facing wrappers in `harness/mod.rs`:

- **`audit_until_stable(name, budget, scene)`** — auto-discovers the
  warmup count. Probes one frame at a time until `STABLE_RUN`
  consecutive frames have stayed within budget, then audits a fixed
  64-frame window. **Use this for new fixtures** so warmup numbers
  don't have to be eyeballed per scene; cache settling rates differ
  across scenes and machines.
- **`run_audit(name, warmup, audit, budget, scene)`** — explicit
  warmup count. Use when debugging the harness itself or pinning a
  specific multi-phase behavior.

Both run a fixed 800×600 logical display, drive `Ui::new()`, and on
budget violation dump captured backtraces (filtered to user code via
`format::user_frames`) before panicking.

## Trace filtering

`user_frames` resolves the captured backtrace and emits only:
- `src/...` frames (palantir library code), and
- the *first* `tests/alloc/fixtures/...` frame (the entry point into
  the scene closure).

Everything else — std/runtime, hashbrown/cosmic-text/etc., the
harness machinery itself — is dropped. Demangled names are stripped
of the `alloc::` test-binary-crate prefix and the `::h<hash>` suffix.

Set `PALANTIR_ALLOC_FULL_BT=1` to bypass the filter and dump the raw
unfiltered backtrace; useful when the filter rejects something it
shouldn't.

## Status

### Infrastructure ✅
- `allocator.rs` — counting wrapper around `System` + `with_audit`.
- `harness/mod.rs` — `run_audit` + `audit_until_stable` with
  `AllocBudget`.
- `harness/format.rs` — `user_frames` backtrace filter.
- `harness_tests.rs` — unit tests for the harness itself.

### Fixtures
- `empty_frame` ✅ — `Ui` with no widgets, budget 0. Sanity baseline.
- `button_only` ✅ — single `Button::label("hello")`, budget 0. Pins
  the static-string label round-trip at zero allocs.

Both use `audit_until_stable`, so warmup is auto-discovered.

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

1. Re-run the failing test — the harness captures one backtrace per
   audit-window alloc and dumps them all on failure, filtered to
   user code, so you usually see the offending call site directly.
2. If the trace is ambiguous (deep inside a generic), set
   `PALANTIR_ALLOC_FULL_BT=1` to see the unfiltered stack, or add
   `dbg!(...)` around suspect spans inside the frame loop to bisect
   which pass introduced it.
3. The fix is almost always: lift a `Vec::new()` to a retained scratch
   field, `.clear()` instead of replacing, or `with_capacity` with a
   sane initial size at construction time.
