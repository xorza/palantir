# Open issues

- `Staging::abort` (`src/gpu/surface/staging.rs`) unmaps a buffer whose `MapState` reads pending. wgpu-core 30's `Buffer::map`, run by a device poll, sets the buffer's map state to idle for the length of the map, so an abort on one thread while another thread polls the same device can unmap a buffer wgpu holds as idle: a "Buffer is not mapped" validation error, which `device_health` turns into a panic. The Windows CI runner hit it in `gpu::surface::map_state` tests on the shared test device. Waits on the wgpu release that carries https://github.com/gfx-rs/wgpu/pull/9959 (merged after 30.0.1, not on the `v30` branch).

- `examples/reel/cube.rs` and `palantir-showcase/src/pages/gpu_view.rs` each define the same `VERTICES`, `IDXS`, `SHADER` and `DEPTH_FORMAT`. `palantir-showcase/tests/visual/fixtures/gpu_view.rs` holds a third `DEPTH_FORMAT`.

- `gates::on_gpu::still_tree_frame_costs_the_empty_floor` takes 1.01 s on an Apple laptop, over the 1 s limit for one test.

- `ShapeStore::keep` inserts into `kept`, and `RecyclePool::reserve_for` reserves, on the frame an entry expires. A page whose kept count crosses a capacity there allocates on that frame, which `gates::full_tree_cpu_frame_alloc_free` does not reach on its one page.
