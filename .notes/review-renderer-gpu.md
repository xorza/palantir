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
