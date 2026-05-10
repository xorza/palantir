# Damage rendering

## Done

- **Subtree-level damage cull at the encoder.** `encode_node` early-
  returns when no damage rect intersects the node's `screen_rect`.
  Bench shows ~4.6 % saving on sparse-damage frames over a 1k-node
  grid — see `multi-rect-damage.md` "Bench findings" for numbers.
- **`Cascade.visible_rect` fed to damage.** Cascade now carries
  `screen_rect ∩ ancestor_clip` alongside the raw transformed rect;
  damage diffs against `visible_rect` so scrolled-offscreen children
  inside a clipped viewport don't inflate the dirty region with
  pixels that never reach the framebuffer. Pinned by
  `src/ui/damage/tests.rs`.

## Next

- **Incremental hit-index rebuild.** Only update `HitIndex` for dirty
  - cascade-changed nodes.
- **Debug overlay: flash dirty nodes.** Rect outline landed with
  Step 6 of multi-rect; per-node flash uses `Damage.dirty` (now
  always populated in production).

## Later — lower-impact

- **Buffer-age awareness.** Multi-rect needs to diff against the
  right past frame when the swapchain is mailbox / multi-buffered.
  Iced's frame ringbuffer or Slint's `RepaintBufferType::SwappedBuffers`
  are the references. Defer until wgpu mailbox bites.
- **Stencil-mask damage path.** Single union scissor + stencil clip.
  Re-evaluate if we ever ship LCD subpixel text — per-rect scissor
  wraps a glyph cell incorrectly; stencil over the union doesn't.
- **Tighter damage on parent-transform animation.** A transformed
  subtree damages prev + curr screen rects (covered by
  `animated_parent_transform_unions_old_and_new_positions` ✓ — now
  produces 2 rects). A dedicated transform-cascade pass could
  collapse deep-subtree damage further. Workload-gated.
- **Manual damage verification.** Visual A/B against `damage = None`
  to catch missed diffs.
- **Damage × rounded clip fixture.** Partial-damage frames inside a
  `Surface::rounded(...)` panel are untested. Theory: `LoadOp::Clear(0)`
  per frame plus cmd-buffer replay handles it (every paint redraws
  the mask), but no fixture pins it. See `src/renderer/rounded-clip.md`.

## Open hazards (from review)

- **AA fringe leakage at scissor boundaries.** Encoder filter uses
  unpadded rect; backend scissor is padded by `DAMAGE_AA_PADDING`.
  Adjacent leaves whose AA fringe extends into the padded scissor
  may show stale pixels. No fixture today. See
  `multi-rect-damage.md` for the full symmetric framing.
