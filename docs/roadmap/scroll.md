# Scroll

## Now

- **`Scroll::scroll_to(WidgetId)`.** Compute target rect from
  `LayoutResult.rect`, set `ScrollState.offset`, clamp. One-frame-stale
  for just-recorded targets — defer the fallback.

## Next — drag-to-pan follow-ups

- **Hover-grow thumb.** `thumb_hover` already lights up; add a width
  bump on hover via the theme.

## Next

- **Dominant-font wheel step.** Today's `Scroll` derives the line
  step from `ui.theme.text` (`font_size * line_height_mult`). The
  future-polish version walks the scrolled content to find the
  dominant font and uses *its* line height — useful when a panel
  overrides the text theme but the surrounding chrome doesn't.

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
