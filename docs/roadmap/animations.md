# Animations

Today there's no animation primitive. Widgets that want motion (hover
fade, scroll-to, tab indicator slide) hand-roll an `f32` in
`StateMap`, advance it from elapsed time on each frame, and call
`ui.request_repaint()` until done. Works, doesn't compose, every
widget reinvents easing.

## Why this matters

Animation is what separates "renders correctly" from "feels right."
The cost of *not* having a primitive isn't that things look bad — it's
that nobody bothers, because the per-widget bookkeeping (clock, easing
curve, request-repaint loop, terminate-on-removal) is tedious. With a
primitive, animating becomes `let t = ui.animate(id, 200ms,
EaseOutCubic)` and the rest is interpolation.

## What we want

Animation as a thin reader on top of `StateMap` + the existing
repaint-request path:

- **`Animation` struct in `StateMap`.** Per-`(WidgetId, slot)`
  start-time, duration, easing curve. `ui.animate(id, dur, ease)`
  returns the current 0..1 progress and registers a repaint request
  for the next frame if not finished.
- **Multiple slots per widget.** Hover, press, focus, custom — each
  is its own (id, slot). No clash.
- **Spring option.** Critically-damped spring (stiffness, damping,
  velocity) alongside duration-based easing — better for
  drag-release ("toss the panel back into place") and continuously
  retargetable values.
- **Frame budget.** Animations clamp to a max-substep so a hitched
  frame doesn't teleport. Settle threshold ends the animation
  cleanly so we stop requesting repaints.
- **Eviction.** Removed `WidgetId` drops the animation row in the
  same `removed` sweep as `StateMap`/caches.

## What it solves

- **Hover/press/focus transitions** without per-widget clock
  bookkeeping.
- **`scroll_to` smoothness** — pairs with the scroll roadmap's
  programmatic `scroll_to`.
- **Tab/selection indicators** that slide between targets.
- **Reveal/dismiss** for popups and tooltips (paired with layering).
- **Repaint scheduling** — one place that decides "frame is animating,
  request another" instead of every widget reinventing it.

## What it explicitly is not

- Not a timeline / keyframe editor. One value, one curve, one slot.
  Compose by stacking slots.
- Not a state-machine layer. Widgets still drive their own
  `Hover→Pressed→Released` logic; animation just smooths the
  resulting target value.
- Not GPU-side. Interpolation runs on the CPU during record;
  rendering sees only the final values.

Block on a real workload that wants it (tab transitions in showcase,
or scroll-to landing). Premature without that — the slot/spring
shapes calcify wrong.
