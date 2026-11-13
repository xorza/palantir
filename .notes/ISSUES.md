# Open issues

- `atomic/text_atlas/gpu/stable_keys/prepare` and `steady_warm/prepare`, which rasterize nothing,
  run 8 to 15 % slower on the atlas pool than on its base (`1851f60e2`) on every bench host, about
  0.7 ns a glyph replayed.
