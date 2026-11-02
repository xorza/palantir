//! The device and queue Palantir draws with.

use crate::gpu::device::device_health::DeviceHealth;
use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::error::UnmetRequirements;
use std::iter;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// How long [`Gpu::wait_until`] waits for the device before it gives up, as Zed's headless readback
/// does. It bounds only a hang no system resets: Windows (TDR) and amdgpu reset a GPU job that runs
/// past 2 s, which ends a hang as a loss long before. A wait that returns early tells its caller
/// the work is done when it is not, so the bound sits far above a slow drain, such as a test's on
/// software Vulkan.
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// A graphics device and its queue.
///
/// An application that opened its own wraps the pair here for
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder); one that used
/// [`RequestedGpu`](crate::RequestedGpu) finds it in [`gpu`](crate::RequestedGpu::gpu).
/// Either makes the textures the host renders into through [`Self::device`]. Cloning
/// clones the handles, not the device, and every clone sees the device's loss.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    health: DeviceHealth,
    pass_traffic: PassTraffic,
    /// Held across each pipeline build on the device, so builds take turns: see
    /// [`Self::create_render_pipeline`].
    pipeline_turn: Arc<Mutex<()>>,
}

impl Gpu {
    /// Draw through a device the caller owns. It must have been requested with
    /// [`DeviceRequirements`]; the host builders check that.
    ///
    /// Takes over the device's lost callback and its uncaptured-error handler, which Palantir
    /// needs to know of a loss, so a callback or a handler set on `device` before is replaced.
    /// The handler still panics on an error while the device is healthy, as wgpu's default
    /// does. Wrap a device once and clone the `Gpu`: a second `new` on the same device takes the
    /// callback from the first.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            health: DeviceHealth::watch(&device),
            pass_traffic: PassTraffic::of(&device.adapter_info()),
            device,
            queue,
            pipeline_turn: Arc::default(),
        }
    }

    /// The device, to make the textures a host renders into.
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The device's queue.
    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub(crate) fn requirements_met(&self) -> Result<(), UnmetRequirements> {
        DeviceRequirements::met_by(&self.device)
    }

    /// The device's `max_texture_dimension_2d`: the ceiling on surfaces, registered images,
    /// `GpuView` targets, the atlases and the backdrop blur's textures. Every texture made on the
    /// device reads it here; the CPU side reads the copy a host attaches to its `SharedDevice`.
    ///
    /// A zero limit panics rather than joining [`DeviceRequirements::met_by`]'s
    /// `Result`: no adapter reports one.
    pub(crate) fn max_texture_dim(&self) -> NonZeroU32 {
        NonZeroU32::new(self.device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
    }

    /// A buffer as `desc` describes, created holding `bytes` at its start. On a device lost before
    /// or inside the call, wgpu hands back an invalid buffer whose range does not map: it comes
    /// back unfilled, as every call on a lost device is dropped. Refused: `create_buffer_init`
    /// and an `expect` on the mapped range, which end the process on a loss no call has reported
    /// yet.
    ///
    /// `desc.mapped_at_creation` is ignored; its size must be a multiple of
    /// [`wgpu::COPY_BUFFER_ALIGNMENT`] and hold `bytes` rounded up to one.
    pub(crate) fn create_filled_buffer(
        &self,
        desc: &wgpu::BufferDescriptor<'_>,
        bytes: &[u8],
    ) -> wgpu::Buffer {
        // A mapped range spans whole `COPY_BUFFER_ALIGNMENT` units.
        let mapped = (bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
        debug_assert!(mapped <= desc.size, "a filled buffer holds its bytes");
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            mapped_at_creation: true,
            ..desc.clone()
        });
        if mapped != 0 {
            match buffer.slice(..mapped).get_mapped_range_mut() {
                Ok(mut view) => view.slice(..bytes.len()).copy_from_slice(bytes),
                Err(error) => debug_assert!(self.is_lost(), "a healthy device maps: {error}"),
            }
        }
        buffer.unmap();
        buffer
    }

    /// A render pipeline as `desc` describes, built while no other build on the device runs.
    ///
    /// wgpu-hal's Direct3D 12 backend takes a reference to a cached FXC blob under its cache's
    /// lock and releases it outside, and FXC counts references without atomics, so two threads
    /// building from one shader can free a blob the cache still holds
    /// ([gfx-rs/wgpu#10547](https://github.com/gfx-rs/wgpu/issues/10547), open at wgpu 30.0.1).
    /// See `AGENTS.md`, "Pipeline builds".
    #[expect(
        clippy::disallowed_methods,
        reason = "the one place a render pipeline is built, under the device's turn"
    )]
    pub(crate) fn create_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> wgpu::RenderPipeline {
        let _turn = self.take_pipeline_turn();
        self.device.create_render_pipeline(desc)
    }

    /// The device's pipeline turn. A build that panicked, as the uncaptured-error handler does on
    /// a validation error, leaves nothing half-done behind the lock, so poison is ignored.
    fn take_pipeline_turn(&self) -> MutexGuard<'_, ()> {
        self.pipeline_turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// What a render pass on the device moves, which decides where a partial frame paints.
    pub(crate) const fn pass_traffic(&self) -> PassTraffic {
        self.pass_traffic
    }

    /// Whether the device is lost: it draws nothing more, and a host attaches a new one.
    pub(crate) fn is_lost(&self) -> bool {
        self.health.is_lost()
    }

    /// Run the map and work-done callbacks of the submissions the device has finished, without
    /// blocking.
    ///
    /// An empty submit, not `Device::poll`, which wgpu 30 makes fatal on a lost device: a submit
    /// runs the callbacks too (wgpu `Buffer::map_async`), and wgpu drops its error once the device
    /// is lost. Nothing happens on a device already lost.
    pub(crate) fn poll(&self) {
        if !self.is_lost() {
            self.queue.submit(iter::empty());
        }
    }

    /// Block until the device has finished every submission made before the call, for at most
    /// [`WAIT_TIMEOUT`]; at once on a lost device. A map callback can still be on its way when it
    /// returns, as [`Self::wait_until`] says; wait for one there.
    #[cfg(feature = "internals")]
    pub(crate) fn wait(&self) {
        let _ = self.drain();
    }

    /// Block until the device has finished every submission made before the call, a wait of at
    /// most [`WAIT_TIMEOUT`], then until `landed` holds: the wait for a map callback, whose result
    /// `landed` reads. Returns how the wait ended.
    ///
    /// Once the device's wait returns, every map of a finished submission is collected, by the
    /// wait's own poll or an earlier one on another thread, which runs the callback outside
    /// wgpu's lock, as soon as it can, so the spin for it is short. A wait that took the device's
    /// finished work for the callbacks run missed one map in 20,000 with four threads polling
    /// (high-end NVIDIA GPU, Vulkan), and two in three when a callback slept 1 ms.
    pub(crate) fn wait_until(&self, landed: &dyn Fn() -> bool) -> GpuWait {
        let drained = self.drain();
        if drained != GpuWait::Done {
            return drained;
        }
        while !landed() {
            if self.is_lost() {
                return GpuWait::DeviceLost;
            }
            thread::yield_now();
        }
        GpuWait::Done
    }

    /// The blocking wait behind [`Self::wait_until`]: wgpu's `PollType::Wait`, as wgpu's examples
    /// and Zed's readback wait.
    ///
    /// The wait is fatal on a device it finds lost, so only a caller that asks to block reaches it:
    /// an offscreen host's `wait_for_gpu`, a read-back, a bench, a test. A device its owner
    /// destroyed is not lost to the driver, so the wait returns and runs its lost callback.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one blocking wait, behind the health check"
    )]
    fn drain(&self) -> GpuWait {
        if self.is_lost() {
            return GpuWait::DeviceLost;
        }
        let waited = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(WAIT_TIMEOUT),
        });
        if self.is_lost() {
            return GpuWait::DeviceLost;
        }
        if let Err(error) = waited {
            tracing::warn!(%error, "the device did not finish its work in {WAIT_TIMEOUT:?}");
            return GpuWait::TimedOut;
        }
        GpuWait::Done
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;

    impl Gpu {
        /// A compute pipeline as `desc` describes, built in the device's turn, as
        /// [`Self::create_render_pipeline`] builds.
        #[expect(
            clippy::disallowed_methods,
            reason = "the one place a compute pipeline is built, under the device's turn"
        )]
        pub(crate) fn create_compute_pipeline(
            &self,
            desc: &wgpu::ComputePipelineDescriptor<'_>,
        ) -> wgpu::ComputePipeline {
            let _turn = self.take_pipeline_turn();
            self.device.create_compute_pipeline(desc)
        }

        /// Lose the device, as a GPU hang does: `Device::destroy` makes every later call a no-op,
        /// as wgpu's own device-loss tests use it, and the wait runs the lost callback, as a real
        /// loss runs it inside the call that finds the loss.
        pub(crate) fn lose(&self) {
            self.device.destroy();
            self.wait();
            debug_assert!(self.is_lost(), "the lost callback ran");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::device::gpu_wait::GpuWait;
    use crate::gpu::surface::map_state::MapState;
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use crate::internals::panic_probe;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    /// A buffer of a size no copy may map, which wgpu reports as a validation error.
    fn invalid_buffer(device: &wgpu::Device) {
        let _ = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.invalid"),
            size: 3,
            usage: wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: true,
        });
    }

    /// A compute pipeline with an empty entry point, for a build that compiles a shader.
    fn empty_compute(gpu: &Gpu) -> wgpu::ComputePipeline {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("palantir.test.empty_compute"),
                source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
            });
        gpu.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("palantir.test.empty_compute"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        })
    }

    /// A build on a clone of the `Gpu` waits while the device's turn is held: it can finish only
    /// after the release, so a build seen done during the 50 ms hold means builds do not take
    /// turns. A turn a panic poisoned still passes to the next build.
    #[test]
    fn pipeline_builds_on_one_device_take_turns() {
        let gpu = isolated_headless_test_gpu();
        let clone = gpu.handles();
        let built = AtomicBool::new(false);
        thread::scope(|scope| {
            let turn = gpu.take_pipeline_turn();
            scope.spawn(|| {
                empty_compute(&clone);
                built.store(true, Ordering::Release);
            });
            thread::sleep(Duration::from_millis(50));
            assert!(
                !built.load(Ordering::Acquire),
                "the build waits for the turn"
            );
            drop(turn);
        });
        assert!(built.load(Ordering::Acquire));

        panic_probe::assert_panics_with("a build panicked", || {
            let _turn = gpu.take_pipeline_turn();
            panic!("a build panicked");
        });
        assert!(gpu.pipeline_turn.is_poisoned());
        empty_compute(&gpu);
    }

    /// A healthy device reads healthy and still panics on a validation error, as wgpu's default
    /// handler does. Once lost, it reads lost, the same error goes unreported, and `wait` and
    /// `poll` return at once.
    #[test]
    fn a_lost_device_reads_lost_and_reports_nothing() {
        let gpu = isolated_headless_test_gpu();
        assert!(!gpu.is_lost());
        panic_probe::assert_panics_with("wgpu error", || invalid_buffer(&gpu.device));
        gpu.lose();
        assert!(gpu.is_lost());
        invalid_buffer(&gpu.device);
        gpu.wait();
        gpu.poll();
    }

    /// `wait_until` returns once a copy's map callback ran, though four other threads poll the
    /// device all along and collect callbacks themselves. Each callback sleeps 1 ms before it
    /// records, so one a poller runs lands after the wait's own work-done callback: a wait that
    /// trusted the order missed two maps in three.
    #[test]
    fn wait_until_waits_for_a_map_another_thread_runs() {
        const ROUNDS: u32 = 50;
        let gpu = isolated_headless_test_gpu();
        let polling = Arc::new(AtomicBool::new(true));
        let pollers: Vec<_> = (0..4)
            .map(|_| {
                let gpu = gpu.handles();
                let polling = Arc::clone(&polling);
                thread::spawn(move || {
                    while polling.load(Ordering::Acquire) {
                        gpu.poll();
                    }
                })
            })
            .collect();
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.mapped"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for round in 0..ROUNDS {
            let bytes = round.to_le_bytes();
            gpu.queue.write_buffer(&buffer, 0, &bytes);
            gpu.poll();
            let mapped = Arc::new(AtomicBool::new(false));
            let landed = Arc::clone(&mapped);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    thread::sleep(Duration::from_millis(1));
                    landed.store(result.is_ok(), Ordering::Release);
                });
            assert_eq!(
                gpu.wait_until(&|| mapped.load(Ordering::Acquire)),
                GpuWait::Done,
                "round {round}"
            );
            assert_eq!(&*buffer.slice(..).get_mapped_range().unwrap(), &bytes);
            buffer.unmap();
        }
        polling.store(false, Ordering::Release);
        for poller in pollers {
            poller.join().unwrap();
        }
    }

    /// A healthy device's filled buffer holds the bytes at its start, padded to whole
    /// `COPY_BUFFER_ALIGNMENT` units; a lost device's comes back unfilled and the call returns,
    /// where an `expect` on the mapped range panics.
    #[test]
    fn a_filled_buffer_holds_its_bytes_and_a_lost_device_returns_one_unfilled() {
        let desc = wgpu::BufferDescriptor {
            label: Some("palantir.test.filled"),
            size: 8,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        };
        let gpu = isolated_headless_test_gpu();
        let healthy = gpu.create_filled_buffer(&desc, &[1, 2, 3, 4, 5, 6]);
        let map = MapState::new();
        map.map_read(&healthy);
        assert_eq!(gpu.wait_until(&|| !map.is_pending()), GpuWait::Done);
        assert!(map.is_mapped());
        assert_eq!(
            &*healthy.slice(..).get_mapped_range().unwrap(),
            &[1, 2, 3, 4, 5, 6, 0, 0]
        );
        gpu.lose();
        let lost = gpu.create_filled_buffer(&desc, &[1, 2, 3, 4]);
        assert_eq!(lost.size(), desc.size);
    }
}
