# Palantir GUI — Design Doc

A Rust GUI crate: **immediate-mode authoring API**, **WPF-style two-pass layout**, **wgpu rendering**.

## Goals

- Author UIs imperatively each frame (`Button::new().label("x").show(&mut ui)`, `HStack::show(ui, |ui| { ... })`).
- Children auto-size; parents arrange. No manual coordinates.
- Single-frame stable layout (no first-frame jitter).
- wgpu-only renderer; no platform widgets.
- Steady-state allocation-free after warmup — per-frame `Vec::new()`/`HashMap` rebuilds are regressions.

## Core Idea: Record → Measure → Arrange → Cascade → Encode/Paint

Pure immediate-mode hits a paradox: parents need child sizes before placing them, but the user code declares children inside the parent. WPF solves this with retained `Measure(available) → Arrange(final)`. We keep the immediate-mode *call site* but defer layout/paint by **building a transient tree each frame**, then running WPF's two passes plus a cascade and a paint pass on it.

```
user closures ──► [1] Record    (append per-node columns + Shapes; no layout, no paint)
                  [2] Measure   (post-order, bottom-up)  — desired size given available
                  [3] Arrange   (pre-order, top-down)    — parent assigns final Rect to each child
                  [4] Cascade   (pre-order, top-down)    — flatten disabled/visibility/clip/transform
                  [5] Encode + Paint                     — emit RenderCmds, group by scissor, submit
                  [*] Hit-test next frame's input against last frame's cascade
```

The tree is rebuilt every frame but laid out fresh — no stale cached sizes, no jitter. Identity is by stable IDs (`WidgetId`, hashed call-site + user key) so animation/state/focus survive across frames.

**Cascade is its own pass** (not folded into encoder or hit-test) precisely so the encoder *and* the hit-index read the same flattened state — they can't drift on disabled/clipped/transformed subtrees.

## Tree shape

Arena `Tree`, **SoA** — columns indexed by `NodeId.0`:

- `layout: Vec<LayoutCore>` — mode/size/padding/margin/align/visibility (read by measure + arrange).
- `paint: Vec<PaintCore>` — `PaintAttrs` (1-byte sense/disabled/clip) + extras index (read by cascade/encoder/hit-test).
- `widget_ids: Vec<WidgetId>` — hit-test + future state map.
- `subtree_end: Vec<u32>` — pre-order topology, `i + 1 == subtree_end[i]` for a leaf. Drives every walk.
- `shapes: Vec<Shape>` + `shape_starts: Vec<u32>` — per-node paint primitives, sliced flat.

Splitting by reader keeps each pass touching only the columns it needs. Measured `desired`/`rect` live on `LayoutResult` keyed by `NodeId`, **not** on the tree — the tree is input, results are derived.

`Shape` (paint primitive: `RoundedRect`, `Text`, `Line`, …) is stored flat in `Tree.shapes`. `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning. **Layout passes ignore Shapes and `PaintCore`; paint pass ignores hierarchy beyond `subtree_end`.** This decoupling is load-bearing.

## Sizing model (WPF-aligned)

Per-axis `Sizing`:

- **`Fixed(n)`** — outer = exactly `n` (incl. padding). WPF explicit.
- **`Hug`** — outer = content + padding. WPF `Auto`.
- **`Fill(weight)`** — takes leftover, distributed by weight across `Fill` siblings. WPF `*`.

Canonical impl: `resolve_axis_size` in `src/layout/mod.rs`. Pinned by `src/layout/{stack,wrapstack,zstack,canvas,grid}/tests.rs` — change the math, update the tests in the same change.

## Layout dispatch

No `trait Layout`. A `LayoutEngine` dispatches on a `LayoutMode` enum (`Leaf`/`HStack`/`VStack`/`WrapHStack`/`WrapVStack`/`ZStack`/`Canvas`/`Grid`) into per-driver modules under `src/layout/`. Each driver exports a `measure(engine, tree, node, ...) -> Size` and an `arrange(engine, tree, node, ...)`.

**Per-axis `Align` semantics by parent layout mode:**

- `HStack` reads `align_y` (cross axis); ignores `align_x` (main axis position is determined by stack order + gap + justify).
- `VStack` reads `align_x` (cross axis); ignores `align_y`.
- `ZStack` reads both — children are layered, both axes are free.
- `Canvas` ignores both — children are placed at their absolute `position`. Mixing alignment with absolute placement muddles coordinate semantics; if you want centered placement, use `ZStack`.
- `Leaf` has no children, so alignment doesn't apply.

Native panels only — no Taffy, no flex/grid backend dependency. Grid is implemented natively against the same `Sizing` vocabulary.

## Identity

`WidgetId` is hashed from a user-supplied key. Stability across frames is what makes persistent state survive.

- Auto-deriving constructors (`Button::new`, `Text::new`, …) use `WidgetId::auto_stable()` + `#[track_caller]` so calls at different source lines get distinct ids.
- **`#[track_caller]` does not propagate through closure bodies** — helpers that build widgets inside a closure passed to e.g. `Panel::show(ui, |ui| { ... })` resolve every call site to the same closure literal, producing collisions. Inside such helpers, give widgets explicit ids (`Text::with_id((tag, key), text)`, `Button::with_id(...)`); annotating the helper with `#[track_caller]` doesn't help.
- Collisions are detected in debug via `SeenIds` tracking on `Ui`.

## State outside the tree (planned)

Per-widget state (scroll offset, text cursor, animation) is intended to live in a `WidgetId → Any` map. The tree is throwaway; state persists. **Not yet implemented** — `Ui` currently holds input state directly. The infrastructure (stable `WidgetId`, seen-id tracking) is in place; the generic store is next.

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

**Hit-test is rect-only today.** Hit shapes per node (`RoundedRect`/`Path`/`None` for click-through overlays) and explicit layers (`LayerId = (Order, AreaIndex)` with `Background/Main/Foreground/Tooltip/Popup/Debug`) are open extensions — the cascade snapshot can carry per-node hit shapes and a layer field whenever a real workload (rounded buttons rejecting corners, popup ordering) demands them.

## Rendering

Paint pass walks the cascade and emits `Vec<RenderCmd>`. The composer groups draws by scissor, snaps to physical pixels, and submits instanced quads through `WgpuBackend`. Text runs via `glyphon` + `cosmic-text` interleave with quads inside each scissor group, sharing one `TextAtlas` + `SwashCache`.

Single render pass per surface, instanced draws. `wgpu::RenderBundle` for unchanged subtrees is a future optimization; full-redraw is fine until profiling says otherwise.

## Non-Goals (v1)

- Accessibility tree (add later via `accesskit`).
- Animation framework (state map + tween crate is enough).
- Stylesheet language. Inline style structs only.
- Multi-window. Single surface.
- Routed events (tunnel/bubble).

## Open Questions

- **Re-measure on size mismatch during arrange.** WPF allows constrained re-measure. Currently one pass each. If a widget reports a measured-vs-arranged mismatch in practice, add an egui-style `request_discard` second-frame fallback. Not yet motivated.
- **State store API.** `WidgetId → Any` map is committed in principle; the borrowing shape (`&mut Ui` reentrancy, lifetime of borrowed state across child closures) is the part still to design.
- **Hit shapes + layers.** Both proposed above. Adding them is straightforward; deferred until a workload demands non-rect hit-testing or explicit popup ordering.

## Prior Art Worth Studying

- **WPF** — the measure/arrange contract itself.
- **egui** — immediate-mode in Rust; uses prior-frame sizes + `request_discard` for two-pass. We do better by recording first.
- **Clay** (C) — deferred immediate mode; closest analogue to this design.
- **Taffy** — flex/grid/block engine. Considered and declined for v1; native panels stay in core.
- **Quirky** — retained wgpu UI in Rust, for renderer reference.

See `references/SUMMARY.md` for the full per-framework index.
