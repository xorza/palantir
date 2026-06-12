# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass layout with flex-shrink sizing**, **wgpu rendering**.

Read the **Architecture** section below for the full design rationale before making non-trivial changes.

## Posture

State-of-the-art UI framework, craft-driven. **No external consumers, no published API** — treat it as sports programming.

- **Break things freely.** Rename, refactor, big-bang migrations welcome — no deprecation shims, compat aliases, feature flags, or migration helpers. Bar is "fmt + clippy + tests pass and the showcase still feels right by eye."
- **Per-frame allocation is a real metric.** Steady-state must be heap-alloc-free after warmup. New per-frame `Vec::new()` / `HashMap` rebuild = regression; push onto retained scratch with capacity reuse.
- **API ergonomics matter.** Builder chains read like prose, defaults are right, surprise behavior gets a pinning test. When in doubt, prioritize call-site readability.
- **Optimize aggressively when motivated.** Micro-wins (struct packing, const fns, scratch reuse, cache layout) are encouraged even without a workload demanding them.
- **Ship in measurable slices.** One feature with tests + a showcase tab beats a half-finished cluster. If a change is structurally complex with no motivating workload, say "too early" and shelve with a note rather than ship speculation.
- **Docs are starting positions, not commitments.** Treat `docs/*.md`, `references/*`, and this file as evolving and possibly wrong. When a doc contradicts user intent or current code, double-question rather than defer — flag the conflict, ask, and update the doc.

## Code style

- **"Non-paintable scalar" predicate:** when guarding on a magnitude (stroke width, alpha, radius — any scalar where ≤ 0, NaN, or near-zero means "skip / emit nothing"), use `crate::primitives::approx::noop_f32(v)`. It captures all three cases in one comparison and is the shared predicate behind `Stroke::is_noop` / `Color::is_noop` / `Shape::is_noop`. Don't hand-roll `if !(v > 0.0)` or `if v <= 0.0 || v.is_nan()` — they drift apart over time. `approx::EPS = 1e-4`; for sub-`EPS` thresholds use the constant directly.
- **Tests in `lib.rs` pin layout semantics.** Add a test whenever you change measure/arrange behavior. Don't add wgpu code paths to the layout/tree modules.
- **`bytemuck::Pod` structs use `#[padding_struct::padding_struct]`.** The proc macro injects trailing padding fields so the struct's size is a multiple of its alignment, satisfying `Pod`'s no-padding-bytes invariant. Don't hand-add `_pad: u32` fields — they rot when a new field shifts the layout. Construct via `Self { real_field: x, ..bytemuck::Zeroable::zeroed() }` so the spread fills whatever padding the macro generated; `unsafe { std::mem::zeroed() }` for `const` sentinels. Existing examples: `DrawPolylinePayload` / `DrawMeshPayload` (`src/renderer/frontend/cmd_buffer/mod.rs`), `TextCacheKey` (`src/text/mod.rs`).
- **`WidgetId`** is hashed from a user-supplied key — keep IDs stable across frames so persistent state survives. Auto-deriving constructors (`Button::new`, `Text::new`, `Panel::hstack`, …) use `WidgetId::auto_stable()` + `#[track_caller]` so calls at different source lines get distinct ids. `#[track_caller]` does **not** propagate through closure bodies, so a helper that builds widgets inside a closure passed to e.g. `Panel::show(ui, |ui| { ... })` resolves every call to the same source location — but `Ui::node` silently disambiguates auto-id collisions by mixing in a per-id occurrence counter, so loops and closure helpers Just Work. Per-widget state keys on the disambiguated id and is therefore positional within the colliding call site, so reordering helper calls or conditionally inserting one will re-key state for the affected occurrences. When call order isn't stable, override with `.with_id(key)` (the builder method on `Configure`) where `key` is something stable like the item's domain id. Explicit-key collisions are caller bugs: `SeenIds::record` disambiguates them the same way as auto ids and pushes a `CollisionRecord` onto `forest.collisions`. After the regular paint walk, the encoder (`encoder::emit_collision_overlays`) emits a magenta 3px stroke quad over each colliding node's arranged rect — unclipped, on top of every layer. Always on, no opt-in flag.

## Architecture

Five passes per frame on an arena `Tree` rebuilt every frame (with `tree.post_record` finalizing `subtree_end` + per-node + subtree-rollup hashes between record and measure):

1. **Record** — user code (`Button::new().label("x").show(&mut ui)`) appends per-node columns + `Shape`s.
2. **Measure** (post-order) — node returns desired size given available; `MeasureCache` short-circuits whole subtrees on `(WidgetId, subtree_hash, available_q)` hits. Single dispatch (no WPF-style grow loop).
3. **Arrange** (pre-order) — parent assigns final `Rect` to each child.
4. **Cascade** (pre-order) — `CascadesEngine::run` flattens disabled/invisible/clip/transform and builds the hit index in the same walk, producing a frozen `Cascades` result (`src/ui/cascade/`) consumed by damage diff, hit-test, _and_ the encoder so they can't drift.
5. **Encode + Compose + Paint** — `Encoder` walks the tree and emits a `RenderCmdBuffer` from scratch each frame; `Composer` groups by scissor, snaps to physical pixels; `WgpuBackend` submits instanced quads. `Damage` returns `Full` / `Partial(rect)` / `Skip` and filters which leaves the encoder paints. No encode or compose caches — both were implemented and removed after profiling; the encoder + composer are already memcpy-shaped and a per-frame rebuild beat a per-subtree cache replay.

Widget _state_ (scroll offset, text cursor, animation) lives in a `WidgetId → Box<dyn Any>` map (`StateMap` on `Ui`). Access via `Ui::state_mut::<T>(id)`; rows for `WidgetId`s not recorded this frame are dropped in `post_record` via the same `removed` slice that `Damage`, `TextShaper`, and `MeasureCache` consume.

**Tree = SoA columns indexed by `NodeId.0`:** `records: Soa<NodeRecord>` (via `soa-rs`) packs five per-node columns — `widget_id` (hit-test + state map + damage), `shape_span: Span` (slice into the flat shape buffer covering this node's subtree), `subtree_end: SubtreeEnd` (a `u32` newtype; pre-order skip — `i + 1 == subtree_end` for a leaf — every walk), `layout: LayoutCore` (mode/size/padding/margin/align/visibility, bundled because measure reads all six together), `attrs: NodeFlags` (1-byte sense/disabled/clip/focusable — cascade/encoder). Adjacent on the tree but outside the SoA: `shapes: Shapes` (flat per-frame `ShapeRecord` buffer; variable-length payloads for `Polyline`/`Mesh`/gradients live on `FrameArena`), and a packed per-node `extras_idx: Vec<ExtrasIdx>` whose three `Slot` fields (`bounds`, `panel`, `chrome`) niche-encode `u16::MAX` for absent and otherwise index dense `bounds_table: Vec<BoundsExtras>` (transform / position), `panel_table: Vec<PanelExtras>` (grid cell, scroll axes), and `chrome_table: Vec<ChromeRow>` (panel chrome **plus** mask radius for `ClipMode::Rounded` — a `ChromeRow` is allocated even when paint is `is_noop` so the encoder can read the radius for the stencil-mask path). `paint_anims: PaintAnims` is a shape-keyed registry for paint-only animations (alpha mods today); `rollups: SubtreeRollups` carries per-node + subtree hashes, populated in `post_record`; key for cross-frame caches. soa-rs lays each `NodeRecord` field out as its own contiguous slice, so each pass touches only the columns it needs. Atomic push across the SoA columns means `open_node` writes all five per-node fields together — they can't drift. Measured `desired`/`rect`/`text_shapes`/`scroll_content`/`available_q` live on `LayoutResult` keyed by `NodeId`, not on the tree.

**Cross-frame work-skip cache.** `MeasureCache` (`src/layout/cache/`) is keyed on `(WidgetId, subtree_hash, available_q)`. A hit blits last frame's subtree (`desired` + `text_shapes`) and skips recursion. The `removed` sweep evicts it alongside `StateMap`, `AnimMap`, and `TextShaper`. Encode and compose ran the same keying historically but contributed <1% of frame time; both were removed. **`Damage`** is `enum Damage { Skip, Full, Partial(DamageRegion) }`; `Damage::Skip` is the "nothing changed, just present" skip signal. `Ui::invalidate_prev` rewinds the prev-frame snapshot when the host failed to actually present.

**Layered recording.** `Forest` (`src/forest/mod.rs`) holds one `Tree` per `Layer` variant (`Main`/`Popup`/`Modal`/`Tooltip`/`Debug`); `Ui::layer(layer, anchor, body)` switches the active arena for the body's duration. Pipeline passes iterate `Layer::PAINT_ORDER` bottom-up for paint and reverse for hit-test (topmost-first, so popups reject pointers without per-node z-index). `Popup` widget (`src/widgets/popup.rs`) is the canonical consumer.

**`Shape`** (paint primitive: `RoundedRect`, `Line`, `Polyline`, `CubicBezier`, `QuadraticBezier`, `Text`, `Mesh`, `Image`, `Shadow`) lowered at authoring time into `ShapeRecord`s in `Tree.shapes.records`, sliced per-node via `records.shape_span()[i]` (a `Span` into the buffer); variable-length payloads (polyline points/colors, mesh verts/indices, gradients) live on `FrameArena`. `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning. Layout passes ignore Shapes and `attrs`; paint pass ignores hierarchy beyond `subtree_end`. **This decoupling is load-bearing — keep it.**

**Colour pipeline.** Linear-RGB f32 everywhere on the CPU side; sRGB encoding happens on the GPU at swapchain write. Specifically: `Color { r, g, b, a: f32 }` (`src/primitives/color.rs`) stores **straight-alpha linear-RGB** values in 0..1 (or >1 for HDR-shaped tween outputs). User-facing constructors `Color::rgb(r,g,b)` / `Color::hex(0x...)` / `Color::rgb_u8(...)` interpret their input as **sRGB perceptual** and linearise via a cubic Hejl-Burgess-Dawson fit (`srgb_to_linear`, max error ~1.5e-3 — pinned by `cubic_srgb_max_error_under_two_thousandths`). `Color::linear_rgb` / `Color::linear_rgba` skip the linearisation for tween outputs and physically-derived values. Storage in `Background`, `Stroke`, `Brush::Solid`, `Quad`, etc. is always linear. All AA / blend / `Animatable::lerp` math runs in linear. The render surface is configured to an sRGB texture format (`is_srgb()` pick in `src/winit_host/gpu.rs`); **every pipeline** (quad / mesh / image) uses `BlendState::PREMULTIPLIED_ALPHA_BLENDING`. **Shader contract: straight-alpha linear in, premultiplied linear out** — `From<Color> for ColorU8` is a straight-alpha quantize, so vertex / instance colours arriving at the shader are straight (`rgb`, `a` independent); the fragment shader writes `vec4(rgb * a, a)` so the premul blend equation receives correctly-shaped source. The GPU does the linear→sRGB encode automatically because the render target is sRGB-format. **Don't write sRGB-encoded values into `Color`** (skips the linearisation contract); use `Color::rgb`/`hex` for sRGB-perceptual input, `linear_rgb` for already-linear input. `ColorU8` (`src/primitives/color.rs`) is a 4-byte **linear-u8** storage type for places where 8-bit linear precision suffices and footprint matters (currently `Stop.color` in gradients). Default `From<Color>` / `From<ColorU8>` are straight linear quantize pairs — no sRGB encode. For the sRGB-encoded form (CSS-style hex input) call `Color::to_srgb_u8` or use `ColorU8::hex` / `hexa` explicitly. The gradient LUT atlas uses `Rgba16Float` (linear, no auto-decode) with `ColorF16` row texels (`gradient_atlas::LutRowTexels`); the shader samples and handles premul directly. f16, not 8-bit linear: a dark stop linearises to a tiny value (`#1a1a2e`'s red ≈ 3/255), so an 8-bit *linear* row crushes the dark half of a dark→bright gradient onto ~16 integer levels and bands visibly — f16's ~11-bit mantissa at that magnitude keeps the row smooth (pinned by `gradient_atlas::tests::dark_gradient_row_has_no_banding`).

**Sizing (flex-shrink with min-content floor):** `Fixed(n)` = exactly `n` (hard contract; can exceed parent's available). `Hug` = `min(content, available)` floored at `intrinsic_min`. `Fill(weight)` = `available` floored at `intrinsic_min`; with Fill siblings, each gets `leftover * weight / total_weight`, but a sibling whose floor exceeds its share _freezes_ at floor and the rest re-divide (CSS Flexbox-style). The `intrinsic_min` floor is the largest non-shrinkable thing on this axis (Fixed descendant, explicit `min_size`, longest unbreakable word). Children clamp DOWN to fit parent — no WPF-style parent-grow. Overflow only happens when rigid descendants don't fit; downstream tolerates it. **A stack measures its children against its own committed main extent, not `∞`** — so a `Fixed`/`max_size` bound on any ancestor flows down and constrains descendants (CSS `max-height`/`max-width` semantics, not WPF's free-stacking-axis); an _unbounded_ stack still passes `∞` on its main axis, so children report their natural main size and the stack grows to fit. This is what lets a nested `WrapVStack`/`WrapHStack` wrap (or a `Scroll` bound) against a capped ancestor without a cap of its own. Canonical impl: `resolve_axis_size` in `src/layout/support.rs` + the Pass-1 `main_avail` measure + freeze loop in `src/layout/stack/mod.rs::measure`. Pinned by `src/layout/{stack,wrapstack,zstack,canvas,grid}/tests.rs` and `src/layout/cross_driver_tests/convergence.rs`.

**Stack vs. Grid Fill — same contract, deliberately different solvers.** Both resolve `Fill` as "weighted leftover, each child clamped to `[intrinsic_min, max]`, violators freeze and the rest re-divide" — but with different freeze cadences: `stack::freeze_distribute` freezes *every* violator per pass, `grid::resolve_axis` Phase 3 uses constraint-by-exclusion. For *mixed* min/max violations on the same axis the two can converge to different sizes. This is an **accepted, hand-synced divergence** (see the doc comment on `stack::freeze_distribute`), not a bug to silently "DRY up": a shared solver would change one driver's edge-case output, so it needs a deliberate target-semantics decision first, not a refactor. The common cases (all-min or all-max violations, single Fill, no violations) agree.

## Project layout

- `src/primitives/` — pure geometry + leaf types: Rect/Size/Color (incl. `ColorU8`/`ColorF16`)/Stroke/Corners/Spacing/Transform/Background/Brush/Shadow/Image/Mesh/WidgetId/bezier/num/approx/urect/span/half_simd/interned_str/lane_serde/paint (`LutRow` + paint wire types)
- `src/shape.rs` — Shape enum (RoundedRect, Line, Polyline, CubicBezier, QuadraticBezier, Text, Mesh, Image, Shadow)
- `src/forest/` — `Forest` + `Layer` enum + `CollisionRecord` list (per-layer arenas, `mod.rs`), `tree/` (per-layer `Tree`: SoA records + packed `ExtrasIdx` + dense `bounds_table`/`panel_table`/`chrome_table` (`ChromeRow` holds chrome+`ClipMode::Rounded` radius) + `Shapes` + `GridArena` + `SubtreeRollups` + `PaintAnims`, `NodeId`), `element/` (Element builder, `LayoutCore`/`NodeFlags`/`LayoutMode`, `Configure`), `node.rs` (`NodeRecord`/`SubtreeEnd`), `frame_arena.rs` (`FrameArena` — variable-length shape payloads), `per_layer.rs` (`PerLayer`), `rollups.rs` (per-node + subtree hashes), `shapes/` (`ShapeRecord` + add/clear), `seen_ids.rs`, `visibility.rs`
- `src/text/` — `TextShaper` (cosmic-text measurement + per-`(WidgetId, ordinal)` reuse cache) + the rendering glue against `src/renderer/backend/text/`; mono fallback for headless
- `src/layout/` — LayoutEngine + drivers (stack/wrapstack/zstack/canvas/grid/scroll), intrinsic, cache; `layout/types/` (Sizing/Align/Justify/Display/Track/GridCell/ClipMode — layout vocabulary; `Sense` lives in `src/input/sense.rs`, `Visibility` in `src/forest/visibility.rs`, `Span` in `src/primitives/span.rs`)
- `src/input/` — InputState, keyboard/pointer/sense/shortcut/subscriptions/policy (per-frame hit lookup goes through the frozen `Cascades` result directly — `Ui::on_input(event, &Cascades)` calls `cascades.hit_test*`; no separate `HitIndex` type)
- `src/renderer/` — `frontend/` (encode/compose) + `backend/` (wgpu, including `backend/text/` — the **custom wgpu text rendering backend**: glyph atlas, batch shape, GPU upload path through palantir's staging belt + `DynamicBuffer`) + top-level GPU types `quad.rs` (`Quad`) / `render_buffer.rs` (`RenderBuffer`) / `gradient_atlas.rs` (`GradientAtlas`, `Rgba16Float` LUT) / `image_registry.rs` (image-upload lifecycle) / `caches.rs` + `stroke_tessellate/` (polyline → fringe-AA mesh)
- `src/ui/` — Ui recorder, cascade pass, seen-id tracking, damage, frame state/report/stats, state map
- `src/widgets/` — Button, Checkbox, RadioButton, ToggleSwitch, Slider, DragValue, Spinner, ProgressBar, Separator, Frame, Panel (HStack/VStack/ZStack/Canvas), Grid, Text, TextEdit, ComboBox, Scroll, Popup, Modal, Tooltip, ContextMenu, Theme (+ internal `toggle` shared toggle-look/interaction helper behind Checkbox/Switch); `widgets/tests/` (cross-widget integration tests)
- `src/animation/` — value-interpolation animation only: `Animatable` trait + tween/spring drivers (state-map keyed); `anim-derive/` workspace member provides `#[derive(Animatable)]`. Paint-only (shape-keyed) animations (`PaintAnim`/`PaintMod`) live with their per-tree registry in `src/forest/tree/paint_anims.rs`, sampled at encode time
- `src/common/` — shared scaffolding: `LiveArena` (cross-frame cache backing, `live_arena.rs`), hashing helpers, platform/time shims, `clipboard.rs` (process-wide clipboard: arboard + in-memory fallback, `get`/`set`, used by `TextEdit`). (`FrameArena`/`PerLayer` live in `src/forest/`, not here.)
- **Test/bench reach-in surface:** per-module `#[cfg(any(test, feature = "internals"))] pub mod test_support` blocks (no `src/support/` aggregator). Top-level modules are `pub` so external benches/integration tests can reach them as `palantir::foo::bar::test_support::*`.
- `src/window_renderer.rs` — `WindowRenderer`: per-window state (its `Ui` recorder + a per-window `Frontend` encode/compose scratch + the persistent `Backbuffer` + frame-scheduling/occlusion clock) that drives the **one shared** `WgpuBackend` (passed `&mut` into every method). N windows render through one GPU renderer; each `WindowRenderer` carries only this window's data. Also holds the `FramePresent` scheduling enum; public entry `WindowRenderer::frame` (swapchain). Headless render-to-`wgpu::Texture` for benches / the visual harness goes through `src/offscreen_host.rs` — `OffscreenHost`, the internals-gated offscreen peer of `WinitHost` (bundles the `pub(crate)` `WgpuBackend` + one `WindowRenderer` behind a `pub` facade). The shared `WgpuBackend` (built from a `RenderContext`) lives in `src/renderer/backend/` and owns the per-format pipeline map (`HashMap<TextureFormat, FormatPipelines>`, lazy), the glyph + gradient atlases, the image texture cache, the device/queue, and the shared frame arena / render caches / shaper / GPU-stats handle (cloned into each window's `Ui`/`Frontend`).
- `src/winit_host/` — `WinitHost<T>` + `WinitHostConfig`: winit `ApplicationHandler` glue. Owns the one shared `WgpuBackend` plus N `WindowRenderer`s (one per OS window), routes events by `WindowId`, picks sRGB swapchain, maps each window's `FramePresent` → one `ControlFlow`, forwards `WindowEvent`s to `Ui::on_input`. Config exposes title, initial/min logical size, present mode, power preference. The swapchain is always double-buffered (`desired_maximum_frame_latency: 1`).
- `src/debug_overlay.rs` — `DebugOverlayConfig` on `Ui` (damage-rect / clear-damage / frame-stats visualizations)
- `src/showcase/` — multi-page demo content; `src/main.rs` — showcase binary (uses `WinitHost`)
- `examples/` — `dump_theme` (theme TOML round-trip), `counter`, `frame_visual`
- `tests/alloc/` — per-frame allocation audit suite (a `CountingAllocator` global allocator + shared fixtures/harness; see `tests/alloc/alloc-testing.md`); integration-level sibling to the `alloc_free` bench, run via `cargo test --test alloc`
- `tests/visual/` — headless wgpu → PNG → golden-diff suite (`cargo test --test visual --features internals`); the canonical eyeball-replacement for rendering changes. Golden PNGs are gitignored and per-machine (auto-created on first run); after an intentional render change, regenerate with `UPDATE_GOLDEN=1 cargo test --test visual --features internals <filter>` and inspect the diff artifacts under `tests/visual/output/<name>/` first. Full reference: `tests/visual/visual-testing.md`
- `benches/` — criterion (alloc_free, alloc_free_gpu, alloc_resize, caches, damage, frame, input_throughput, scrollzoom, text_atlas; only `alloc_free`, `alloc_resize`, `input_throughput` build without `--features internals`); `docs/` — in-flight notes + `roadmap/` (per-feature design notes); the **Architecture** section above is the full rationale

Key deps: `wgpu`+`winit`, `cosmic-text` (the wgpu text rendering backend lives in-tree at `src/renderer/backend/text/`), `glam`, `rustc-hash`, `rayon`, `bytemuck`, `soa-rs` (per-node SoA storage on `Tree`). Pinned `*` (lockfile is source of truth).

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
- **Text** → `tmp/cosmic-text`, `tmp/parley`
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
might be affected by `internals`), run the full feature
matrix instead:

```sh
scripts/test-all.sh       # fmt + clippy + tests across all feature combos
FAST=1 scripts/test-all.sh # skip fmt + clippy, just run tests per combo
```

For changes that touch **rendering** (shaders, encoder/composer, gradient
or text atlas, colour pipeline, layout that moves pixels), also run the
visual suite — `cargo test` alone won't catch a render regression:

```sh
cargo test --test visual --features internals
```

If goldens legitimately move (an intentional visual change), inspect the
`tests/visual/output/<name>/{actual,expected,diff}.png` artifacts, then
regenerate with `UPDATE_GOLDEN=1` and re-run to confirm green.

Fix anything that fails. Don't tell the user a change is complete unless these all pass.

## Hot-path struct sizes

`src/lib.rs` has an `#[ignore]`-d test, `hot_struct_sizes::print_hot_struct_sizes`,
that prints `size_of` / `align_of` for every per-frame struct touched
by layout / cascade / encode / compose / damage. Run it with:

```sh
cargo test --lib print_hot_struct_sizes -- --nocapture --ignored
```

When changing any hot row (`NodeRecord`, `LayoutCore`, `ShapeRecord`,
`Brush`, `DrawRectPayload`, `CascadeInputHash`, `DamageRegion`, `Quad`, etc.)
re-run the test and eyeball the printed sizes against the previous run
to catch regressions.
