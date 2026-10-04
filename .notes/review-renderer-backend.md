# Review: renderer backend (`src/gpu`)

Whoever addresses an item deletes it.

Scope: `src/gpu` (`WgpuBackend` and everything it draws through), the
backend half of the renderer. Test code is out of scope.

## Two texture-binding shapes where the code says there is one

- [ ] `src/gpu/resource/gpu_gradient_atlas.rs:53-58`, `:87`, `:92` —
  `GpuGradientAtlas` builds its own `texture_binding::layout` and
  `texture_binding::sampler`. These are the same shape and the same
  sampler as `ImageBinding` (`image_binding.rs:13`), which the images, the
  `GpuView` targets, the blit and the backbuffer share. Thus the quad and
  curve pipelines bind a second, equal layout, against the backbuffer
  doc's claim of "the one layout every sampled texture here shares".
  Target: one `TextureBinding` value (the current `ImageBinding`, renamed),
  built once by the backend and cloned into the gradient atlas, the image
  store, the view targets and the backbuffer. Then `GpuGradientAtlas`
  keeps no `bgl` or `sampler` of its own.

## Arguments threaded down the schedule walk

- [ ] `src/gpu/frame/schedule/mod.rs:210`, `:522`, `:555` — `buffer`,
  `damage_scissor`, `masks`, `cursors` and `state` go through
  `for_each_step` → `drain_text_batches` (6 arguments) and
  `emit_group_body` (7 arguments), and `PassState` already holds the walk's
  other state. Target: one walk struct (`PassState` widened, or a `Walk`
  that owns it) that holds the frame's inputs and cursors, with
  `drain_text_batches` and `emit_group_body` as its methods.
- [ ] `src/gpu/frame/schedule/mod.rs:55` — `build_mask_plan` is a free
  `pub(crate)` function that fills a `&mut MaskPlan`. Target:
  `MaskPlan::build(&mut self, buffer, masks)`.

## A per-type rule checked at run time, and an alignment rule not checked

- [ ] `src/gpu/resource/dynamic_buffer.rs:57-68` — `DynamicBuffer::new`
  asserts `size_of::<T>() != 0` at run time, although it is a property of
  `T`. The belt write also requires the byte count to be a multiple of
  `COPY_BUFFER_ALIGNMENT` (4), and nothing checks that. A `T` with an odd
  size panics in `StagingBelt::write_buffer` on the first upload of an odd
  count. Target: `const { assert!(size_of::<T>() != 0 &&
  size_of::<T>() % wgpu::COPY_BUFFER_ALIGNMENT as usize == 0) }`, which
  fails when the type is instantiated.

## Docs that still describe the raster sampler and the atlas-size immediates

These were removed on the `shaders` branch, but the text below still
describes them.

- [ ] `src/gpu/wgpu_backend/mod.rs:174` — `raster` field: "The one shader,
  group-0 layout and sampler". Remove "and sampler".
- [ ] `src/gpu/wgpu_backend/mod.rs:559-564` — the text-prepare comment says
  that the atlas-size params ride the immediate region and that
  `RasterPass::render_batch` pushes them per batch. Remove those sentences.
- [ ] `src/gpu/wgpu_backend/mod.rs:780-782` — `Bound::Raster`: "the bind
  group, the atlas extents and the vertex buffer differ". Remove "the atlas
  extents".
- [ ] `src/gpu/raster/text_backend/mod.rs:26-27` — "No `Viewport` object.
  Atlas sizes ride the shared immediate region as two `u32`s, pushed per
  batch". Remove the bullet, or say that the shader reads texels by index.
- [ ] `src/gpu/raster/raster_atlas/mod.rs:8-9`, `:187` — "plus the layout
  and sampler their bind groups are built against", and "Everything group
  0 needs to sample". There is no sampler, and the shader loads texels.
- [ ] `src/gpu/raster/raster_atlas/mod.rs:258` — "Order matches
  `ContentType as usize`: [Mask, RgbaF32]". The variant is `Color`.
- [ ] `src/gpu/resource/texture_binding.rs:58-59` — "The raster atlases
  build their own, and should: they sample at exactly one texel per pixel
  and want `Nearest`". They have no sampler now.
- [ ] `src/gpu/resource/wgpu_image_store.rs:160-161` — "one texel per pixel
  with `Nearest`". Say that the raster shader reads texels by index.
- [ ] `src/gpu/pipeline/mesh_pipeline/mod.rs:136` — "re-pushed by the
  backend's `rebind!`". `rebind` is a function, not a macro.

## The debug overlay repeats the quad bind sequence

- [ ] `src/gpu/frame/overlay_pass.rs:194` — `draw_quads` repeats
  `QuadPipeline::bind_buffer` (`quad_pipeline/mod.rs:86`): set the pipeline,
  the gradient group and the vertex buffer. Target: make `bind_buffer`
  `pub(crate)` (or give `QuadPipeline` a method that binds a
  `SingleQuadBuffer` or an overlay buffer), so that the overlay calls it.
