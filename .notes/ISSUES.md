# Issues

- `examples/theme.toml` is stale: `[checkbox]` / `[radio]` / `[switch]` are
  missing `padding`, `margin`, `check_pts`, and `track_aspect`, which
  `ToggleTheme` serializes without a skip attribute.
- `cargo doc` fails on `src/shape/mod.rs:44`: the public `Lower` trait's doc
  links `ShapeRecord` to `crate::scene::shapes::record`, a private module.
