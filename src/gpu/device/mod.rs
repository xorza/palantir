//! Opening the device: what Palantir asks of an adapter, and the handles it
//! hands out.

pub(crate) mod device_health;
pub(crate) mod device_requirements;
pub(crate) mod gpu;
pub(crate) mod gpu_wait;
pub(crate) mod pass_traffic;
pub(crate) mod requested_gpu;
pub(crate) mod tiler_family;
pub(crate) mod upload_staging;
