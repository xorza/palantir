# Renderer / GPU

## Later — workload-gated

- **Instance buffer capacity-retention audit.** Confirm encode →
  compose → backend retain `Vec` capacity.
- **wgpu staging belt.** Replace ad-hoc `queue.write_buffer` with
  `StagingBelt`.
- **Offscreen render targets / mask layer.** Blocks blur, masked
  compositing, tab transitions. (Drop shadows don't need it — see
  below.)
- **SDF drop shadows.** Add `Shape::Shadow { rect, corners, blur,
  spread, color, offset }` with a closed-form Gaussian approximation
  in the shader (Evan Wallace's erf-trick — same one GPUI uses).
  One instanced primitive type, batched alongside rounded rects, no
  offscreen pass needed for the common case (solid shadow under a
  rounded rect). Cheap, and it's the visual line between "looks like
  a UI toolkit" and "looks designed." Inner shadows + multi-shadow
  stacks fall out of the same shader.
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
