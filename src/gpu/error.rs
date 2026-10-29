//! What the device request and the window surface report when they fail.

use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};

use crate::gpu::device::device_features::DeviceFeatures;
#[cfg(feature = "winit")]
use crate::gpu::surface::surface_usages::SurfaceUsages;

/// A device or adapter cannot run Palantir's pipelines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmetRequirements {
    /// A required feature is absent.
    Features {
        /// The features needed and missing.
        missing: DeviceFeatures,
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
                "graphics device is missing {missing}, which Palantir requires"
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
            Self::RequestAdapter { source } => {
                write!(f, "failed to find a graphics adapter: {source}")
            }
            Self::Requirements { source } => {
                write!(f, "the graphics adapter cannot run Palantir: {source}")
            }
            Self::RequestDevice { source } => {
                write!(f, "failed to create the graphics device: {source}")
            }
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
        missing: SurfaceUsages,
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
            Self::Create { source } => write!(f, "failed to create a window surface: {source}"),
            Self::Incompatible => {
                f.write_str("graphics adapter cannot present to the window surface")
            }
            Self::MissingSrgb => f.write_str("window surface has no sRGB format and color space"),
            Self::MissingUsages { missing } => write!(
                f,
                "window surface cannot do {missing}, which Palantir's compositor needs"
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
