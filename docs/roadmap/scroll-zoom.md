# Scroll + Zoom

Proposal: extend `Scroll` so it can optionally zoom (uniform scale) its
content alongside the existing pan, gated per instance via a builder
flag.

## Motivation

Several common use cases want a viewport that pans **and** zooms:
diagram editors, image viewers, timeline / waveform views, large grids,
node graphs. Today, getting that requires the user to roll their own
transform widget on top of `Scroll` — duplicating wheel handling,
clamping, and gutter math. We already have all the pieces:

- `TranslateScale` is uniform-scale + translate (`primitives/transform.rs`),
  composes through the cascade, and the encoder already plumbs scale
  through clip / shape / text emit (`renderer/frontend/encoder/tests.rs:438`
  pins `xform.scale = 2.0`).
- `Scroll` already sets `inner.transform = Translate(-offset)`. Swapping
  in `TranslateScale::from_translate_scale_about(-offset, pivot, scale)`
  is a one-line content transform change.
- `ScrollLayoutState` is the natural home for one extra `f32` zoom field
  next to `offset`.

What's missing is an opt-in policy for the widget, an input source for
zoom deltas, and a measure-pass story that keeps content sizing sane
under scale.

## Survey

### egui — `Scene` (separate widget)

`egui/crates/egui/src/containers/scene.rs`. `Scene` is a sibling of
`ScrollArea`, not an extension of it: no scrollbars, no scroll limits,
just a `TSTransform` (scale + translate) over its child.
`register_pan_and_zoom` (line 229) reads `i.zoom_delta()` (cmd-scroll on
desktop, pinch on touch) and `i.smooth_scroll_delta()` for pan, then
applies a pivot-anchored zoom about the pointer:

```text
to_global = to_global
  * Translation(pointer_in_scene)
  * Scale(zoom_delta)
  * Translation(-pointer_in_scene)
```

Zoom range is clamped (`zoom_range: Rangef`); double-click resets via
`fit_to_rect_in_scene`. Demo at `egui_demo_lib/src/demo/scene.rs:55`
calls `.zoom_range(0.1..=2.0)`.

Takeaway: **egui keeps zoom out of the scroll area**. Different
contract — `Scene` has no measure-time content extent and no bars.

### Floem — `pan-zoom` example

`tmp/floem/examples/pan-zoom/src/pan_zoom_view.rs`. Custom view holding a
`kurbo::Affine`, animates between current and target scale over a
duration (`zoom_start_time`, `run_zoom_animation`). Pan is a drag delta
on the affine's translation; zoom is wheel-driven with a pivot at the
pointer. Notifies via `on_pan_zoom(Affine)` callback.

Takeaway: animation is opt-in polish; the core math is the same
pivot-anchored TS compose.

### Iced — `image::viewer`

`tmp/iced/widget/src/image/viewer.rs`. Specialized image viewer with
fixed min/max scale, pan-on-drag, zoom-on-scroll. Not a generic
container.

### WPF — `ScrollViewer` is pan-only

WPF separates `ScrollViewer` (pan + bars) from `ZoomBox` / a
`LayoutTransform` on the content. Same split as egui.

### Conventions across libs

- **Input:** `Cmd/Ctrl + wheel` zooms; bare wheel pans. Touchpad pinch
  (winit `WindowEvent::PinchGesture`) is the touch path.
- **Pivot:** at the pointer (so the point under the cursor is fixed
  across the zoom step). Without a pivot, content drifts and feels
  broken.
- **Clamping:** `zoom_range`, typically `0.1..=10.0` or tighter.
- **Bars under zoom:** the consensus is *don't*. Either drop bars
  (egui Scene) or treat the scaled content as a virtual content size
  for the bars. Floem and Iced go bars-less.

## Current implementation summary

`src/widgets/scroll.rs` (343 lines) builds an outer ZStack + inner
`LayoutMode::Scroll` panel. `ScrollLayoutState`
(`src/layout/scroll/mod.rs:74`) stores `offset / viewport / outer /
content / overflow / seen` keyed by inner-viewport `WidgetId` on
`LayoutEngine.scroll_states`. The scroll driver runs children with
`INF` on panned axes during measure (`scroll/mod.rs:103`), arrange
re-clamps offset, and the widget reads/mutates this row at record time
(`Ui::scroll_state`).

Wheel deltas: `InputState.frame_scroll_delta` accumulates logical
pixels with the sign flipped so positive == "advance the scroll
offset" (`input/mod.rs:106-119`: `LineDelta` is multiplied by
`SCROLL_LINE_PIXELS`, `PixelDelta` is divided by `scale_factor`, both
then negated). `scroll_delta_for(id)` returns the delta when `id` is
the current scroll hit-target. Modifiers are already tracked
(`input/mod.rs:209`, `modifiers_from_winit` at line 138). No pinch
handling today.

Transform plumbing already supports scale: `inner.transform: Option<TranslateScale>`,
cascade composes (`ui/cascade.rs:200`), encoder honors scale in
`PushTransform` and through `apply_rect` for clips and shapes
(`renderer/frontend/encoder/tests.rs`). Hit-test runs against
post-cascade screen rects, so scaled content hits correctly **for free**.

What's pan-only today and needs touching for zoom:

1. `Scroll::show` constructs `inner.transform = from_translation(-offset)`
   — no scale.
2. `ScrollLayoutState` has no zoom field, and `arrange`'s offset clamp
   uses raw `content - viewport` (`scroll/mod.rs:179-182`), unaware of
   scale.
3. `frame_scroll_delta` is consumed unconditionally by the active
   scroll target — there's no way to distinguish "ctrl+wheel for zoom"
   from "wheel for pan" at the route level. No pinch event ingest.
4. Bar reservation + thumb math (`bar_geometry`,
   `widgets/scroll.rs:51-70`; `bar_reservation`, line 33) reads
   `content` and `viewport` in pre-scale (content) coordinates, so
   under scale they'd be wrong.

Measure does **not** need changes: the driver runs children with `INF`
on panned axes (`scroll/mod.rs:111-128`), so children already report
natural extent independent of the eventual paint-time scale.

## Design

**Single widget, opt-in zoom, two-axis only.** Don't fork into a
`ZoomScroll`. Add a `zoom: Option<ZoomConfig>` to `Scroll`. When
`None` (default), behaviour is byte-identical to today.

**Zoom is restricted to `Scroll::both`.** Uniform scale on a single-axis
scroll has no clean answer — `Scroll::vertical` doesn't pan X, but
zooming would push content past the viewport on X with no way to reach
it (no horizontal bar, no horizontal wheel). Forcing the cross-axis to
become pannable under zoom contradicts the constructor's promise. The
clean rule: only `Scroll::both` accepts a `ZoomConfig`. `with_zoom*`
called on a `Scroll::vertical` / `Scroll::horizontal` instance asserts
at record time (`assert!(matches!(axes, ScrollAxes::Both), "zoom
requires Scroll::both")`) — caller bug, hard error.

```rust
pub struct ZoomConfig {
    pub range: RangeInclusive<f32>,    // default 0.1..=10.0
    pub step: f32,                     // multiplicative per wheel notch, default 1.1
    pub modifier: ZoomModifier,        // Ctrl | Cmd | Always | Pinch
    pub pivot: ZoomPivot,              // Pointer (default) | Center
}

pub enum ZoomModifier {
    /// Ctrl+wheel on desktop. Default.
    CtrlOrCmd,
    /// Plain wheel zooms (rare; for image viewers without pan).
    Always,
    /// Pinch only — wheel always pans. Touch-first apps.
    PinchOnly,
}

impl Scroll {
    /// Enable zoom with default `ZoomConfig`. Asserts `ScrollAxes::Both`.
    pub fn with_zoom(self) -> Self;
    /// Enable zoom with explicit config. Asserts `ScrollAxes::Both`.
    pub fn with_zoom_config(self, cfg: ZoomConfig) -> Self;
}
```

### State

Extend `ScrollLayoutState`:

```rust
pub(crate) struct ScrollLayoutState {
    pub(crate) offset: Vec2,
    pub(crate) zoom: f32,        // NEW; 1.0 default
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    pub(crate) content: Size,    // unscaled (pre-zoom)
    pub(crate) overflow: (bool, bool),
    pub(crate) seen: bool,
}
```

`Default::default()` sets `zoom = 1.0` (custom impl, since `f32::default()`
is `0.0`).

### Input: zoom delta

Add to `InputState`:

```rust
pub(crate) frame_zoom_delta: f32,        // multiplicative; 1.0 = no zoom
pub(crate) frame_zoom_pivot: Option<Vec2>, // pointer at gesture start
```

Sources, accumulated each frame:

- `WindowEvent::PinchGesture { delta, .. }` → `frame_zoom_delta *= 1.0 + delta as f32`
  (winit's delta is a small float per tick).
- `MouseWheel` is **always** ingested into `frame_scroll_delta` as
  today; routing into zoom vs pan is decided at the widget, since
  only the widget knows its own `ZoomConfig`. `frame_scroll_delta` is
  in sign-flipped logical pixels — to convert into a multiplicative
  factor at the widget, divide by `SCROLL_LINE_PIXELS` to recover
  "notches", then `step.powf(-notches.y)` (negative because positive
  `frame_scroll_delta.y` means scroll-down, which conventionally
  zooms *out*).

`Scroll::show` decides routing based on its `ZoomConfig`. For
`modifier = CtrlOrCmd`, the gate is `mods.ctrl || mods.meta` —
`Modifiers` (`src/input/keyboard.rs:53`) has no `command` field;
`any_command()` includes `alt`, which we don't want for zoom. When the
gate matches, the wheel delta is converted to a zoom factor and the
pan delta is *not* applied; otherwise pan as today. Modifier state is
sampled once at record time per scroll widget — releasing Ctrl
mid-frame still routes that frame's wheel ticks as zoom, which is
fine.

`PinchGesture` is unconditional: any `frame_zoom_delta` from pinch
applies regardless of `ZoomConfig::modifier`, since touch pinch
already disambiguates intent.

### Pivot-anchored compose

Each frame, given delta `dz` and pivot `p_widget` (in widget-local
coords, i.e. pointer position minus widget rect origin):

```rust
let new_zoom = (state.zoom * dz).clamp(*range.start(), *range.end());
// Effective dz after clamp:
let dz = new_zoom / state.zoom;
// Keep pivot fixed: solve for new offset so that
//   apply_point(p_widget) under (new_offset, new_zoom) == apply_point(p_widget) under (old_offset, old_zoom)
state.offset = (state.offset + p_widget) * dz - p_widget;
state.zoom = new_zoom;
```

Then re-clamp `offset` against `(content * zoom - viewport).max(0)` per
axis.

### Inner transform

```rust
inner.transform = Some(TranslateScale::new(-offset, zoom));
```

Drops the existing translate-only branch — `from_translation` becomes
a special case (`zoom == 1.0` works fine, no special path needed).

### Measure under zoom

The cleanest contract: **content is measured unscaled**. The transform
applies at paint/cascade time, not at measure time. So
`scroll/mod.rs::measure` passes the same `inner_avail` it always did.
Under zoom-out, content reflows according to the unscaled viewport
size; under zoom-in, content stays at its natural width. This matches
egui's `Scene` (unbounded measure) and avoids re-measuring the subtree
every zoom tick (which would defeat `MeasureCache`).

A future "fit to width" mode could measure with `viewport / zoom`
instead, but defer until a workload demands it.

### Bars under zoom

Bar geometry currently uses `(viewport, content)`. Under zoom, the
"effective content" for the user is `content * zoom`. Update
`push_bar` to take `content * zoom` along both axes (we're in the
`ScrollAxes::Both` branch); thumb size and offset then express the
visible fraction of the *scaled* content, which is what the user
perceives. `bar_reservation` still keys off overflow, and overflow
becomes `content * zoom > viewport`.

The same scaling has to land in `scroll/mod.rs::arrange`'s offset
clamp (currently `max_x = (content.w - viewport.w).max(0.0)`,
`scroll/mod.rs:179`). Rewrite as `(content.w * zoom -
viewport.w).max(0.0)` so wheel pan past zoomed-content's edge is
clamped correctly. `state.offset` lives in viewport (post-scale)
pixels — that matches today's behaviour at `zoom == 1.0`.

### Hit-test, damage, encode

No changes. Cascade already composes the `TranslateScale`, hit-test
runs against post-cascade screen rects, encoder already scales clips
and shapes. Damage rects come out of cascade-space rects, also fine.

Caveat: text under fractional zoom triggers re-shape per zoom value
unless we snap. Snap zoom to a quantized ladder for the cache key —
e.g. round to the nearest `2^(1/8)` step (~9% increments). Pin in a
test against `text_shaper_measure_calls` via `support::internals`.

### `Configure` / API

`Scroll` is unusual in that it wraps two elements (outer ZStack +
inner). Zoom config is a widget concern, not an `Element` concern, so
it lives on `Scroll` directly:

```rust
let resp = Scroll::both()
    .with_zoom_config(ZoomConfig {
        range: 0.5..=4.0,
        modifier: ZoomModifier::CtrlOrCmd,
        ..Default::default()
    })
    .show(ui, |ui| { /* ... */ });
```

Read-back: `ui.scroll_state(id.with("__viewport")).zoom` for tests /
external sync. Public API: keep zoom as opaque widget state for v1; if
a need arises (mini-map, "zoom: 100%" indicator), add
`Response::scroll_zoom` later.

## Implementation steps

Each step lands behind tests; all must keep `cargo test` green.

1. **State + arrange clamp.** Add `zoom: f32` to `ScrollLayoutState`
   (custom `Default` so it's `1.0`), thread it through `arrange`'s
   offset clamp at `scroll/mod.rs:179-182` (`max_x = (content.w * zoom
   - viewport.w).max(0.0)`, same on `y`). No widget changes yet — at
   `zoom == 1.0` behaviour is byte-identical.

2. **`InputState::frame_zoom_delta` + winit ingest.** Add the field,
   handle `WindowEvent::PinchGesture`, end-of-frame reset. Add a
   `scroll_input_for(id) -> ScrollFrameInput` helper. Existing tests
   still pass (`scroll_delta_for` unchanged).

3. **`ZoomConfig` + `Scroll::with_zoom*`.** Pure builder additions
   storing an `Option<ZoomConfig>` on `Scroll`. `with_zoom*` asserts
   the underlying `ScrollAxes` is `Both`. No behaviour yet.

4. **Wheel-zoom routing.** In `Scroll::show`, when `zoom.is_some()` and
   `cfg.modifier` matches current `Modifiers`, route
   `frame_scroll_delta` into zoom (multiplicative, with `step`),
   zeroing pan. Otherwise route to pan as today. Pivot from
   `ui.input.pointer_pos()` minus the widget's screen rect origin
   (we have `response_for(id).rect`).

5. **Pivot-anchored offset + zoom mutation + clamp.** The math above,
   in `Scroll::show`'s state-mutation block. Update `inner.transform`
   to `TranslateScale::new(-offset, zoom)`.

6. **Bar geometry under zoom.** Multiply `content` by `zoom` when
   computing reservation + thumb, and when comparing against viewport
   for `overflow`. Add a test in
   `src/widgets/tests/scroll.rs` driving zoom from 1.0 → 2.0 and
   asserting bar thumb shrinks proportionally.

7. **Pinch gesture path.** Wire `PinchGesture` through to
   `frame_zoom_delta`. Test with a synthetic pinch event in
   `input/tests.rs`.

8. **Zoom-step ladder for text cache.** Snap `state.zoom` to the
   ladder before writing it (or before passing to children); pin
   `text_shaper_measure_calls` doesn't grow on continuous zoom within
   one ladder bucket.

9. **Showcase tab.** Add a `pan-zoom` page under `examples/showcase/`
   with a large image / dense grid that exercises pivot anchoring
   visually. Eyeball: cursor-pinned point shouldn't drift across zoom
   ticks.

10. **`scroll_to(WidgetId)` under zoom.** When `scroll_to` lands
    (already in `docs/roadmap/scroll.md` "Now"), it must **preserve
    the current `state.zoom`** and center the target's rect in the
    viewport. Math:

    ```text
    target_center_local = layout.rect[target].center() - inner_origin
    desired_offset = target_center_local * zoom - viewport.size * 0.5
    state.offset = clamp(desired_offset, 0, content * zoom - viewport)
    ```

    Centering, not edge-aligning, is the right default — under zoom
    the user almost always wants the target visible *with context
    around it*, not pinned to a corner. Resetting zoom to 1.0 would
    surprise the user mid-navigation; explicit `Scroll::reset_zoom()`
    can be added separately if needed.

11. **Roadmap entries.** Cross-link from `docs/roadmap/scroll.md`
    "Now" items (`Drag-to-pan thumb`, `scroll_to(WidgetId)`) to this
    doc so the zoom-aware behaviour is implemented in the same change
    set. Add an "inertia / smooth zoom" `Later` bullet for Floem-style
    animation.

## Resolved decisions

- **Axes:** zoom is `Scroll::both`-only. Single-axis stays pan-only.
- **`scroll_to(target)`:** preserves the current `zoom`, centers the
  target's rect in the viewport, clamps offset to scaled content.
- **Modifier gate:** `mods.ctrl || mods.meta` (not `any_command()`,
  which includes `alt`).
- **Measure:** content measures unscaled. Zoom is a paint-time
  transform; `MeasureCache` keeps hitting across zoom changes.
- **Bar coordinates:** bars use `content * zoom` for thumb +
  reservation + overflow.

## Open questions

- **Persistent zoom vs reset shortcut.** Zoom persists in
  `ScrollLayoutState`. A `Scroll::reset_zoom()` programmatic API and
  a Cmd+0 keyboard binding are trivial follow-ups; defer until
  someone wants them.
- **Damage under continuous zoom.** Every zoom tick is full-viewport
  damage on the inner clip. Acceptable for v1; flag in
  `docs/roadmap/damage.md` if a workload surfaces it.
- **Zoom step shape.** Multiplicative `step.powf(notches)` is
  proposed. Some apps prefer additive on touchpad (smoother). Revisit
  after the showcase tab lands and we can feel it.
