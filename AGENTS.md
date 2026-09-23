# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass
layout with flex-shrink sizing**, **wgpu rendering**.

## Posture

- **Break things freely, once asked.** Big-bang renames and migrations are
  welcome — no deprecation shims, compat aliases, feature flags, or migration
  helpers. Who decides is **Public API**, below.
- **Per-frame allocation is a regression.** Steady state is heap-alloc-free
  after warmup; push onto retained scratch with capacity reuse, never rebuild
  a map per frame.
- **API ergonomics matter.** Builder chains read like prose, defaults are
  right, surprising behavior gets a pinning test. When in doubt, favor
  call-site readability.
- **Micro-optimize freely** — struct packing, const fns, scratch reuse, cache
  layout — even without a workload demanding it.
- **Ship in measurable slices.** One feature with tests and a showcase section
  beats a half-finished cluster. A structurally complex change with no
  motivating workload is "too early": shelve it with a note.
- **Docs are starting positions**, this file included. When one contradicts
  user intent or current code, flag the conflict and ask rather than defer.

## Public API

**Never change it on your own.** Adding, renaming, removing, or re-signing an
exported item — a type, a method, an argument, a trait bound — waits for my
go-ahead. Propose it, then stop.

**Design it against its neighbours.** Before proposing new surface, read what
it will sit beside — the widget's other setters, the sibling type's methods,
the trait it joins, the `Response` its peers return — and match their names,
argument shapes, and order. A new item that reads unlike its neighbours is a
bug, however good it looks alone.

**Widgets use only the public API.** A widget here reaches nothing an outside
crate could not: no `pub(crate)` helper, private field, or crate-only trait or
macro. When it needs more, make that public first, with the docs a stranger
needs. The test: someone could reimplement the widget outside the crate, line
for line.

## Architecture

Five passes per frame over a tree rebuilt every frame: **record → measure →
arrange → cascade → encode + compose + paint**. Colour is linear-RGB f32 on
the CPU side; sRGB encoding happens on the GPU at swapchain write.

Read `benches/AGENTS.md` before measuring or reaching for `perf`: it holds the
A/B protocol and the traps that cost a wasted capture each.

## Verification

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --test alloc --features internals,bench,golden,gpu-debug-markers
```

`--all-features` is clippy's alone: `profile-with-tracy` starts the Tracy
client before `main`, so a test binary under it opens the profiler's socket
and prints `SymInitialize FAILED with code 87`. The test run names `alloc`
rather than `--tests` because `golden` would pull in `visual`.

Rendering changes (shaders, encoder/composer, atlases, colour pipeline, layout
that moves pixels) also run the visual suite:

```
cargo test --test visual --features internals,golden
```

Its goldens in `tests/visual/golden/` are local and show whatever tree last
wrote them; a missing one is written and then failed. Run the suite on the
unchanged tree first — if it fails there, rewrite the stale goldens with
`UPDATE_GOLDEN=1` before changing anything. A failure leaves `actual.png`,
`expected.png`, and `diff.png` in `tests/visual/output/<name>/`.

A change a user can see ends with a look at `cargo run --example showcase`.

## Gated reach-in modules

Test and bench code that reaches past a file's privates goes in one gated
`pub(crate)` module at the end of that file, named for who reaches in:

- **`internals`** — from *outside* the crate (`tests/visual`, `tests/alloc`,
  the showcase). Always `#[cfg(any(test, feature = "internals"))]`;
  `src/lib.rs` re-exports the published subset through `pub mod internals`.
- **`test_support`** — from *inside* the crate only (other modules' unit
  tests, `bench.rs` drivers). Its `cfg` is exactly the builds those consumers
  exist in — `test`, `feature = "bench"`, or both — since anything wider is
  dead code that `-W dead_code` reports.

Helpers only the file's own `mod tests` uses live in `mod tests`. Support that
is a subsystem rather than a reach-in — `ui::harness`, `host::test_gpu`,
`text::mono` — is a module of its own under the same `cfg`, named for what it
is.
