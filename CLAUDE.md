# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass layout with flex-shrink sizing**, **wgpu rendering**.

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

- **Comments:** none except non-obvious *why*. Code is short and self-explanatory; keep it that way. **Be terse.** One short line is the target — never multi-paragraph essays, never narration of what the code does, never "this used to…/we changed it because…" history. If a comment can't fit in one line and still earn its place, delete it.
- **Asserts:** default to release `assert!` for invariants, not `debug_assert!` — `debug_assert!` is stripped in release and hides logic bugs in the build users actually run. Reserve it for checks too expensive for release (e.g. O(n) inside a hot loop), and call out the tradeoff.
- **"Non-paintable scalar" predicate:** when guarding on a magnitude (stroke width, alpha, radius — any scalar where ≤ 0, NaN, or near-zero means "skip / emit nothing"), use `crate::primitives::approx::noop_f32(v)`. It captures all three cases in one comparison and is the shared predicate behind `Stroke::is_noop` / `Color::is_noop` / `Shape::is_noop`. Don't hand-roll `if !(v > 0.0)` or `if v <= 0.0 || v.is_nan()` — they drift apart over time. `approx::EPS = 1e-4`; for sub-`EPS` thresholds use the constant directly.
- **Edition 2024.** Dependencies pinned to `*` for now (lockfile pins actual versions) — fine for prototype, pin before publishing.
- **Tests in `lib.rs` pin layout semantics.** Add a test whenever you change measure/arrange behavior. Don't add wgpu code paths to the layout/tree modules.
- **All test-only code lives in test modules.** No `#[cfg(test)] pub(crate) fn …` on production types. If a test needs internals, expose the field as `pub(crate)` and call production code paths, OR move the test inside the module's `#[cfg(test)] mod tests`. Test-only methods on production types creep, drift, and signal a "real consumer coming any day" that never arrives.
- **Cross-module test/bench observability goes through `support::internals`.** The module is gated `#[cfg(any(test, feature = "internals"))]` at `lib.rs`, so it doesn't ship in production builds. When integration tests, benches, or fixtures need to peek into a private cache or counter (e.g. `text_shaper_measure_calls`, `text_shaper_has_reuse_entry`, `clear_measure_cache`), add a free function in `support/internals.rs` taking `&Ui` / `&TextShaper` / etc. and reading whatever `pub(crate)` field it needs. Production types stay clean (no `#[allow(dead_code)] pub(crate) fn` debug accessors); the gated module owns the reach-in surface. Internal in-tree tests can also use them via `crate::support::internals::*` since `cfg(test)` enables the module.
- **Prefer extending existing tests over adding atomic ones.** When pinning a new invariant, look for a nearby test exercising the same fixture or feature and add the assertion there. Combine related axes into table-driven sweeps (one fixture, multiple cases) instead of one test per case. Refactors that touch a feature then update one or two tests, not a dozen — fewer pin-points to migrate, less duplicated setup, the same coverage. Split into a new test only when a clean fixture for the new behavior would dominate the existing test, or when the failure mode is different enough that one assertion message wouldn't be useful.
- **Split fat-test files** into `foo/{mod.rs, tests.rs}` when tests dominate (>40% or >150 lines).
- **Visibility:** default to narrowest; demote `pub` → `pub(crate)` → private whenever nothing outside uses the item. `pub(crate)` on fields is fine — invariants live in the mutating methods, not in encapsulation theater. No `pub(in path)` / `pub(super)` — exotic noise; use `pub(crate)` for any cross-module access.
- **No trivial accessors — prefer direct field access.** If a method body is just `self.field` / `&self.field` / `self.field = v`, or a one-hop call into a built-in collection method (`self.foo.len()`, `self.foo.is_empty()`, `self.foo.contains_key(k)`), delete it and make the field `pub(crate)`. Same for the inner crate boundary: another module reaching for `cache.snapshots.len()` is fine — don't wrap it in `cache.snapshot_count()`. Inline accessors are fine when they do real work (computation, invariant enforcement, returning a derived view).
- **No tuple returns.** Give a named result struct next to the function. `Option`/`Result` excepted.
- **No inline `crate::foo::bar::Type` paths** in expressions or patterns. Add a `use` at the top — surface dependencies in the imports block, don't bury them.
- **No re-exports inside the crate.** Only `lib.rs` `pub use`s items to define the published surface. Intermediate `mod.rs` files don't re-export — make submodules `pub(crate)` and import via the canonical path (`use crate::primitives::color::Color`, not `use crate::primitives::Color`). One canonical path per item.
- **`bytemuck::Pod` structs use `#[padding_struct::padding_struct]`.** The proc macro injects trailing padding fields so the struct's size is a multiple of its alignment, satisfying `Pod`'s no-padding-bytes invariant. Don't hand-add `_pad: u32` fields — they rot when a new field shifts the layout. Construct via `Self { real_field: x, ..bytemuck::Zeroable::zeroed() }` so the spread fills whatever padding the macro generated; `unsafe { std::mem::zeroed() }` for `const` sentinels. Existing examples: `EnterSubtreePayload` (`src/renderer/frontend/cmd_buffer/mod.rs`), `TextCacheKey` (`src/text/mod.rs`).
- **`WidgetId`** is hashed from a user-supplied key — keep IDs stable across frames so persistent state survives. Auto-deriving constructors (`Button::new`, `Text::new`, `Panel::hstack`, …) use `WidgetId::auto_stable()` + `#[track_caller]` so calls at different source lines get distinct ids. `#[track_caller]` does **not** propagate through closure bodies, so a helper that builds widgets inside a closure passed to e.g. `Panel::show(ui, |ui| { ... })` resolves every call to the same source location — but `Ui::node` silently disambiguates auto-id collisions by mixing in a per-id occurrence counter, so loops and closure helpers Just Work. Per-widget state keys on the disambiguated id and is therefore positional within the colliding call site, so reordering helper calls or conditionally inserting one will re-key state for the affected occurrences. When call order isn't stable, override with `.with_id(key)` (the builder method on `Configure`) where `key` is something stable like the item's domain id. Explicit-key collisions are caller bugs and hard-assert in `Ui::node`.

## Architecture

Five passes per frame on an arena `Tree` rebuilt every frame (with `tree.post_record` finalizing `subtree_end` + per-node + subtree-rollup hashes between record and measure):

1. **Record** — user code (`Button::new().label("x").show(&mut ui)`) appends per-node columns + `Shape`s.
2. **Measure** (post-order) — node returns desired size given available; `MeasureCache` short-circuits whole subtrees on `(WidgetId, subtree_hash, available_q)` hits. Single dispatch (no WPF-style grow loop).
3. **Arrange** (pre-order) — parent assigns final `Rect` to each child.
4. **Cascade** (pre-order) — `Cascades::run` flattens disabled/invisible/clip/transform and builds the hit index in the same walk; consumed by encoder *and* hit-test so they can't drift.
5. **Encode + Compose + Paint** — `Encoder` walks the tree and emits a `RenderCmdBuffer` from scratch each frame; `Composer` groups by scissor, snaps to physical pixels; `WgpuBackend` submits instanced quads. `Damage` returns `Full` / `Partial(rect)` / `Skip` and filters which leaves the encoder paints. No encode or compose caches — both were implemented and removed after profiling (see `src/renderer/frontend/encoder/encode-cache.md`); the encoder + composer are already memcpy-shaped and a per-frame rebuild beat a per-subtree cache replay.

Widget *state* (scroll offset, text cursor, animation) lives in a `WidgetId → Box<dyn Any>` map (`StateMap` on `Ui`). Access via `Ui::state_mut::<T>(id)`; rows for `WidgetId`s not recorded this frame are dropped in `post_record` via the same `removed` slice that `Damage`, `TextShaper`, and `MeasureCache` consume.

**Tree = SoA columns indexed by `NodeId.0`:** `records: Soa<NodeRecord>` (via `soa-rs`) packs five per-node columns — `widget_id` (hit-test + state map + damage), `shape_span: Span` (slice into the flat shape buffer covering this node's subtree), `subtree_end: u32` (pre-order skip; `i + 1 == subtree_end` for a leaf — every walk), `layout: LayoutCore` (mode/size/padding/margin/align/visibility, bundled because measure reads all six together), `attrs: NodeFlags` (1-byte sense/disabled/clip/focusable — cascade/encoder). Adjacent on the tree but outside the SoA: `shapes: Shapes` (flat per-frame `ShapeRecord` buffer + per-variant payloads for `Polyline`/`Mesh`), `parents: Vec<NodeId>` (O(1) parent lookup), and four sparse side tables — `bounds: SparseColumn<BoundsExtras>` (transform / position), `panel: SparseColumn<PanelExtras>` (grid cell, scroll axes), `chrome: SparseColumn<Background>` (panel chrome), `clip_radius: SparseColumn<Corners>` (mask radius for `ClipMode::Rounded`). `rollups: SubtreeRollups` carries per-node + subtree hashes + a `paints` bitset, populated in `post_record`; key for cross-frame caches. soa-rs lays each `NodeRecord` field out as its own contiguous slice, so each pass touches only the columns it needs. Atomic push across the SoA columns means `open_node` writes all five per-node fields together — they can't drift. Measured `desired`/`rect`/`text_shapes`/`scroll_content`/`available_q` live on `LayoutResult` keyed by `NodeId`, not on the tree.

**Cross-frame work-skip cache.** `MeasureCache` (`src/layout/cache/`) is keyed on `(WidgetId, subtree_hash, available_q)`. A hit blits last frame's subtree (`desired` + `text_shapes`) and skips recursion. The `removed` sweep evicts it alongside `StateMap`, `AnimMap`, and `TextShaper`. Encode and compose ran the same keying historically but contributed <1% of frame time; both were removed (see `src/renderer/frontend/encoder/encode-cache.md`). **`Damage` is a tri-state** `DamagePaint` (`Full` / `Partial(Rect)` / `Skip`); `Ui::invalidate_prev_frame` rewinds the prev-frame snapshot when the host failed to actually present.

**Layered recording.** `Forest` (`src/forest/mod.rs`) holds one `Tree` per `Layer` variant (`Main`/`Popup`/`Modal`/`Tooltip`/`Debug`); `Ui::layer(layer, anchor, body)` switches the active arena for the body's duration. Pipeline passes iterate `Layer::PAINT_ORDER` bottom-up for paint and reverse for hit-test (topmost-first, so popups reject pointers without per-node z-index). `Popup` widget (`src/widgets/popup.rs`) is the canonical consumer.

**`Shape`** (paint primitive: `RoundedRect`, `Line`, `Polyline`, `CubicBezier`, `QuadraticBezier`, `Text`, `Mesh`) lowered at authoring time into `ShapeRecord`s in `Tree.shapes.records`, sliced per-node via `records.shape_span()[i]` (a `Span` into the buffer); `Polyline`/`Mesh` payloads live in `Tree.shapes.payloads`. `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning. Layout passes ignore Shapes and `attrs`; paint pass ignores hierarchy beyond `subtree_end`. **This decoupling is load-bearing — keep it.**

**Colour pipeline.** Linear-RGB f32 everywhere on the CPU side; sRGB encoding happens on the GPU at swapchain write. Specifically: `Color { r, g, b, a: f32 }` (`src/primitives/color.rs`) stores **premultiplied-friendly linear-RGB** values in 0..1 (or >1 for HDR-shaped tween outputs). User-facing constructors `Color::rgb(r,g,b)` / `Color::hex(0x...)` / `Color::rgb_u8(...)` interpret their input as **sRGB perceptual** and linearise via a cubic Hejl-Burgess-Dawson fit (`srgb_to_linear`, max error ~1.5e-3 — pinned by `cubic_srgb_max_error_under_two_thousandths`). `Color::linear_rgb` / `Color::linear_rgba` skip the linearisation for tween outputs and physically-derived values. Storage in `Background`, `Stroke`, `Brush::Solid`, `Quad`, etc. is always linear. All AA / blend / `Animatable::lerp` math runs in linear. The render surface is configured to an sRGB texture format (`is_srgb()` pick in `examples/showcase/main.rs`); the quad pipeline uses `BlendState::PREMULTIPLIED_ALPHA_BLENDING` and the fragment shader writes premultiplied linear-RGB (`vec4(rgb * a, a)` in `quad.wgsl`) — the GPU does the linear→sRGB encode automatically because the render target is sRGB-format. **Don't write sRGB-encoded values into `Color`** (skips the linearisation contract); use `Color::rgb`/`hex` for sRGB-perceptual input, `linear_rgb` for already-linear input. `Srgb8` (`src/primitives/color.rs`) is a 4-byte sRGB-packed storage type for places where 8-bit display precision suffices and footprint matters (currently `Stop.color` in gradients). Conversion via `From<Color>` / `From<Srgb8>` uses the same cubic-Newton inverse pair. The future gradient LUT atlas (slice 2, see `docs/roadmap/brushes-slice-2-plan.md`) will use `Rgba8UnormSrgb` texture format storing baked stops as straight (non-premul) sRGB bytes; sampling auto-decodes back to linear, the shader handles premul.

**Sizing (flex-shrink with min-content floor):** `Fixed(n)` = exactly `n` (hard contract; can exceed parent's available). `Hug` = `min(content, available)` floored at `intrinsic_min`. `Fill(weight)` = `available` floored at `intrinsic_min`; with Fill siblings, each gets `leftover * weight / total_weight`, but a sibling whose floor exceeds its share *freezes* at floor and the rest re-divide (CSS Flexbox-style). The `intrinsic_min` floor is the largest non-shrinkable thing on this axis (Fixed descendant, explicit `min_size`, longest unbreakable word). Children clamp DOWN to fit parent — no WPF-style parent-grow. Overflow only happens when rigid descendants don't fit; downstream tolerates it. Canonical impl: `resolve_axis_size` in `src/layout/support.rs` + freeze loop in `src/layout/stack/mod.rs::measure`. Pinned by `src/layout/{stack,wrapstack,zstack,canvas,grid}/tests.rs` and `src/layout/cross_driver_tests/convergence.rs`.

## Project layout

- `src/primitives/` — pure geometry: Rect/Size/Color/Stroke/Corners/Spacing/Transform/bezier/mesh/num/approx/urect/stroke_tessellate
- `src/shape.rs` — Shape enum (RoundedRect, Line, Polyline, CubicBezier, QuadraticBezier, Text, Mesh)
- `src/forest/` — `Forest` (per-layer arenas, `mod.rs`), `tree/` (per-layer `Tree`: SoA + sparse columns + `Shapes` + `GridArena` + `SubtreeRollups`, `NodeId`, `Layer`), `element/` (Element builder, `LayoutCore`/`NodeFlags`/`LayoutMode`, `Configure`), `node.rs` (`NodeRecord`), `rollups.rs` (per-node + subtree hashes + `paints` bitset), `shapes.rs` (`ShapeRecord` + payload arenas), `seen_ids.rs`, `visibility.rs`, `widget_id.rs`
- `src/text/` — `TextShaper` (cosmic-text measurement + per-`(WidgetId, ordinal)` reuse cache) + glyphon rendering glue; mono fallback for headless
- `src/layout/` — LayoutEngine + drivers (stack/wrapstack/zstack/canvas/grid/scroll), intrinsic, cache; `layout/types/` (Sizing/Align/Justify/Sense/Visibility/Display/Track/Span/GridCell — layout vocabulary)
- `src/input/` — InputState, HitIndex (O(1) by-id lookup over Cascades)
- `src/renderer/` — frontend (encode/compose) + backend (wgpu) + gpu (Quad/RenderBuffer)
- `src/ui/` — Ui recorder, cascade pass, seen-id tracking, damage, `DebugOverlayConfig` (per-frame damage-rect / clear-damage visualizations)
- `src/widgets/` — Button, Frame, Panel (HStack/VStack/ZStack/Canvas), Grid, Text, TextEdit, Scroll, Popup, Theme
- `src/animation/` — `Animatable` trait + tween/spring drivers (state-map keyed); `anim-derive/` workspace member provides `#[derive(Animatable)]`
- `src/common/` — shared scaffolding: `CacheArena`, `SparseColumn`, hashing helpers
- `src/support/` — `internals` (cfg-gated test/bench reach-in surface) + `testing` fixtures
- `examples/{helloworld.rs, showcase/}` — minimal driver + multi-page demo
- `benches/` — criterion (layout, measure_cache); `docs/` — in-flight notes; `DESIGN.md` — full rationale

Key deps: `wgpu`+`winit`, `glyphon`+`cosmic-text`, `glam`, `rustc-hash`, `rayon`, `bytemuck`, `soa-rs` (per-node SoA storage on `Tree`). Pinned `*` (lockfile is source of truth).

## References

`./references/` has 29 per-framework notes + a cross-cutting synthesis. **Read `references/SUMMARY.md` first** — it indexes every doc, takes positions on Palantir's design choices, lists anti-patterns + open questions. Each per-framework doc cites `tmp/` source with `file:line` and ends with copy/avoid/simplify recommendations. SUMMARY's "Quick-lookup matrix" (§13) maps task → docs.

**Use `./tmp/` for any in-project scratch — log captures, traces, intermediate
build artifacts.** The whole directory is gitignored (`**/tmp/`) and lives
inside the project root, so writes don't trigger the "out-of-tree access"
permission prompt that `/tmp/` does. Reuse a stable filename
(`tmp/showcase.log`, `tmp/trace-foo.txt`) so the latest run overwrites
the previous; don't accumulate dated artifacts.

The same `./tmp/` also holds the reference source clones, populated by
`./scripts/fetch-refs.sh` (re-runnable). Go to `tmp/<crate>/` only when
a reference note doesn't cover the question. Most relevant by topic:

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

For changes that touch feature-gated code (anything under
`#[cfg(feature = ...)]`, exposed via `support::internals`, or that
might be affected by `internals`/`bench-deep`), run the full feature
matrix instead:

```sh
scripts/test-all.sh       # fmt + clippy + tests across all feature combos
FAST=1 scripts/test-all.sh # skip fmt + clippy, just run tests per combo
```

Fix anything that fails. Don't tell the user a change is complete unless these all pass.

## Finding duplicated code

Before refactoring or hunting for similar code by reading files, run jscpd — it's fast (~500ms) and avoids burning tokens:

```sh
npm_config_cache="$TMPDIR/npm-cache" npx --yes jscpd src/ --min-lines 5 --min-tokens 50 --ignore "**/tests.rs,**/tests/**" --reporters console
```

Drop the `--ignore` to include tests. Reports exact `file:line` ranges for each clone pair.
