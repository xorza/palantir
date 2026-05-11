# Scroll

## Now

- **`Scroll::scroll_to(WidgetId)`.** Compute target rect from
  `LayoutResult.rect`, set `ScrollState.offset`, clamp. One-frame-stale
  for just-recorded targets — defer the fallback.

## Next — drag-to-pan follow-ups

- **Click-on-track to page.** Track is still a shape, not a leaf — add
  a `Sense::CLICK` track leaf under the overlay, page on press.
- **Hover-grow thumb.** `thumb_hover` already lights up; add a width
  bump on hover via the theme.

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
