# What wgpu 30 allows on a swapchain (2026-10-29)

Read from the sources of wgpu 30.0.1, wgpu-core 30.0.1 and wgpu-hal 30.0.1 in the cargo
registry. Paths are relative to each crate's `src/`.

## Contents

- **An acquired image is uninitialized.** `Surface::get_current_texture` registers the texture
  as `TextureUses::UNINITIALIZED` (wgpu-core `present.rs:247`). Its init tracker starts empty.
- **A load of it reads zeros, not the last frame.** A pass with `LoadOp::Load` records
  `MemoryInitKind::NeedsInitializedMemory` (wgpu-core `command/render.rs:1155-1168`), and wgpu
  clears the texture before the pass. The pixels of an earlier frame are not reachable through
  wgpu on any backend.
- **A present of an image nothing wrote clears it first.** `Queue::prepare_surface_texture_for_present`
  (wgpu-core `device/queue.rs:125`); the wgpu docs of `Queue::present` say the same.
- **Inside one frame the image is an ordinary texture.** A pass that stores to it, then a pass
  that loads it, or a copy that reads it, reads what the frame wrote. wgpu inserts the barriers.

## Usages a surface offers

`SurfaceCapabilities::usages`, from each backend's `surface_capabilities`:

| Backend | Usages | Source |
|---|---|---|
| Metal | `COLOR_TARGET`, `COPY_SRC`, `COPY_DST`, storage | wgpu-hal `metal/adapter.rs:474` |
| DX12 | `COLOR_TARGET`, `COPY_SRC`, `COPY_DST` | wgpu-hal `dx12/adapter.rs:1360` |
| GL / GLES | `COLOR_TARGET` | wgpu-hal `gles/adapter.rs:1379` |
| Vulkan | the driver's `supportedUsageFlags`; `SAMPLED` gives `RESOURCE` | wgpu-hal `vulkan/swapchain/native.rs:173`, `vulkan/conv.rs:358` |

- **A usage the surface does not offer is an error.** `configure` checks
  `caps.usage.contains(config.usage)` and fails with `UnsupportedUsage`, a validation error
  (wgpu-core `device/surface_config.rs:175`).
- **No portable sampled swapchain.** Metal, DX12 and GL never offer `TEXTURE_BINDING`; only
  some Vulkan drivers do.
- **Metal's framebuffer-only mode** is set only for a usage of `COLOR_TARGET` alone (wgpu-hal
  `metal/surface.rs:260`). A surface that takes a copy usage is not framebuffer-only.

## Configuration

- **Present modes.** `AutoVsync` resolves to `FifoRelaxed` or `Fifo`, `AutoNoVsync` to
  `Immediate`, `Mailbox` or `Fifo`; FIFO is always supported, so both always configure (wgpu-core
  `device/surface_config.rs:66-74`).
- **Frame latency** is clamped to the surface's range (wgpu-core `device/resource.rs:5305`).
- **A zero width or height is an error** (`ZeroArea`, wgpu-core `device/surface_config.rs:181`).
- **`configure` while an acquired `SurfaceTexture` lives** fails with `PreviousOutputExists`
  (wgpu-core `device/resource.rs:5370`); the wgpu docs of `Surface::configure` say it panics.
- **`configure` waits for the device to idle** (wgpu #7447: 100 ms and more).
- **A view of an image can outlive a reconfigure**, unchecked (wgpu-core
  `device/resource.rs:5374`, wgpu #4105): nothing stops a stale view, so code must not keep one.

## Acquire and present

- **One image at a time.** A second acquire before the present fails with `AlreadyAcquired`
  (wgpu-core `present.rs:51`).
- **Order.** Submit the frame's work, then `Queue::present` (wgpu `api/surface.rs` docs of
  `get_current_texture`). A `SurfaceTexture` dropped without a present is discarded (wgpu
  `api/surface_texture.rs`).
- **Outcomes** (wgpu `api/surface_texture.rs`, `CurrentSurfaceTexture`):
  - `Success`, `Suboptimal`: a usable image; `Suboptimal` asks for a reconfigure.
  - `Timeout`: skip the frame, try again.
  - `Occluded`: skip until visible.
  - `Outdated`: reconfigure, try again.
  - `Lost`: recreate the surface with `Instance::create_surface`, then configure. A configure of
    a lost surface fails with `InvalidSurface` (wgpu-core `device/resource.rs:5386`).
  - `Validation`: an error scope or the uncaptured-error handler took a validation error.
