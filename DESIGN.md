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
Hit-testing uses *last frame's* arranged rects (the standard immediate-mode trick). Layout and rendering are always current. One-frame input delay is imperceptible and avoids the chicken-and-egg.

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
