# Open issues

- A drop shadow's quad is its source moved and grown by `ShadowGeom::halo`
  (`3σ + spread`), but its coverage reaches `AA_HALF_WIDTH` further: an
  unblurred shadow whose edge falls between pixel centres loses the outer
  half of its antialiasing ramp to the quad edge.
