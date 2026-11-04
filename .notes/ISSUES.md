# Open issues
- `Staging::abort` (`src/gpu/surface/staging.rs`) unmaps a buffer whose `MapState` reads pending. wgpu-core 30's `Buffer::map`, run by a device poll, sets the buffer's map state to idle for the length of the map, so an abort on one thread while another thread polls the same device can unmap a buffer wgpu holds as idle: a "Buffer is not mapped" validation error, which `device_health` turns into a panic. The Windows CI runner hit it in `gpu::surface::map_state` tests on the shared test device. Waits on the wgpu release that carries https://github.com/gfx-rs/wgpu/pull/9959 (merged after 30.0.1, not on the `v30` branch).
- `PassTraffic::of` reads only the vendor id, which DX12 reports as `0x4D4F4351` for Qualcomm and GLES derives from the vendor string, so tilers read as `Drawn` there.
- `Gpu::wait_until` spins with no bound once the device wait has drained.
- `Staging::unpad_into` unmaps the buffer and leaves its `MapState` mapped.
- `SingleQuadBuffer::is_uploaded` stays true after the first upload, so the `bind_clear` assert cannot catch a missed upload.
