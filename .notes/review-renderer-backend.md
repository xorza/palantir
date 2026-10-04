# Review: renderer backend (`src/gpu`)

Whoever addresses an item deletes it.

Scope: `src/gpu` (`WgpuBackend` and everything it draws through), the
backend half of the renderer. Test code is out of scope.

## The debug overlay repeats the quad bind sequence

- [ ] `src/gpu/frame/overlay_pass.rs:194` — `draw_quads` repeats
  `QuadPipeline::bind_buffer` (`quad_pipeline/mod.rs:86`): set the pipeline,
  the gradient group and the vertex buffer. Target: make `bind_buffer`
  `pub(crate)` (or give `QuadPipeline` a method that binds a
  `SingleQuadBuffer` or an overlay buffer), so that the overlay calls it.
