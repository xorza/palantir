//! Shared headless GPU lifecycle for feature-gated tests.

use crate::gpu::power_preference::PowerPreference;
use std::env;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::gpu::error::GpuRequestError;
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
    /// The device and queue, as a host takes them.
    pub fn handles(&self) -> Gpu {
        Gpu::new(self.device.clone(), self.queue.clone())
    }

    /// A new device under this process's GPU lock, which the first device
    /// takes and every later one shares.
    fn request() -> Self {
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
                    panic!(
                        "lease headless test gpu after {:?}: {error}",
                        started.elapsed()
                    );
                }
            }
        };
        Self {
            queue: gpu.gpu.queue,
            device: gpu.gpu.device,
        }
    }
}

impl Drop for HeadlessTestGpuLease {
    fn drop(&mut self) {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("finish headless GPU lease work");
    }
}

/// Lease the one GPU shared by this process.
///
/// Initialization takes an interprocess OS lock that remains held until the
/// test process exits, preventing another Palantir test binary from entering
/// its GPU section concurrently.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    static GPU: OnceLock<HeadlessTestGpuLease> = OnceLock::new();
    let gpu = GPU.get_or_init(HeadlessTestGpuLease::request);
    HeadlessTestGpuLease {
        queue: gpu.queue.clone(),
        device: gpu.device.clone(),
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
    HeadlessTestGpuLease::request()
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
