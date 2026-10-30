//! The device and queue Palantir draws with.

use crate::gpu::device::device_health::DeviceHealth;
use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::error::UnmetRequirements;
use std::iter;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// How long [`Gpu::wait`] waits before it gives up, as Zed's headless readback does. It bounds only
/// a hang no system resets: Windows (TDR) and amdgpu reset a GPU job that runs past 2 s, which
/// ends a hang as a loss long before. A wait that returns early tells its caller the work is done
/// when it is not, so the bound sits far above a slow drain, such as a test's on software Vulkan.
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
            device,
            queue,
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

    /// The device's `max_texture_dimension_2d`: the ceiling on registered
    /// images, `GpuView` targets, and the glyph and gradient atlases.
    ///
    /// A zero limit panics rather than joining [`DeviceRequirements::met_by`]'s
    /// `Result`: no adapter reports one.
    pub(crate) fn max_texture_dim(&self) -> NonZeroU32 {
        NonZeroU32::new(self.device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
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

    /// Block until the device has finished every submission and every map callback asked for
    /// before the call has run, for at most [`WAIT_TIMEOUT`]; at once on a lost device.
    ///
    /// wgpu's blocking wait, `PollType::Wait`, as wgpu's examples and Zed's readback wait. A poll
    /// on another thread can collect the callbacks and run them after this wait returns, so the
    /// wait also waits for a work-done callback of its own: WebGPU runs every map callback asked
    /// for before it first (wgpu-core `UserClosures::fire`). The wait is fatal on a device it finds
    /// lost, so only a caller that asks to block reaches it: an offscreen host's `wait_for_gpu`, a
    /// read-back, a bench, a test. A device its owner destroyed is not lost to the driver, so the
    /// wait returns and runs its lost callback.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one blocking wait, behind the health check"
    )]
    pub(crate) fn wait(&self) {
        if self.is_lost() {
            return;
        }
        let ran = Arc::new(AtomicBool::new(false));
        let marker = Arc::clone(&ran);
        self.queue
            .on_submitted_work_done(move || marker.store(true, Ordering::Release));
        let waited = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(WAIT_TIMEOUT),
        });
        if let Err(error) = waited {
            tracing::warn!(%error, "the device did not finish its work in {WAIT_TIMEOUT:?}");
            return;
        }
        while !ran.load(Ordering::Acquire) && !self.is_lost() {
            thread::yield_now();
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;

    impl Gpu {
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
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use crate::internals::panic_probe;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    /// A buffer of a size no copy may map, which wgpu reports as a validation error.
    fn invalid_buffer(device: &wgpu::Device) {
        let _ = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.invalid"),
            size: 3,
            usage: wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: true,
        });
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

    /// `wait` returns once the device finished what was submitted, with the copies' map callbacks
    /// run: a buffer mapped after its submit is mapped once the wait returns, though another
    /// thread polls the device all along and can collect the callback and run it itself.
    #[test]
    fn wait_runs_the_map_callbacks_of_what_was_submitted() {
        let gpu = isolated_headless_test_gpu();
        let polling = Arc::new(AtomicBool::new(true));
        let poller = {
            let gpu = gpu.handles();
            let polling = Arc::clone(&polling);
            thread::spawn(move || {
                while polling.load(Ordering::Acquire) {
                    gpu.poll();
                }
            })
        };
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.mapped"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, &[1, 2, 3, 4]);
        gpu.poll();
        let mapped = Arc::new(AtomicBool::new(false));
        let landed = Arc::clone(&mapped);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                landed.store(result.is_ok(), Ordering::Release);
            });
        gpu.wait();
        assert!(mapped.load(Ordering::Acquire));
        assert_eq!(
            &*buffer.slice(..).get_mapped_range().unwrap(),
            &[1, 2, 3, 4]
        );
        polling.store(false, Ordering::Release);
        poller.join().unwrap();
    }
}
