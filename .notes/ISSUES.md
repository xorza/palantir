# Open issues

- `BackdropFilter::with_max_luminance_for` and `with_min_luminance_for` `debug_assert!` that the
  tint's darkest channel times its alpha clears the sRGB curve's linear toe; their `# Panics`
  sections do not name it, so a faint tint (alpha 0) panics in a debug build only.
