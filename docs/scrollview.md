# Scroll — roadmap

Status: v1 shipped (`Scroll::vertical/horizontal/both`, wheel input, state
persistence, clip-cull at composer). What's left, ranked by impact.

## P1 — Scrollbars

Single missing affordance with the largest UX impact. Without bars, users
have no visible signal that content is scrollable, no position indicator,
no drag-to-pan, no click-on-track to page.

Scope:

- A thin overlay scrollbar per panned axis, drawn by `Scroll` itself
  using `Shape::RoundedRect` (track + thumb).
- Thumb size = `viewport / content`, position = `offset / (content - viewport)`.
  Both already on `ScrollState`.
- v1 always-visible overlay. Auto-hide (macOS-style fade) needs an
  animation tick — defer until something else wants the same infra.
- v1 reads-only (no drag): just the position indicator. Drag-on-thumb +
  click-on-track to page come second once `drag_delta` plumbing is wired
  through to a generic capture target (already exists on `InputState`,
  just needs a `Scroll`-side consumer).

Design questions to resolve up front:

- **Overlay vs reservation** — overlay (drawn over content) is simpler
  and modern-default; reservation (viewport shrinks) needs measure-pass
  changes. Pick overlay.
- **Style hooks** — colors via `Theme::scrollbar_*`, width as a constant
  for now.
- **Cross-axis behavior on `ScrollXY`** — two perpendicular bars; the
  corner where they meet stays empty (or a small gutter swatch).

Tests: thumb position/size for known offset+content, drag pans by
`viewport / content` ratio, click-on-track jumps a page.

Showcase: a tab with all three scroll flavors and visible bars.

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
