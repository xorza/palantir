# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass
layout with flex-shrink sizing**, **wgpu rendering**.

## Posture

State-of-the-art UI framework, craft-driven.

- **Break things freely, once asked.** Rename, refactor, big-bang migrations
  welcome — no deprecation shims, compat aliases, feature flags, or migration
  helpers. Releases break; that's what pre-1.0 buys. Bar is "fmt + clippy +
  tests pass and the showcase still feels right by eye." Who decides is
  **Public API**, below.
- **Per-frame allocation is a real metric.** Steady-state must be heap-alloc-free
  after warmup. New per-frame allocation or map rebuilding is a regression; push
  onto retained scratch with capacity reuse.
- **API ergonomics matter.** Builder chains read like prose, defaults are right,
  surprise behavior gets a pinning test. When in doubt, prioritize call-site
  readability.
- **Optimize aggressively when motivated.** Micro-wins (struct packing, const
  fns, scratch reuse, cache layout) are encouraged even without a workload
  demanding them.
- **Ship in measurable slices.** One feature with tests and a showcase section
  beats a half-finished cluster. If a change is structurally complex with no
  motivating workload, say "too early" and shelve with a note rather than ship
  speculation.
- **Docs are starting positions, not commitments.** Treat roadmaps, module
  docs, and this file as evolving and possibly wrong. When a doc contradicts
  user intent or current code, double-question rather than defer — flag the
  conflict and ask.

## Public API

**Never change it on your own.** Adding, renaming, removing, or re-signing an
exported item — a type, a method, an argument, a trait bound — waits for my
go-ahead. Propose it, then stop.

**Design it against its neighbours, not from scratch.** Before proposing new
surface, read what it will sit beside: the same widget's other setters, the
sibling type's methods, the trait it joins, the `Response` its peers return.
Match their names, argument shapes, and order. A new item that reads unlike
the ones around it is a bug, however good it looks alone.

## Widgets use the public API

A widget in this crate is written the way a widget outside it would be. It
reaches nothing an outside crate could not: no `pub(crate)` helper, no private
field, no crate-only trait or macro doing what a public path cannot. Widgets
have no exclusive access to the system.

When a widget needs something the public API does not offer, make it public
first, with the docs a stranger needs, then use it from the widget. The test is
that another person could reimplement the widget outside the crate, line for
line, against the published surface.

## Architecture

Five passes per frame over a tree rebuilt every frame: **record → measure →
arrange → cascade → encode + compose + paint**. Colour is linear-RGB f32
everywhere on the CPU side; sRGB encoding happens on the GPU at swapchain
write.

Rendering changes (shaders, encoder/composer, atlases, colour pipeline, layout
that moves pixels) need the visual suite run as well — the unit tests alone
won't catch a render regression.

Performance work starts at `benches/AGENTS.md` — the manual for both bench
harnesses and for `scripts/bench-perf.sh`. Read it before measuring or reaching
for `perf`; it carries the A/B protocol, the profiling recipes, and the traps
that otherwise get rediscovered one wasted capture at a time.

## Verification

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --test alloc --features internals,bench,golden,gpu-debug-markers
```

`--all-features` is clippy's alone. `profile-with-tracy` starts the Tracy client
before `main`, so a test binary under it opens the profiler's socket and prints
`SymInitialize FAILED with code 87`. The test run names every feature but that one,
and names `alloc` rather than taking `--tests`, because `golden` would pull in
`visual`, whose baselines are gitignored.

## Gated reach-in modules

Test and bench code that needs past a file's privates goes in one gated module
at the end of that file, `pub(crate)`, named for who reaches in:

- **`internals`** — reached from *outside* the crate: `tests/visual`,
  `tests/alloc`, the showcase example. Always
  `#[cfg(any(test, feature = "internals"))]`, and `src/lib.rs` re-exports the
  published subset through `pub mod internals`.
- **`test_support`** — reached from *inside* the crate only: another module's
  unit tests, a `bench.rs` driver, or both. Its `cfg` is exactly the builds
  those consumers exist in — `test`, `feature = "bench"`, or a disjunction of
  them — because anything wider is dead code in the builds that miss the
  consumer, and `-W dead_code` says so.

Helpers only the file's own `mod tests` uses get no module of their own; they
live in `mod tests`.

Support that is a subsystem rather than a reach-in — `ui::harness`,
`host::test_gpu`, `text::mono` — stays a module of its own under the same
`cfg`, named for what it is. The rule above is about reaching past *one
file's* privates.
