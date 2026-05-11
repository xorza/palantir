# Speculative — profile-gated

- **Skip cascade/encode recursion under empty clip.** Composer-level
  cull already drops leaves; recursion skip trickier (Active /
  future focus may want off-screen live).
- **Coarser `available_q` quantization (measure).** Bump from 1 px on
  sub-pixel parent drift.
- **Cold-cache mitigations (measure cache).** Skip-collapsed,
  size-threshold, amortized compact — if resize jank shows.
- **Spatial index for hit-test at high N.** Quad-tree / BVH; matters
  at thousands of nodes.
- **Contiguous children slices.** Clay's `int32_t*`-into-shared-array
  for cache locality and BFS (SUMMARY §5).
