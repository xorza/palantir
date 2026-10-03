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

Each pass has one top-level module: `scene` holds what record writes,
`layout` measures and arranges it, `cascade` derives the tables input and
paint read, `damage` decides what repaints, `renderer` encodes and
composes, and `gpu` paints. `layout`, `cascade` and `damage` each keep their
result type in `mod.rs` and their engine in `engine.rs`.
`primitives` is the value vocabulary below all of them, `widget_core` the
framework the bundled `widgets` are written on.

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

## Non-shipping code

Code that does not ship has four homes, apart from a file's own
`#[cfg(test)] mod tests` and the helpers only that module uses.

- **`internals` at the end of a file** — a reach-in: test or bench code
  that needs that file's private items. One per file, the last item before
  `mod tests`. Its `cfg` is exactly the builds its callers exist in —
  `test`, `feature = "bench"`, `feature = "internals"`, or a mix — since
  anything wider is dead code that `-W dead_code` reports. A wider module
  narrows single items with their own `cfg`, and never needs a lint allow.
- **`crate::internals`** (`src/internals/`) — the subsystems: the frame
  harness, the paint capture, the panic probe, the shared fixtures. It is
  `palantir::internals` under `any(test, feature = "internals")`, and each
  submodule carries its own narrower gate. Test code that is a subsystem
  rather than a reach-in goes here, never among production modules.
- **`bench.rs`** beside the code it measures, under `feature = "bench"`,
  reached through the `bench` facade in `src/lib.rs`.
- **`crate::golden`** (`src/golden/`) — golden-image comparison, under
  `feature = "golden"`. It is public API rather than a reach-in, because
  suites outside this crate that draw through Palantir use it too.

Visibility says who reaches in: `pub` when code outside the crate calls it
(`tests/visual`, `tests/alloc`, the showcase, `benches/`), `pub(crate)` when
only the crate's own tests and benches do.

Two subsystems stay outside `src/internals/`. `gpu::test_gpu` and
`gpu::bench_gpu` hold wgpu types, which `clippy.toml` keeps inside
`crate::gpu`, so `crate::internals` re-exports the test GPU. `text::mono` is
a measurement backend beside `cosmic`, gated `any(test, feature =
"internals")`.
