# Open issues

- **A theme-file length of `1e30` loads, and the frame that draws it fails.** The file accepts
  any finite, non-negative length. Rendering then panics for `toggle.box_size` (at
  `widgets/radio/mod.rs:93`), `scrollbar.thickness`, `scrollbar.gap`, the splitter
  `grab_thickness` and `rule_thickness`, `tabs.cap_thickness`, `tabs.trailing_inset`,
  `expander.indent`, and two `width: 1.5` fields (lines 339 and 739 of the default theme's
  RON). For `color_picker.field_width`, `field_height` and `bar_thickness`, the process aborts
  on an allocation of about 900 GB. The walk in `theme/tests/file_values.rs` finds them with
  `1e30` in `SUSPECTS`.

- **The default theme does not survive a save and load.** `Palette` border alphas such as
  `border_soft`'s `0.18` are off the 8-bit grid the hex color format holds, so a saved default
  theme loads back with `0.18039216`.
