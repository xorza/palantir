# ScrollView — implementation plan

Status: design + step plan. No code landed yet.

## What we already have

The rendering side is mostly in place — scroll = clip + translate, both
exist:

- **Clip**: `Element.clip` → `PaintAttrs::is_clip()` (bit 4); resolved
  in `src/ui/cascade.rs:157` (intersects screen-rect with parent clip);
  applied in `src/renderer/frontend/encoder/mod.rs:168` via
  `push_clip`/`pop_clip`. Clip is pre-transform; transform applies
  inside.
- **Transform**: `Element.transform: Option<TranslateScale>` lives in
  extras (`src/tree/element/mod.rs:86`); composed in
  `src/ui/cascade.rs:152-156`
  (`parent_transform.compose(node_transform)`); affects descendants
  only. The panel's *own* paint stays in parent space — exactly what we
  want for scroll (the viewport doesn't move; the content does).
  `examples/showcase/transform.rs` exercises the path end-to-end.
- **State map**: `Ui::state_mut::<T>(id: impl Hash)` returns a
  per-`WidgetId` row, swept on `SeenIds.removed`. The natural home for
  scroll offset.
- **Damage-filter skip in encoder** (`encoder/mod.rs:181`): infrastructure
  for clip-culling already exists — it skips leaf emission when a node's
  screen rect misses a filter rect. We can reuse it for "cull leaves
  outside the active clip."

## What's missing — concrete blockers

In rough dependency order:

1. **Scroll-wheel input**. `InputEvent` (`src/input/mod.rs:23`) has no
   `Scroll` variant; `from_winit` returns `None` for
   `WindowEvent::MouseWheel` at line 56. Add `InputEvent::Scroll(Vec2)`
   in logical pixels (after `LineDelta` → pixel conversion using a
   fixed step constant; cosmic font-metric-aware step is a v2 polish),
   accumulate into `InputState.frame_scroll_delta: Vec2`, clear at
   `end_frame`.
2. **Scroll routing**. Once delta is on `InputState`, decide which
   widget consumes it. v1: hit-test the pointer position with a new
   `Sense::Scroll` variant; the topmost scroll-sense ancestor of the
   pointer wins. (Existing `Sense::hover` returns the deepest
   hover-sense node — usually a child, not the scroll panel — so we
   need a separate sense bit.)
3. **No `LenReq::Unbounded`** (this corrects the previous draft).
   `src/layout/intrinsic.rs:23` has only `MinContent` / `MaxContent`,
   and the intrinsic-query path is for grid track sizing — not the
   right tool here. Scroll instead calls regular `measure(child,
   available)` with `available[main_axis] = f32::INFINITY` so the
   child reports its full natural size on the scrolled axis.
4. **One-frame-stale offset clamp**. Arrange runs through `&Tree` and
   can't mutate `node_extras`. So Scroll writes its transform at
   **record** time using the previous frame's `(content_size,
   viewport)` saved on its state row. After this frame's arrange, we
   need to update that row with the new measurements. Two options:
   - (4a) Side-list `scroll_nodes: Vec<(WidgetId, NodeId)>` populated
     at record; `Ui::end_frame` walks it after arrange and copies
     `result.rect[node]` size + measured content size into the state
     row. Picked for v1 — simple and doesn't perturb existing passes.
   - (4b) Post-arrange transform pass that mutates `node_extras` in
     place. Cleaner conceptually but requires a new pass. Defer.

## Widget shape (v1)

`Scroll` widget — single Element node that:

- Carries `clip = true` and `sense = Scroll`.
- Owns one logical child group (the content). Records like Panel:
  `Scroll::vertical().show(ui, |ui| { ... })`. Internally it's an
  `LayoutMode::VStack` (vertical scroll) so the child group lays out
  exactly like a VStack would.
- At record time:
  1. Reads `ScrollState { offset, content_size, viewport }` from
     `state_mut` (zeroes on first frame).
  2. Adds `ui.input.frame_scroll_delta` if this widget's id matches
     the scroll-hit-test result.
  3. Clamps offset to `[0, max(0, content_size - viewport)]` using the
     **previous frame's** content/viewport (one-frame stale; invisible
     for normal wheel rates).
  4. Sets `element.transform = Some(TranslateScale::translation(-offset))`.
  5. Registers `(WidgetId, NodeId)` in `Ui.scroll_nodes` for end-frame
     state update.
- In measure: parent treats it like any node (`Hug` / `Fixed` / `Fill`
  on the cross axis; usually `Fill` or `Fixed` on the main axis). The
  scroll node's *own* measure dispatch passes `available[main] =
  f32::INFINITY` to its children so they report full content size; the
  scroll node itself returns the parent-given main size (it's a
  viewport, not a hugger).
- In arrange: existing VStack arrange does the right thing — children
  are placed at natural positions; rects can extend past the viewport
  rect (clip will hide them).
- After arrange (`Ui::end_frame`): walks `scroll_nodes`, reads
  `result.rect[node]` (viewport size) and the measured content size
  (stashed during measure dispatch), writes back to state row for next
  frame's clamp.

The key insight: arrange-time mutation of `transform` isn't required
because we use last-frame numbers for clamp. One-frame staleness is
fine for wheel and touch deltas; it would be visible only if you
teleport the offset (programmatic scroll), which isn't a v1 feature.

## Step-by-step implementation

Each step is a self-contained slice with tests; ship one before
starting the next.

### Step 1 — `InputEvent::Scroll` + winit translation + `frame_scroll_delta`

**Goal**: scroll wheel deltas reach `InputState`. Nothing consumes them yet.

**Edits**:
- `src/input/mod.rs`:
  - Add `Scroll(Vec2)` variant to `InputEvent`. Logical-pixel deltas;
    positive y = scroll down.
  - Extend `from_winit`: handle `WindowEvent::MouseWheel`. For
    `MouseScrollDelta::LineDelta(x, y)` use `(x, y) * LINE_PIXELS`
    where `const LINE_PIXELS: f32 = 40.0` (matches winit/egui
    convention; revisit with cosmic font metrics later). For
    `MouseScrollDelta::PixelDelta(p)` divide by `scale_factor`.
  - Add `frame_scroll_delta: Vec2` to `InputState`. Accumulate in
    `on_input(InputEvent::Scroll)`. Clear in `end_frame`.
  - Expose pub(crate) accessor `InputState::frame_scroll_delta() -> Vec2`.

**Tests** (`src/input/tests.rs`):
- `from_winit` with `LineDelta(0.0, 1.0)` produces `Scroll(Vec2::new(0,
  40))`.
- `from_winit` with `PixelDelta(...)` divides by scale factor.
- `on_input` accumulates two `Scroll` events into one frame's delta.
- `end_frame` clears the accumulator.

**No showcase change** yet. No widget consumes the delta.

### Step 2 — `Sense::Scroll` + scroll routing

**Goal**: a single hit-tested widget id can claim the frame's scroll
delta.

**Edits**:
- `src/layout/types/sense.rs`: add `Sense::Scroll` variant (and
  `ClickAndScroll` if it falls out — probably not for v1; keep narrow).
  Update bit-packing in `PaintAttrs` (currently 3 bits = 5 senses; one
  more is fine).
- `src/input/mod.rs`: add `Sense::scroll()` predicate.
  `InputState::scroll_target: Option<WidgetId>` — recomputed alongside
  hover via `cascades.hit_test(pos, Sense::scroll)`.
- `Ui` exposes `pub(crate) fn scroll_delta_for(&self, id: WidgetId) ->
  Vec2` returning `frame_scroll_delta` iff `scroll_target == Some(id)`,
  else `Vec2::ZERO`.

**Tests**: hit-test with two stacked scroll-sense nodes returns the
topmost; `scroll_delta_for` non-target returns zero.

### Step 3 — `Scroll` widget (vertical only)

**Goal**: minimal working scrollable column. Showcase tab demonstrates
it.

**Edits**:
- New `src/widgets/scroll.rs`:
  - `pub(crate) struct ScrollState { offset: f32, content: f32,
    viewport: f32 }`.
  - `Scroll::vertical()` builder; sets `sense = Scroll`, `clip = true`,
    `mode = VStack`.
  - `show(ui, body)`: read state → add `ui.scroll_delta_for(id).y` →
    clamp → set `element.transform = translation(0, -offset)` → record
    children → register node id for end-frame update.
- `src/ui/mod.rs`:
  - Add `scroll_nodes: Vec<(WidgetId, NodeId)>` (capacity-retained).
  - In `Ui::end_frame`, after arrange + before cascade: for each
    `(wid, node)`, compute viewport (`result.rect[node].size.h`) and
    content (sum of children's bottom edge minus node's top — see
    measure detail below). Write back to state row.
- `src/layout/mod.rs`: in measure dispatch for the scroll node, pass
  `available[main] = f32::INFINITY` to children. Stash measured
  content size somewhere readable from end_frame — simplest is to
  reuse `result.rect[node].size.h` if we don't clamp the rect to
  viewport in arrange. Alternative: a per-node `Vec<f32>
  scroll_content_main_size` keyed on NodeId, written by measure when
  the node has scroll mode. Pick this; it's local.
- `src/widgets/scroll.rs` exports via `widgets/mod.rs` and `lib.rs`.
- New showcase tab `examples/showcase/scroll.rs`: tall colored
  rectangle column inside a fixed-height scroll panel.

**Tests**:
- Scroll node arrange produces expected `transform` for a known offset.
- Offset clamps to `[0, content - viewport]` when negative or too large.
- Non-overflowing content has clamped offset = 0 regardless of input
  delta.
- `state_mut` row survives across frames as long as the widget records.
- Showcase tab renders without panicking; `cargo test` passes.

### Step 4 — Horizontal axis + both-axes

**Goal**: `Scroll::horizontal()` and `Scroll::both()`.

**Edits**: parameterize on `Axis` (or a `(bool, bool)` axis-mask). Set
`mode` to `HStack` for horizontal; `both()` is more involved (children
need 2D layout — can punt, or use ZStack-style absolute positioning
inside scroll). Keep `both()` simple: 2D scroll over a single ZStack
child with its own size.

### Step 5 — Drag delta on `Active` capture

**Goal**: track press_pos/last_pos for general drag plumbing. Reuse
later for touch-drag scroll and scrollbar thumb.

Independent from scroll; lives at `InputState` level. Spec already in
roadmap.

### Step 6 — Scrollbars

**Goal**: visible scrollbar rendered as a sibling overlay, thumb drag
pans the offset.

Separate widget (`ScrollBar`) drawn on top of the Scroll node. Reads
the parent's `ScrollState` row (we'll need a way to address it — keyed
by the scroll widget's id). Thumb drag uses step 5's `drag_delta`.

## Out of scope for v1

- Momentum / overscroll / elastic animation
- Virtualization (separate item in `roadmap.md`)
- Nested scroll-chaining policy (which scroll claims wheel when scrolls
  are nested?). v1: innermost wins (deepest hit-test).
- Sticky headers
- Programmatic scroll-to / scroll-into-view (would require
  arrange-time transform mutation — see 4b above)

## Off-screen cost — what each pass does

Without virtualization, scroll content pays full per-pass cost
regardless of what's visible:

- **Measure / arrange**: unconditional per node. Scroll passes
  `available[main] = INF` so the child reports full intrinsic size;
  arrange positions every grandchild at its natural place. Inherent —
  measure has to know total content size to clamp. Cost is
  `O(content)`, not `O(viewport)`.
- **Cascade**: walks every node. Clip intersection in `cascade.rs:157`
  shrinks off-screen rects to empty, which is what makes hit-testing
  correctly ignore them. Cheap.
- **Encode**: walks every node pre-order; today emits leaf shapes for
  every visible (non-`Hidden`) node regardless of clip.
- **GPU**: scissor discards off-screen fragments, so no shading. CPU
  encode + compose + instance-buffer write still runs.

So a 10k-row list in a 600px viewport still does 10k of measure,
arrange, cascade, encode every frame, with the GPU scissoring ~9.97k
of the emitted instances. Correct, but only fine up to hundreds of
items.

### Cheap wins to fold in (post-v1)

1. **Clip-cull the encoder.** Reuse the `damage_filter` machinery: while
   a `push_clip` is active, skip leaf shape emission for descendants
   whose screen rect doesn't intersect the current clip. Push/pop pairs
   still emit so composer state stays coherent. ~Free order-of-magnitude
   on encode for tall scroll content; also helps any `clip = true`
   panel.
2. **Skip cascade/encode recursion under empty clip.** When a subtree
   root's screen rect is fully outside the root viewport, short-circuit
   descent. Trickier — `Active` capture and (future) focus may want
   off-screen rects to stay live. Defer until a workload asks.

Real virtualization (the "virtual children" hook in `roadmap.md`) is
the only path to `O(viewport)` measure cost, and is a separate, larger
project.

## Open questions

- **First-frame size**: scroll content's desired size is unknown frame
  0 → offset clamp uses zero bounds frame 0. Acceptable (one-frame
  visual blip — first frame can't have a wheel event anyway).
- **Scroll capture vs hover**: should a scrolled-into widget keep
  focus/hover during the scroll, or does the gesture suppress hover?
  Match egui (suppress) unless a workload says otherwise.
- **Nested scroll chaining**: v1 = innermost hit wins. Browsers chain
  to parent when child reaches its end; defer.
- **Wheel step**: 40 logical px/line is winit/egui convention. Once
  cosmic is integrated for text, consider line-height-aware step for
  text-heavy scroll.
