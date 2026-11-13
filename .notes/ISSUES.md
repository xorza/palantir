# Open issues

- `atomic/text_atlas/gpu/stable_keys/prepare` and `steady_warm/prepare`, which rasterize nothing,
  run 8 to 15 % slower on the atlas pool than on its base (`1851f60e2`) on every bench host, about
  0.7 ns a glyph replayed.
- On a device whose passes move their whole target, a frame that bakes a cutout table or blurs a
  backdrop result that lies on a page opens a load-and-store pass over the whole page, the cost
  a drain's moves avoid by copy.
- The spun raster pipelines compile with the flat ones for every pair of slot sets a session meets
  and at startup, whether or not a frame turns a glyph or an icon.
