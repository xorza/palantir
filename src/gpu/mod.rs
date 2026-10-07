//! Every wgpu call in the crate: pipelines, atlases, the device request and the surface live here, so no other module names a `wgpu` type.
//!
//! [`WgpuBackend`](wgpu_backend::WgpuBackend) is the one GPU renderer: it opens the device through [`device`], records frames through [`frame`], draws with the [`pipeline`]s and [`raster`] tenants over the [`resource`]s, and lands on a [`surface`].

#![expect(
    clippy::disallowed_types,
    reason = "the one module that names wgpu; `clippy.toml` stops every other module doing the same"
)]
#[cfg(feature = "bench")]
pub(crate) mod bench;
#[cfg(feature = "bench")]
pub(crate) mod bench_gpu;
pub(crate) mod device;
pub(crate) mod error;
pub(crate) mod frame;
pub(crate) mod pipeline;
pub(crate) mod raster;
pub(crate) mod resource;
pub(crate) mod surface;
#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_gpu;
pub(crate) mod wgpu_backend;
