# Open issues

- State rows keyed by a derived id that no node records are never swept, because the sweep removes
  only ids recorded last frame and absent this frame. They outlive their widget:
  `ColorField` and `ColorStrip` keep a `ColorSurface` (an `ImageHandle` and its CPU `Image`) under
  `id.with("surface")`, `DragValue` keeps `SuffixScratch` under `id.with("suffix")`, and `DockView`
  keeps `TabDrag` under `dock_id.with("drag")`.
- `Tooltip` keeps its process-wide warmup clock as a state row under a fabricated id
  (`"palantir.tooltip.global"`), a global stored as one widget's state.
- A widget that declares an input scope and reads its own keys with `Ui::key_pressed` before it
  opens its node reads as the enclosing scope, not as itself, so under any enclosing scope (an app
  root's, a popup's) the key is granted to the widget and the read misses. `Slider`, the toggles
  (`ToggleChrome::activated`), `Expander`'s header, `Button`, `ColorButton`, `ComboBox`,
  `MenuItem` and the `Splitter` divider all read this way.
