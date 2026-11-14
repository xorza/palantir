//! What the device request and the window surface report when they fail.

use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};

use crate::gpu::flag_names;

/// A device or adapter cannot run Palantir's pipelines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmetRequirements {
    /// A required feature is absent.
    Features {
        /// The features of [`DeviceRequirements::REQUIRED_FEATURES`](crate::DeviceRequirements::REQUIRED_FEATURES)
        /// the device lacks.
        missing: wgpu::Features,
    },
    /// A required capability of the backend is absent, as compute shaders are on OpenGL ES 3.0.
    Downlevel {
        /// The flags of [`DeviceRequirements::REQUIRED_DOWNLEVEL`](crate::DeviceRequirements::REQUIRED_DOWNLEVEL)
        /// the adapter lacks.
        missing: wgpu::DownlevelFlags,
    },
    /// A limit sits below the floor Palantir needs.
    Limit {
        /// The device-limit field name.
        name: &'static str,
        /// The floor Palantir needs.
        required: u64,
        /// What the device offers.
        available: u64,
    },
}

impl Display for UnmetRequirements {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Features { missing } => write!(
                f,
                "graphics device is missing {}, which Palantir requires",
                fmt::from_fn(|f| flag_names::write(f, missing.iter_names()))
            ),
            Self::Downlevel { missing } => write!(
                f,
                "graphics backend lacks {}, which Palantir requires",
                fmt::from_fn(|f| flag_names::write(f, missing.iter_names()))
            ),
            Self::Limit {
                name,
                required,
                available,
            } => write!(
                f,
                "graphics device limit {name} is {available}, but Palantir requires {required}"
            ),
        }
    }
}

impl Error for UnmetRequirements {}

/// Failure while asking a driver for a device.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuRequestError {
    /// Palantir was compiled without a wgpu backend for the current target.
    NoBackend,
    /// No graphics adapter matched the requested power policy.
    RequestAdapter {
        /// What wgpu reported.
        source: wgpu::RequestAdapterError,
    },
    /// The adapter that answered cannot run Palantir's pipelines.
    Requirements {
        /// Which requirement went unmet.
        source: UnmetRequirements,
    },
    /// The adapter could not create the logical device.
    RequestDevice {
        /// What wgpu reported.
        source: wgpu::RequestDeviceError,
    },
}

impl Display for GpuRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBackend => f.write_str("no wgpu backend is compiled in for this target"),
            Self::RequestAdapter { .. } => f.write_str("failed to find a graphics adapter"),
            Self::Requirements { .. } => f.write_str("the graphics adapter cannot run Palantir"),
            Self::RequestDevice { .. } => f.write_str("failed to create the graphics device"),
        }
    }
}

impl Error for GpuRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NoBackend => None,
            Self::RequestAdapter { source } => Some(source),
            Self::Requirements { source } => Some(source),
            Self::RequestDevice { source } => Some(source),
        }
    }
}

impl From<wgpu::RequestAdapterError> for GpuRequestError {
    fn from(source: wgpu::RequestAdapterError) -> Self {
        Self::RequestAdapter { source }
    }
}

impl From<UnmetRequirements> for GpuRequestError {
    fn from(source: UnmetRequirements) -> Self {
        Self::Requirements { source }
    }
}

impl From<wgpu::RequestDeviceError> for GpuRequestError {
    fn from(source: wgpu::RequestDeviceError) -> Self {
        Self::RequestDevice { source }
    }
}

/// A native window surface Palantir cannot present through. Separate from
/// [`GpuRequestError`] because a surface is per window.
#[cfg(feature = "winit")]
#[derive(Debug)]
#[non_exhaustive]
pub enum SurfaceError {
    /// The platform could not create a presentation surface for the window.
    Create {
        /// What wgpu reported.
        source: wgpu::CreateSurfaceError,
    },
    /// The selected adapter cannot present to this window's surface.
    Incompatible,
    /// The surface cannot satisfy Palantir's linear-to-sRGB output contract.
    MissingSrgb,
    /// The surface lacks texture usages Palantir's compositor needs.
    MissingUsages {
        /// The usages the compositor needs and the surface lacks.
        missing: wgpu::TextureUsages,
    },
}

/// What opening a windowed host's device and first surface reports: either half can fail.
#[cfg(feature = "winit")]
#[derive(Debug)]
pub(crate) enum SurfaceStartError {
    Gpu { source: GpuRequestError },
    Surface { source: SurfaceError },
}

#[cfg(feature = "winit")]
impl From<GpuRequestError> for SurfaceStartError {
    fn from(source: GpuRequestError) -> Self {
        Self::Gpu { source }
    }
}

#[cfg(feature = "winit")]
impl From<SurfaceError> for SurfaceStartError {
    fn from(source: SurfaceError) -> Self {
        Self::Surface { source }
    }
}

#[cfg(feature = "winit")]
impl Display for SurfaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { .. } => f.write_str("failed to create a window surface"),
            Self::Incompatible => {
                f.write_str("graphics adapter cannot present to the window surface")
            }
            Self::MissingSrgb => f.write_str("window surface has no sRGB format and color space"),
            Self::MissingUsages { missing } => write!(
                f,
                "window surface cannot do {}, which Palantir's compositor needs",
                fmt::from_fn(|f| flag_names::write(f, missing.iter_names()))
            ),
        }
    }
}

#[cfg(feature = "winit")]
impl Error for SurfaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create { source } => Some(source),
            Self::Incompatible | Self::MissingSrgb | Self::MissingUsages { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::error::{GpuRequestError, UnmetRequirements};

    /// A source converts into its own case, so `?` lands the failure where `source` finds it. wgpu's
    /// device error has private fields, so only wgpu builds one.
    #[test]
    fn each_source_converts_into_its_case() {
        let missing = wgpu::Features::empty();
        assert!(matches!(
            GpuRequestError::from(UnmetRequirements::Features { missing }),
            GpuRequestError::Requirements {
                source: UnmetRequirements::Features { .. }
            }
        ));
        assert!(matches!(
            GpuRequestError::from(wgpu::RequestAdapterError::EnvNotSet),
            GpuRequestError::RequestAdapter {
                source: wgpu::RequestAdapterError::EnvNotSet
            }
        ));
    }
}
