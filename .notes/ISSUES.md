# Open issues

- State rows keyed by a derived id that no node records are never swept, because the sweep removes
  only ids recorded last frame and absent this frame. They outlive their widget:
  `ColorField` and `ColorStrip` keep a `ColorSurface` (an `ImageHandle` and its CPU `Image`) under
  `id.with("surface")`, `DragValue` keeps `SuffixScratch` under `id.with("suffix")`, and `DockView`
  keeps `TabDrag` under `dock_id.with("drag")`.
- `Tooltip` keeps its process-wide warmup clock as a state row under a fabricated id
  (`"palantir.tooltip.global"`), a global stored as one widget's state.
