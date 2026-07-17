# Animations — design

Implementation: `src/animation/`. This doc captures the durable design
rationale — what posture we picked, what we deliberately didn't build,
and the one load-bearing architectural split.

## Posture

- **Side-channel readback, not a reactive layer.** Animation rows
  live next to `StateMap`. Widget code calls `let v = ui.animate(...)`
  during record and uses `v` inline. No signals, no effects, no
  animator-as-widget (Makepad regret), no CSS-style declarative
  transitions on properties (Vizia regret), no thread-local reactive
  runtime (Floem regret).
- **Two specs, one entry point.** Duration + easing for designed
  motion; validated damped spring for retargetable motion. Durations
  are finite and at most 60 seconds. Springs must decay by at least
  1/s and fit within the bounded adaptive-integration budget. Caller
  picks; primitive dispatches.
- **Frame-driven, not wallclock.** WindowDriver hands `now: Duration` to
  `Ui::frame`; the frame runtime's `dt` is derived. No `Instant::now()` in widget
  code — keeps animation deterministic and host-portable.
- **No new authoring model.** A widget still does
  `Button::new().show(ui)`. Animation is something it reaches for
  inside `show()` to soften a discrete state change.
- **Animation is opt-in.** Default `theme.button.anim = None` means
  snap. Tests pass `dt = 0.0` and stay deterministic.

## Damage interaction (load-bearing)

A widget that animates a *visible* property — color, stroke width,
text color — flows the new value into a `Shape`, which mutates the
per-node hash, which the existing `Damage` pass picks up and paints.
**No new damage hook needed.**

The `repaint_requested` flag is orthogonal: it forces *the next frame
to run* even when input is idle. Without it, the host sleeps until
the user moves the mouse and frames between settled-input +
finished-animation never paint. Damage on the next frame still
decides what redraws.

This split is the one architectural decision that affects readers of
other subsystems (damage, frame loop). Don't fuse the two — paint and
wake have different consumers.

## Non-goals

- **Timeline / keyframes.** One value, one curve, one slot. Compose
  by stacking slots.
- **State machines.** Widgets still drive `Hover→Pressed→Released`
  with booleans. Animation only smooths the resulting target.
- **Per-property declarative transitions.** No `transition: bg-color
  200ms` syntax — no styling layer to attach it to, and Vizia's
  experience says it creates a dual source-of-truth with imperative
  setters.
- **GPU-side interpolation.** All math is CPU during record. WindowDriver
  sees only finalized values.
