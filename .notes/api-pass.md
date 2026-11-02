# API pass

The public API changes the palantir review proposed and its batches did not
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
- **`MenuItem::selected(bool)`** with a `Selection` axis in `MenuItemTheme`, so the combo list
  and the tab overflow menu mark the pick; their rows take stable ids, and a keyboard open focuses
  the selected row.
- **`MenuBar::open/close/is_open(ui, bar)`** and public `title_id` / `menu_id`, as ContextMenu and
  ComboBox have.
- **Overlay part ids**: one part word for the overlay a trigger drops (today `popup`, `body`,
  `bubble`, `panel`, `list`) and public `*_id` functions (`PopupTrigger::popup_id`,
  `ContextMenu::menu_id`, `Tooltip::bubble_id`); `Tooltip::on` drops the `'a` on its snapshot.
- **`Popup::style` and `PopupTrigger::style`** taking `impl Into<Option<&PopupTheme>>`, as
  `PopupTheme`'s doc names them; `ColorPickerTheme::popup` becomes a `PopupTheme`.
- **One key map per role** in `axis_keys`: a slider map for Slider and ColorStrip (whose scope
  takes Up and Down and acts on neither), a spin-button map with Page Up/Down and Home/End for
  DragValue; `DragValue::step`.
- **`DragValue::edit` and `is_editing(ui, id)`**, as EditableLabel has.
- **`DragNum` renamed** for both users (`BoundNum`), with `Num` and `StoreRule` in its module.
- **`ColorStrip::hue` and `alpha`** in place of `for_hue` and `for_alpha`, as `for_` is `for_id`'s.
- **Hover and press looks** for ColorButton and ColorSwatch: a button-shaped slot in
  `ColorPickerTheme`, applied through `wear`.
- **`MenuItem::show(self, ui)`**: the menu's scope closes on any activated row, so no row threads a
  `&CloseHandle`.
- **`TabbedView::show<R>`** with `TabbedViewResponse::inner: Option<R>`, and `closable` defaulting
  to `false`.

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

## Theme walk

- **`#[animate(text)]`** on the `Animatable` derive, emitting the theme's text-slot walk
  (`for_each_text`), so the 15 hand-written walks go. The attribute widens the public derive's
  contract, so it waits with the `Lerp` split above. Refused: one walk over a single tree plus a
  pairing helper, as `Theme::share_text_axes` reads both ends of a lerp at once, and pairing two
  single walks needs scratch per lerp, an allocation each frame of a theme transition.

## GPU

- **`Gpu::create_render_pipeline` and its compute twin** made public and used by the examples'
  `GpuPaint::init`, so an app's builds take the pipeline-build turn; `Gpu::new` documents that its
  turn is not shared with another `Gpu` on the same device.
- **`DeviceFeatures` and `SurfaceUsages` removed**: the two error variants name the wgpu flag
  types, and `wgpu_mirror!` goes. The mirrors exist for a wgpu-free surface that `pub use wgpu`
  and `RequestedGpu::headless(PowerPreference, wgpu::Features)` already contradict.
