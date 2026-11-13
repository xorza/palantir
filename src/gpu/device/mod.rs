//! Opening the device: what Palantir asks of an adapter, the handles it hands out, and how a
//! frame leaves a backbuffer on it.

pub(crate) mod backbuffer_out;
pub(crate) mod device_health;
pub(crate) mod device_requirements;
pub(crate) mod gpu;
pub(crate) mod gpu_wait;
pub(crate) mod pass_traffic;
pub(crate) mod power_preference;
pub(crate) mod requested_gpu;
pub(crate) mod tiler_family;
