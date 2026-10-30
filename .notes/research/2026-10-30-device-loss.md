# Device loss: causes, wgpu 30's behavior, and how others recover (2026-10-30)

wgpu facts are read from the sources of wgpu 30.0.1, wgpu-core 30.0.1 and wgpu-hal 30.0.1 in
the cargo registry. Paths are relative to each crate's `src/`.

## When a device is lost

- **A GPU hang (TDR).** Windows resets a GPU that does not finish or preempt a job within the
  `TdrDelay`, 2 s by default, and the app's device is removed
  ([Microsoft: TDR](https://learn.microsoft.com/windows-hardware/drivers/display/timeout-detection-and-recovery)).
  A long dispatch is the usual trigger; the advice is to split it
  ([Puget Systems](https://www.pugetsystems.com/labs/hpc/Working-around-TDR-in-Windows-for-a-better-GPU-computing-experience-777/)).
- **A GPU reset on Linux.** amdgpu resets a ring whose job runs past `lockup_timeout`, 2000 ms
  for every queue by default
  ([amdgpu module parameters](https://docs.kernel.org/gpu/amdgpu/module-parameters.html)); Vulkan apps then get
  `VK_ERROR_DEVICE_LOST` and must recreate their contexts
  ([DRM reset RFC](https://lkml.rescloud.iu.edu/2306.2/04926.html),
  [Mesa #7181](https://gitlab.freedesktop.org/mesa/mesa/-/issues/7181)).
- **Suspend and resume.** The NVIDIA Linux driver keeps only essential video memory unless
  `NVreg_PreserveVideoMemoryAllocations=1`
  ([NVIDIA README](https://download.nvidia.com/XFree86/Linux-x86_64/460.32.03/README/powermanagement.html));
  Android loses the EGL context after sleep
  ([GLSurfaceView.Renderer](https://developer.android.com/reference/kotlin/android/opengl/GLSurfaceView.Renderer)).
- **A driver crash or update, an eGPU unplugged, a GPU switch** on hybrid graphics.

## What wgpu 30 does

- **Every backend reports a loss.** `DeviceError::Lost` is returned by the Vulkan, DX12, Metal and
  GL backends (wgpu-hal `vulkan/mod.rs`, `dx12/device.rs`, `metal/device.rs`, `gles/device.rs`).
- **A loss runs the lost callback first.** wgpu-core `Device::handle_hal_error` calls `lose`
  (`device/resource.rs:5415`), which marks the device invalid and runs the callback set by
  `Device::set_device_lost_callback` (wgpu `api/device.rs:681`) with `DeviceLostReason::Unknown`.
  The callback must be `Send + 'static`; a device has one.
- **`Device::destroy` loses the device too.** The callback runs with `DeviceLostReason::Destroyed`
  on the next maintain (wgpu-core `device/resource.rs:986`). It is the one call that loses a
  device on purpose.
- **Errors of a lost device are dropped.** `handle_error_inner` returns early for
  `ErrorType::DeviceLost` (wgpu `backend/wgpu_core.rs:304`): a submit, a create or a write on a
  lost device does not reach the uncaptured-error handler.
- **Any other uncaptured error panics by default.** `default_error_handler` panics
  (wgpu `backend/wgpu_core.rs:692`); `Device::on_uncaptured_error` replaces it
  (`api/device.rs:506`).
- **`Device::poll` panics on a lost device.** A wait or fence read that returns `Lost` is not a
  `PollError` (which holds only `Timeout` and `WrongSubmissionIndex`), so `poll` calls
  `handle_error_fatal` (wgpu `backend/wgpu_core.rs:1916-1926`). Under `panic = "abort"` that ends
  the process. A poll after the loss is known can be skipped; a poll that discovers the loss
  cannot be caught. `Instance::poll_all` is fatal the same way (`backend/wgpu_core.rs:888`).
  wgpu trunk (checked 2026-10-30) keeps the mapping, and its `PollError` adds only `CantWait`.
  Downstream wgpu 30 apps report the same panic and wrap `poll` in `catch_unwind`
  ([NEAT-AI-Discovery #2363](https://github.com/stSoftwareAU/NEAT-AI-Discovery/issues/2363),
  [Aurora-Viewer #1275](https://github.com/Aurora-Viewer/Aurora-Viewer/pull/1275),
  [lightcraft #1984](https://github.com/storytold/lightcraft/pull/1984)).
- **A submit runs the callbacks and survives a loss.** The docs of `Buffer::map_async` and
  `Queue::on_submitted_work_done` name `queue.submit` as a call that runs them
  (`api/buffer.rs:363`, `api/queue.rs:310`). wgpu-core runs `device.maintain(Poll)` after each
  submit (`device/queue.rs:1541`), an empty one included (`:1531`), and a submit's error goes to
  `handle_error` (`backend/wgpu_core.rs:2109-2115`), which drops a lost device's error.
- **A destroyed device raises validation errors until its lost callback runs.** After
  `Device::destroy` the device is invalid at once, but the callback waits for a maintain; a
  resource made in between is invalid, and its use (an unmap of a staging buffer) is a
  `Validation` error, which the uncaptured-error handler gets. A loss the HAL reports runs the
  callback inside the call that found it, so no such window exists (wgpu-core
  `device/resource.rs:5415`). Found by the recovery tests on 2026-10-30.
- **A belt allocation on a lost device does not map.** `BufferSlice::get_mapped_range_mut`
  returns `MapRangeError` for the invalid buffer, and `util::StagingBelt::write_buffer` panics on
  it (wgpu `util/belt.rs:163`).
- **A surface configured on a new device releases its old swapchain through that device.**
  wgpu-core takes the old presentation and calls the HAL `configure` with the new device
  (`device/resource.rs:5366-5380`); Vulkan's `Surface::configure` releases the old swapchain's
  resources with the device it is given and passes the old swapchain to the new one
  (wgpu-hal `vulkan/instance.rs:1004-1021`). A dropped surface unconfigures on the device of its
  presentation.

## How others recover

- **WebGPU** advises a new adapter and device after any loss but `destroyed`
  ([MDN `GPUDevice.lost`](https://developer.mozilla.org/docs/Web/API/GPUDevice/lost)).
- **Zed (gpui on wgpu)** rebuilds its renderer on Linux since PR
  [#50898](https://github.com/zed-industries/zed/pull/50898); its atlas "clears all textures and
  cached tiles" and refills lazily ([#54349](https://github.com/zed-industries/zed/pull/54349)).
  It panicked before ([#42114](https://github.com/zed-industries/zed/pull/42114)).
  Its images stay in RAM as CPU data, so the atlas can refill.
- **DirectX (DeviceResources)** tells the app: `IDeviceNotify::OnDeviceLost`, then
  `OnDeviceRestored`, after which the app makes its device resources again
  ([DirectXTK](https://github.com/microsoft/DirectXTK/wiki/Using-DeviceResources),
  [Microsoft](https://learn.microsoft.com/previous-versions/windows/apps/dn458383(v=win.10))).
- **Android GL** tells the renderer through `onSurfaceCreated`, where it reloads its textures
  ([GLSurfaceView.Renderer](https://developer.android.com/reference/kotlin/android/opengl/GLSurfaceView.Renderer)).
- **Chromium** ends the GPU process where a driver cannot recover
  ([shared_context_state.cc](https://chromium.googlesource.com/chromium/src/+/master/gpu/command_buffer/service/shared_context_state.cc)).
