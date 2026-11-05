# Open issues

- `Staging::abort` (`src/gpu/surface/staging.rs`) unmaps a buffer whose `MapState` reads pending. wgpu-core 30's `Buffer::map`, run by a device poll, sets the buffer's map state to idle for the length of the map, so an abort on one thread while another thread polls the same device can unmap a buffer wgpu holds as idle: a "Buffer is not mapped" validation error, which `device_health` turns into a panic. The Windows CI runner hit it in `gpu::surface::map_state` tests on the shared test device. Waits on the wgpu release that carries https://github.com/gfx-rs/wgpu/pull/9959 (merged after 30.0.1, not on the `v30` branch).

- `cargo reel png` on slide 8 of the reel draws a magenta outline around the window and around the `GpuView` box.

- The first `enumerate_adapters` in a process takes about 800 ms on an Apple laptop, which `GpuRequest::candidates` pays at startup and the first GPU test of a suite pays against its 1 s limit.
