//! Shared headless GPU lifecycle for feature-gated tests.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use pollster::FutureExt;

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
        let adapter = request_headless_adapter();
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aperture.headless_test.device"),
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request headless test device");
        Self {
            queue,
            device,
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
/// test process exits, preventing another Aperture test binary from entering
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
    std::fs::create_dir_all(&scratch).expect("create Aperture scratch directory");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(scratch.join("gpu-test.lock"))
        .expect("open Aperture GPU test lock");
    file.lock().expect("lock Aperture GPU test process");
    file
}

fn request_headless_adapter() -> wgpu::Adapter {
    let started = Instant::now();
    loop {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
        {
            Ok(adapter) => return adapter,
            Err(_) if started.elapsed() < ADAPTER_RETRY_TIMEOUT => {
                thread::sleep(ADAPTER_RETRY_INTERVAL);
            }
            Err(error) => {
                panic!("request headless test adapter after {ADAPTER_RETRY_TIMEOUT:?}: {error:?}");
            }
        }
    }
}
