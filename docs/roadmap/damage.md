# Damage rendering

## Done

- **Multi-rect damage.** N=8 region with LVGL-style merge + Slint
  min-growth fallback at the cap; backend replays one render pass
  per rect; coverage threshold 0.7. Design rationale + open
  follow-ups in `multi-rect-damage.md`.

## Next

- **Subtree-level damage cull at the encoder.** Today the encoder
  walks every viewport-visible subtree and uses the damage filter
  only to skip leaf paint cmds (`DrawRect`/`DrawText`); Push/Pop
  pairs and recursion still happen. Mirror the existing viewport
  cull at `encoder/mod.rs:147` against `region.any_intersects(
  screen_rect)` so subtrees outside damage skip recursion entirely.
  **Soundness caveat:** `Cascade.screen_rect` is the node's own
  rect, not subtree bbox (`cascade.rs:176`) — descendants of
  Canvas / non-clipped panels / transformed nodes may overflow.
  The existing viewport cull already trusts this assumption "by
  convention"; damage cull inherits the same. Bench-gate before
  shipping: 1000-node tree + small damage rect → measure encoder
  time with vs. without.
- **Incremental hit-index rebuild.** Only update `HitIndex` for dirty
  + cascade-changed nodes.
- **Debug overlay: flash dirty nodes.** Rect outline landed with
  Step 6 of multi-rect; per-node flash needs `Damage.dirty`
  reintroduced to production (currently `#[cfg(test)]`).

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
  may show stale pixels. No fixture today.
- **`frame.damage` stale if host skips submit.** A debug-assert in
  `WgpuBackend::submit` ("we haven't seen `end_frame` since last
  submit") would catch host-loop bugs. Defer until filed.
