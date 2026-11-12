# Open issues

- A theme file's field that no theme type names reads as nothing and the rest loads: a file that
  still names a renamed field, as `blur` for a material's `backdrop_blur`, loads with that field at
  its default and no error.
- A raster atlas side that grew past its soft budget (`RasterAtlas::allocate` through
  `PackedAtlas::grow_past_budget`) keeps that edge for the life of the device, though
  `src/gpu/AGENTS.md` (GPU memory) has a cache shrink to its budget once its live set fits: one
  spike leaves each of the four text and icon sides at up to 16 MiB.
- The gradient LUT atlas only grows (`CpuGradientAtlas::grow`, "a one-way ratchet", up to 4096
  rows: 8 MiB of texture and 8 MiB of CPU rows), the high-water mark `src/gpu/AGENTS.md` (GPU
  memory) refuses.
- A backdrop result wider or taller than the results atlas's budget edge (2048 texels), as a glass
  toolbar across a 2560 px surface at full resolution, keeps the atlas past its budget for good:
  `PackedAtlas::shrink_to_budget` cannot place it, so a 4096² `Rgba16Float` atlas, 128 MiB, stays
  while 10.4 MiB is live (`cargo atlas-memory`, `backdrop/glass/*`).
