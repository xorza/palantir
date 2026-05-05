# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-style two-pass layout**, **wgpu rendering**.

Read `DESIGN.md` for the full design rationale before making non-trivial changes.

## Posture

State-of-the-art UI framework, craft-driven. **No external consumers, no published API** — treat it as sports programming.

- **Break things freely.** Rename, refactor, big-bang migrations welcome — no deprecation shims, compat aliases, feature flags, or migration helpers. Bar is "fmt + clippy + tests pass and the showcase still feels right by eye."
- **Per-frame allocation is a real metric.** Steady-state must be heap-alloc-free after warmup. New per-frame `Vec::new()` / `HashMap` rebuild = regression; push onto retained scratch with capacity reuse.
- **API ergonomics matter.** Builder chains read like prose, defaults are right, surprise behavior gets a pinning test. When in doubt, prioritize call-site readability.
- **Optimize aggressively when motivated.** Micro-wins (struct packing, const fns, scratch reuse, cache layout) are encouraged even without a workload demanding them.
- **Ship in measurable slices.** One feature with tests + a showcase tab beats a half-finished cluster. If a change is structurally complex with no motivating workload, say "too early" and shelve with a note rather than ship speculation.
- **Docs are starting positions, not commitments.** Treat `docs/*.md`, `DESIGN.md`, `references/*` as evolving and possibly wrong. When a doc contradicts user intent or current code, double-question rather than defer — flag the conflict, ask, and update the doc.

## Code style

- **Comments:** none except non-obvious *why*. Code is short and self-explanatory; keep it that way.
- **Asserts:** default to release `assert!` for invariants, not `debug_assert!` — `debug_assert!` is stripped in release and hides logic bugs in the build users actually run. Reserve it for checks too expensive for release (e.g. O(n) inside a hot loop), and call out the tradeoff.
- **Edition 2024.** Dependencies pinned to `*` for now (lockfile pins actual versions) — fine for prototype, pin before publishing.
- **Tests in `lib.rs` pin layout semantics.** Add a test whenever you change measure/arrange behavior. Don't add wgpu code paths to the layout/tree modules.
- **All test-only code lives in test modules.** No `#[cfg(test)] pub(crate) fn …` on production types. If a test needs internals, expose the field as `pub(crate)` and call production code paths, OR move the test inside the module's `#[cfg(test)] mod tests`. Test-only methods on production types creep, drift, and signal a "real consumer coming any day" that never arrives.
- **Split fat-test files** into `foo/{mod.rs, tests.rs}` when tests dominate (>40% or >150 lines).
- **Visibility:** default to narrowest; demote `pub` → `pub(crate)` → private whenever nothing outside uses the item. `pub(crate)` on fields is fine — invariants live in the mutating methods, not in encapsulation theater. No `pub(in path)` / `pub(super)` — exotic noise; use `pub(crate)` for any cross-module access.
- **No trivial accessors — prefer direct field access.** If a method body is just `self.field` / `&self.field` / `self.field = v`, or a one-hop call into a built-in collection method (`self.foo.len()`, `self.foo.is_empty()`, `self.foo.contains_key(k)`), delete it and make the field `pub(crate)`. Same for the inner crate boundary: another module reaching for `cache.snapshots.len()` is fine — don't wrap it in `cache.snapshot_count()`. Inline accessors are fine when they do real work (computation, invariant enforcement, returning a derived view).
- **No tuple returns.** Give a named result struct next to the function. `Option`/`Result` excepted.
- **No inline `crate::foo::bar::Type` paths** in expressions or patterns. Add a `use` at the top — surface dependencies in the imports block, don't bury them.
- **No re-exports inside the crate.** Only `lib.rs` `pub use`s items to define the published surface. Intermediate `mod.rs` files don't re-export — make submodules `pub(crate)` and import via the canonical path (`use crate::primitives::color::Color`, not `use crate::primitives::Color`). One canonical path per item.
- **`bytemuck::Pod` structs use `#[padding_struct::padding_struct]`.** The proc macro injects trailing padding fields so the struct's size is a multiple of its alignment, satisfying `Pod`'s no-padding-bytes invariant. Don't hand-add `_pad: u32` fields — they rot when a new field shifts the layout. Construct via `Self { real_field: x, ..bytemuck::Zeroable::zeroed() }` so the spread fills whatever padding the macro generated; `unsafe { std::mem::zeroed() }` for `const` sentinels. Existing examples: `EnterSubtreePayload` (`src/renderer/frontend/cmd_buffer/mod.rs`), `TextCacheKey` (`src/text/mod.rs`).
- **`WidgetId`** is hashed from a user-supplied key — keep IDs stable across frames so persistent state survives. Auto-deriving constructors (`Button::new`, `Text::new`, `Panel::hstack`, …) use `WidgetId::auto_stable()` + `#[track_caller]` so calls at different source lines get distinct ids. `#[track_caller]` does **not** propagate through closure bodies, so a helper that builds widgets inside a closure passed to e.g. `Panel::show(ui, |ui| { ... })` resolves every call to the same source location — but `Ui::node` silently disambiguates auto-id collisions by mixing in a per-id occurrence counter, so loops and closure helpers Just Work. Per-widget state keys on the disambiguated id and is therefore positional within the colliding call site, so reordering helper calls or conditionally inserting one will re-key state for the affected occurrences. When call order isn't stable, override with `.with_id(key)` (the builder method on `Configure`) where `key` is something stable like the item's domain id. Explicit-key collisions are caller bugs and hard-assert in `Ui::node`.

## Architecture

Five passes per frame on an arena `Tree` rebuilt every frame:

1. **Record** — user code (`Button::new().label("x").show(&mut ui)`) appends per-node columns + `Shape`s.
2. **Measure** (post-order) — node returns desired size given available size.
3. **Arrange** (pre-order) — parent assigns final `Rect` to each child.
4. **Cascade** (pre-order) — `Cascades::rebuild` flattens disabled/invisible/clip/transform; consumed by encoder *and* hit-index so they can't drift.
5. **Encode + Paint** (pre-order) — `renderer::encode` → `Vec<RenderCmd>`; `Composer` groups by scissor, snaps to physical pixels; `WgpuBackend` submits instanced quads.

Widget *state* (scroll, focus, animation) will live in a separate `Id → Any` map keyed by `WidgetId` — see Status.

**Tree = SoA columns indexed by `NodeId.0`:** `layout: Vec<LayoutCore>` (mode/size/padding/margin/align/visibility — measure/arrange), `paint: Vec<PaintCore>` (`PaintAttrs` 1-byte sense/disabled/clip + extras index — cascade/encoder/hit-test), `widget_ids: Vec<WidgetId>` (hit-test + future state map), `subtree_end: Vec<u32>` (pre-order topology; `i + 1 == subtree_end[i]` for a leaf — every walk). Splitting by reader keeps each pass touching only the columns it needs. Measured `desired`/`rect` live on `LayoutResult` keyed by `NodeId`, not on the tree.

**`Shape`** (paint primitive: `RoundedRect`, `Text`, `Line`, …) stored flat in `Tree.shapes`, sliced per-node via `Tree.shape_starts` (length `node_count() + 1`). `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning. Layout passes ignore Shapes and `PaintCore`; paint pass ignores hierarchy beyond `subtree_end`. **This decoupling is load-bearing — keep it.**

**Sizing (WPF-aligned):** `Fixed(n)` outer = exactly `n` (incl. padding); `Hug` outer = content + padding (WPF `Auto`); `Fill(weight)` takes leftover, distributed by weight across `Fill` siblings (WPF `*`). Canonical impl: `resolve_axis_size` in `src/layout/mod.rs`; pinned by `src/layout/{stack,wrapstack,zstack,canvas,grid}/tests.rs`.

## Project layout

- `src/primitives/` — pure geometry: Rect/Size/Color/Stroke/Corners/Spacing/Transform/Visuals/num/approx/urect
- `src/shape.rs` — Shape enum (RoundedRect, Line, Text)
- `src/tree/` — Tree (SoA + subtree_end), NodeId, GridDef, hash; `tree/element/` (Element builder, LayoutCore/PaintCore columns, PaintAttrs, Configure); `tree/widget_id.rs`
- `src/text/` — cosmic-text measurement + glyphon rendering glue
- `src/layout/` — LayoutEngine + drivers (stack/wrapstack/zstack/canvas/grid), intrinsic, cache; `layout/types/` (Sizing/Align/Justify/Sense/Visibility/Display/Track/Span/GridCell — layout vocabulary)
- `src/input/` — InputState, HitIndex (O(1) by-id lookup over Cascades)
- `src/renderer/` — frontend (encode/compose) + backend (wgpu) + gpu (Quad/RenderBuffer)
- `src/ui/` — Ui recorder, cascade pass, seen-id tracking, damage
- `src/widgets/` — Button, Frame, Panel (HStack/VStack/ZStack/Canvas), Grid, Text, Styled, Theme
- `examples/{helloworld.rs, showcase/}` — minimal driver + multi-page demo
- `benches/` — criterion (layout, measure_cache); `docs/` — in-flight notes; `DESIGN.md` — full rationale

Key deps: `wgpu`+`winit`, `glyphon`+`cosmic-text`, `glam`, `rustc-hash`, `rayon`, `bytemuck`. Pinned `*` (lockfile is source of truth).

## References

`./references/` has 29 per-framework notes + a cross-cutting synthesis. **Read `references/SUMMARY.md` first** — it indexes every doc, takes positions on Palantir's design choices, lists anti-patterns + open questions. Each per-framework doc cites `tmp/` source with `file:line` and ends with copy/avoid/simplify recommendations. SUMMARY's "Quick-lookup matrix" (§13) maps task → docs.

`./tmp/` (gitignored) holds the source clones, populated by `./scripts/fetch-refs.sh` (re-runnable). Go to `tmp/` only when a reference note doesn't cover the question. Most relevant by topic:

- **Layout / measure-arrange** → `tmp/wpf` (the model we emulate), `tmp/taffy`, `tmp/morphorm`, `tmp/yoga`, `tmp/clay` (arena tree)
- **Immediate-mode patterns** → `tmp/egui`, `tmp/imgui`, `tmp/clay`, `tmp/nuklear`
- **wgpu renderer / batching** → `tmp/egui` (`crates/egui-wgpu`), `tmp/iced` (`wgpu` crate), `tmp/quirky`, `tmp/vello`, `tmp/wgpu`
- **Text** → `tmp/glyphon`, `tmp/cosmic-text`, `tmp/parley`
- **Vector shapes** → `tmp/lyon`, `tmp/kurbo`, `tmp/vello`
- **Reactive / retained Rust UIs (contrast)** → `tmp/iced`, `tmp/xilem`, `tmp/dioxus`, `tmp/floem`, `tmp/slint`, `tmp/makepad`

For dependency API lookups (signatures, version-specific behavior, internal types), grep `tmp/<crate>/src` first — same version Palantir builds against, faster than `cargo doc`. Fall back to `~/.cargo/registry/src/...` only if not in `fetch-refs.sh`.

## Before reporting work as done

Always run, in this order, before confirming any code change:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Fix anything that fails. Don't tell the user a change is complete unless these all pass.

## Finding duplicated code

Before refactoring or hunting for similar code by reading files, run jscpd — it's fast (~500ms) and avoids burning tokens:

```sh
npm_config_cache="$TMPDIR/npm-cache" npx --yes jscpd src/ --min-lines 5 --min-tokens 50 --ignore "**/tests.rs,**/tests/**" --reporters console
```

Drop the `--ignore` to include tests. Reports exact `file:line` ranges for each clone pair.
