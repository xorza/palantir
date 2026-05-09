# Animations

Primitive shipped (`src/animation/` + `palantir-anim-derive`). See
`src/animation/animations.md` for design rationale (posture,
non-goals, damage-split decision). What's left is follow-up
consumers and unresolved tuning questions.

## Deferred consumers

Each is tracked here because it wants animation but is blocked on
something else; not "more animation primitive work."

- **Sliding tab-indicator bar.** Material-style underline that
  physically moves between active tabs. Today the toolbar tab swap
  fades via the per-button color animation — the *slide* variant
  needs a separate overlay rect with `Vec2` spring. Low value vs the
  fade-only version we already have.
- **Popup reveal/dismiss (alpha + scale).** Needs an API change so
  the popup widget controls when to stop recording — otherwise
  dismissal is instant (the popup vanishes the frame the host flips
  `open = false`, no chance to fade out). Track alongside
  `docs/popups.md`-equivalent work.
- **Smooth `Scroll::scroll_to(WidgetId)`.** Trivial spring upgrade
  once `scroll_to` exists; the scroll roadmap (`docs/roadmap/scroll.md`)
  has it as a Now item.

## Open questions

- **Spring physics quality at high refresh.** Semi-implicit Euler is
  fine at 60+ Hz. If 120 / 240 Hz hosts surface stiffness explosion
  at very-stiff springs, switch to the analytical critically-damped
  solution. No reports yet; park.
- **`request_repaint` granularity.** Bool today (next frame, period).
  If we ever animate at sub-refresh rates (e.g., 2 Hz pulse), upgrade
  to `Option<Duration>` ("repaint within at most N ms") so the host
  can sleep between ticks.
- **Cross-frame continuity on widget reappearance.** A popup that
  fades out, gets removed, then re-shows starts at `current = target`
  (no anim — first-touch snaps). If we ever want continuity, persist
  the row by domain key in a separate side-table; don't bolt it onto
  `WidgetId`.
- **Snap-if-close epsilon for compound types.** Currently shares the
  spring's `POS_EPS = 0.001` / `VEL_EPS = 0.01`. Tuned for normalized
  0..1 values; pixel-scale `Vec2` works because we hit settle quickly
  anyway. May need per-type thresholds if a `Background` animation
  visibly stutters before settling.
