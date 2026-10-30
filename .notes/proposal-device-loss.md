# Proposal: recover from a lost device (2026-10-30)

Facts and sources: `.notes/research/2026-10-30-device-loss.md`. This file is temporary: when the
work lands, the lasting decisions move to a `## Device loss` section of AGENTS.md, and this file
is deleted.

## Decisions

All approved on 2026-10-30:

- **Recover, do not panic.** On a loss the host gets a new device and rebuilds every GPU object
  on it; the UI state stays and the next frame of every window paints whole. Refused: a panic,
  which a TDR in a long image-processing shader would turn into a crash. Source: MDN
  `GPUDevice.lost`, Zed PR #50898.
- **Registered images: the app uploads them again.** On the first frame after a recovery,
  `Ui::device_restored()` is true, and the app calls `ImageHandle::update` for the images it
  still shows. Until then they paint nothing. Refused: a CPU copy of every image, which doubles
  the RAM of darkroom's 64 MP images (256 MB each as RGBA8). Source: DirectX
  `IDeviceNotify::OnDeviceRestored`, Android `onSurfaceCreated`.
- **`GpuPaint::init` runs again on the new device.** The recovery drops every `GpuView` target,
  so `init` runs as it does when a view comes back; only its doc changes. Refused: a separate
  `device_lost` hook, as Rust drops the old handles when `init` replaces them.
- **`WinitHostError::DeviceLost { source: GpuRequestError }`** when no new device comes back:
  `run()` returns it, and no frame goes to the lost device. Refused: reusing `Gpu`, which a
  caller could not tell from a startup failure.
- **`OffscreenHost` reports, and recovers on a device the app gives.** `frame` stops
  submitting after the loss, `is_device_lost()` returns true, and `recover(gpu)` rebuilds as the
  winit host does. `HeadlessWindow` forwards both and makes its texture again. Refused: a new
  host (loses the UI state) and a panic. Source: DirectX DeviceResources.

- **`Gpu` owns the device's health.** `Gpu::new` sets the device-lost callback and the
  uncaptured-error handler, so every `Gpu`, the app's included, knows when its device is lost.
  The handler panics on an error while the device is healthy, as wgpu's default does, and drops
  it once the device is lost. Refused: setting them in each host, which leaves a `Gpu` without
  a health and two places to install them. Cost: an app that sets its own callback on a device
  it hands to `Gpu::new` loses it; the doc of `Gpu::new` says so. Source: wgpu
  `api/device.rs:506,681`.
- **No poll on the frame path, and no call that panics on a loss.** Each `queue.submit` already
  runs the map and work-done callbacks (wgpu `api/buffer.rs:363`, wgpu-core
  `device/queue.rs:1541`), so the two timing tracers drop their per-frame poll and read their
  readbacks at the next submit. Where nothing submits, `Gpu::poll` runs the callbacks with an
  empty `queue.submit`, whose device error wgpu drops on a lost device (wgpu
  `backend/wgpu_core.rs:304,2109-2115`): only on a still frame while a screenshot copy is on its
  way back. `Gpu::wait` blocks without `device.poll`: it registers `on_submitted_work_done` and
  loops an empty submit and a 100 µs sleep until the callback runs or the device reads lost;
  only `wait_for_gpu`, `read_back` and tests call it. `clippy.toml` forbids
  `wgpu::Device::poll`, `wgpu::Instance::poll_all`, `set_device_lost_callback` and
  `on_uncaptured_error` outside `gpu::device::gpu`. Refused: `device.poll`, which wgpu 30 (and
  trunk) makes fatal on a lost device (`backend/wgpu_core.rs:888,1924`); `catch_unwind`, which
  `panic = "abort"` makes useless, as downstream wgpu apps found. When wgpu gives `PollError` a
  lost case, both bodies return to `device.poll` and no caller changes.
- **The device's limits are shared state.** A new adapter can have another
  `max_texture_dimension_2d`, so `TextureLimit` moves from a copy in each `UiResources` clone to
  one shared cell the recovery updates; the gradient atlas clears to the new ceiling, the
  surfaces clamp again, and a registered image past it paints nothing, with a `tracing::warn`.
  `Ui::max_image_dimension` stops being `const`. Refused: requesting the old limits, which fails
  a recovery onto an integrated GPU that could run the app; keeping the smaller of the two, which
  hides the same overflow.
- **A recovery runs the first init again.** Startup and a recovery attach a device through the
  same calls: `SurfaceManager::open_device` gets the device, `HostDevice::open` builds every
  device-bound object over the CPU resources, `HostCore::attach_window` gives a window its
  stream, and `SurfaceManager::build_window_surface` configures a surface. Startup calls them
  once; a recovery calls them again, and no code tells the two apart. Refused: a recovery path
  of its own, a second copy of the init that drifts from it.
- **A capture the loss broke is taken again.** A screenshot whose copy failed goes back to the
  window's requests and the next frame takes it; `Capture::take` returns `None` for a failed
  map. Refused: no image ever landing, which leaves the app waiting on `screenshot_taken`.
- **Device-bound state is rebuilt, not repaired.** The backend, the window streams and the
  view targets are made again with the constructors they have; no type gets a "reset every
  field" path that a new field could miss. The CPU atlases that the GPU mirrors (gradient rows)
  are cleared and refill as the next frame registers them, as Zed's atlas does. Only the image
  store resets in place, as every `ImageHandle` holds its `Rc`. Refused: a `reset(&Gpu)` on each
  GPU type, a second constructor that a field added later can miss. Source: Zed PR #54349,
  DirectX DeviceResources.

## Design

### One attach flow

Startup today builds the CPU state and the device-bound state in one constructor
(`HostCore::new(gpu, …)`, `WindowDriver::new(…, stream)`, `SurfaceManager::start`). The design
splits each into its CPU half, built once, and its device half, built by a function that startup
and a recovery both call:

| Step | Startup | Recovery |
|---|---|---|
| Get a device | `SurfaceManager::start` → `open_device(&first_surface)` | `open_device(&any_window_surface)`; `OffscreenHost::recover(gpu)` takes the app's |
| Build the device half | `HostCore::new` → `HostDevice::open(gpu, &resources, timing)` | `HostCore::attach(gpu)` → `HostDevice::open(gpu, &resources, timing)` |
| Give a window its stream | `HostCore::open` → `attach_window(&mut driver)` | `attach_window(&mut driver)` for each window |
| Configure a surface | `make_surface` → `build_window_surface(surface, size)` | `build_window_surface(surface, size)` with the window's own `wgpu::Surface` |
| Tell the UI | a `Ui` made after the attach has seen its epoch | the attach bumped the epoch, so `device_restored()` is true once |

The only code a recovery adds is its trigger: the check of `Gpu::is_lost()` before a frame, and
the loop over the windows.

### Ownership

```text
HostCore
├── resources: UiResources      CPU, built once: shaper, icons, gradient CPU rows, registry, state
│   └── device: Rc<SharedDevice>   texture limit + attach epoch, written by `HostDevice::open`
├── frontend: Frontend          CPU, built once; reads its texture limit from SharedDevice
├── image_store: Rc<WgpuImageStore>   built once, detached; `attach(gpu)` gives it a device
└── device: HostDevice          the device half: WgpuBackend, max texture dim; `open` builds it
WindowDriver
├── ui: Ui                      CPU, built once; reads `device_restored()` from the epoch
└── stream: RenderStream        a detached stream until `attach_window` gives it the real one
WindowSurface (winit)           its `wgpu::Surface` is kept; `build_window_surface` makes the rest
```

- **`DeviceHealth`** (`gpu::device::device_health`): `Arc<AtomicBool>`, set by the lost callback
  for any reason; `Gpu::is_lost()` reads it. `Destroyed` counts as lost too: only an app or a
  test destroys a device, and either way it no longer draws.
- **`HostDevice::open(gpu, &UiResources, FrameTiming)`** asserts the requirements, attaches the
  image store (dropping any texture it held), writes the device's limit and bumps the epoch in
  `SharedDevice`, clears the CPU gradient atlas to the new ceiling, and builds the backend. On a
  first open the store and the atlas are empty already, so the same steps do nothing extra.
- **`HostCore::attach_window(&mut WindowDriver)`** hands the driver `backend.open_stream(owner)`
  through `WindowDriver::attach(stream)`, which clears the pane ledger, marks the target's
  contents invalid, and requeues the screenshots whose copy failed. On a new driver these are
  empty already.
- **The epoch** is a `Cell<u64>` in `SharedDevice`. A `Ui` keeps the epoch it last saw, from its
  creation on; its frame start sets `device_restored` for that frame when the two differ.
- **The image store** keeps one `RefCell` over its device, queue, binding and textures, `None`
  before the first attach; no write can come before it, as the registry lives inside the host.
  `write` checks the image against the device's limit and skips one that no longer fits, with a
  `tracing::warn`; the image paints nothing, as one never written.
- **Detection points.** The winit host checks `Gpu::is_lost()` in `window_event` before
  `runtime.draw` and in `about_to_wait`; `OffscreenHost::frame` and `HeadlessWindow::frame`
  check it before the GPU half. The callback runs inside the call that found the loss, so
  every later check sees it.

### Redesigns this needs

- **`HostCore` splits into its CPU half and `HostDevice`,** which `new` and `attach` both build
  with `HostDevice::open`; `HostCore` keeps `FrameTiming` for it.
- **`WindowDriver::new` takes no stream;** it starts on `RenderStream::detached` (promoted from
  tests), and `HostCore::open` attaches the real one.
- **`SurfaceManager::start` splits** into the instance and first surface, `open_device`, and
  `build_window_surface`, each callable again.
- **`UiResources` holds `Rc<SharedDevice>`** in place of a copied `TextureLimit`;
  `SharedGradientAtlas` and `Frontend` read the limit from it.
- **`WgpuImageStore` is built detached** and gets its device from `attach(&Gpu)`.
- **`Capture::take` returns `Option<Image>`,** and a failed one is requeued.
- **`SharedGradientAtlas::clear(max_rows)`** replaces the CPU atlas with an empty one.

## Plan

Each batch leaves `cargo drive gate` green.

### Batch 1: device health and the poll route

Files: `src/gpu/device/gpu.rs`, new `src/gpu/device/device_health.rs`, `clippy.toml`,
`src/gpu/frame/gpu_timings/*`, `src/gpu/wgpu_backend/mod.rs`, `src/gpu/surface/capture.rs`.

- [ ] `DeviceHealth`, installed by `Gpu::new` (lost callback, uncaptured-error handler);
  `Gpu::is_lost`; `Gpu::poll` as an empty submit; `Gpu::wait` as a work-done callback and an
  empty-submit loop; both return at once on a lost device.
- [ ] Drop the per-frame poll of both timing tracers; `deliver_screenshot` calls `Gpu::poll`
  only on a frame that submitted nothing; add the four `disallowed-methods` entries.
- [ ] `Capture::take` → `Option<Image>`; `wait_landed` ends on a failed map.
- [ ] Doc of `Gpu::new`: it takes over the callback and the handler.
- [ ] Tests: a destroyed device reads lost after a wait; a validation error on a healthy device
  still panics; one on a lost device is dropped; `wait` and `poll` on a lost device return;
  `wait` returns once a submitted copy is done, with its map callback run; a timing readback
  and a screenshot land with no `device.poll` on the frame path.

### Batch 2: shared device state

Files: `src/ui/resources/mod.rs`, new `src/ui/resources/shared_device.rs`, `src/ui/mod.rs`,
`src/renderer/frontend/`, `src/renderer/gradient_atlas/shared_gradient_atlas.rs`.

- [ ] `SharedDevice { texture_limit: Cell<TextureLimit>, epoch: Cell<u64> }` in `UiResources`.
- [ ] `Ui::device_restored()` (public, approved), set at frame start from the epoch.
- [ ] `Ui::max_image_dimension` reads the cell, and loses its `const` (public, approved).
- [ ] `SharedGradientAtlas::clear(max_rows)`.
- [ ] Tests: the edge is true for one frame of each `Ui` after a bump, and false after; a
  cleared gradient atlas re-registers a ramp at a row of the new atlas.

### Batch 3: one attach flow

Files: `src/host/core.rs`, new `src/host/host_device.rs`, `src/gpu/resource/wgpu_image_store.rs`,
`src/host/window_driver/mod.rs`, `src/gpu/wgpu_backend/render_stream.rs`,
`src/renderer/gpu_paint/mod.rs`.

- [ ] `HostDevice::open`, called by `HostCore::new` and `HostCore::attach`.
- [ ] `WgpuImageStore` built detached, `attach(&Gpu)`; `write` skips an image past the limit.
- [ ] `WindowDriver::new` without a stream; `WindowDriver::attach(stream)`;
  `HostCore::attach_window`, called by `HostCore::open`.
- [ ] Doc of `GpuPaint::init`: it runs again on a new device.
- [ ] Every existing test still passes: startup now runs through the attach calls.

### Batch 4: the hosts

Files: `src/host/offscreen.rs`, `src/host/headless_window/mod.rs`, `src/host/winit/{mod.rs,
runtime.rs, error/mod.rs}`, `src/gpu/surface/surface_manager/mod.rs`.

- [ ] `SurfaceManager::open_device` and `build_window_surface`, called by `start`, `make_surface`
  and the recovery.
- [ ] `OffscreenHost::{is_device_lost, recover}`; `frame` skips the GPU half when lost.
- [ ] `HeadlessWindow::{is_device_lost, recover}`, which makes its target again.
- [ ] `WinitHostError::DeviceLost`; `WinitRuntime::recover`: `open_device`, `HostCore::attach`,
  then `build_window_surface` and `attach_window` for each window; called before a draw.
- [ ] Regenerate `.notes/API_SURFACE.md`.
- [ ] AGENTS.md `## Device loss`: the decisions above; delete this file.

## Tests

The loss is real, not a flag set by hand: a test calls `Device::destroy` on an isolated device
(`isolated_headless_test_gpu`), as wgpu's own device-loss tests do (`tests/tests/wgpu-gpu/device.rs`,
`DEVICE_DESTROY_THEN_MORE`: "all operations should turn into no-ops"). wgpu marks the device
invalid at once and runs the lost callback with `Destroyed` on the next maintain, which the next
submit or `Gpu::wait` runs; every later call is dropped, as after a hang. The recovery attaches a
second isolated device; the shared test device is never destroyed.

A test helper, `internals::lose_device(&Gpu)`, destroys and runs one maintain, so the health
reads lost. Three moments are simulated:

- **Between frames:** the next frame finds the loss before its record.
- **Inside a frame:** the device is destroyed after the CPU half and before the GPU half, so the
  submit lands on a dead device and the next frame finds the loss.
- **With a copy in flight:** a screenshot's copy is submitted, then the device is destroyed, so
  its map fails and the capture is requeued.

Not simulated: the `Unknown` reason (the callback takes the same path for every reason), and a
real hang. A real one can be checked by hand on the local Radeon with
`cat /sys/kernel/debug/dri/0/amdgpu_gpu_recover` as root while the showcase runs: it resets the
GPU for every app on the machine, so no automated test runs it.

- **Every kind of GPU object comes back.** One scene holds each: a solid and a gradient quad
  (gradient atlas), a shadow with baked cutout tables (cutout atlas), a rounded clip (stencil),
  text (glyph atlas), a mask icon and a color icon (raster atlases), a mesh, a curve and a
  polyline, a registered image (image store), a `GpuView` (view target, `GpuPaint::init`), a
  backdrop pane (backdrop atlas and blur), with GPU timing on (query sets). After the loss and
  the recovery, its frame matches, pixel for pixel, the frame of a fresh host on the second
  device; then a partial frame matches too, which proves the backbuffer is new.
- **The edges.** `device_restored()` is true on the first frame after the recovery alone;
  `GpuPaint::init` ran twice; the image paints nothing until `ImageHandle::update`, then
  matches; a screenshot asked for before the loss lands after it.
- **No submit to a lost device.** Between the loss and the recovery, `frame` returns, submits
  nothing, and the target keeps its pixels.
- **A second loss recovers too.** The same host loses and recovers twice.
- **Limits.** A recovery onto a device with a smaller limit, simulated with requested limits
  below the adapter's, skips an image past it and keeps the gradient atlas inside it.
- **Startup and recovery are one flow.** A host attached twice to the same device paints what a
  host attached once paints, and its `Ui` reads `device_restored()` once: the attach steps need
  no loss to run. Every existing host test runs through the same attach calls.
- **Winit.** A failed `open_device` maps to `DeviceLost`; `build_window_surface` picks the format
  from the caps it is given. No test opens a window.

## Known limits

- **A device on a new adapter can lack an optional feature,** such as timestamps: the backend
  then times on the CPU clock, as it does on such a device at startup.
