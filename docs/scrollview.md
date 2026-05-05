# Scroll — roadmap

Status: v1 + v1a (scrollbar visuals) shipped. `Scroll::vertical/horizontal/both`,
wheel input, state persistence, clip-cull at composer, and overlay
indicator bars (track + thumb, themed via `Theme::scrollbar`) drawn
automatically when content overflows on a panned axis. What's left,
ranked by impact.

## P1 — Drag-to-pan on the thumb (v1.1 of scrollbars)

Scrollbars draw today but aren't interactive. To make the thumb grabbable,
the `Scroll` widget needs a tree restructure (see `scrollbars.md`):

- Wrapper node holds `Sense::Scroll` and the user's sizing.
- Inner content node carries the pan transform + clip + the underlying
  `LayoutMode::Scroll(axes)` measure semantics.
- Per-axis bar leaves as siblings of the content node, with
  `Sense::Drag` and stable derived ids — `drag_delta(bar_id) *
  (content - viewport) / (track - thumb)` accumulates into
  `ScrollState.offset` alongside the existing wheel pan.

Design questions resolved in `scrollbars.md`. Cost: real refactor of the
Scroll widget's tree; modest change to `Ui::end_frame` (wrapper rect for
viewport, inner-node `scroll_content` for content). Click-on-track-to-
page falls out for free once bar leaves exist.

## P2 — `Scroll::scroll_to(WidgetId)` / `scroll_into_view`

Common pattern: list-with-selection wants "ensure selected row is
visible." Cheap to ship: compute target rect from `LayoutResult.rect`,
set `ScrollState.offset`, clamp.

Caveat: clamping uses last frame's `content`/`viewport`, so a
just-recorded target's rect may not exist yet. v1 = next-frame settles
(same one-frame-stale model the wheel path uses); revisit if it bites.

Tests: scroll_to a node above viewport pulls offset back to 0; below
viewport pushes to `target.bottom - viewport`; already-visible target is
a no-op.

## P3 — Wheel step from font metrics

Wheel `LineDelta(0, 1)` currently maps to a fixed 40 logical px/line
(`SCROLL_LINE_PIXELS` in `src/input/mod.rs`). Once cosmic shaping is in
the steady-state path, swap for line-height of the dominant font in the
scroll's content. Modest polish; only matters for text-heavy lists.

## P4 — Polish, defer until motivated

- **Smooth / inertia scrolling.** Velocity decay + `request_repaint`
  loop. Real UX win on touchpads but needs an animation tick infra
  consumer to share. Too early.
- **Bounce / rubber-band at edges.** Pure feel polish.
- **Touch drag-to-scroll.** No touch-input plumbing in winit binding
  today. Wait for a real touch workload.
- **Keyboard scrolling** (`PgUp`/`PgDn`/`Home`/`End`). Needs a focus
  system, which Palantir doesn't have.

## P5 — Out of scope without a workload

- **Sticky / pinned headers.** Layout integration is non-trivial; ship
  when something actually wants them.
- **Virtualization** (only render visible children). Major architectural
  lift — see the separate item in `roadmap.md`. Only path to
  `O(viewport)` measure cost. Today encode/measure are `O(content)` and
  the composer cull keeps GPU/CPU bounded; fine up to hundreds of items.
- **Nested scroll-chaining.** v1 = innermost hit-test wins. Browsers
  chain to parent when child reaches its end; defer until somebody wants
  it.
- **Skip cascade/encode recursion under empty clip.** Composer-level
  cull already drops the leaf shapes; recursion-level skip is trickier
  (Active capture and future focus may want off-screen rects live).
  Defer until a profile asks.

## Open questions

- **First-frame offset is clamped to zero bounds** because content size
  isn't known yet. Acceptable — first frame can't have a wheel event
  anyway. Revisit if `scroll_to` lands and someone tries to teleport
  on frame 0.
- **Hover suppression during scroll**: should a scrolled-into widget
  keep hover during the gesture, or does the gesture suppress it? Match
  egui (suppress) when a real choice has to be made.
