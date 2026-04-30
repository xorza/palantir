# Palantir GUI — Design Doc

A Rust GUI crate: **immediate-mode authoring API**, **WPF-style two-pass layout**, **wgpu rendering**.

## Goals

- Author UIs imperatively each frame (`ui.button(...)`, `ui.stack(...)`).
- Children auto-size; parents arrange. No manual coordinates.
- Single-frame stable layout (no first-frame jitter).
- wgpu-only renderer; no platform widgets.

## Core Idea: Record → Measure → Arrange → Paint

Pure immediate-mode hits a paradox: parents need child sizes before placing them, but the user code declares children inside the parent. WPF solves this with retained `Measure(available) → Arrange(final)`. We keep the immediate-mode *call site* but defer layout/paint by **building a transient tree each frame**, then running WPF's two passes on it.

```
user closures ──► [1] Record tree (no layout)
                  [2] Measure pass (post-order, bottom-up): each node returns desired size given available
                  [3] Arrange pass (pre-order, top-down): parent assigns final rect to each child
                  [4] Paint pass: emit wgpu draw commands from final rects
                  [5] Hit-test next frame's input against last frame's rects
```

The tree is rebuilt every frame but laid out fresh — no stale cached sizes, no jitter. Identity is by stable IDs (hashed call-site + user key) for animation/state/focus continuity.

## Key Directions

### 1. Tree recording, not direct draw
Widget calls push nodes into an arena (`Vec<Node>` with parent/child indices). No painting happens during user code. This is the critical departure from Dear ImGui / egui's "draw as you go" — and the reason two-pass layout works cleanly. Closures for containers (`ui.stack(|ui| { ... })`) run during recording to populate children.

### 2. WPF Measure/Arrange contract
Each widget implements:
```rust
trait Layout {
    fn measure(&self, ctx, available: Size, children: &mut [NodeId]) -> Size; // desired
    fn arrange(&self, ctx, final_rect: Rect, children: &mut [NodeId]);         // assigns child rects
}
```
- `available` may be `Infinity` on an axis → child returns intrinsic size (HStack gives infinite width to children, then sums).
- `arrange` rect ≥ measured desired (parent may stretch). Alignment/margin handled here.
- Bottom-up measure, top-down arrange — exactly WPF.

**Per-axis `Align` semantics by parent layout mode:**
- `HStack` reads `align_y` (cross axis); ignores `align_x` (main axis position is determined by stack order + gap).
- `VStack` reads `align_x` (cross axis); ignores `align_y`.
- `ZStack` reads both `align_x` and `align_y` (children are layered, both axes are free).
- `Canvas` ignores both — children are placed at their absolute `Layout.position`. Mixing alignment with absolute placement muddles coordinate semantics; if you want centered placement, use `ZStack`.
- `Leaf` has no children, so alignment doesn't apply.

### 3. Don't reinvent layout — wrap Taffy (optional path)
For flex/grid/block, integrate **Taffy** as the layout engine. Custom widgets implement Taffy's `MeasureFunc` for intrinsic sizing (text, images). Keep the WPF trait for native panels (Stack, Grid, Dock, Canvas) where Taffy is overkill.

### 4. State outside the tree
Per-widget state (scroll offset, text cursor, animation) lives in a `HashMap<Id, Box<dyn Any>>` keyed by stable ID. The tree is throwaway; state persists.

### 5. Input handled eagerly against last-frame rects

Hit-testing happens **as events arrive**, against the rects from the most recently rendered frame — i.e., the frame the user was looking at when they clicked. Visuals respond with zero lag (a press updates `pressed` immediately, the next redraw paints it). Click identity is preserved across widget movement via ID-based capture.

**Frame protocol**:

```
handle_event(WindowEvent)             // eagerly updates pointer pos + active widget;
                                       // hit-tests against last_rects.
                                      // press → active = hit, release with same hit → click.
begin_frame
build_ui(&mut ui)                     // widgets read response_for(id), which derives
                                       // hovered/pressed/clicked from live state +
                                       // last_rects.
layout::run(...)                      // produces this-frame rects.
end_frame                             // rebuild last_rects from this-frame tree;
                                       // clear clicked_this_frame.
render(...)
```

**ID-based active capture** for press/release across frames:
- On press: hit-test `last_rects` → set `Active = WidgetId`.
- While Active is set, `pressed = (active == self.id)` — visuals pin to the captured widget regardless of where its rect is now.
- On release: hit-test again. If `hit == Active`, emit `clicked`. Clear Active.
- If Active's WidgetId disappears from the tree (conditional rendering), clear it silently in `end_frame`.

This handles every case correctly:
- Static UI: instant press feedback, click on release.
- Widget moved between press and release: still `pressed` while held (id match overrides rect). Click cancels if release point isn't over the same widget — matches user intent ("I clicked the button that *was* there, but it moved away, so cancel").
- Drag (future): `Active` is the captured widget; pointer position tracking gives `drag_delta` regardless of rect.

**Trade-off accepted:** hit-test for press/release uses last-frame's rects. If a widget appeared *just this frame* at the click position, it can't be clicked until next frame. Acceptable; matches every IM library in the corpus (egui §6, imgui §6, iced §8).

**Don't bubble events.** Topmost widget at the point handles, then it's done. Routed events (WPF tunnel/bubble) encourage accidental coupling; egui omitted them and never regretted it.

**Layers, not pure z-index.** `LayerId = (Order, AreaIndex)` where `Order ∈ {Background, Main, Foreground, Tooltip, Popup, Debug}`. Hit-test walks layers top-down. Tooltips and dragged-thing-attached-to-cursor each get their own layer.

**Hit shape per node**, defaults to bounding rect. `HitShape::{Rect, RoundedRect, Path, None}` — needed for proper rounded-corner rejection and click-through overlays. Can be added incrementally; v1 ships rect-only.

**Don't bubble events.** Topmost widget at the point handles, then it's done. Routed events (WPF tunnel/bubble) encourage accidental coupling; egui omitted them and never regretted it.

**Layers, not pure z-index.** `LayerId = (Order, AreaIndex)` where `Order ∈ {Background, Main, Foreground, Tooltip, Popup, Debug}`. Hit-test walks layers top-down. Tooltips and dragged-thing-attached-to-cursor each get their own layer.

**Hit shape per node**, defaults to bounding rect. `HitShape::{Rect, RoundedRect, Path, None}` — needed for proper rounded-corner rejection and click-through overlays. Can be added incrementally; v1 ships rect-only.

### 6. wgpu rendering: batch by primitive
Paint pass walks the laid-out tree and emits into typed batches: rounded-rect quads, glyph quads, SDF icons, clip stacks. One render pass per surface, instanced draws. Use `wgpu::RenderBundle` for unchanged subtrees later as an optimization. Text via `glyphon` or `cosmic-text` + atlas.

### 7. Damage / dirty tracking later
Ship v1 as full-redraw. Add dirty-rect tracking only when profiling demands it — premature for an immediate-mode crate.

## Non-Goals (v1)

- Accessibility tree (add later via `accesskit`).
- Animation framework (state map is enough; users animate via tween crate).
- Stylesheet language. Inline style structs only.
- Multi-window. Single surface.

## Open Questions

- **Closure lifetimes vs. tree arena**: how to let `ui.stack(|ui| ...)` mutate the same arena without `RefCell` everywhere. Likely answer: `&mut Ui` threaded through, parent index passed in.
- **Re-measure on size changes during arrange**: WPF allows constrained re-measure. Do we need it, or is one pass each enough? Start with one each; add `request_discard`-style second frame if a widget reports size mismatch (egui's approach).
- **Taffy vs. native panels**: pick one as primary. Recommendation: native WPF-style panels in core, Taffy as opt-in feature flag.

## Prior Art Worth Studying

- **egui** — immediate-mode in Rust; uses prior-frame sizes + `request_discard` for two-pass. We do better by recording first.
- **Clay** (C) — deferred immediate mode; close to this design.
- **WPF** — the measure/arrange contract itself.
- **Taffy** — flex/grid/block engine, used by Dioxus/Bevy.
- **Quirky** — retained wgpu UI in Rust, for renderer reference.
