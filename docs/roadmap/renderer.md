# Renderer / GPU

## Later — workload-gated

- **Instance buffer capacity-retention audit.** Confirm encode →
  compose → backend retain `Vec` capacity.
- **wgpu staging belt.** Replace ad-hoc `queue.write_buffer` with
  `StagingBelt`.
- **Offscreen render targets / mask layer.** Blocks blur, masked
  compositing, tab transitions. (Drop shadows don't need it — see
  below.)
- **SDF drop shadows.** Slice 1 shipped — see
  `docs/roadmap/shadow.md`. Inset shadows (slice 2) and
  `Background::shadow` sugar (slice 4) still open.
- **Sprite atlas for icons.** Bin-packed texture array (mono +
  polychrome), instanced sampler — same shape as the glyph atlas.
  Replaces ad-hoc icon paths and makes SVG/raster icons cheap to
  draw at scale.
- **Push constants vs shared UBO** for camera / scissor (SUMMARY §12.5).
- **Nested rounded clips.** Today's stencil path handles a single
  rounded level per group via the write/clear cycle. Multi-level
  nesting needs a stencil ref counter (Increment on push, Decrement
  on pop; compare = Equal against the active depth). `Stencil8`
  supports 255 levels. See `src/renderer/rounded-clip.md`.
