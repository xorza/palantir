//! Shared headless GPU lifecycle for feature-gated tests.

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use glam::UVec2;
use std::env;
use std::fmt::{self, Display, Formatter};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::iter;
use std::ops::Deref;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::requested_gpu::GpuRequest;
use crate::gpu::error::GpuRequestError;
use crate::gpu::surface::owned_target::{OwnedTarget, TargetSpec};
use crate::gpu::surface::render_target::{self, TargetFormat};

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
            self.waited, self.source
        )
    }
}

/// A headless device and its queue, waited idle when the lease drops. It lends its [`Gpu`], as a
/// guard does.
#[derive(Debug)]
pub struct HeadlessTestGpuLease {
    gpu: Gpu,
    /// The adapter the device opened on; golden suites record it.
    pub adapter: String,
}

impl Deref for HeadlessTestGpuLease {
    type Target = Gpu;

    fn deref(&self) -> &Gpu {
        &self.gpu
    }
}

impl HeadlessTestGpuLease {
    /// An owned clone of the handles, for an API that takes a [`Gpu`] by value.
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

    /// Every texel of `texture`'s first layer, rows top to bottom with no padding, waited for.
    ///
    /// # Panics
    ///
    /// For a texture made without `COPY_SRC`, or of a compressed or depth format.
    pub fn read_texture(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let texel = texture
            .format()
            .block_copy_size(None)
            .expect("a color texture copies whole texels");
        let row = texture.width() * texel;
        let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.read_texture"),
            size: u64::from(padded) * u64::from(texture.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(texture.height()),
                },
            },
            render_target::extent(render_target::texture_size(texture)),
        );
        self.queue.submit(iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |mapped| {
            tx.send(mapped).ok();
        });
        self.wait();
        rx.recv().unwrap().expect("the read buffer maps");
        let data = slice.get_mapped_range().unwrap();
        data.chunks_exact(padded as usize)
            .flat_map(|line| &line[..row as usize])
            .copied()
            .collect()
    }

    /// A new device on the process's one adapter, whose `max_texture_dimension_2d` is the
    /// adapter's, or `max_texture_dim` where that is below it.
    ///
    /// # Panics
    ///
    /// Panics if the process has no adapter or the device does not open.
    fn request(max_texture_dim: u32) -> Self {
        let adapter = shared_adapter();
        let info = adapter.get_info();
        let request = test_request();
        let mut requirements = DeviceRequirements::negotiate(&adapter, request.optional)
            .unwrap_or_else(|why| panic!("the test adapter runs Palantir: {why}"));
        let limit = &mut requirements.limits.max_texture_dimension_2d;
        *limit = (*limit).min(max_texture_dim);
        let gpu = request
            .open_with(adapter, requirements)
            .unwrap_or_else(|why| panic!("open headless test gpu: {why}"));
        Self {
            gpu: gpu.gpu,
            adapter: adapter_identity(&info),
        }
    }
}

impl Drop for HeadlessTestGpuLease {
    fn drop(&mut self) {
        self.wait();
    }
}

/// The process's one adapter, opened on first use.
///
/// # Panics
///
/// Panics if the process has no adapter.
fn shared_adapter() -> wgpu::Adapter {
    static ADAPTER: OnceLock<Result<wgpu::Adapter, LeaseFailed>> = OnceLock::new();
    match ADAPTER.get_or_init(open_adapter) {
        Ok(adapter) => adapter.clone(),
        Err(why) => panic!("{why}"),
    }
}

/// The adapter every lease opens on, under this process's GPU lock. Its instance is the one the
/// process keeps, as a retry's failed instance drops before any device opens: a second instance
/// opened while another thread named objects on the first crashed the Vulkan loader under
/// lavapipe.
fn open_adapter() -> Result<wgpu::Adapter, LeaseFailed> {
    static PROCESS_LOCK: OnceLock<File> = OnceLock::new();
    PROCESS_LOCK.get_or_init(lock_gpu_process);
    let started = Instant::now();
    let request = test_request();
    loop {
        match GpuRequest::instance().and_then(|instance| request.adapter(&instance)) {
            Ok(adapter) => return Ok(adapter),
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

/// Lease the one GPU shared by this process.
///
/// Initialization takes an interprocess OS lock held until the process exits, so no other Palantir test binary uses the GPU concurrently.
///
/// Without an adapter every lease panics, at once after the first.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    static GPU: OnceLock<HeadlessTestGpuLease> = OnceLock::new();
    let gpu = GPU.get_or_init(|| HeadlessTestGpuLease::request(u32::MAX));
    HeadlessTestGpuLease {
        gpu: gpu.gpu.clone(),
        adapter: gpu.adapter.clone(),
    }
}

/// Lease a device no other test has touched, for tests whose numbers depend on device history (wgpu sizes per-submission allocation by every resource seen). Same lock and adapter as [`headless_test_gpu`].
pub fn isolated_headless_test_gpu() -> HeadlessTestGpuLease {
    HeadlessTestGpuLease::request(u32::MAX)
}

/// [`isolated_headless_test_gpu`] with its `max_texture_dimension_2d` capped at
/// `max_texture_dim`, as a device on a smaller GPU has.
///
/// # Panics
///
/// Panics if the process has no adapter or the device does not open.
#[cfg(test)]
pub(crate) fn isolated_headless_test_gpu_capped(max_texture_dim: u32) -> HeadlessTestGpuLease {
    HeadlessTestGpuLease::request(max_texture_dim)
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
