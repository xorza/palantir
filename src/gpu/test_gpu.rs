//! Shared headless GPU lifecycle for feature-gated tests.

use crate::common::sync::MutexExt;
use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use glam::UVec2;
use std::env;
use std::error::Error;
use std::ffi::c_int;
use std::fmt::{self, Display, Formatter};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem;
use std::ops::Deref;
use std::sync::Mutex;

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::device::requested_gpu::GpuRequest;
use crate::gpu::error::GpuRequestError;
#[cfg(test)]
use crate::gpu::frame::gpu_ctx::GpuCtx;
#[cfg(test)]
use crate::gpu::resource::upload_staging::UploadStaging;
use crate::gpu::surface::owned_target::{OwnedTarget, TargetSpec};
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::surface::texel_readback::TexelReadback;

/// HighPerformance, as the benches use: with several adapters, goldens must record the one users see.
const TEST_POWER: PowerPreference = PowerPreference::HighPerformance;

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

/// A headless device and its queue. It lends its [`Gpu`], as a guard does, and dropping it closes
/// nothing the process shares (`PROCESS`).
#[derive(Debug)]
pub struct TestGpu {
    gpu: Gpu,
    /// The display frame [`Self::frame`] runs next, one a call, so what decays by frames ages
    /// across a test's frames.
    #[cfg(test)]
    frames: std::sync::atomic::AtomicU64,
}

impl Deref for TestGpu {
    type Target = Gpu;

    fn deref(&self) -> &Gpu {
        &self.gpu
    }
}

impl TestGpu {
    const fn new(gpu: Gpu) -> Self {
        Self {
            gpu,
            #[cfg(test)]
            frames: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// An owned clone of the handles, for an API that takes a [`Gpu`] by value. It may outlive
    /// this value.
    pub fn handles(&self) -> Gpu {
        self.gpu.clone()
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
        self.gpu.wait();
    }

    /// Run `encode` on an encoder and a staging belt of its own, then finish, submit and wait for
    /// them: one frame of GPU work a test sets up alone. Returns what `encode` returns.
    #[cfg(test)]
    pub(crate) fn frame<R>(&self, encode: impl FnOnce(&mut GpuCtx<'_>) -> R) -> R {
        let mut belt = UploadStaging::new(&self.gpu.device, 1 << 16);
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let frame = self
            .frames
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let encoded = encode(&mut GpuCtx::new(self, &mut belt, &mut encoder, frame));
        belt.finish_and_recall_on_submit(&encoder);
        self.gpu.queue.submit([encoder.finish()]);
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

/// The process's test GPU, opened by the first test that asks. One instance for the whole process:
/// a second one opened while another thread named objects on the first crashed the Vulkan loader
/// under lavapipe, and a test can then hold a handle or a resource in any order. A static, as
/// libtest offers no hook after its last test. A static is never dropped (RFC 1440), and a device
/// still alive in `exit()` raced the driver's shader-compiler thread and corrupted the heap, so
/// [`close_at_exit`] closes it. Refused: a lease count that closes the instance with the last
/// holder, whose handles outlived it in a dozen places; a process per test as wgpu's nextest runs
/// them, 8 s against 2 s for this suite on an Apple laptop, with a tool every machine must install.
static PROCESS: Mutex<ProcessGpu> = Mutex::new(ProcessGpu::Unopened);

unsafe extern "C" {
    /// The C standard's: handlers run at `exit()`, the one registered last first.
    safe fn atexit(handler: extern "C" fn()) -> c_int;
}

/// Close the process's test GPU before the driver's own exit handlers run. A lock another thread
/// holds, or one a panic poisoned, leaves it open: no panic may cross this callback.
extern "C" fn close_at_exit() {
    #[expect(
        clippy::disallowed_methods,
        reason = "a held or poisoned lock skips the close, as a panic may not cross into C"
    )]
    if let Ok(mut process) = PROCESS.try_lock() {
        let closing = mem::replace(&mut *process, ProcessGpu::Closed);
        drop(process);
        if let ProcessGpu::Open(open) = closing {
            open.shared.wait();
            drop(open);
        }
    }
}

/// How far the process's test GPU has come.
#[derive(Debug)]
enum ProcessGpu {
    Unopened,
    Open(OpenGpu),
    /// Why it did not open, which every ask after the first panics with at once.
    NoAdapter(String),
    /// The process is exiting.
    Closed,
}

impl ProcessGpu {
    /// The open GPU, opened now if none asked yet, or what stops it opening.
    fn open(&mut self) -> Result<&OpenGpu, String> {
        if matches!(self, Self::Unopened) {
            *self = match OpenGpu::open() {
                Ok(open) => {
                    // After the first device: the driver registers its exit handlers while it
                    // opens one, and the last registered runs first.
                    assert_eq!(atexit(close_at_exit), 0, "the C library takes the handler");
                    Self::Open(open)
                }
                Err(why) => Self::NoAdapter(format!("open headless test gpu: {}", Chain(&why))),
            };
        }
        match self {
            Self::Open(open) => Ok(open),
            Self::NoAdapter(why) => Err(why.clone()),
            Self::Closed => Err("the test GPU closed as the process exits".to_owned()),
            Self::Unopened => unreachable!("opened above"),
        }
    }
}

/// `f` on the open GPU. A failure panics past the lock, so the next ask reads its reason.
fn on_open<R>(f: impl FnOnce(&OpenGpu) -> R) -> R {
    let mut process = PROCESS.lock_unpoisoned();
    let outcome = process.open().map(f);
    drop(process);
    outcome.unwrap_or_else(|why| panic!("{why}"))
}

/// The instance's one adapter and the device tests share. Fields drop in order: a device goes
/// before its adapter, and the lock last.
#[derive(Debug)]
struct OpenGpu {
    shared: Gpu,
    /// Keeps the instance alive.
    adapter: wgpu::Adapter,
    /// Held so no other Palantir test binary uses the GPU at once.
    _process_lock: File,
}

impl OpenGpu {
    fn open() -> Result<Self, GpuRequestError> {
        let process_lock = lock_gpu_process();
        // The adapter a device opened on, as a driver can report what its device then refuses; the
        // probe device closes before the shared one opens.
        let adapter = test_request().open(&GpuRequest::instance()?)?.adapter;
        Ok(Self {
            shared: device_on(&adapter, u32::MAX),
            adapter,
            _process_lock: process_lock,
        })
    }
}

/// A new device on `adapter`, whose `max_texture_dimension_2d` is the adapter's, or
/// `max_texture_dim` where that is below it.
///
/// # Panics
///
/// Panics if the device does not open.
fn device_on(adapter: &wgpu::Adapter, max_texture_dim: u32) -> Gpu {
    let request = test_request();
    let mut requirements = DeviceRequirements::negotiate(adapter, request.optional)
        .unwrap_or_else(|why| panic!("the test adapter runs Palantir: {}", Chain(&why)));
    let limit = &mut requirements.limits.max_texture_dimension_2d;
    *limit = (*limit).min(max_texture_dim);
    request
        .open_with(adapter.clone(), requirements)
        .unwrap_or_else(|why| panic!("open headless test gpu: {}", Chain(&why)))
        .gpu
}

/// What every test device asks for. The timing features let the tracer's tests share the device; the
/// backend reads them only under `collect_gpu_stats`.
const fn test_request() -> GpuRequest<'static> {
    GpuRequest {
        label: "palantir.test.device",
        power_preference: TEST_POWER,
        optional: DeviceRequirements::GPU_TIMING_FEATURES,
        compatible_surface: None,
    }
}

/// The device the process's tests share, so tests that run at once share one. It opens with the
/// first ask and closes as the process exits (`PROCESS`).
///
/// Without an adapter every ask panics, at once after the first.
pub fn headless_test_gpu() -> TestGpu {
    on_open(|open| TestGpu::new(open.shared.clone()))
}

/// A new device no other test has touched, for a test that loses it, or whose numbers depend on
/// device history (wgpu sizes per-submission allocation by every resource seen). A test may ask for
/// several, each on the process's one adapter; each closes with its last handle.
pub fn isolated_headless_test_gpu() -> TestGpu {
    on_open(|open| TestGpu::new(device_on(&open.adapter, u32::MAX)))
}

/// [`isolated_headless_test_gpu`] with its `max_texture_dimension_2d` capped at
/// `max_texture_dim`, as a device on a smaller GPU has.
///
/// # Panics
///
/// Panics if the process has no adapter or the device does not open.
#[cfg(test)]
pub(crate) fn isolated_headless_test_gpu_capped(max_texture_dim: u32) -> TestGpu {
    on_open(|open| TestGpu::new(device_on(&open.adapter, max_texture_dim)))
}

/// The adapter every test device opens on, `name (backend, driver)`; golden suites record it.
pub fn headless_test_adapter() -> String {
    on_open(|open| adapter_identity(&open.adapter.get_info()))
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

#[cfg(test)]
mod tests {
    use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};

    /// A handle outlives the value it came from, a later ask gets the same device, and each
    /// isolated ask gets another of its own.
    #[test]
    fn asks_share_one_device_and_each_isolated_ask_opens_its_own() {
        let first = headless_test_gpu();
        let handle = first.handles();
        drop(first);

        let shared = headless_test_gpu();
        assert_eq!(handle.device, shared.device);

        let [one, two] = [(); 2].map(|()| isolated_headless_test_gpu());
        assert_ne!(one.device, shared.device);
        assert_ne!(two.device, shared.device);
        assert_ne!(one.device, two.device);
    }
}
