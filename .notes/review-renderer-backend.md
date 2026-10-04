# Review: renderer backend (`src/gpu`)

Whoever addresses an item deletes it.

Scope: `src/gpu` (`WgpuBackend` and everything it draws through), the
backend half of the renderer. Test code is out of scope.

## One fact, "this frame has a stencil", has two sources

- [ ] `src/gpu/frame/schedule/mod.rs:210-214`, `src/gpu/wgpu_backend/mod.rs:762`,
  `:840` — `for_each_step` and `render_groups` take `use_stencil: bool` and
  a `&MaskPlan` beside it. On a frame with no stencil, the plan is stale
  data from the last stencil frame, and only the bool keeps the walk from
  reading it (`quad_pipeline/mod.rs:58-62` documents this). Target: one
  `Option<&MaskPlan>` (or a stencil enum that holds the plan), so a
  frame with no stencil has no plan that it can read.
- [ ] `src/gpu/pipeline/quad_pipeline/mod.rs:62`, `:160` — the `MaskPlan`
  is the schedule's data, but it is a `pub(crate)` field of `QuadPipeline`,
  and the backend reads it there (`self.quad.mask_indices`). The quad
  pipeline only has to upload the mask quads. Target: the backend (or the
  schedule) owns the `MaskPlan` and the mask-quad scratch, and
  `QuadPipeline::stage_masks` takes the built quads to upload.

## Dependency cycle between the surface and the backend

- [ ] `src/gpu/surface/backbuffer.rs:4`, `:53`, `:105` — `Backbuffer::ensure`
  and `Backbuffer::new` take `&WgpuBackend` only to get the device and
  `backbuffer_bind_group` (`wgpu_backend/mod.rs:1004`). `WgpuBackend`
  imports `Backbuffer`, so `gpu::surface` and `gpu::wgpu_backend` depend on
  each other. Target: `Backbuffer::ensure(slot, device, binding, size,
  format)` with the shared texture binding (see the next-but-one group),
  and remove `WgpuBackend::backbuffer_bind_group`.

## The immediate-region contract is stated in three ways that disagree

- [ ] `src/gpu/pipeline/mod.rs:15-24`, `src/gpu/pipeline/pipeline_recipe.rs:66-69`,
  `src/gpu/surface/viewport.rs:1-6` — all three say that the viewport is
  written once per pass and stays valid across a pipeline switch, because
  every layout declares the same size. `render_groups`
  (`wgpu_backend/mod.rs:796-802`) re-pushes it after every rebind and says
  that it does not trust that contract. The code that runs is the re-push.
  Target: one statement of the rule. Each bind pushes the viewport, and
  every layout declares `IMMEDIATES_BYTES` because the prelude declares
  `Immediates` in every shader. Remove the "valid across a switch"
  reasoning from the three docs.

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
