//! Shared headless GPU lifecycle for feature-gated tests.

use crate::gpu::power_preference::PowerPreference;
use glam::UVec2;
use std::env;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::gpu::error::GpuRequestError;
use crate::gpu::render_target;
use crate::gpu::requested_gpu::Gpu;
use crate::gpu::requested_gpu::RequestedGpu;

const ADAPTER_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const ADAPTER_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

/// A headless device and its queue, waited idle when the lease drops.
#[derive(Debug)]
pub struct HeadlessTestGpuLease {
    /// The leased device's queue.
    pub queue: wgpu::Queue,
    /// The leased device.
    pub device: wgpu::Device,
}

impl HeadlessTestGpuLease {
    /// What every test driver's target allows: draw into it, and copy
    /// either way for readback and clears.
    pub const TARGET_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
        .union(wgpu::TextureUsages::COPY_DST)
        .union(wgpu::TextureUsages::COPY_SRC);

    /// The device and queue, as a host takes them.
    pub fn handles(&self) -> Gpu {
        Gpu::new(self.device.clone(), self.queue.clone())
    }

    /// A 2D `Rgba8UnormSrgb` render target of `size`, with
    /// [`Self::TARGET_USAGES`] — what most tests draw into.
    ///
    /// `label` shows up in RenderDoc and in wgpu's validation errors, so it
    /// should name the test, not the shape.
    pub fn target(&self, label: &str, size: UVec2) -> wgpu::Texture {
        self.target_with(
            label,
            size,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::TARGET_USAGES,
        )
    }

    /// [`Self::target`] in a format and with usages of the caller's
    /// choosing — for a test about a format, or about what a target
    /// without one of the usages does.
    pub fn target_with(
        &self,
        label: &str,
        size: UVec2,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
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

    /// A new device under this process's GPU lock, which the first device
    /// takes and every later one shares — or why there is none, worded
    /// for the panic every caller turns it into.
    fn request() -> Result<Self, String> {
        static PROCESS_LOCK: OnceLock<File> = OnceLock::new();
        PROCESS_LOCK.get_or_init(lock_gpu_process);
        let started = Instant::now();
        let gpu = loop {
            // The same preference the benches take. A test is worth little
            // if it draws on an adapter no user is looking at: where a machine
            // offers more than one — a laptop with its integrated GPU exposed,
            // or anywhere a software rasterizer is installed alongside a real
            // driver — `LowPower` picks the other one, and every golden then
            // records what that other one drew.
            match RequestedGpu::headless(PowerPreference::HighPerformance, wgpu::Features::empty())
            {
                Ok(gpu) => break gpu,
                // Only a missing adapter is worth waiting on — another test
                // binary may still be tearing its own down. No backend at all,
                // an adapter that cannot meet the requirements, or a refused
                // device will say exactly the same thing two seconds later.
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
        Ok(Self {
            queue: gpu.gpu.queue,
            device: gpu.gpu.device,
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
/// Initialization takes an interprocess OS lock that remains held until the
/// test process exits, preventing another Palantir test binary from entering
/// its GPU section concurrently.
///
/// A failed request is kept as well: every later lease in the process
/// panics with the same message at once, instead of each GPU test paying
/// the adapter retry again.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    static GPU: OnceLock<Result<HeadlessTestGpuLease, String>> = OnceLock::new();
    match GPU.get_or_init(HeadlessTestGpuLease::request) {
        Ok(gpu) => HeadlessTestGpuLease {
            queue: gpu.queue.clone(),
            device: gpu.device.clone(),
        },
        Err(why) => panic!("{why}"),
    }
}

/// Lease a device no other test has touched, for a test whose numbers
/// depend on the device's history.
///
/// Part of what wgpu allocates per submission is sized by every resource
/// the device has seen, so on the shared device a test counts what the
/// tests before it left behind. Under the same interprocess lock as
/// [`headless_test_gpu`].
pub fn isolated_headless_test_gpu() -> HeadlessTestGpuLease {
    HeadlessTestGpuLease::request().unwrap_or_else(|why| panic!("{why}"))
}

fn lock_gpu_process() -> File {
    // The scope stays one working copy, as it was when the file sat under the
    // manifest directory: two checkouts test in parallel, two binaries of one
    // checkout take turns. The hashed manifest path carries that scope into a
    // directory every checkout shares. It also keeps two users off one file,
    // which the sticky bit on `/tmp` would leave unopenable for the second.
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
