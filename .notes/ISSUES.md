# Open issues

- **`ColorPicker::show` overrides the caller's `gap`.** `src/widgets/color_picker/mod.rs:168` sets `self.widget.gap(gap)` from the theme, so a caller's `.gap(..)` on a `ColorPicker` is replaced.
