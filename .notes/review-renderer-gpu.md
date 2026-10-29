# Review: renderer and GPU usage, with the backdrop integration (2026-10-29)

Scope: `src/renderer/`, `src/gpu/`, and the code that drives them from `src/host/window_driver/`,
`src/damage/pane_walk.rs` and `src/cascade/pane_row.rs`. Four reviews ran in parallel: backdrop
integration end to end, backend orchestration, pipelines and atlases, and the CPU frontend. Every
bug below was checked against the code. The test suite runs in 4.6 s (2133 tests) with no slow
test.

## Proposal

### Root causes

1. **There is no per-window render stream on the GPU side.** Without it:
   - the host holds GPU attachments (backbuffer, stencil, freshness, cutout plan);
   - the backend keeps parallel owner-keyed maps (timings, backdrop cache, backdrop target
     groups).
2. **Paint-order arbitration has no single owner.** Each draw kind hand-writes cull → order →
   register → pane break in its own shape. One of these paths misses a check (bug B1), and the
   reach rect of a turned quad is derived in two ways (bug B3).
3. **Shared mechanics live inside one consumer or in a hub.**
   - `ShaderBody` holds every shader's path and constants, which gives `pipeline ⇄ raster` and
     `pipeline ⇄ shader_body` module cycles.
   - Each pipeline hand-rolls the same scaffolding with a different bind and upload shape.
4. **Rust and WGSL logic mirrors are not pinned.** The shadow spread rules and the shadow box are
   written twice, and no test compares the two copies. (The buffer structs are pinned by naga
   reflection: `ShaderBody::assert_struct_layout`.)

### Target design

- **`Backdrops` (gpu), now a slot allocator `(owner, key) → Kept` under the pane ledger.**
  - It is made on the first frame that has panes, not at backend start (8.4 MB atlas, 5 blur
    pipelines).
  - Its work unit is the break (`BlurBreak`): timing, copies and blurs go one break at a time.
  - `BackdropBlur` borrows the jobs and does not copy them. `BlurJob` carries its own footprint.
- **`RenderStream` (gpu, per owner).** It holds the backbuffer with its blur-source group, the
  stencil, the backbuffer freshness, the timings, the cutout plan, and the cached sampled groups
  of the swapchain images. It retires as one unit. The host passes `submit(owner, target,
  &FramePlan)`, and the backend builds a pure, testable `FramePasses` plan and runs it through one
  `DrawCtx`.
- **`PaintOrder` (composer).** It owns the batch, the higher-kind rects, the group cursors, the
  occlusion pruner and the pane break, with one `admit(lane, bounds)` over the total order
  Quad < Text < Mesh < Image < Icon < Curve. Each draw kind's lowering hands it the one reach rect
  that its instance type defines, the same rect the pruner uses.
- **Pipeline families.**
  - Each family owns its shader source, its constants, and an instance layout derived from
    `offset_of!`.
  - They share an `InstancedPipeline<T>` with one upload, bind and draw shape.
  - `RasterProgram` moves into `gpu::pipeline`.
  - Quad draws are keyed once per instance (`QuadDraw::{Form, Shadow}`), and the pipeline knows
    which groups each key binds. The backend does not branch on `QuadForm::Backdrop`.

## Plan

### Batch 6: the backend's pass plan and one draw context

Files: `src/gpu/wgpu_backend/`, `src/gpu/pipeline/quad_pipeline/`, `src/gpu/frame/schedule/`.
Needs batch 5.

- [ ] `wgpu_backend/mod.rs:277-411,562-687`.
  - Defect: the pass plan is decided inline while encoding: the dim pre-pass, each segment's
    LoadOp, the breaks, copy-out against `draw_onto`, the overlay pass. The backend has no unit
    test.
  - Target: a pure `FramePasses::plan(..) -> [PassStep]`, tested like `Schedule`, and a thin
    executor.
- [ ] `wgpu_backend/mod.rs:614-860` `render_groups`.
  - Defect: each tenant's binding is built by hand:
    - `fmt.X.select(stencil)` appears 8 times;
    - the mark and debug-group triad appears 8 times;
    - `render_groups` has 6 parameters besides `self` and `run_main_segment` has 5;
    - `(from, barrier): (u32, Option<u32>)` is a tuple parameter, also in `Walk::run_between`.
  - Target: a `DrawCtx`, one wrapper for the mark and debug group, and a named `GroupSpan`.
- [ ] `wgpu_backend/mod.rs:783-791`, `quad_form/mod.rs:16-30`, `quad_pipeline/mod.rs:287-307,357-371`,
  `schedule/mod.rs:302-316`.
  - Defect: the backend branches on `run.key == QuadForm::Backdrop` to choose the bind method. The
    backdrop form is chosen by entry point, not by the `QUAD_FORM` override. One quad stream is
    split three times: shadows, forms, shadow entries.
  - Target: `QuadDraw { Form(QuadForm) | Shadow(ShadowEntry) }`, computed once per instance, and
    `QuadPipeline::bind_draw(key, groups)`.
- [ ] `wgpu_backend/mod.rs:10-12`.
  - Defect: the module doc says every pipeline "spells the same six steps"; text and icon do not.
  - Target: a doc that matches, or nothing.

### Batch 7: one owner of paint order in the composer

Files: `src/renderer/frontend/composer/` (`session.rs`, `mod.rs`, `rect_grid/`, `tile_index.rs`,
`occlusion_pruner.rs`, `higher_kind_rects.rs`, `pane_break.rs`), `src/renderer/frontend/paint_sink/`,
`src/renderer/frontend/payload/draw_quad_payload/`, `src/renderer/frontend/encoder/plane_ctx.rs`.
The bugs these paths caused are fixed; this batch removes their cause.

- [ ] `session.rs` (1281 lines).
  - Defect: per-kind lowering is mixed with paint-order arbitration. Quad (806-815), text
    (760-793) and the tiers (`admit_higher_kind`, 1147-1162) each hand-write cull, order and
    register: `pane_break.paint` is called three times, and polyline clamps twice.
  - Target: `PaintOrder` with one `admit(lane, bounds)`. Text's cross-group batch stays the one
    documented exception. Per-kind lowering goes in its own files.
- [ ] `draw_quad_payload/mod.rs:118-123,270-272`, `session.rs:288,296,799,858-866`,
  `plane_ctx.rs:85-91`.
  - Defect: backdrop branches leak into the generic quad code. `DrawPanePayload` holds the
    backdrop twice (the `fill_axis` lanes and `backdrop`), and `faded` updates only the lanes.
    `push_quad(_, Option<Pane>)` and `paint_rect(.., pane: Option<(LoweredBackdrop, PaneKey)>, ..)`
    are `Option` parameters that select behavior, the second also a five-parameter fn with a tuple.
  - Target: the pane is its own payload, lowered to a `Quad` once.
- [ ] `tile_index.rs:110-131`, `rect_grid/mod.rs:43-51`.
  - Defect: `RectGrid` inserts a rect into every tile it spans. A viewport-wide curve bbox at 4K
    is 2040 inserts, and `OcclusionPruner` already bypasses large entries over the same
    `TileIndex`.
  - Target: the large-entry lane moves into `TileIndex` and serves both.
- [ ] `composer/mod.rs:166-174`.
  - Defect: four `BatchState` fields are reset one by one, and "`open_grid` non-empty ⇔
    `open.is_some()`" is not encoded anywhere.
  - Target: `BatchState::start_frame` and `close`.
- [ ] `paint_sink/mod.rs:107-180`.
  - Defect: eight identical fade → `is_noop` → forward gates. Polyline instances rely on a zeroed
    `fill_kind` that matches `FillKind::SOLID` (`session.rs:692-705,719-730`).
  - Target: one gate over a payload trait. Set `fill_kind` explicitly.
- [ ] `session.rs:660-677,691,712-713`.
  - Defect: `seg_colors` runs three times per segment (about 30k fades for a 10k-point polyline).
  - Target: once per segment, into `PolylineScratch`.
- [ ] Tests:
  - order and cull of turned draws;
  - a pane under an opaque quad;
  - the AA-fringe overlap.
  - `composer/bench.rs` gets quad, clip, rounded-clip, prune, polyline and pane cases.

### Batch 8: shader ownership and pipeline families

Files: `src/gpu/pipeline/` (all families, `shader_body/`, `format_pipelines.rs`),
`src/gpu/raster/raster_program.rs`, `raster_atlas/shader.wgsl`, `raster_quad.rs`,
`src/gpu/backdrop/backdrop_blur/`. Needs batch 6.

- [ ] `shader_body/mod.rs:3-18,36-129`.
  - Defect: one hub holds every shader's path and constants, which gives the cycles
    `quad_pipeline ⇄ shader_body`, `raster ⇄ shader_body` and `backdrop ⇄ shader_body`.
  - Target: each family owns `const SHADER` and `constants()`. `shader_body` keeps the prelude
    and `specialize_source`, and the list that the tests walk is one slice in
    `gpu/pipeline/mod.rs`.
- [ ] `format_pipelines.rs:9`, `raster_program.rs:185-188`.
  - Defect: `gpu::pipeline` and `gpu::raster` import each other. `RasterProgram` is a pipeline
    filed among the atlases.
  - Target: move it, with `RasterQuad` and its shader, to `gpu/pipeline/raster_pipeline/`.
- [ ] Each `*_ATTRS` (`quad_pipeline/mod.rs:437-463`, `curve_pipeline/mod.rs:137-168`,
  `image_pipeline/mod.rs:150-172`, `mesh_pipeline/mod.rs:174-210`, `raster_quad.rs:94-132`,
  `cutout_tables.rs:168-186`).
  - Defect: offsets packed by `vertex_attr_array!`, then checked against `offset_of!` by about 40
    hand asserts.
  - Target: attributes built from `offset_of!` in a `const`.
- [ ] `quad_pipeline/mod.rs:136-430`, `curve_pipeline/mod.rs:111-134`,
  `mesh_pipeline/mod.rs:103-160`, `image_pipeline/mod.rs:110-147`, `raster_pass.rs:258-279`.
  - Defect: five bind shapes and four upload shapes. The quad "upload" also records the cutout
    bake render pass.
  - Target: `InstancedPipeline<T: GpuInstance>`, and a `prepare_cutouts` step for the bake.
- [ ] `quad_pipeline/shader.wgsl:1097-1101` against `prelude.wgsl:90-94`.
  - Defect: `srgb_decode` is the prelude's `srgb_to_linear`.
  - Target: delete it, and move `srgb_encode` to the prelude.
- [ ] `shader.wgsl:1027-1045` against `1196-1215`.
  - Defect: `fs_backdrop` copies `fs`'s tail.
  - Target: one `shade_rect(in, fill, d)`.
- [ ] `quad_pipeline/shader.wgsl:6-7,642-652` against `curve_pipeline/shader.wgsl:59-60,418-434`.
  - Defect: the gradient bindings and the LUT read are written twice.
  - Target: a shared include for the shaders that bind the gradient group.
- [ ] `cutout_plan/mod.rs:428-475,520-532` against `shader.wgsl:519-529,1225-1250`.
  - Defect: `spread_radius` and the shadow box exist in Rust and in WGSL. The comment at line 519
    says "Only the shader applies it", which is false. `radius_rules_follow_the_shader` never runs
    the shader.
  - Target: fix the comment, and add a GPU cross-check of both copies on one table.
- [ ] `cutout_plan/mod.rs:225-229,293-317`, `cutout_tables.rs:133`.
  - Defect: one 16 B `CornerTables` per quad of the frame (+25 % over the 64 B quad stream) is
    uploaded whenever any shadow exists. In `Shade` mode it is all `NONE` and still uploaded.
  - Target: a compact per-shadow array indexed by the shadow's ordinal in `fill_lut_row`, and a
    static `NONE` stream in `Shade` mode.
- [ ] `shader.wgsl:436-454,607-631`, `quad_pipeline/mod.rs:417`, `pipeline_recipe.rs:89`.
  - Defect: nine fields re-bound to locals, narrating comments, and receiver-less associated fns
    that construct nothing.
  - Target: trim them, and make the two fns free fns.
- [ ] `blit_pipeline/mod.rs:64-65`.
  - Defect: the blit writes every texel with blending off but loads with `Clear`.
  - Target: `LoadOp::DontCare`, an invalidate on GLES.
- [ ] Tests:
  - the `QuadForm::of` table adds `Backdrop` and the shadow tags;
  - `curve_indices_tile_adjacent_cross_sections` (`curve_pipeline/mod.rs:177-200`) checks hand
    indices, not the generator's own formula;
  - `shader_body/tests.rs` (`reachable_functions`, `strip_comments`) walks the WGSL call graph
    by hand; naga, now a dev-dependency, gives the module's call graph;
  - remove `a_span_past_the_keys_panics` (`runs.rs:80-85`, std's bounds check) and
    `raster_quad_is_32_bytes` (already pinned by `hot_struct_sizes.rs:218`).

### Batch 9: the spin vocabulary

Files: `src/renderer/frontend/payload/spin.rs` and the payloads, `src/renderer/render_buffer/`
(`text_draw_row.rs`, `icon_draw_row.rs`), `src/gpu/raster/raster_pass.rs:18`,
`src/renderer/frontend/encoder/plane_ctx.rs:221-227`.

- [ ] Defect:
  - `render_buffer` and `gpu::raster` import `frontend::payload::spin::Spin`, which gives the cycle
    `payload ⇄ render_buffer` (`draw_image_payload.rs:8` → `render_buffer::image_flags`).
  - One type names a logical pivot in a payload and a physical pivot in a row.
  - The payloads use four pivot conventions, and the triangle is turned on the CPU.
- [ ] Target: one logical `Spin` (pivot relative to the payload origin) on every payload, the
  triangle included, and a physical `RowSpin` owned by `render_buffer`.

### Batch 10: present strategies

Files: `src/host/offscreen.rs:109-150`, `src/host/window_driver/mod.rs:197-216`,
`src/host/headless_window/mod.rs:95`, `benches/AGENTS.md:51-57`.

- [ ] Defect:
  - `retained_target(true)` says "small damage repaints straight into the target", but it maps to
    `DirectAdaptive`, which paints every partial frame below 0.4 coverage through the backbuffer
    and copies the whole surface out: 179 µs per partial frame on the 680M (PERF.md).
  - A direct full frame leaves the backbuffer stale, so the next partial frame becomes whole.
  - `HeadlessWindow` always sets the flag. The benches rely on it to emulate the winit host.
- [ ] Target: a retained strategy whose partial frame loads and paints the target itself, and an
  internals knob for the benches' swapchain emulation.

### Batch 11: smaller items

- [ ] `src/renderer/gradient_atlas/mod.rs:225-229` `grow`.
  - Defect: `mark_below(grown)` uploads every row, the zero rows too: 8 MiB in one frame for
    2048 → 4096. The comment's reason is inverted. The `counters.rs` doc of `rows_uploaded` is
    stale.
  - Target: a texture-to-texture copy of the old rows, which uploads nothing, and a correct doc.
- [ ] `frontend/mod.rs:43,79`, `composer/mod.rs:79`, `texture_limit/mod.rs:7-10`.
  - Defect: `TextureLimit` says it is "the one ceiling", but `Frontend` and `Composer` take a raw
    `NonZeroU32`. `TEST_MAX_TEXTURE_DIM` calls 8192 the downlevel default; that default is 2048,
    and 8192 is `Limits::default`.
  - Target: pass `TextureLimit`, and fix the doc.
- [ ] `composer/mod.rs:24-25`, `compose_rig.rs`.
  - Defect: a test and bench harness in its own gated module, which is none of AGENTS.md's four
    homes.
  - Target: move it to the composer's end-of-file `internals` or to `src/internals/`.
- [ ] `src/gpu/surface/owned_target/mod.rs:134-205` against `surface/capture.rs:67-246`.
  - Defect: `read_back` re-implements `Capture` (row padding, staging, swizzle). Texture to `UVec2`
    is written three times, and `Extent3d` is built by hand four times.
  - Target: `Capture` reads any `&Texture`, and `render_target::extent` everywhere.
- [ ] `src/gpu/resource/texture_region.rs`.
  - Defect: one caller, whose `first_row` is always 0. Its doc names a gradient-atlas user that no
    longer exists.
  - Target: inline it into `WgpuImageStore::write`.
- [ ] `src/primitives/paint/brush/gradient/color_ramp.rs:177`.
  - Defect: a test in `primitives` imports `renderer::gradient_atlas::bake`, an edge to the right.
  - Target: move the cross-check to `gradient_atlas/tests/bake.rs`.
- [ ] Stale references:
  - `prelude.wgsl:65` and `mesh_pipeline/shader.wgsl:14` cite an AGENTS.md "Color pipeline"
    section that does not exist;
  - `curve_pipeline/shader.wgsl:36` cites `render_buffer/curve.rs`, not `curve_instance.rs`;
  - `evict_one` is named in `text_backend/encode/cache/mod.rs:131`, `text_backend/bench.rs:45`,
    `text_backend/tests/mod.rs:22` and `raster_atlas/tests.rs:79`;
  - `mesh_pipeline/mod.rs:4` says the vertex stream is "content-stable", but it uploads every
    frame;
  - `gpu_ctx.rs:1-9` lists its fields;
  - the `FrameDraw.stats` doc (`wgpu_backend/mod.rs:167-170`) gives a reason that does not
    follow;
  - the `PresentStrategy` doc repeats `present_path`.
- [ ] `src/gpu/raster/raster_atlas/shader.wgsl:150-171`.
  - Defect: the color side stores straight alpha and premultiplies per tap, while
    `WgpuImageStore` premultiplies at upload.
  - Target: premultiply at upload.
- [ ] `src/gpu/error.rs` `DriverError` (public API, approved).
  - Defect: it boxes `RequestAdapterError`, `RequestDeviceError` and `CreateSurfaceError` as
    `dyn Error`, though the crate does `pub use wgpu`.
  - Target: one variant per failure, each holding its wgpu error type as its `source`.
    Regenerate `.notes/API_SURFACE.md`.
- [ ] `src/gpu/test_gpu.rs:46-60`.
  - Defect: the lease keeps device and queue apart, and `handles()` clones a `Gpu` on each call.
  - Target: the lease holds a `Gpu`.

### Batch 12: recover from a lost device

Decided 2026-10-29: recover, as the WebGPU spec advises for a loss the app did not cause and as
Zed's gpui does on wgpu (zed-industries/zed PR #50898). Needs batch 5, whose `RenderStream` is
what a recovery rebuilds. The public API shape (the `GpuPaint` hook, the error variant) is
proposed before it lands.

- [ ] `src/gpu/device/gpu.rs:56-63`, `src/gpu/surface/capture.rs:221-226`,
  `src/gpu/surface/window_surface.rs:104-106`.
  - Defect: no `set_device_lost_callback` or `on_uncaptured_error` is set. A GPU reset panics in
    `Gpu::wait`, in `Capture::take`, or in wgpu's default handler on the next submit. Surface
    `Lost` is folded into `Outdated`, which only rebuilds the swapchain.
  - Target:
    - a device-lost callback, for any reason but `Destroyed`, sets a flag that the host reads
      before each frame;
    - the host requests a new adapter and device, and rebuilds the backend, every window's
      stream and surface, the atlases, and the registered images from their CPU copies; the next
      frame of every window paints whole, and the UI state stays;
    - `GpuPaint` gets a hook that builds its resources again on the new device;
    - when no adapter or device comes back, `run()` returns a device-lost error, and no frame is
      submitted to the lost device.
  - Tests: a headless host recovered through the recovery's internal entry paints the same
    pixels as a fresh one. wgpu has no call that loses a device for a reason other than
    `Destroyed`, so the callback's flag is set by hand.
