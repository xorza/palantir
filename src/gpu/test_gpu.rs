//! Shared headless GPU lifecycle for feature-gated tests.

use crate::common::sync::MutexExt;
use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use glam::UVec2;
use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::device::requested_gpu::GpuRequest;
#[cfg(test)]
use crate::gpu::device::upload_staging::UploadStaging;
use crate::gpu::error::GpuRequestError;
#[cfg(test)]
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::surface::owned_target::{OwnedTarget, TargetSpec};
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::surface::texel_readback::TexelReadback;

/// HighPerformance, as the benches use: with several adapters, goldens must record the one users see.
const TEST_POWER: PowerPreference = PowerPreference::HighPerformance;

const ADAPTER_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const ADAPTER_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

/// No headless test device: why the last request failed, and how long the lease tried.
#[derive(Debug)]
struct LeaseFailed {
    /// How long the lease retried.
    waited: Duration,
    /// The last request's failure.
    source: GpuRequestError,
}

impl Display for LeaseFailed {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lease headless test gpu after {:?}: {}",
            self.waited,
            Chain(&self.source)
        )
    }
}

/// An error and each cause [`Error::source`] reaches under it, joined by `": "`, as anyhow's
/// `{:#}` prints a chain: palantir's errors name their cause in `source` alone, so a panic that
/// prints the error by itself hides what failed under it.
struct Chain<'a>(&'a dyn Error);

impl Display for Chain<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            write!(f, ": {error}")?;
            cause = error.source();
        }
        Ok(())
    }
}

/// A headless device and its queue, waited idle when the lease drops. It lends its [`Gpu`], as a
/// guard does.
///
/// The leases own the device: the last one to drop destroys it, and the adapter with the last
/// device on it. No static holds either, as a static is never dropped (RFC 1440): the shared
/// device once lived in one, so it was still alive in `exit()`, where the driver's teardown raced
/// its own shader-compiler thread and corrupted the heap.
#[derive(Debug)]
pub struct HeadlessTestGpuLease {
    device: Lent<OpenDevice>,
    /// The display frame [`Self::frame`] runs next, one a call, so what decays by frames ages
    /// across a test's frames.
    #[cfg(test)]
    frames: std::sync::atomic::AtomicU64,
}

impl Deref for HeadlessTestGpuLease {
    type Target = Gpu;

    fn deref(&self) -> &Gpu {
        &self.device.gpu
    }
}

impl HeadlessTestGpuLease {
    /// The adapter the device opened on, `name (backend, driver)`; golden suites record it.
    pub fn adapter(&self) -> &str {
        &self.device.identity
    }

    /// An owned clone of the handles, for an API that takes a [`Gpu`] by value. Keep the lease for
    /// as long as the clone: a clone that outlives every lease keeps a device, and its instance,
    /// that the next lease does not reuse.
    pub fn handles(&self) -> Gpu {
        self.device.gpu.clone()
    }

    /// A 2D `Rgba8UnormSrgb` window target of `size`; `label` names the test in RenderDoc and
    /// validation errors.
    pub fn target(&self, label: &str, size: UVec2) -> OwnedTarget {
        OwnedTarget::window(self, label, size, TargetFormat::RGBA8_SRGB)
    }

    /// A 2D `Rgba8UnormSrgb` target of `size` that takes copies but gives none, as a texture an
    /// app renders into may be.
    pub fn uncopied_target(&self, label: &str, size: UVec2) -> OwnedTarget {
        OwnedTarget::new(
            self,
            TargetSpec {
                label,
                size,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            },
        )
    }

    /// Block until every submission on the device has finished.
    pub fn wait(&self) {
        self.device.gpu.wait();
    }

    /// Run `encode` on an encoder and a staging belt of its own, then finish, submit and wait for
    /// them: one frame of GPU work a test sets up alone. Returns what `encode` returns.
    #[cfg(test)]
    pub(crate) fn frame<R>(&self, encode: impl FnOnce(&mut GpuCtx<'_>) -> R) -> R {
        let mut belt = UploadStaging::new(&self.device.gpu.device, 1 << 16);
        let mut encoder = self
            .device
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let frame = self
            .frames
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let encoded = encode(&mut GpuCtx::new(self, &mut belt, &mut encoder, frame));
        belt.finish_and_recall_on_submit(&encoder);
        self.device.gpu.queue.submit([encoder.finish()]);
        self.wait();
        encoded
    }

    /// Every texel of `texture`'s first layer, rows top to bottom with no padding, waited for.
    ///
    /// # Panics
    ///
    /// For a texture made without `COPY_SRC`, of a compressed or depth format, and when the copy
    /// does not map.
    pub fn read_texture(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let mut texels = Vec::new();
        let read = TexelReadback::new(self, texture).read(self, texture, &mut texels);
        assert_eq!(read, GpuWait::Done, "the read buffer maps");
        texels
    }
}

/// Waits idle; the device then goes as its `Lent` lets it go.
impl Drop for HeadlessTestGpuLease {
    fn drop(&mut self) {
        self.wait();
    }
}

/// An open device, and the adapter it opened on, which keeps the instance alive for as long as
/// any device on it.
#[derive(Debug)]
struct OpenDevice {
    gpu: Gpu,
    /// `name (backend, driver)`, as [`adapter_identity`] writes it.
    identity: String,
    /// After `gpu`, as fields drop in order: a device goes before its adapter.
    _adapter: Arc<wgpu::Adapter>,
}

/// An `Arc` [`REGISTRY`] lent out, dropped under its lock: the last holder lets go of a device and
/// its adapter before another lease opens new ones, so one instance is open at a time.
#[derive(Debug)]
struct Lent<T>(ManuallyDrop<Arc<T>>);

impl<T> Lent<T> {
    const fn new(lent: Arc<T>) -> Self {
        Self(ManuallyDrop::new(lent))
    }
}

impl<T> Deref for Lent<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> Drop for Lent<T> {
    fn drop(&mut self) {
        let registry = registry();
        // SAFETY: the `Arc` is not read again, and this is its one drop.
        unsafe { ManuallyDrop::drop(&mut self.0) };
        drop(registry);
    }
}

/// What the process knows between leases: no handle that keeps a device or an adapter alive.
#[derive(Debug)]
struct Registry {
    adapter: Weak<wgpu::Adapter>,
    /// The device [`headless_test_gpu`] shares, while a lease holds it.
    shared: Weak<OpenDevice>,
    /// Why the adapter did not open, so every lease after the first panics at once instead of
    /// retrying for [`ADAPTER_RETRY_TIMEOUT`].
    failed: Option<LeaseFailed>,
}

/// Every open and every drop of a lease runs under this lock ([`Lent`]): a second instance
/// opened while another thread named objects on the first crashed the Vulkan loader under
/// lavapipe.
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    adapter: Weak::new(),
    shared: Weak::new(),
    failed: None,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock_unpoisoned()
}

impl Registry {
    /// A lease of the shared device, opened when no lease holds it.
    fn lend_shared(&mut self) -> HeadlessTestGpuLease {
        let device = self.shared.upgrade().unwrap_or_else(|| {
            let device = self.open_device(u32::MAX);
            self.shared = Arc::downgrade(&device);
            device
        });
        HeadlessTestGpuLease {
            device: Lent::new(device),
            #[cfg(test)]
            frames: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A lease of a new device no other lease holds; see [`Self::open_device`].
    fn lend_isolated(&mut self, max_texture_dim: u32) -> HeadlessTestGpuLease {
        HeadlessTestGpuLease {
            device: Lent::new(self.open_device(max_texture_dim)),
            #[cfg(test)]
            frames: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A new device on the adapter every open device shares, whose `max_texture_dimension_2d` is the
    /// adapter's, or `max_texture_dim` where that is below it.
    ///
    /// # Panics
    ///
    /// Panics if the process has no adapter or the device does not open.
    fn open_device(&mut self, max_texture_dim: u32) -> Arc<OpenDevice> {
        let adapter = self.adapter();
        let request = test_request();
        let mut requirements = DeviceRequirements::negotiate(&adapter, request.optional)
            .unwrap_or_else(|why| panic!("the test adapter runs Palantir: {}", Chain(&why)));
        let limit = &mut requirements.limits.max_texture_dimension_2d;
        *limit = (*limit).min(max_texture_dim);
        let gpu = request
            .open_with(wgpu::Adapter::clone(&adapter), requirements)
            .unwrap_or_else(|why| panic!("open headless test gpu: {}", Chain(&why)));
        Arc::new(OpenDevice {
            gpu: gpu.gpu,
            identity: adapter_identity(&adapter.get_info()),
            _adapter: adapter,
        })
    }

    /// The adapter every open device shares, opened when no device holds it.
    ///
    /// # Panics
    ///
    /// Panics if the process has no adapter.
    fn adapter(&mut self) -> Arc<wgpu::Adapter> {
        if let Some(why) = &self.failed {
            panic!("{why}");
        }
        if let Some(adapter) = self.adapter.upgrade() {
            return adapter;
        }
        match open_adapter() {
            Ok(adapter) => {
                let adapter = Arc::new(adapter);
                self.adapter = Arc::downgrade(&adapter);
                adapter
            }
            Err(why) => panic!("{}", self.failed.insert(why)),
        }
    }
}

/// The adapter every lease opens on, under this process's GPU lock. A retry's failed instance
/// drops before any device opens, so one instance is open at a time ([`REGISTRY`]).
fn open_adapter() -> Result<wgpu::Adapter, LeaseFailed> {
    static PROCESS_LOCK: OnceLock<File> = OnceLock::new();
    PROCESS_LOCK.get_or_init(lock_gpu_process);
    let started = Instant::now();
    let request = test_request();
    loop {
        // The adapter a device opened on, as a driver can report what its device then refuses;
        // the probe device closes before any lease opens its own.
        match GpuRequest::instance().and_then(|instance| request.open(&instance)) {
            Ok(probe) => return Ok(probe.adapter),
            // Only a missing adapter is worth retrying: another test binary may still be tearing its own down.
            Err(GpuRequestError::RequestAdapter { .. })
                if started.elapsed() < ADAPTER_RETRY_TIMEOUT =>
            {
                thread::sleep(ADAPTER_RETRY_INTERVAL);
            }
            Err(source) => {
                return Err(LeaseFailed {
                    waited: started.elapsed(),
                    source,
                });
            }
        }
    }
}

/// What every lease asks for. The timing features let the tracer's tests share the device; the
/// backend reads them only under `collect_gpu_stats`.
const fn test_request() -> GpuRequest<'static> {
    GpuRequest {
        label: "palantir.test.device",
        power_preference: TEST_POWER,
        optional: DeviceRequirements::GPU_TIMING_FEATURES,
        compatible_surface: None,
    }
}

/// Lease the device the process's tests share. It opens with the first lease and is destroyed
/// with the last, so tests that run at once share one, and none is alive when the process exits.
///
/// The first adapter takes an interprocess OS lock held until the process exits, so no other Palantir test binary uses the GPU concurrently.
///
/// Without an adapter every lease panics, at once after the first.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    registry().lend_shared()
}

/// Lease a device no other test has touched, for tests whose numbers depend on device history (wgpu sizes per-submission allocation by every resource seen). Same lock and adapter as [`headless_test_gpu`].
pub fn isolated_headless_test_gpu() -> HeadlessTestGpuLease {
    registry().lend_isolated(u32::MAX)
}

/// [`isolated_headless_test_gpu`] with its `max_texture_dimension_2d` capped at
/// `max_texture_dim`, as a device on a smaller GPU has.
///
/// # Panics
///
/// Panics if the process has no adapter or the device does not open.
#[cfg(test)]
pub(crate) fn isolated_headless_test_gpu_capped(max_texture_dim: u32) -> HeadlessTestGpuLease {
    registry().lend_isolated(max_texture_dim)
}

/// `name (backend, driver driver_info)`, omitting driver fields the backend does not report.
fn adapter_identity(info: &wgpu::AdapterInfo) -> String {
    let driver = [info.driver.as_str(), info.driver_info.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if driver.is_empty() {
        format!("{} ({:?})", info.name, info.backend)
    } else {
        format!("{} ({:?}, {driver})", info.name, info.backend)
    }
}

fn lock_gpu_process() -> File {
    // One scope per working copy: two checkouts test in parallel, two binaries of one checkout take turns; the hashed manifest path carries that scope into a shared directory and keeps users off each other's file.
    let mut hasher = DefaultHasher::new();
    env!("CARGO_MANIFEST_DIR").hash(&mut hasher);
    let path = env::temp_dir().join(format!("palantir-gpu-test-{:016x}.lock", hasher.finish()));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open Palantir GPU test lock {}: {error}", path.display()));
    file.lock().expect("lock Palantir GPU test process");
    file
}
