# Open issues

- A drop shadow's quad is its source moved and grown by `ShadowGeom::halo`
  (`3σ + spread`), but its coverage reaches `AA_HALF_WIDTH` further: an
  unblurred shadow whose edge falls between pixel centres loses the outer
  half of its antialiasing ramp to the quad edge.
- `ShadowGeom::halo` cuts a drop shadow's quad at `3σ` on the claim that
  the Gaussian's tail is below one 8-bit step there. That holds in linear
  light only: the cut leaves `Φ(−3) ≈ 0.00135` of coverage, which a light
  shadow over a dark background encodes to about 4 sRGB steps, so a glow
  ends in a visible ledge at the quad edge.
