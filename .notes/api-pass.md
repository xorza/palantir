# API pass

The public API changes the palantir review (`review-palantir.md`) proposed and the batches did not
apply, each waiting for a go-ahead (AGENTS.md, "Public API"). Whoever applies one deletes it, and
regenerates `API_SURFACE.md` with `cargo drive surface`.

## Players

- **Harness recovery**: `Harness::is_device_lost` and `recover`, as `OffscreenHost` has, or the
  harness reopening its own headless device, so darkroom's MCP session survives a TDR.
- **`PlayError`**: `Duration` in place of `seconds: f32` for `NoFrame` and `Busy`; drop `step`
  from `Unsettled` and `Busy` (darkroom rewrites it, and the two players format it differently).
- **`Remote`**: `find`; a play-limits config or `with_wait(Duration)` (darkroom cannot set its
  bounds for the window backend); `&mut self` on `play`, `settle`, `shot` in place of `Cell`s.

## Theme

- **`Theme::frost_floats`** without the `Palette` argument: the inks come from the theme frosted.
- **`Lerp` split** from `Animatable`, with its own derive.
- **`DragValueTheme`** reshaped to `{ looks, defaults, caret, caret_width, selection,
  placeholder }`, the editor slot derived at `show`; 8 dead `selected` looks go.
- **Drop the bundles' `Default`** (each is the dark recipe), or name the scheme.

## Widgets

- **`ComboBox::menu_style`**.
- **`Expander::open/close/is_open(ui, id)`** in place of the `&mut bool` binding named `open`.
- **Wrapper hooks made public**: `PopupTrigger::layer`, `PopupTrigger::menu_look`,
  `Popup::menu_look`, `Splitter::min_ratio`, `TabStrip::drop_caret`.
- **`default_line_gap`** and the rest of `ThemeDefaults` a widget needs (`default_justify` exists).
- **`EditableLabel`**: focusable, opened by Enter or F2, with `stop_editing(ui, id)`.
- **Disabled looks** for Slider and the color widgets (a look plan with a disabled state changes
  their theme types).

## Animation

- **An animate sibling returning `{ value, settled }`**.
- **One spelling of "no motion"**: `AnimationSpec` with serde default `SNAP`, or `SNAP` removed.
- **Reduced motion for paint animations**: a decorative marker on `PaintAnimation`, or a doc gate
  on `add_shape_animated`.
- **`Easing::curve(self) -> PaintCurve`**; `PaintCurve`'s doc corrected to `[0, 1]`.

## Values

- **`Span`**: `TryFrom<Range<usize>>` in place of a `From` that panics.
- **`Corners::new`** asserting lanes as `Spacing::new` does, with a crate-private saturating twin.

## Images after a device loss

- **`ImageHandle::is_resident()`**, cleared by `WgpuImageStore::attach`, in place of
  `Ui::device_restored()` as the signal to upload again. It questions a recorded decision
  (AGENTS.md, "Device loss": "The app uploads its images again when `Ui::device_restored()` is
  true"): the signal is per window for host-wide state, so an app with two windows uploads each
  image twice, and no in-tree consumer calls it (darkroom's previews stay blank after a winit
  recovery). The policy stays; the truth moves onto the handle.
