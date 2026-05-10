# Drag on Canvas

## 1. Goal

User grabs a widget (handle or whole body) sitting on a `Canvas` parent and
moves it to a new position by dragging. The widget's `position` updates
during the drag and persists across frames. One frame, one widget at a
time, mouse first; touch and keyboard are follow-ups. State lives in
`StateMap` keyed by `WidgetId` like every other persistent widget
concern. No drop targets, no payloads, no inter-widget swap in v1 — just
"move this thing across this surface."

## 2. Prior art

- **egui** — `Sense::drag()` + `Response::drag_delta()` / `drag_started()` /
  `is_being_dragged()`. Threshold lives on `InputOptions` (`max_click_dist:
  6.0`, `max_click_duration: 0.8`, `tmp/egui/crates/egui/src/input_state/mod.rs:115`)
  and the "decidedly dragging" latch flips when *either* dimension is
  exceeded (`input_state/mod.rs:1117`, `:1245`). Active widget is tracked
  on `Context::interaction()`; press_origin lives on the pointer
  (`input_state/mod.rs:1026`). Drag-and-drop with typed payload is layered
  on top in `drag_and_drop.rs` as a plugin — orthogonal to plain drag.
  Demo uses `ui.dnd_drag_source(item_id, payload, |ui| {...})` +
  `ui.dnd_drop_zone::<T, _>(...)`
  (`tmp/egui/crates/egui_demo_lib/src/demo/drag_and_drop.rs:64-100`).
- **imgui** — `IsItemActive()` + `GetMouseDragDelta()`. No threshold for
  "drag started" by default; the app reads delta every frame and decides.
  `BeginDragDropSource()` / `BeginDragDropTarget()` for payload-style dnd.
  Stripped-down "active id" model: one global `ActiveId` per frame.
- **nuklear** — pure pointer-down + pointer-pos arithmetic, no API
  sugar. Each widget reads `nk_input.mouse.buttons[].down` and
  `mouse.delta` and updates its own state. No threshold, no capture.
- **iced** — retained; drag-as-payload is per-widget (`PaneGrid`,
  `Scrollable`'s scrollbar at `tmp/iced/widget/src/scrollable.rs`).
  Widget tree owns the state, so the dragged widget literally
  re-parents in the tree on drop. Doesn't apply to immediate mode but
  shows the "named drop targets" model.
- **floem** — full drag-and-drop lifecycle with pointer-capture-first
  protocol: `cx.request_pointer_capture()` → on capture-gained call
  `cx.start_drag(token, DragConfig {threshold: 3.0, ...}, true)` →
  threshold trip emits `DragSourceEvent::Start`. State machine: pending
  → active → released-with-animation. Default `threshold: 3.0` px,
  default snap-back animation 300 ms
  (`tmp/floem/src/event/drag_state.rs:96-106`). Cleanly separates
  source-only events (pan/slider use case, `track_targets: false`) from
  source+target dnd (`tmp/floem/src/event/drag_state.rs:82-93`).
- **slint** — `DragArea` element with `mime-type` + `data` + `dropped`
  callback (`tmp/slint/internal/core/items/drag_n_drop.rs`). Retained,
  declarative; same payload-typed model as egui plugin.
- **xilem** — `drag_n_drop` module under window options; system-level
  drag (OS file-drag), not widget-on-canvas. Not relevant to v1.
- **web platform** — `setPointerCapture()` routes all subsequent
  pointer events to the capturing element until pointerup/cancel;
  browsers auto-release on up. The MDN-blessed pattern for drag
  ([dev.to](https://dev.to/nishinoshake/smooth-drag-interactions-with-pointer-events-5e2j),
  [r0b.io](https://blog.r0b.io/post/creating-drag-interactions-with-set-pointer-capture-in-java-script/)).
  Palantir's `InputState.active` is already this capture mechanism — the
  active id receives moves regardless of where the pointer is.

## 3. Best practices distilled

- **Threshold before "drag started."** 3-6 px is the standard
  (floem 3, egui 6). Under the threshold the gesture stays a click; once
  exceeded it latches as a drag for the press lifetime. Without this,
  every click that wiggles becomes a drag. Touch needs more (~10 px
  + ~50 ms) but mouse can stay tight.
- **Active-id pointer capture.** The widget that captured the press
  receives all subsequent moves until release, regardless of whether
  the pointer is over its rect. Palantir already does this via
  `InputState.active` + `press_pos` and exposes it as
  `InputState::drag_delta` (`src/input/mod.rs:437`).
- **Click vs drag disambiguation.** Click fires on release *only if*
  the gesture never crossed the threshold and the release landed on
  the same widget. egui issue #547 documents the long tail of users
  getting this wrong with naive `if released { click() }`.
- **Drag handle vs whole-body.** Two patterns: (a) a designated
  sub-widget (titlebar) carries `Sense::Drag`, mutates parent's
  position via shared state; (b) the whole widget carries
  `Sense::Drag`. Both want the same primitive — what differs is which
  `WidgetId` owns the drag state. Palantir's `state_mut` already
  supports indirection (scroll uses `id.with("__viewport")`).
- **Position the body, not the cursor.** Apply
  `new_position = press_origin_position + drag_delta`, not
  `cursor_pos - rect_origin`. The latter snaps the grab point to the
  cursor on threshold trip and loses the grip offset.
- **Snap-back / animation is optional.** floem ships it; egui doesn't.
  v1 ships without — palantir's `animate::<Vec2>()` can layer it on top
  when needed.
- **Accessibility.** Keyboard drag (focus + arrow-keys-to-nudge) is a
  hard requirement for any shipped framework but is independent of the
  pointer drag path. Punt to v2 with a note.

## 4. Pitfalls to avoid

- **Lost pointer-up.** Window blur / pointer leaves surface mid-drag.
  egui clears active on cursor-left; palantir today does *not* clear
  `active` on `PointerLeft`, only on `PointerReleased` and on
  cascade-evict at `end_frame` (`src/input/mod.rs:289-293,
  :381-386`). Pointer-up still arrives if the OS keeps delivering
  events to a captured window, but losing the pointer + losing the
  window means stuck-drag. Mitigation: on `PointerLeft` while
  `active.is_some()`, hold capture but stop emitting delta; on regain,
  resume. On `WindowEvent::Focused(false)` (not currently translated),
  drop active. **Add a winit → palantir event for focus-lost.**
- **First-frame jitter.** Threshold-tripping frame must emit a delta
  starting from the press origin, not from the threshold-crossing
  position. Otherwise the widget jumps by ~6 px on drag start. Our
  `drag_delta` is `pointer - press_pos` so this is already correct —
  pin it with a test.
- **Click fires after drag.** `clicked_this_frame` is set on
  pointer-release when release hit the same widget. After a drag we
  must *not* set it. Today the check is rect-based, so a drag that
  ends with the pointer over a different widget already won't click —
  but a drag that ends back on the originator *will* falsely click.
  Need a "this gesture crossed threshold" sticky bit that suppresses
  the release-click.
- **Drag through scroll.** A scroll container is `Sense::Scroll` (not
  Click/Drag) so it doesn't capture press; the inner draggable wins
  hit-test. That's the desired behavior — pin it.
- **Z-order during drag.** The dragged widget should paint on top of
  siblings even if it's not last in the canvas. Cheap fix: hoist it
  to `Layer::Popup` while dragging (recorded via `ui.layer(...)`),
  drop back to `Main` on release. Or: paint the canvas children in an
  order that puts the active-drag last. Pick the layer trick — it's
  one `if` at the call site, no canvas changes.
- **Position written, then snapped back by user code.** If the app
  also writes `position` from its source-of-truth every frame, our
  drag state loses the race. Document: drag emits `delta` and lets the
  app fold it into its own state, OR drag owns the position and the
  app reads it back. We pick "app owns" (see §5).
- **Touch screens dragging the window instead of the widget** (egui
  issue #5625) — irrelevant until palantir runs windowed on touch.

## 5. Proposed design for Palantir

### 5.1 Sense

`Sense::Drag` and `Sense::ClickAndDrag` exist (`src/input/sense.rs:24,
:25`). Reuse. No bits to add.

### 5.2 DragState (StateMap row)

```rust
// src/widgets/drag.rs (new)
#[derive(Default)]
pub(crate) struct DragState {
    pub(crate) origin: Vec2,      // widget position at press time
    pub(crate) accumulated: Vec2, // last committed delta (== ui.drag_delta on press frame)
    pub(crate) latched: bool,     // threshold has been crossed this gesture
}
```

Stored as `Ui::state_mut::<DragState>(id)`. Lifecycle:
- press on widget → next frame `ui.drag_delta(id)` returns
  `Some(delta)`; on the first frame where `delta.length() >=
  DRAG_THRESHOLD` (=4.0 px), latch and record `origin = current
  position`.
- subsequent frames: `accumulated = delta` (rect-independent — from
  `InputState::press_pos`, already implemented).
- release: `latched` reset to `false`, `accumulated` cleared. `origin`
  is left at whatever value — only read while latched.

The row is auto-evicted at `end_frame` when the widget doesn't record.
Same model as scroll.

### 5.3 Ui API

Three free functions on `Ui`, all threshold-gated (sub-threshold
wiggle never visible to caller):

```rust
pub fn drag_position(&mut self, id: WidgetId, current: Vec2) -> Vec2;
pub fn drag_delta(&mut self, id: WidgetId) -> Option<Vec2>;
pub fn drag_started(&mut self, id: WidgetId) -> bool;
```

- `drag_position` — the 95% case for Canvas drags. Caller passes the
  app-owned position, gets back the (possibly drag-modified) new
  position. One line at the call site. Internally stashes origin in
  `DragState`.
- `drag_delta` — raw delta for callers that fold it themselves
  (per-axis lock, snap-to-grid, drag-to-resize where delta drives
  width not position). Trivial wrapper around `InputState::drag_delta`
  + latch check. Costs nothing to expose.
- `drag_started` — one-frame edge for reacting to the press itself
  (raise z-order to `Layer::Popup`, play a sound, kick an animation).
  Without it the call site would have to compare last/this-frame
  `drag_position` returns, which is ugly.

The wrap-widget pattern (a `Draggable::new(body)` builder) is rejected
— it forces ownership of layout knobs we don't need to own and
duplicates `Element`. Same reasoning as why scroll isn't a wrapper.

### 5.4 Canvas integration

The app owns the position. Canvas reads `Element::position` (the
existing field, `src/forest/element/mod.rs:143`); the app writes it
from a `Vec2` held in app state, mutated by the drag delta read from
`ui.drag_delta(id)`. Concretely:

```rust
// pseudo, in the showcase tab
let id = WidgetId::stable("card-a");
let pos = ui.state_mut::<Vec2>(id);
if let Some(delta) = ui.drag_delta(id) {
    *pos = drag_origin + delta;   // drag_origin captured at press
}
Button::new("drag me")
    .with_id(id)
    .position(*pos)
    .sense(Sense::Drag)
    .show(ui);
```

The `drag_origin` capture is what `DragState.origin` is for. We can
either hand it back through the API (return `(origin, delta)` — but
tuple returns banned) or have `Ui::drag_delta` internally apply it
and return the *absolute new position*, not the delta. Better:

```rust
pub fn drag_position(&mut self, id: WidgetId, current: Vec2) -> Vec2 {
    // on press-frame-after-threshold: stash current as origin, return current
    // on subsequent drag frames: return origin + delta
    // on no drag: return current unchanged
}
```

App writes `let pos = ui.drag_position(id, app_pos)`; one line, no
manual origin tracking. `drag_delta` and `drag_started` (§5.3) cover
the off-canvas use cases.

### 5.5 Active-drag tracking on Ui/InputState

Already done. `InputState.active: Option<WidgetId>` + `press_pos:
Option<Vec2>` + `drag_delta(id)` (`src/input/mod.rs:188, :198,
:437`). Need to add a latch flag — see §5.6.

### 5.6 Threshold

New const in `input/sense.rs` or alongside the drag widget:

```rust
pub(crate) const DRAG_THRESHOLD: f32 = 4.0; // logical px, matches floem-ish
```

Latch lives on `InputState` (not per-widget): one drag at a time, one
bit:

```rust
pub(crate) drag_latched: bool, // set when |drag_delta| crosses threshold
```

Set inside `on_input(PointerMoved)` when `active.is_some()` and
`(now - press_pos).length() >= DRAG_THRESHOLD`. Cleared on
`PointerReleased`. `Ui::drag_position` reads
`active == Some(id) && drag_latched` to decide whether to apply.

The latch doubles as the "suppress click after drag" bit: in
`PointerReleased`, only insert into `clicked_this_frame` when
`!drag_latched`. This fixes the §4 false-click pitfall.

### 5.7 Hit-test during drag

Already correct — `InputState::drag_delta` is rect-independent because
it uses `press_pos`. The active widget receives delta even when the
pointer leaves its rect. No changes.

### 5.8 Damage / cache interaction

Position change re-arranges but doesn't remeasure: the dragged child
sits inside a Canvas, and Canvas measure uses `child_pos + d` per
axis — that *is* sensitive to position, so the canvas's
`subtree_hash` changes when a child's `position` changes (assuming
`Element::position` feeds into `ElementExtras` hashing — verify in
`src/forest/element/mod.rs:178`, where `h.write` already hashes
position). Subtree hash flips → measure cache miss on the canvas,
but children's hashes don't depend on parent position so they hit.
Net: O(visible children) re-arrange, no remeasure of subtrees, no
re-encode of glyph caches. Same cost profile as scrolling.

For damage: position change → `Damage::Partial(union(old_rect,
new_rect))`. The encoder cache key includes the node's own subtree
hash, so the dragged subtree paints from scratch each frame it
moves; siblings hit cache. Acceptable.

### 5.9 Z-order during drag

In the showcase, the call site wraps the dragged widget in
`ui.layer(Layer::Popup, anchor, |ui| { ... })` while `ui.drag_delta(id)
.is_some()`. Free, no canvas changes. Document as the recommended
pattern.

## 6. Implementation steps

Ordered, each shippable, each with one or two tests:

1. **Threshold latch on `InputState`.** Add `drag_latched: bool`. Set
   on `PointerMoved` when `(now - press_pos).length() >=
   DRAG_THRESHOLD`. Clear on `PointerReleased`. Test in
   `src/input/tests.rs`: press, move 3 px (not latched), move 5 px
   (latched), release (cleared).
2. **Click suppression after drag.** In `PointerReleased`, skip
   `clicked_this_frame.insert(a)` when `drag_latched`. Test: press →
   move 10 px → release-on-origin → no click recorded.
3. **Ui drag API.** Add `drag_position(id, current) -> Vec2`,
   `drag_delta(id) -> Option<Vec2>`, `drag_started(id) -> bool`. All
   read `active`, `drag_latched`, `press_pos`, `pointer.pos`.
   `drag_position` stashes origin in `state_mut::<DragState>(id)` on
   the latch-up frame and returns `origin + delta` thereafter.
4. **Showcase tab: a Canvas with two draggable cards.** Each card is
   a Button with `sense(Sense::Drag)` and a per-card `Vec2`
   position read/written via `drag_position`. Wrap the card in
   `ui.layer(Layer::Popup, ...)` gated on `drag_started(id) ||
   drag_delta(id).is_some()` for z-order during drag.
5. **Tests pinning:** (a) sub-threshold gesture leaves position
   unchanged; (b) supra-threshold gesture moves the widget by the
   pointer delta; (c) position survives across frames; (d) release
   re-grounds the new position; (e) two-card scenario, only the
   pressed card moves; (f) inner-click-on-draggable: card with a
   close-button child — click on button fires click only, drag on
   button-area does not move card, drag on card-not-button moves
   card.
6. **PointerLeft + focus-lost handling.** Hold `active` across
   `PointerLeft` (native convention — OS keeps delivering moves to
   the focused window). Wire `WindowEvent::Focused(false) →
   InputState::clear_active()` in the winit event translator (~5
   lines). Test: fake focus-lost mid-drag, assert `active` clears
   and `drag_latched` resets.
7. **Follow-ups (separate PRs):** drop targets (probably a
   `Sense::DropTarget` and an `InputState::current_drop_target`
   lookup); typed payloads (layer on top à la egui plugin);
   keyboard drag (focus + arrow-keys move position by 1 px / 10 px
   with shift); snap-to-grid as a wrapper around `drag_position`;
   spring snap-back via `ui.animate::<Vec2>`.

## 7. Open questions / deferred

Resolved (now part of §5–§6):

- API surface: ship `drag_position` + `drag_delta` + `drag_started`.
- `DragState.origin` stashed internally; `drag_started` exposed for
  one-frame edge reactions (z-order hoist, animation kick).
- `PointerLeft`: hold capture; clear on `WindowEvent::Focused(false)`.
- Drag-handle / inner-click: hit-test topmost-matching + threshold
  latch resolves it; pin with step-5 test (f).
- Z-order during drag: caller wraps in `ui.layer(Layer::Popup, ...)`.

Deferred to v2 (not in v1, noted as future work):

- **Multi-touch.** `InputState.active` stays single-id. v1 is mouse
  only. End state when touch lands: per-pointer-id active map
  (`FxHashMap<PointerId, WidgetId>`). Not now.
- **`Canvas::raise_active_drag` flag.** Auto-reorder canvas children
  so the active drag paints last. `ui.layer(Layer::Popup, ...)` is
  fine for 1-2 cards; this only earns its weight on N-card boards
  (>5). Wait for that workload.
- **Drop targets + typed payloads.** Separate PR. Probably
  `Sense::DropTarget` + `InputState::current_drop_target`, layered
  payload API like egui's plugin.
- **Keyboard drag.** Focus + arrow-keys-to-nudge for accessibility.
  Independent of the pointer path.
- **Spring snap-back.** Layer `ui.animate::<Vec2>` over
  `drag_position` when needed.

## 8. References

- egui drag state machine: `tmp/egui/crates/egui/src/input_state/mod.rs:1026-1294`
- egui drag-and-drop plugin: `tmp/egui/crates/egui/src/drag_and_drop.rs`
- egui dnd demo: `tmp/egui/crates/egui_demo_lib/src/demo/drag_and_drop.rs:64-100`
- floem drag tracker (full lifecycle, pending → active → animation):
  `tmp/floem/src/event/drag_state.rs:170-805`
- slint DragArea: `tmp/slint/internal/core/items/drag_n_drop.rs`
- palantir Sense: `src/input/sense.rs`
- palantir capture / press_pos / drag_delta: `src/input/mod.rs:188, :198, :437`
- palantir Canvas: `src/layout/canvas/mod.rs`
- palantir Element::position: `src/forest/element/mod.rs:143, :178`
- palantir StateMap usage: `src/ui/mod.rs:443`
- web pointer-events drag pattern:
  [dev.to/nishinoshake](https://dev.to/nishinoshake/smooth-drag-interactions-with-pointer-events-5e2j),
  [blog.r0b.io](https://blog.r0b.io/post/creating-drag-interactions-with-set-pointer-capture-in-java-script/)
- egui click-vs-drag discussion: [github.com/emilk/egui#547](https://github.com/emilk/egui/issues/547)
- egui freely-drag widgets: [github.com/emilk/egui#1926](https://github.com/emilk/egui/discussions/1926)
- egui drag holding window bug: [github.com/emilk/egui#5625](https://github.com/emilk/egui/issues/5625)
