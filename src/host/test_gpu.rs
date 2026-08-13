//! Shared headless GPU lifecycle for feature-gated tests.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::host::error::HeadlessGpuError;
use crate::host::headless_gpu::HeadlessGpu;

const ADAPTER_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const ADAPTER_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct ProcessGpu {
    queue: wgpu::Queue,
    device: wgpu::Device,
    _process_lock: File,
}

impl ProcessGpu {
    fn new() -> Self {
        let process_lock = lock_gpu_process();
        let started = Instant::now();
        let gpu = loop {
            // The same preference the benches take, and the one a windowed
            // host is normally configured with. A test is worth little if it
            // draws on an adapter no user is looking at: where a machine
            // offers more than one — a laptop with its integrated GPU exposed,
            // or anywhere a software rasterizer is installed alongside a real
            // driver — `LowPower` picks the other one, and every golden then
            // records what that other one drew.
            match HeadlessGpu::new(
                wgpu::PowerPreference::HighPerformance,
                wgpu::Features::empty(),
            ) {
                Ok(gpu) => break gpu,
                // Only a missing adapter is worth waiting on — another test
                // binary may still be tearing its own down. No backend at all,
                // an adapter that cannot meet the requirements, or a refused
                // device will say exactly the same thing two seconds later.
                Err(HeadlessGpuError::RequestAdapter { .. })
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
            queue: gpu.queue,
            device: gpu.device,
            _process_lock: process_lock,
        }
    }
}

/// Borrowed handles to the process-static headless GPU.
#[derive(Debug)]
pub struct HeadlessTestGpuLease {
    pub queue: wgpu::Queue,
    pub device: wgpu::Device,
    gpu: &'static ProcessGpu,
}

impl Drop for HeadlessTestGpuLease {
    fn drop(&mut self) {
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("finish headless GPU lease work");
    }
}

/// Lease the one GPU owned by this process.
///
/// Initialization takes an interprocess OS lock that remains held until the
/// test process exits, preventing another Palantir test binary from entering
/// its GPU section concurrently.
pub fn headless_test_gpu() -> HeadlessTestGpuLease {
    static GPU: OnceLock<ProcessGpu> = OnceLock::new();
    let gpu = GPU.get_or_init(ProcessGpu::new);
    HeadlessTestGpuLease {
        queue: gpu.queue.clone(),
        device: gpu.device.clone(),
        gpu,
    }
}

fn lock_gpu_process() -> File {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    std::fs::create_dir_all(&scratch).expect("create Palantir scratch directory");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(scratch.join("gpu-test.lock"))
        .expect("open Palantir GPU test lock");
    file.lock().expect("lock Palantir GPU test process");
    file
}
