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

### 3. Don't reinvent layout — wrap Taffy (optional path)
For flex/grid/block, integrate **Taffy** as the layout engine. Custom widgets implement Taffy's `MeasureFunc` for intrinsic sizing (text, images). Keep the WPF trait for native panels (Stack, Grid, Dock, Canvas) where Taffy is overkill.

### 4. State outside the tree
Per-widget state (scroll offset, text cursor, animation) lives in a `HashMap<Id, Box<dyn Any>>` keyed by stable ID. The tree is throwaway; state persists.

### 5. Input lags one frame, render does not

Hit-testing uses arranged rects from the **most recently rendered** frame — i.e., the frame the user was looking at when they clicked. Layout and rendering are always current. One-frame input lag is imperceptible.

**Frame protocol** (refines egui's "hit-test against prev rects"):

```
begin_frame                           // swap response tables
build_ui(&mut ui)                     // widgets read prev_responses[id] → Response
layout::run(...)                      // pure layout; produces this-frame rects
process_input(events_since_last_frame, &this_frame_rects)
                                      // → next_responses[id] for next frame
end_frame                             // rebuild rect index, swap response tables
render(...)
```

Why post-layout processing (not egui's during-recording approach): a click at time *T* targets whatever was visible at *T* — i.e., the just-rendered frame's rects, not the previous frame's. Processing events after `layout::run` uses those exact rects. The Response surfaced to the user lags by one frame in *delivery* but is correct in *attribution*.

**ID-based active capture** for press/release across frames:
- On press: hit-test current rects → set `Active = WidgetId`.
- While Active is set, all pointer events route to that widget regardless of where its rect is now.
- On release: emit `clicked` only if the release point is over the same widget (by ID, not by rect). Clear Active.
- If Active's WidgetId disappears from the tree (conditional rendering), clear it silently.

This makes drag-and-move-button cases correct: identity tracks across rect changes; orphaned active widgets fail open, not closed.

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
