//! Shared headless GPU lifecycle for feature-gated tests.

use crate::gpu::device::power_preference::PowerPreference;
use glam::UVec2;
use std::env;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::device::requested_gpu::RequestedGpu;
use crate::gpu::error::GpuRequestError;
use crate::gpu::surface::render_target;

const ADAPTER_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const ADAPTER_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

/// A headless device and its queue, waited idle when the lease drops.
#[derive(Debug)]
pub struct HeadlessTestGpuLease {
    /// The queue.
    pub queue: wgpu::Queue,
    /// The device.
    pub device: wgpu::Device,
    /// The adapter the device opened on; golden suites record it.
    pub adapter: String,
}

impl HeadlessTestGpuLease {
    /// Usages every test target allows: draw into it, copy either way.
    pub const TARGET_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
        .union(wgpu::TextureUsages::COPY_DST)
        .union(wgpu::TextureUsages::COPY_SRC);

    /// Borrowed handles to the device and queue.
    pub fn handles(&self) -> Gpu {
        Gpu::new(self.device.clone(), self.queue.clone())
    }

    /// A 2D `Rgba8UnormSrgb` render target of `size` with [`Self::TARGET_USAGES`]; `label` names the test in RenderDoc and validation errors.
    pub fn target(&self, label: &str, size: UVec2) -> wgpu::Texture {
        self.target_with(
            label,
            size,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::TARGET_USAGES,
        )
    }

    /// [`Self::target`] in a chosen format and usages.
    pub fn target_with(
        &self,
        label: &str,
        size: UVec2,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        render_target::internals::texture(&self.device, label, size, format, usage)
    }

    /// Block until every submission on the device has finished.
    pub fn wait(&self) {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("wait for the headless test device");
    }

    /// A new device under this process's GPU lock, or why there is none, worded for the caller's panic.
    fn request() -> Result<Self, String> {
        static PROCESS_LOCK: OnceLock<File> = OnceLock::new();
        PROCESS_LOCK.get_or_init(lock_gpu_process);
        let started = Instant::now();
        let gpu = loop {
            // HighPerformance, as the benches use: with several adapters, goldens must record the one users see.
            match RequestedGpu::headless(PowerPreference::HighPerformance, wgpu::Features::empty())
            {
                Ok(gpu) => break gpu,
                // Only a missing adapter is worth retrying: another test binary may still be tearing its own down.
                Err(GpuRequestError::RequestAdapter { .. })
                    if started.elapsed() < ADAPTER_RETRY_TIMEOUT =>
                {
                    thread::sleep(ADAPTER_RETRY_INTERVAL);
                }
                Err(error) => {
                    return Err(format!(
                        "lease headless test gpu after {:?}: {error}",
                        started.elapsed()
                    ));
                }
            }
        };
        let info = gpu.adapter.get_info();
        Ok(Self {
            queue: gpu.gpu.queue,
            device: gpu.gpu.device,
            adapter: adapter_identity(&info),
        })
    }
}

impl Drop for HeadlessTestGpuLease {
    fn drop(&mut self) {
        self.wait();
    }
}

/// Lease the one GPU shared by this process.
///
/// Initialization takes an interprocess OS lock held until the process exits, so no other Palantir test binary uses the GPU concurrently.
///
/// A failed request is cached: later leases panic with the same message at once.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    static GPU: OnceLock<Result<HeadlessTestGpuLease, String>> = OnceLock::new();
    match GPU.get_or_init(HeadlessTestGpuLease::request) {
        Ok(gpu) => HeadlessTestGpuLease {
            queue: gpu.queue.clone(),
            device: gpu.device.clone(),
            adapter: gpu.adapter.clone(),
        },
        Err(why) => panic!("{why}"),
    }
}

/// Lease a device no other test has touched, for tests whose numbers depend on device history (wgpu sizes per-submission allocation by every resource seen). Same lock as [`headless_test_gpu`].
pub fn isolated_headless_test_gpu() -> HeadlessTestGpuLease {
    HeadlessTestGpuLease::request().unwrap_or_else(|why| panic!("{why}"))
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
