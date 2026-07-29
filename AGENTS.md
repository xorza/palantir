# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass layout with flex-shrink sizing**, **wgpu rendering**.

Read the **Architecture** section below for the full design rationale before making non-trivial changes.

## Posture

State-of-the-art UI framework, craft-driven.

- **Break things freely.** Rename, refactor, big-bang migrations welcome — no deprecation shims, compat aliases, feature flags, or migration helpers. Releases break; that's what pre-1.0 buys. Bar is "fmt + clippy + tests pass and the showcase still feels right by eye."
- **Per-frame allocation is a real metric.** Steady-state must be heap-alloc-free after warmup. New per-frame `Vec::new()` / `HashMap` rebuild = regression; push onto retained scratch with capacity reuse.
- **API ergonomics matter.** Builder chains read like prose, defaults are right, surprise behavior gets a pinning test. When in doubt, prioritize call-site readability.
- **Optimize aggressively when motivated.** Micro-wins (struct packing, const fns, scratch reuse, cache layout) are encouraged even without a workload demanding them.
- **Ship in measurable slices.** One feature with tests + a showcase section (on the matching tab — tabs group related features, e.g. all form controls share `controls`) beats a half-finished cluster. If a change is structurally complex with no motivating workload, say "too early" and shelve with a note rather than ship speculation.
- **Docs are starting positions, not commitments.** Treat `docs/roadmap/`, module `//!` docs, and this file as evolving and possibly wrong. When a doc contradicts user intent or current code, double-question rather than defer — flag the conflict, ask, and update the doc. Working notes and audit backlogs live in `.notes/` and are not part of the tracked record.

## Architecture

Five passes per frame on an arena `Tree` rebuilt every frame (with `tree.post_record` finalizing `subtree_end` + per-node + subtree-rollup hashes between record and measure):

1. **Record** — user code (`Button::new().label("x").show(&mut ui)`) appends per-node columns + `Shape`s. Inside every `show`, the widget-authoring primitive is `let w = ui.widget(node)` (resolves the frame-stable `WidgetId`, exposing `w.id()` for `response_for` / state / theme picking / child-id derivation and `w.node` for pre-record mutation) followed by exactly one `w.record(ui, chrome, body)` that opens, records, and closes the node. Child nodes keyed by `.id(parent.with("x"))` use the same two calls back to back; `examples/custom_widget.rs` is the worked example.
2. **Measure** (post-order) — node returns desired size given available; `MeasureCache` short-circuits whole subtrees on `(WidgetId, subtree_hash, available_q)` hits. Single dispatch (no WPF-style grow loop).
3. **Arrange** (pre-order) — parent assigns final `Rect` to each child.
4. **Cascade** (pre-order) — `CascadesEngine::run` flattens disabled/invisible/clip/transform and builds the hit index, producing a frozen `Cascades` result (`src/scene/cascade/`) consumed by damage diff, hit-test, _and_ the encoder so they can't drift. A tree-wide paint-excluding static-authoring hash plus exact identity/structure/layout-rect comparisons route geometry or inherited-state changes to a full rebuild; paint-only changes retain rows, skip unchanged subtrees by their authoring rollup, and repair dirty paint spans plus ancestor paint bounds in place.
5. **Encode + Compose + Paint** — `Encoder` walks the tree and paints each operation straight into a `ComposeSession` (the production `PaintSink`) from scratch every frame; `Composer` groups by scissor, snaps to physical pixels; `WgpuBackend` submits instanced quads. There is no intermediate command stream — the encoder and composer are one pass, so a paint op is lowered once and consumed immediately. `Damage` returns `Full` / `Partial(rect)` / `Skip` and filters which leaves the encoder paints. No encode or compose caches — both were implemented and removed after profiling; the encoder + composer are already memcpy-shaped and a per-frame rebuild beat a per-subtree cache replay.

**Colour pipeline.** Linear-RGB f32 everywhere on the CPU side; sRGB encoding happens on the GPU at swapchain write.

## Before reporting work as done

For changes that touch **rendering** (shaders, encoder/composer, gradient
or text atlas, colour pipeline, layout that moves pixels), also run the
visual suite — `cargo test` alone won't catch a render regression:

```sh
cargo test --test visual --features internals
```

If goldens legitimately move (an intentional visual change), inspect the
`tests/visual/output/<name>/{actual,expected,diff}.png` artifacts, then
regenerate with `UPDATE_GOLDEN=1` and re-run to confirm green.

## Hot-path struct sizes

`src/lib.rs`'s `hot_struct_sizes` module drives two tests from one
`hot_structs!` inventory of every per-frame struct touched by layout /
cascade / encode / compose / damage (the SoA columns, the per-shape /
per-chrome lowered forms, the encoder↔composer wire payloads, and the
GPU instance types):

- **`hot_struct_sizes_are_pinned`** — a real (non-ignored) gate that
  asserts `(size, align)` for every entry. A silent footprint
  regression (added field, stop-cap bump, an enum variant re-inlining a
  boxed payload) fails `cargo test` instead of diffusing through the
  codebase.
- **`print_hot_struct_sizes`** (`#[ignore]`) — prints the live table.
  Run it to read off a new number when a layout change is intentional:

```sh
cargo test --lib print_hot_struct_sizes -- --nocapture --ignored
```
