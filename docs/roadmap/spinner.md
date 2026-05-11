# Spinner

**Status:** design / proposal. Motivated by app code that wants a
generic "work in progress" indicator — busy buttons, async list
loaders, saving toasts.

Visual target: the 12-spoke macOS / Aqua-style throbber. Spokes
arranged at 30° intervals, each spoke a round-capped capsule, opacity
fading from a "head" spoke (full alpha) backward through the wheel
(tail ≈ 10–15 % alpha). The head advances one slot per `cycle / N`
seconds. Continuous, no settle state.

## Sub-problems

Today's primitives don't cover this on their own. Each blocker has
its own scope and ships standalone.

### A. No public hook for perpetual repaint

The spinner never settles. Today `Ui::repaint_requested` is
`pub(crate)` and only flipped by `Ui::animate` for a non-settled
animation row (`src/ui/mod.rs:374`). No widget-facing way to say
"tick me again next frame."

Add two public hooks on `Ui`:

```rust
// src/ui/mod.rs
impl Ui {
    /// Force the next frame to paint even when input is idle. Used
    /// by perpetual animations (spinner, marquee) that don't fit
    /// `Ui::animate`'s settle model.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
    }

    /// Last `now` argument passed to `run_frame`. Monotonic.
    /// Animation widgets compute phase from this so they don't
    /// thread `dt` through their own state.
    pub fn time(&self) -> std::time::Duration {
        self.time
    }
}
```

This aligns with `animations.md`'s open question about
`request_repaint` granularity and is what `TextEdit` already fakes
via `animate` for caret blink. If the open question's
`Option<Duration>` upgrade ever lands, `request_repaint` grows a
parameter.

### B. Authoring-time geometry without a measured rect

`Ui::node` runs the body closure during the *record* pass, before
measure / arrange. A widget cannot read its own arranged rect at
authoring time — `LayoutResult` doesn't exist yet. So the spinner
can't naively compute "leaf rect center + radius" inside its
recording closure.

Two real options:

- **(c1) Read prev-frame rect via `ui.response_for(id).rect`**, the
  pattern scrollbars use today (`scroll.md` lists the F+1 settle as
  a known bug). Needs a sensible fallback for F+0.
- **(c2) Constrain the spinner to a *deterministic* size** — set
  `element.size = Sizes::Fixed(DEFAULT_SIZE)` in `Spinner::new` so
  the resolved size equals the builder's declaration regardless of
  parent. Author spoke endpoints in *leaf-local* coords (origin =
  leaf top-left, size = `self.size`); the encoder translates by
  `owner_rect.min` at emit time, just like `RoundedRect` /
  `Text::local_rect: Some`. No prev-frame state, no F+1 settle.

**Recommend (c2).** Spinners are intrinsically small fixed-size
controls; a Fill/Hug spinner is a non-feature. (c2) costs a single
`.size(...)` line in `Spinner::new` and rules out a class of
first-frame visual bugs by construction.

`Shape::Line` endpoints are owner-relative — convention parity
with `RoundedRect`.

---

## Line renderer

`Shape::Line` is fully wired end-to-end via the polyline path
(`src/forest/shapes.rs`, encoder, composer) with structural `Hash`
and a `Lines` showcase tab. Cap/join style covered by
`Shape::Polyline`.

---

## Spinner widget

Built on top of §A, §B, and the existing line renderer. ~80 LOC,
no new abstractions.

### API

```rust
ui.show(Spinner::new());                    // 24×24, theme defaults
ui.show(Spinner::new().size(48.0));         // explicit square size
ui.show(Spinner::new().style(custom));      // custom theme
ui.show(Spinner::new().paused(!loading));   // freeze phase + skip repaint
```

```rust
pub struct Spinner {
    element: Element,
    style: Option<SpinnerTheme>,
    paused: bool,
}

impl Spinner {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.size = Sizes::from(SpinnerTheme::DEFAULT_SIZE);
        Self { element, style: None, paused: false }
    }
    pub fn size(self, px: f32) -> Self { /* element.size = Sizes::from(px); */ }
    pub fn style(self, t: SpinnerTheme) -> Self { … }
    pub fn paused(self, p: bool) -> Self { … }
    pub fn show(&self, ui: &mut Ui);
}

impl Configure for Spinner {
    fn element_mut(&mut self) -> &mut Element { &mut self.element }
}
```

`Spinner::show` returns `()`, not `Response` — no hover/click
semantics by default (apps wrap in a `Button` if they want one).

### Theme — `widgets/theme.rs`

Added next to `ButtonTheme` / `ScrollbarTheme`:

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    pub spoke_count: u8,         // 12
    pub cycle_secs: f32,         // 1.0
    pub spoke_color: Color,      // palette::TEXT_MUTED
    pub head_alpha: f32,         // 1.0
    pub tail_alpha: f32,         // 0.15
    pub spoke_thickness: f32,    // 2.0 logical px
    pub inner_radius_ratio: f32, // 0.40 of half-extent
    pub outer_radius_ratio: f32, // 0.95
    pub easing: Easing,          // Easing::OutCubic on the head→tail ramp
}

impl SpinnerTheme {
    pub const DEFAULT_SIZE: f32 = 24.0;
}
```

Wire into `Theme::default()` and `Theme::spinner: SpinnerTheme`
the same way `button: ButtonTheme` is wired.

### Recording

The spinner's resolved size equals its `element.size = Sizes::Fixed(s)`
declaration regardless of parent (Fixed is hard-contract per the
architecture doc), so spoke geometry is fully determined at
authoring time. No prev-frame state. Endpoints are owner-local —
the encoder translates by `owner_rect.min`.

```rust
pub fn show(&self, ui: &mut Ui) {
    let style = self.style.clone().unwrap_or_else(|| ui.theme.spinner.clone());
    let element = self.element;
    let s = match element.size.w {
        Sizing::Fixed(v) => v,
        // Spinner::new always sets Fixed; .size() preserves Fixed.
        // A hypothetical Hug-spinner would need a measured-rect path.
        _ => unreachable!("spinner size must be Fixed; see docs/roadmap/spinner.md §B"),
    };
    let half = s * 0.5;
    let center = Vec2::splat(half);   // leaf-local
    let r0 = half * style.inner_radius_ratio;
    let r1 = half * style.outer_radius_ratio;
    let n = style.spoke_count as f32;

    let phase = if self.paused {
        0.0
    } else {
        (ui.time().as_secs_f32() / style.cycle_secs).rem_euclid(1.0)
    };

    ui.node(element, |ui| {
        for i in 0..style.spoke_count {
            let theta = std::f32::consts::TAU * (i as f32 / n);
            let dir = Vec2::new(theta.cos(), theta.sin());
            let delta = ((i as f32 / n) - phase).rem_euclid(1.0);
            let t = style.easing.apply(delta);
            let alpha = (1.0 - t) * style.head_alpha + t * style.tail_alpha;
            let color = Color { a: style.spoke_color.a * alpha, ..style.spoke_color };
            ui.add_shape(Shape::Line {
                a: center + dir * r0,
                b: center + dir * r1,
                width: style.spoke_thickness,
                color,
            });
        }
    });

    if !self.paused {
        ui.request_repaint();
    }
}
```

(`Color` carries no `with_alpha` helper — struct-update syntax
multiplies the theme's own alpha by the per-spoke ramp, which is
the desired behavior when a theme deliberately picks a
semi-transparent base color.)

Per frame: `spoke_count` (default 12) `Shape::Line` pushes onto
`Tree.shapes`. Zero allocations — `Vec<Shape>` reuses capacity. No
`StateMap` row, no `AnimMap` row — fully derived from `ui.time()`.

### Tests — `widgets/tests/spinner.rs`

Following CLAUDE.md's "extend existing tests" guidance, group as a
single table-driven test where it makes sense:

- 12 spokes at theme defaults emit 12 `Shape::Line` entries; the
  spoke at index `floor(N * phase)` carries `head_alpha`, alphas
  monotonically decrease modulo `N`.
- `paused(true)` emits the same shapes as `phase = 0` and does
  **not** flip `FrameOutput::repaint_requested`. Existing
  `ui/tests.rs:580–636` repaint-flag pattern fits — extend it
  with a `paused`/`!paused` axis.
- Advance `now` by `cycle_secs / N` between two frames — the
  highest-alpha spoke advances by exactly one slot.
- `Spinner::new().show(ui)` reports a square `DEFAULT_SIZE` rect;
  `.size(48.0)` reports a 48×48 rect — both via
  `Response::rect()`-equivalent on the leaf.
- DPI smoke: at scale = 2.0 the encoded line width is at least 1
  physical px (anti-zero-width clamp in the line renderer).

### Showcase

New `examples/showcase/` tab "Spinner":

- Default 24 px spinner.
- Sizes row (16 / 24 / 48 / 96 px) showing thickness scales sensibly.
- Inside a button (`<spinner> Loading…` row) — verifies layout
  composition and asymmetric padding.
- Slider for `cycle_secs` (0.4 → 4.0) and `spoke_count` (3 → 24).
- "Pause" toggle proving `paused(true)` collapses repaint requests
  (debug HUD's `repaint_requested` flag goes quiet).

---

## Order of work

Each step ships standalone with its own tests + showcase wiring.

1. **`Ui::request_repaint()` + `Ui::time()` public** (§A). Trivial
   visibility flip + doc comments. Pin: calling `request_repaint`
   flips `FrameOutput.repaint_requested` for one frame and resets
   at the next `run_frame` (extends `ui/tests.rs:580–636`).
2. **`SpinnerTheme` + `Spinner` widget**. Pure composition over
   (1) and the existing line renderer. Showcase tab "Spinner".

Step 1 is independently useful for any future continuous
animation — even if step 2 changed shape we'd want it.

## Non-goals

- **Determinate / progress variant.** Single moving arc with
  `progress: f32` is a different widget (`ProgressArc`). Add later
  once arc / pie shapes show up; don't conflate.
- **Pulse / breathing variants.** Same primitive (one `f32` phase)
  but different visual; add via a separate widget when there's a
  caller.
- **`Animatable` slot for phase.** Phase isn't a tween between two
  values — it's `ui.time()` mod cycle. `AnimMap` is the wrong tool.
- **Hug / Fill spinner sizing.** Forces a measured-rect read with
  F+1 settle for no real authoring win. Locked to Fixed by
  construction.
- **CPU-side tessellation of capsules.** A pre-tessellated
  triangle-mesh path would land via `mesh-shapes.md`; spinners
  don't justify it on their own.

## Open questions

- **Repaint cost when many spinners are visible.**
  `request_repaint` is global — N spinners idempotently set the
  same flag. But a tab full of 50 spinners means 50 leaf encode
  misses per frame from alpha mutation. Park; if it surfaces, a
  virtualization clamp or hoisting phase into a single
  `Animatable f32` consumed by every spinner removes the per-leaf
  miss.
- **Sub-frame phase precision.** `ui.time()` ticks at host frame
  rate. At 60 Hz with `cycle = 1.0 s` the head moves ~83 ms / slot
  (5 frames) — visually smooth. At very fast cycles (< 0.3 s) the
  alpha ramp's fractional `delta` carries the smoothness. Pin a
  visual test once motion is real.
- **Anti-aliased thin lines at fractional logical width.** For
  `spoke_thickness < 1.0` the segment is sub-pixel; verify the
  line renderer's AA handling at first report.
