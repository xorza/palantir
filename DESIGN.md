# Palantir GUI — Design Doc

A Rust GUI crate: **immediate-mode authoring API**, **WPF-style two-pass layout (flex-shrink sizing)**, **wgpu rendering**.

## Goals

- Author UIs imperatively each frame (`Button::new().label("x").show(&mut ui)`, `HStack::show(ui, |ui| { ... })`).
- Children auto-size; parents arrange. No manual coordinates.
- Single-frame stable layout (no first-frame jitter).
- wgpu-only renderer; no platform widgets.
- Steady-state allocation-free after warmup — per-frame `Vec::new()`/`HashMap` rebuilds are regressions.

## Core Idea: Record → Measure → Arrange → Cascade → Encode/Paint

Pure immediate-mode hits a paradox: parents need child sizes before placing them, but the user code declares children inside the parent. WPF solves this with retained `Measure(available) → Arrange(final)`. We keep the immediate-mode *call site* and the WPF measure/arrange *contract* but use **CSS Flexbox-style sizing** within it (children shrink with parent rather than overflowing it — see "Sizing model" below). Layout/paint defers by **building a transient tree each frame**, then running the two passes plus a cascade and a paint pass on it.

```
user closures ──► [1] Record    (append per-node columns + Shapes; no layout, no paint)
                  [*] end_frame finalize (subtree_end rollup + per-node + subtree-rollup hashes)
                  [2] Measure   (post-order, bottom-up)  — desired size given available
                                                          short-circuited per subtree by MeasureCache
                  [3] Arrange   (pre-order, top-down)    — parent assigns final Rect to each child
                  [4] Cascade   (pre-order, top-down)    — flatten disabled/visibility/clip/transform
                                                          + build hit index in same walk
                  [5] Encode + Compose + Paint            — emit RenderCmds (subtree-skip via EncodeCache),
                                                          compose to physical-px quads (ComposeCache),
                                                          submit; damage tri-state (Full/Partial/Skip)
                  [*] Hit-test next frame's input against last frame's cascade
```

The tree is rebuilt every frame but laid out fresh — no stale cached sizes, no jitter. Identity is by stable IDs (`WidgetId`, hashed call-site + user key) so animation/state/focus survive across frames. Cross-frame *work skipping* lives in side caches keyed on `(WidgetId, subtree_hash, available_q)`: identical authoring + identical incoming `available` ⇒ blit last frame's `desired` / `RenderCmd` slice and skip recursion. The tree itself stays throwaway.

**Cascade is its own pass** (not folded into encoder or hit-test) precisely so the encoder *and* the hit-index read the same flattened state — they can't drift on disabled/clipped/transformed subtrees. The hit index is built inside the cascade walk so they share one allocation.

## Tree shape

Arena `Tree`, **SoA** — `records: Soa<NodeRecord>` (via `soa-rs`) indexed by `NodeId.0`. `NodeRecord` packs six logically-disjoint columns into one push site; `soa-rs` lays each field out as its own contiguous slice, so each pass reads only the bytes it needs:

- `layout: LayoutCore` — mode/size/padding/margin/align/visibility (read by measure + arrange as a bundle; all six fields touched together so they stay packed in one column).
- `attrs: PaintAttrs` — 1-byte packed sense/disabled/clip/focusable. Read by cascade / encoder.
- `widget_id: WidgetId` — hit-test, state map, damage diff.
- `end: u32` — pre-order topology, `i + 1 == end` for a leaf. Drives every walk; densest column at 4 B/node.
- `kinds: Span`, `shapes: Span` — encoder span lookups into the kinds stream and shape buffer.

Adjacent storage on the tree, not part of the SoA:

- `kinds: Vec<TreeOp>` — tagged event stream interleaving `NodeEnter` / `Shape` / `NodeExit` in record order; encoder walks it linearly.
- `shapes: Vec<Shape>` — flat shape buffer; per-node ranges live in `records.shapes()[i]`.
- `extras: SparseColumn<ElementExtras>` — out-of-line side table for rare fields (`transform`, `position`, `grid` cell). Sparse: nodes with default extras don't allocate a row.
- `chrome: SparseColumn<Background>` — panel chrome, sparse for the same reason.
- `grid: GridArena` — frame-scoped `Vec<GridDef>` for `LayoutMode::Grid(idx)` panels.
- `hashes: NodeHashes` — `node` (per-node authoring hash), `subtree` (rollup of node + children's subtree hashes), `subtree_has_grid` (fast-path bit). Populated in `end_frame` after `end` rolls up. Keys both the cross-frame measure cache and the encode cache.

Atomic-push across the SoA columns means `open_node` writes all six per-node fields together and they can't drift; the `assert_recording_invariants` length check collapses to a single comparison against the kinds stream. Measured `desired`/`rect`/`text_shapes`/`scroll_content`/`available_q` live on `LayoutResult` keyed by `NodeId`, **not** on the tree — the tree is input, results are derived.

`Shape` (paint primitive: `RoundedRect`, `Text`, `Line`) is stored flat in `Tree.shapes`. `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning. **Layout passes ignore Shapes and `attrs`; paint pass ignores hierarchy beyond `end`.** This decoupling is load-bearing.

## Sizing model (flex-shrink with min-content floor)

Per-axis `Sizing`:

- **`Fixed(n)`** — outer = exactly `n` (incl. padding). Hard contract; can exceed parent's available.
- **`Hug`** — outer = `min(content, available)`, floored at `intrinsic_min`. Shrinks with parent down to the largest rigid descendant, then stops.
- **`Fill(weight)`** — outer = `available`, floored at `intrinsic_min`. With Fill siblings, each gets `leftover * weight / total_weight`; siblings whose floor exceeds their share *freeze* at floor and the remaining leftover redistributes among the rest (CSS Flexbox-style freeze loop).

The "min-content" floor (`intrinsic_min`) is the largest non-shrinkable thing on this axis: a `Fixed(v)` descendant's `v`, an explicit `min_size`, or the longest unbreakable word inside a wrapping `Text`. Computed via `LayoutEngine::intrinsic(node, axis, MinContent)` (cached per `(node, axis, slot)`).

Two ways desired can exceed parent's available:
1. Rigid descendant doesn't fit (`intrinsic_min > available`).
2. Explicit `min_size` or `Fixed(v)` overrides.

When that happens the rect overflows; downstream (cascade/composer/backend) tolerates it, same posture as the root-vs-surface overflow.

This matches CSS Flexbox's default `flex-shrink: 1` with `min-width: auto`. **Departs from WPF**: WPF's `Auto`/`*` allow children to exceed the parent (parent grows or content overflows up the tree). Palantir parents *don't grow* past available — children clamp down to fit. The WPF model created two-pass convergence headaches and didn't match user expectation that "shrinking the window shrinks the UI."

Canonical impl: `resolve_axis_size` in `src/layout/support.rs` (the per-axis math) plus the freeze loop in `src/layout/stack/mod.rs::measure` (the Fill-sibling distribution). Pinned by `src/layout/{stack,wrapstack,zstack,canvas,grid}/tests.rs` and `src/layout/cross_driver_tests/convergence.rs` — change the math, update the tests in the same change.

## Layout dispatch

No `trait Layout`. A `LayoutEngine` dispatches on a `LayoutMode` enum (`Leaf`/`HStack`/`VStack`/`WrapHStack`/`WrapVStack`/`ZStack`/`Canvas`/`Grid(u16)`/`Scroll(ScrollAxes)`) into per-driver modules under `src/layout/`. Each driver exports three free `pub(crate) fn`s — `measure`, `arrange`, `intrinsic` — matched into `LayoutEngine::measure_dispatch`, `arrange`, and `intrinsic::compute`. Adding a driver = new variant + new module + match arms; exhaustive matches catch the missing arms at compile time.

**Single dispatch.** `measure` runs the driver once. The old WPF-style "grow loop" (re-dispatch when content exceeds available) is gone — under flex-shrink semantics, `intrinsic_min` is computed up front, `available` is floored at `intrinsic_min` before dispatch, and `resolve_axis_size` clamps the result. Every driver's content size is monotone in `available`, so a re-dispatch would converge to the same value. Pinned by `cross_driver_tests::convergence`.

**Scroll viewports** measure children with the panned axis fed `INFINITY`, stash the raw extent in `LayoutResult.scroll_content`, and return zero on the panned axis(es) so the parent's hugging falls through to the user's `Sizing` instead of growing with content. End-of-frame, `Ui` refreshes each registered scroll widget's `ScrollState` row (viewport / outer / content) and re-clamps `offset` against the up-to-date numbers.

**Per-axis `Align` semantics by parent layout mode:**

- `HStack` reads `align_y` (cross axis); ignores `align_x` (main axis position is determined by stack order + gap + justify).
- `VStack` reads `align_x` (cross axis); ignores `align_y`.
- `ZStack` reads both — children are layered, both axes are free.
- `Canvas` ignores both — children are placed at their absolute `position`. Mixing alignment with absolute placement muddles coordinate semantics; if you want centered placement, use `ZStack`.
- `Leaf` has no children, so alignment doesn't apply.

Native panels only — no Taffy, no flex/grid backend dependency. Grid is implemented natively against the same `Sizing` vocabulary.

## Identity

`WidgetId` is hashed from a user-supplied key. Stability across frames is what makes persistent state survive.

- Auto-deriving constructors (`Button::new`, `Text::new`, `Panel::hstack`, …) use `WidgetId::auto_stable()` + `#[track_caller]` so calls at different source lines get distinct ids.
- `#[track_caller]` does not propagate through closure bodies, so a helper that builds widgets inside a closure passed to e.g. `Panel::show(ui, |ui| { ... })` resolves every call to the same source location. `Ui::node` handles this by silently disambiguating auto-id collisions via a per-id occurrence counter — loops and closure helpers Just Work. Per-widget state then keys on the disambiguated id and is positional within that callsite, so reordering or conditional insertion re-keys state for the colliding slots. When call order isn't stable across frames, override with `.with_id(key)` (the builder method on `Configure`) where `key` is something stable like a domain id.
- Explicit-key collisions (two `.with_id("same")` calls) hard-assert in `Ui::node` — they're always caller bugs. Auto/explicit is tracked by `Element::auto_id`.
- Collisions and removed-widget diff are tracked by `SeenIds` on `Ui`.

## State outside the tree

Per-widget state (scroll offset, text cursor selection, animation, focus flags) lives in a `WidgetId → Box<dyn Any>` map (`StateMap` on `Ui`). The tree is throwaway; state persists. Access via `Ui::state_mut::<T>(id)` — creates with `T::default()` on first touch, panics on type mismatch (collision = caller bug). Rows for any `WidgetId` not recorded this frame are dropped in `end_frame` via the same `removed` slice fed to `Damage`, `TextShaper`, and `MeasureCache` — one source of truth for "this widget went away."

Focus is a separate field (`InputState.focused: Option<WidgetId>`) since it's global, not per-widget. `FocusPolicy` controls whether pressing on a non-focusable surface clears focus or preserves it.

## Input

Hit-testing happens **as events arrive**, against the cascade snapshot from the most recently rendered frame — i.e., the frame the user was looking at when they clicked. Visuals respond with zero lag (a press updates `pressed` immediately, the next redraw paints it). Click identity is preserved across widget movement via ID-based capture.

**Frame protocol:**

```
handle_event(WindowEvent)   // updates pointer pos + active widget; hit-tests against last cascade.
                            // press → active = hit; release with same hit → click.
begin_frame
build_ui(&mut ui)           // widgets read response_for(id), deriving hovered/pressed/clicked
                            // from live input state + last cascade.
measure → arrange → cascade // produces this-frame rects + flattened state.
end_frame                   // rebuild HitIndex from this-frame cascade; clear clicked_this_frame.
encode + paint
```

**ID-based active capture** for press/release across frames:

- On press: hit-test → set `Active = WidgetId`.
- While Active is set, `pressed = (active == self.id)` — visuals pin to the captured widget regardless of where its rect is now.
- On release: hit-test again. If `hit == Active`, emit `clicked`. Clear Active.
- If Active's WidgetId disappears from the tree (conditional rendering), clear it silently in `end_frame`.

Cases handled:

- Static UI: instant press feedback, click on release.
- Widget moved between press and release: still `pressed` while held (id match overrides rect). Click cancels if release point isn't over the same widget — matches user intent ("I clicked the button that *was* there, but it moved away, so cancel").
- Drag (future): `Active` is the captured widget; pointer-position tracking gives `drag_delta` regardless of rect.

**Trade-off accepted:** hit-test for press/release uses last-frame's cascade. If a widget appeared *just this frame* at the click position, it can't be clicked until next frame. Acceptable; matches every IM library in the corpus.

**Don't bubble events.** Topmost widget at the point handles, then it's done. Routed events (WPF tunnel/bubble) encourage accidental coupling; egui omitted them and never regretted it.

**Hit-test is rect-only today.** Hit shapes per node (`RoundedRect`/`Path`/`None` for click-through overlays) are still an open extension — the cascade snapshot can carry per-node hit shapes whenever a real workload (rounded buttons rejecting corners) demands them.

**Layers are explicit.** `Layer` (`Main`/`Popup`/`Modal`/`Tooltip`/`Debug`) is an enum on the recorder; `Ui::layer(layer, anchor, body)` switches the active arena for the body's duration. The tree is a `Forest` of one `Tree` per layer (`src/forest/mod.rs`); pipeline passes iterate `Layer::PAINT_ORDER` bottom-up for paint and reverse for hit-test, so popups paint above and reject pointers first without per-node z-index. Explicit z-order within a layer (Clay-style `zIndex`) is deferred until two siblings in the same layer need it.

## Rendering

Paint pass walks the cascade and emits a `RenderCmdBuffer` (logical-px). The composer turns commands into physical-px instanced quads grouped by scissor; `WgpuBackend` submits one render pass per surface. Text runs via `glyphon` + `cosmic-text` interleave with quads inside each scissor group, sharing one `TextAtlas` + `SwashCache`. A single `TextShaper` handle (mono fallback for tests, real cosmic shaper for hosts) is shared between `Ui` (via `set_text_shaper`) and `WgpuBackend` so layout-time measurement and render-time shaping hit the same buffer cache; wrapping leaves reshape against the parent-committed width during the bottom-up measure pass, and a `(WidgetId, ordinal)`-keyed reuse cache short-circuits unchanged leaves.

**Subtree-skip caches** mirror the measure cache:

- **Encode cache** — keyed by `(WidgetId, subtree_hash, available_q)`, stores subtree-relative `RenderCmdBuffer` slices. On replay the encoder translates by the *current* frame's `rect.min`, so a cached subtree survives parent origin shifts (scroll, resize, sibling reflow) without invalidating. Bypassed on partial-damage frames since those need per-cmd `screen_rect` filtering.
- **Compose cache** — same key, stores composed quads.

**Damage** is a tri-state (`DamagePaint`): `Full` (re-paint everything), `Partial(rect)` (encoder filters cmds whose screen rect intersects), `Skip` (no diff vs prev frame, no submit). `Ui::invalidate_prev_frame` rewinds the prev-frame snapshot when the host failed to actually present (surface lost / occluded / outdated) so the next `end_frame` is forced to `Full`.

**Debug overlay.** `Ui::debug_overlay: Option<DebugOverlayConfig>` (in `src/ui/debug_overlay.rs`) gates per-frame visualizations: `damage_rect` strokes the damaged region, `clear_damage` flips `Partial` frames' main-pass `LoadOp::Clear` so the undamaged region flashes the clear color (damage scissor still narrows draws). Drawn after the backbuffer→surface copy so they don't ghost across frames.

Single render pass per surface, instanced draws. `wgpu::RenderBundle` for unchanged subtrees is a possible future addition on top of the encode cache.

## Non-Goals (v1)

- Accessibility tree (add later via `accesskit`).
- Animation framework (state map + tween crate is enough).
- Stylesheet language. Inline style structs only.
- Multi-window. Single surface.
- Routed events (tunnel/bubble).

## Open Questions

- **Re-measure on size mismatch during arrange.** WPF allows constrained re-measure. Currently one pass each. If a widget reports a measured-vs-arranged mismatch in practice, add an egui-style `request_discard` second-frame fallback. Not yet motivated.
- **Hit shapes + layers.** Both proposed above. Adding them is straightforward; deferred until a workload demands non-rect hit-testing or explicit popup ordering.
- **Render bundles.** `wgpu::RenderBundle` for unchanged subtrees on top of the encode/compose caches is a candidate when profiling motivates it; the cache key is already in place.

## Prior Art Worth Studying

- **WPF** — the measure/arrange contract itself.
- **egui** — immediate-mode in Rust; uses prior-frame sizes + `request_discard` for two-pass. We do better by recording first.
- **Clay** (C) — deferred immediate mode; closest analogue to this design.
- **Taffy** — flex/grid/block engine. Considered and declined for v1; native panels stay in core.
- **Quirky** — retained wgpu UI in Rust, for renderer reference.

See `references/SUMMARY.md` for the full per-framework index.
