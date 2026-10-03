//! Opening the device: what Palantir asks of an adapter, the handles it
//! hands out, and the contexts a `GpuPaint` receives.

pub(crate) mod backend_config;
pub(crate) mod backend_resources;
pub(crate) mod device_requirements;
pub(crate) mod gpu_ctx;
pub(crate) mod gpu_frame_ctx;
pub(crate) mod gpu_init_ctx;
pub(crate) mod power_preference;
pub(crate) mod requested_gpu;
