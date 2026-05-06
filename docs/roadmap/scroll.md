# Scroll

## Now

- **First-frame bar settle.** Bars don't appear on the frame a scroll
  is first recorded — `Scroll::show` reads `ScrollState.viewport` /
  `.content` from the *previous* frame's measure, so on cold mount
  (showcase tab switch, scroll appearing inside a freshly-mounted
  subtree) `bar_geometry` returns `None` and no bar shapes get
  pushed. Frame N+1 then has populated state and bars appear — but
  the host (showcase / hello world) only re-requests a redraw on
  input, so until the user moves the mouse the bars stay invisible.
  Pre-existing 2-frame settle (`record_two_frames` in
  `widgets/tests/scroll.rs` is exactly this), surfaced now that
  scrollbars are the user's first stop. Three options ordered by
  invasiveness: (a) `FrameOutput.needs_settle: bool` set when any
  scroll's state was default-initialized this frame, host force-
  schedules another redraw; (b) push bar shapes from a post-arrange
  step that reads the current frame's `LayoutResult.scroll_content`
  instead of stale `ScrollState`, killing the settle entirely;
  (c) measure-time bar emission as a layout-pass output. (b) is the
  cleanest — eliminates the footgun for any user-built widget that
  depends on measured size.
- **Drag-to-pan scrollbar thumb.** Replace overlay shapes with per-axis
  bar leaf nodes (`Sense::Drag`, derived ids `("scroll-vbar", parent_id)`).
  `state.offset.main += drag_delta * (content - viewport) / (track - thumb)`,
  clamp. Click-to-page + hover-grow fall out once leaves exist.
- **`Scroll::scroll_to(WidgetId)`.** Compute target rect from
  `LayoutResult.rect`, set `ScrollState.offset`, clamp. One-frame-stale
  for just-recorded targets — defer the fallback.

## Next

- **Wheel step from font metrics.** Drop fixed 40 px/line
  (`SCROLL_LINE_PIXELS`); use line-height of dominant font in
  scrolled content.

## Later — workload-gated

- **Virtualization** — virtual-children hook over Flutter's slivers;
  only path to O(viewport) measure.
- **Inertia scrolling** — velocity decay + `request_repaint`. Needs
  animation-tick consumer.
- **Bounce / rubber-band.** Pure feel.
- **Touch drag.** No touch plumbing today.
- **Keyboard scrolling** (PgUp/Dn/Home/End). Needs focus.
- **Sticky / pinned headers.** Non-trivial layout integration.
- **Nested scroll-chaining.** Browsers chain to parent at child end;
  v1 = innermost wins.
