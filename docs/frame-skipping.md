# Frame skipping — implementation plan

A focused plan for Stage 1 of `incremental-rendering.md`, designed so
the bookkeeping it introduces extends cleanly into Stage 3 (damage-
region rendering).

## What "frame skipping" means here

If nothing relevant changed since the last frame, **don't run the
pipeline at all** — not the user's UI-building closure, not layout,
not encode/compose, not GPU submit. The OS keeps showing the last
presented framebuffer and our process sleeps until the next event
that warrants work.

The trick is deciding "nothing relevant changed" cheaply, before
we've done any work that we're trying to skip.

## What our existing code already has

- `Ui::on_input(InputEvent)` — every host event flows through here.
- `Ui::set_scale_factor(f32)` — DPI changes go through here.
- Showcase host (`examples/showcase/main.rs`):
  - `WindowEvent::CursorMoved | CursorLeft | MouseInput` → calls
    `window.request_redraw()`. Good.
  - `WindowEvent::Resized` → calls `state.draw()` directly. Good.
  - `WindowEvent::ScaleFactorChanged` → calls `request_redraw()`.
  - **`about_to_wait` schedules a redraw every 16 ms unconditionally**
    (lines 157-168). This is the constant 60 fps loop. **Removing
    this is the change that makes idle frames cost zero.**

So the host integration is mostly there. We just need a "should we
even bother?" gate in the redraw handler.

## The Stage 1 design

### Public API

```rust
impl Ui {
    /// True if the UI changed since the last successful `end_frame()`.
    /// Hosts call this in their redraw handler and skip the pipeline
    /// when it returns false.
    pub fn should_repaint(&self) -> bool;

    /// Request a repaint on the next host tick. Idempotent; cheap.
    /// Used for animations, async state landing, theme changes, or
    /// anything outside the normal input path.
    pub fn request_repaint(&mut self);
}
```

### Internal flag

One `bool` on `Ui`:

```rust
struct Ui {
    // ... existing fields ...
    repaint_requested: bool,
}
```

Default: `true` (so the first frame always renders). Cleared at the
end of `end_frame()`. Set by:

- `on_input(event)` — any input event that arrived. Conservative;
  even pointer moves that don't change hover index still set it
  (so the next frame can re-hit-test).
- `set_scale_factor(f)` — DPI change.
- `request_repaint()` — explicit call.
- A future animation tick (out of scope for Stage 1).

### Host integration (showcase)

```rust
fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    let Some(state) = self.state.as_ref() else {
        event_loop.set_control_flow(ControlFlow::Wait);
        return;
    };
    if state.first_paint || state.ui.should_repaint() {
        state.window.request_redraw();
    }
    event_loop.set_control_flow(ControlFlow::Wait);
}
```

The constant 16 ms wake disappears entirely. The control flow
becomes pure event-driven: hosts only redraw when input arrives,
the UI explicitly asks, or animations are running.

`RedrawRequested` handler stays the same — the `should_repaint`
guard catches the cases where the OS asks us to redraw (e.g.,
window expose, focus change) but our own state hasn't changed.

### What about animations?

Out of scope for Stage 1 itself, but the API anticipates it:

- A widget animating "open me over 200 ms" calls
  `ui.request_repaint()` on each frame it wants to be re-rendered.
  Naturally chains: each frame triggers the next via the host loop.
- When the animation completes, the widget stops calling
  `request_repaint()` and the loop quiesces.

A future `request_repaint_after(Duration)` (egui-style) is a clean
addition — it'd schedule a wake-up timer in the host. Not needed
for MVP.

### What about hovered/pressed state?

Hover state lives in `InputState` and is read by widgets via
`response_for(id)`. When the pointer moves over a hoverable widget,
the hover index changes, but our test for "should repaint" is
upstream of that — any pointer event sets the flag, which causes
the next frame to run, which lets the response state update.

The conservative "any input → repaint" rule subsumes hover/press
detection. Refining ("only repaint if hover index actually
differs") would require running a hit-test inside `on_input`,
which the input layer already does for press/release routing — but
it's not worth the complexity for Stage 1.

### Tests

- `should_repaint_returns_true_initially_and_after_input`.
- `should_repaint_returns_false_after_end_frame_without_changes`.
- `request_repaint_persists_until_next_end_frame`.
- `set_scale_factor_requests_repaint`.

### Estimated cost

~30 LOC across `Ui`, `InputState` dispatch, and showcase host
loop. No layout, encoder, or backend changes.

## Designing Stage 1 with Stage 3 in mind

Stage 3 (damage-region rendering) needs per-node "did this change"
detection, which is a strict superset of Stage 1's "did anything
change" boolean. The temptation is to do both at once. Resist it:
**Stage 1 stands alone with its boolean flag, Stage 3 adds the
per-node machinery on top without modifying Stage 1's API.**

The reasons to keep them separate:

1. **Cost asymmetry.** Stage 1's flag is one byte and zero
   per-frame work beyond setting/clearing it. Stage 3's hash table
   is ~16 B/node + ~50 ns/node every frame (not 100% free; only
   pays back when the win materializes). Don't pay Stage 3's cost
   until you're actually using it.

2. **Different decision points.** Stage 1 decides
   *should we run the pipeline at all?* — that decision happens
   *before* the user's UI closure runs. Stage 3 decides *which
   parts of the pipeline can skip work?* — that decision happens
   *after* recording, when we know what nodes the user produced.
   The two decisions are at different points; merging them
   adds coupling without saving work.

3. **Observability.** A `bool` is trivial to test; per-node hash
   diffs need a richer test surface. Easier to land the simple
   thing, then layer the complex thing.

### Where Stage 3 hooks in (not for now, but plan)

When Stage 3 ships, it adds:

- `Tree::hashes: Vec<u64>` column, indexed by `NodeId`. Filled
  during `push_node` and `add_shape` from a running per-node hash.
- `Ui::prev_hashes: HashMap<WidgetId, u64>` persistent across frames.
- After `end_frame()` recording but before layout: walk current
  hashes, compare to `prev_hashes` keyed on `WidgetId`. Changed
  nodes go into a per-frame `dirty: Vec<NodeId>` (or bitset).
- Encoder uses `dirty` to filter `DrawRect` / `DrawText` emission.
- Backend uses the bounding union of dirty rects + scissor +
  `LoadOp::Load`.

Stage 1's `should_repaint()` flag is **not** affected. It's still
set by inputs, animations, and explicit requests. Stage 3 just adds
fine-grained "what changed" knowledge that the encoder consumes.

In other words: Stage 1 says "*should* we repaint this frame at
all?" Stage 3 says "*how much* of the surface should we touch
when we do?"

### Naming choices that pay off later

- **`should_repaint` (not `is_dirty`)** — anticipates the boundary
  between "we need to render again" and "the layout state is dirty
  (some node hash differs from last frame)." Stage 3 introduces
  the latter; keeping the names distinct now prevents future
  shadowing.
- **`request_repaint` (not `mark_dirty`)** — same reason. Stage 3
  may have widgets calling `mark_dirty(self_id)` to opt into
  per-node dirty tracking; that's a different concept from
  "schedule a repaint."

## Things Stage 1 deliberately does NOT do

- **Track which input events change which widgets.** Conservative
  "any input → repaint" is fine. Refinement is Stage 3's job.
- **Skip layout when only paint state changed** (e.g., hover →
  hovered visuals). Same reason.
- **Coalesce multiple `request_repaint()` calls into one.** Already
  idempotent.
- **Implement `request_repaint_after(Duration)`.** Animation API
  comes with the first widget that needs animation.
- **Compute or expose any "what changed" data.** That's Stage 3.

## Migration plan

1. Add `repaint_requested: bool` field to `Ui` (default `true`).
2. Add `should_repaint()` and `request_repaint()` methods.
3. Wire `on_input` and `set_scale_factor` to set the flag.
4. Clear the flag at the end of `end_frame()`.
5. Update showcase host's `about_to_wait` and `window_event` to
   gate `request_redraw()` on `should_repaint()`.
6. Tests for the four scenarios listed above.
7. Verify the showcase still feels responsive on cursor/click but
   sits at 0% CPU when idle.

Estimated effort: ~30 LOC, ~4 tests, no architectural changes to
layout/encoder/backend.

## Future-revisit triggers

- **First widget needing time-driven animation** → add
  `request_repaint_after(Duration)`.
- **Profile shows a busy idle pipeline** despite frame skipping →
  audit which input events are over-firing the flag.
- **Cursor "live" hover changes feel sluggish** → benchmark
  whether running the full pipeline per pointer move is the cost,
  or just the GPU present. If pipeline, look at Stage 2/3.

When *any* of those triggers, Stage 1's `should_repaint()` API
remains intact; the new mechanisms (animation timer, damage rect)
slot in alongside.
