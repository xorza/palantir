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
  - `RasterProgram` moves into `gpu::pipeline`.
  - Quad draws are keyed once per instance (`QuadDraw::{Form, Shadow}`), and the pipeline knows
    which groups each key binds. The backend does not branch on `QuadForm::Backdrop`.

## Plan

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
- [ ] `src/gpu/pipeline/raster_pipeline/shader.wgsl:150-171`.
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
