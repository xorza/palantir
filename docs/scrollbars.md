# Scrollbars — design

v1 of scroll affordance: always-visible overlay bar per panned axis, with
drag-to-pan on the thumb. Click-on-track-to-page deferred to v1.1, fade /
auto-hide deferred to whenever an animation tick lands.

## Tree shape

`Scroll` becomes a 2-level structure instead of one node:

```
ScrollWrapper          (outer, user's widget id, sense = Scroll)
├─ ScrollContent       (clip = true, transform = -offset, axes-driven layout)
│   └─ user body
├─ VBar                (Leaf, sense = Drag)            [if axes pans Y]
└─ HBar                (Leaf, sense = Drag)            [if axes pans X]
```

`ScrollWrapper` is a `Canvas` — children at absolute positions inside the
viewport rect. Why this matters:

- Bars are **siblings** of `ScrollContent`, not descendants → the
  content-pan `transform` doesn't apply to them.
- Bars are positioned by `extras.position` against the wrapper's inner
  rect — sit in the right/bottom edge regardless of content extent.
- `ScrollContent` keeps its current job: clip + transform + run the
  underlying stack/zstack measure with the panned axis fed `INF`.

Migration cost: small. The `ScrollState` row stays keyed on the
wrapper's `WidgetId` (unchanged from today). `LayoutMode::Scroll(axes)`
moves from the old single node to `ScrollContent`. `Sense::Scroll` stays
on the wrapper so wheel-routing still hits the outer id.

## Bar geometry

Per axis (call the panned dimension `main`, cross `cross`):

```
track_length = viewport.main - (overlap on the far end if both axes pan)
thumb_size   = max(MIN_THUMB_PX, viewport.main / content.main * track_length)
thumb_pos    = (offset.main / (content.main - viewport.main)) * (track_length - thumb_size)
              [defined only when content.main > viewport.main; else hide bar]
bar_x/bar_y  = anchored to the far edge of the viewport on the cross axis
```

- `MIN_THUMB_PX = 24` so a tiny thumb in a long list stays grabbable.
- `content.main <= viewport.main` ⇒ bar is hidden entirely
  (`Visibility::Collapsed`); pan is impossible anyway.
- `ScrollXY`: VBar height = `viewport.h - bar_width` so it doesn't
  overlap the HBar; HBar width same treatment. The corner is left
  empty (no "dead square" widget needed for v1).

Both numbers come straight from `ScrollState.{viewport, content,
offset}` populated in `Ui::end_frame`. No new layout output needed.

## Drawing

Each bar = one Leaf node with two `Shape::RoundedRect`s? **No** — a Leaf
has one arranged rect, and `RoundedRect` paints the owner's full rect.
Two-shape track-plus-thumb in one leaf would draw both at the same rect.

Two options:

1. **Two leaf nodes per bar** — `BarTrack` (full bar rect) and
   `BarThumb` (positioned + sized via `extras.position` and `Sizing::Fixed`).
   Both children of the wrapper Canvas. ~4 leaf nodes for a `ScrollXY`.
2. **A new `Shape::PositionedRect` variant** that carries its own
   `Rect` instead of inheriting the owner's. Tree-wide change but
   reduces nodes and matches what scrollbars (and future progress
   indicators, sliders) actually want.

**Pick #1** for v1. Cheaper; doesn't perturb the shape model. Revisit
#2 if scrollbars + sliders + progress bars all want positioned rects
within one owner.

## Drag-to-pan

Each bar leaf gets `Sense::Drag` and a stable `WidgetId` derived from
the wrapper id (`("scroll-vbar", parent_id)` etc.). At record time:

```
let drag = ui.input.drag_delta(bar_id).unwrap_or(Vec2::ZERO);
// Convert thumb pixel motion → content offset delta on the panned axis.
//   thumb moves track_length-thumb_size for offset 0..max
//   so 1 thumb-px = (content - viewport) / (track_length - thumb_size) content-px
let scale = (content.main - viewport.main) / (track_length - thumb_size).max(1.0);
state.offset.main = (state.offset.main + drag.main * scale).clamp(0.0, max);
```

Same one-frame-stale clamp model as the wheel path (`viewport`/`content`
are last frame's). `drag_delta` returns `Some` only while the bar is the
active capture target, so the math is gated on a real grab.

Existing wheel pan still flows through the wrapper's `Sense::Scroll`
hit-test. The two delta sources sum into `ScrollState.offset` in
record-time order: wheel first, then bar drag.

## Theme

```rust
pub struct ScrollbarTheme {
    pub width: f32,           // 8.0 logical px
    pub min_thumb_px: f32,    // 24.0
    pub track_color: Color,   // transparent for true overlay; some grey for visibility
    pub thumb_color: Color,
    pub thumb_hover: Color,
    pub thumb_active: Color,
    pub radius: f32,          // half of width = pill thumb
}
```

Defaults aim at a neutral macOS-style overlay (transparent track, dark
semitransparent thumb). Apps that want classic always-visible bars
override `track_color` to opaque.

## Tests to pin

- `thumb_size = viewport / content * track_length` for known geometry.
- `thumb_pos = 0` when `offset = 0`, `track_length - thumb_size` when
  `offset = max`.
- `MIN_THUMB_PX` floor honored when `viewport / content` is tiny.
- Bar collapses (`Visibility::Collapsed`) when `content <= viewport`.
- Drag delta of `1px` on a thumb of `T` over track length `L` advances
  offset by `(content - viewport) / (L - T)`.
- `ScrollXY`: VBar height excludes HBar width (no overlap).
- Showcase tab renders all three Scroll flavors with visible bars; no
  panic, no regression in other tabs.

## Out of scope (v1)

- **Click-on-track to page** — straightforward follow-up: VBar gets
  `Sense::ClickAndDrag`, click on track minus thumb pages by `viewport`.
- **Auto-hide / fade** — needs an idle timer + animation tick.
- **Hover-grow thumb** (Windows-style) — needs hover state on the bar
  leaves; cheap once the bars exist.
- **Reservation layout** (viewport shrinks for bar) — overlay only for v1.

## Open questions

- **Where does `ScrollState` get refreshed for the *wrapper*?** Today
  `Ui::end_frame` walks `scroll_nodes` and reads `result.rect[node]` /
  `result.scroll_content[node]` keyed by the scroll node. After the
  restructure, `viewport` should still come from the wrapper's rect (it
  is the visible area), but `scroll_content` is written on
  `ScrollContent`. Likely: `ScrollNode { id, wrapper_node,
  content_node }` so `end_frame` reads `rect[wrapper]` and
  `scroll_content[content]` separately.
- **Does the wrapper need its own `LayoutMode::Canvas`, or can we
  reuse `LayoutMode::Scroll` semantics?** Canvas is correct for
  positioned children; Scroll's own dispatch arm assumes a single
  scrolling content child. Cleanest: wrapper = Canvas, content = the
  Scroll-mode node.
